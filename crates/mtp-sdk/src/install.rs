//! Install, status and remove: the contract the CLI, the daemon and the GUI call.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::archive::{self, ExtractLimits};
use crate::download::{DownloadRequest, part_path};
use crate::error::{Error, IoContext, Result};
use crate::event::{Cancel, Event};
use crate::fsutil;
use crate::hash::{Digest, Sha1Hex};
use crate::home::Home;
use crate::license::{
    ANDROID_SDK_LICENSE, Decision, GPL_3, LicensePrompt, LicenseState, LicenseText, gpl3_text,
};
use crate::magisk::{self, MagiskRecord};
use crate::manifest::{MAX_PACKAGE_PATH_TAIL, Manifest, PackagePin, SetId};
use crate::repository::{Repository, check_pin};
use crate::source::Client;
use crate::store::{self, CheckReport, InstalledPackage, Notice, RootRecord, RootState, SdkRoot};

const MAX_XML_BYTES: u64 = 16 << 20;
/// Google's binaries don't use long paths, so every path in a root must fit in this.
const WINDOWS_MAX_PATH: usize = 260;

/// What `install` will do for one package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackageAction {
    /// Already in the target root.
    Reuse,
    /// Hard-link from another root installed from the same zip.
    Link {
        from: SetId,
    },
    /// A complete zip with the pinned hash is already in `downloads/`.
    Extract,
    Download,
}

/// What `install` would do, computed without any network request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub set: SetId,
    pub packages: Vec<(String, PackageAction)>,
    pub magisk_installed: bool,
    pub root_ready: bool,
}

impl Plan {
    /// True when `install` has nothing to do.
    pub fn is_complete(&self) -> bool {
        self.root_ready
            && self.magisk_installed
            && self
                .packages
                .iter()
                .all(|(_, action)| *action == PackageAction::Reuse)
    }
}

/// Options for `install`.
#[derive(Clone, Copy, Debug, Default)]
pub struct InstallOptions {
    /// Hash installed files instead of checking only their sizes.
    pub verify: bool,
    /// Keep the zips after the set reaches `ready`.
    pub keep_downloads: bool,
}

/// The outcome of `install`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    pub set: SetId,
    pub root: PathBuf,
    /// Files actually transferred. Zero on a second run.
    pub downloads: u32,
}

/// What `remove` should remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoveTarget {
    Set(SetId),
    /// Every root that is neither the manifest's set nor leased, and old Magisk tags.
    Unused,
    /// The zips and partial files MTPlayer downloaded, including ones from earlier pins.
    Downloads,
}

/// What `remove` removed and what it left.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Removed {
    pub roots: Vec<String>,
    pub magisk_tags: Vec<String>,
    pub downloads: bool,
    /// Roots `Unused` left in place because instances lease them.
    pub leased: Vec<String>,
    /// Directories under `tools/magisk` that `Unused` left in place, because no record says
    /// MTPlayer made them.
    pub skipped: Vec<String>,
}

/// One license line of `status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LicenseStatus {
    pub id: String,
    /// `accepted` or `not-accepted`, from the record alone, because `status` makes no
    /// network request and so has no live text of Google's to compare with. `text-changed`
    /// applies only to the GPL, whose live text is the one embedded in this release.
    pub state: String,
    pub accepted_at: Option<String>,
    pub text_sha1: Option<String>,
    pub authority: Option<String>,
    /// True when the accepted text isn't the one reviewed for this MTPlayer release. This is
    /// a note for the reader; the install gate compares with the live text instead.
    pub differs_from_reviewed: bool,
}

/// One root line of `status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RootStatus {
    pub set: String,
    pub path: PathBuf,
    /// `installing`, `ready`, or `unreadable` when its record can't be read.
    pub state: String,
    /// True when this is the manifest's set.
    pub current: bool,
    pub packages: Vec<String>,
    pub check: CheckReport,
    pub leases: Vec<String>,
    pub notices: Vec<Notice>,
    /// Why the record can't be read, for an `unreadable` root.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The Magisk line of `status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MagiskStatus {
    pub tag: String,
    pub present: bool,
    pub hash_ok: bool,
}

/// One leftover file in `downloads/`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PendingDownload {
    pub name: String,
    pub bytes: u64,
}

/// The whole of `status`. It makes no network request and takes no lock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub home: PathBuf,
    pub set: String,
    pub licenses: Vec<LicenseStatus>,
    pub roots: Vec<RootStatus>,
    pub magisk: MagiskStatus,
    pub pending_downloads: Vec<PendingDownload>,
    /// False when a recorded file is missing or modified, or a record can't be read.
    pub ok: bool,
}

/// Provisions one manifest's set into one home.
#[derive(Clone, Debug)]
pub struct Installer {
    home: Home,
    manifest: Manifest,
    client: Client,
    limits: ExtractLimits,
}

fn slug(path: &str) -> String {
    path.replace(';', "-")
}

impl Installer {
    /// An installer for `manifest`, downloading through `client`.
    pub fn new(home: Home, manifest: Manifest, client: Client) -> Self {
        Self {
            home,
            manifest,
            client,
            limits: ExtractLimits::default(),
        }
    }

    /// Replaces the extraction limits.
    pub fn with_limits(mut self, limits: ExtractLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The home in use.
    pub fn home(&self) -> &Home {
        &self.home
    }

    /// The manifest in use.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Works out what `install` would do. Trusts a record only after its files pass the
    /// check, and makes no network request.
    pub fn plan(&self, deep: bool) -> Result<Plan> {
        self.plan_cancellable(deep, &Cancel::new())
    }

    /// `plan`, stopping when `cancel` is set. Planning can hash whole zips and every installed
    /// file, which takes minutes on a slow disk.
    pub fn plan_cancellable(&self, deep: bool, cancel: &Cancel) -> Result<Plan> {
        let set = &self.manifest.set;
        let (roots, damaged) = self.home.roots_with_damaged()?;
        if let Some(root) = damaged.iter().find(|root| root.set == *set) {
            return Err(Error::root_damaged(set, root.error.clone()));
        }
        let target = SdkRoot::open(&self.home, set)?;
        let root_ready = target
            .as_ref()
            .is_some_and(|r| r.record().state == RootState::Ready);
        if root_ready && !store::has_platform_tools(&store::root_path(&self.home, set)) {
            return Err(Error::root_damaged(
                set,
                format!("missing: {}", store::PLATFORM_TOOLS_DIR),
            ));
        }

        let mut packages = Vec::new();
        for pin in &self.manifest.packages {
            let recorded = target.as_ref().and_then(|r| {
                r.record()
                    .packages
                    .iter()
                    .find(|p| p.path == pin.path)
                    .map(|p| (r, p))
            });
            let action = match recorded {
                Some((_, package)) if root_ready && !package.matches(pin) => {
                    return Err(Error::root_differs(
                        set,
                        format!("{} was installed from another pin", pin.path),
                    ));
                }
                Some((_, package)) if !package.matches(pin) => {
                    self.source_action(pin, &roots, cancel)?
                }
                Some((root, package)) => {
                    // A size can't vouch for a file a crash may have cut short with zeroes,
                    // so a resumed install hashes everything it keeps.
                    match package_damage(root, package, deep || !root_ready, cancel)? {
                        None => PackageAction::Reuse,
                        Some(reason) if root_ready => {
                            return Err(Error::root_damaged(set, reason));
                        }
                        Some(_) => self.source_action(pin, &roots, cancel)?,
                    }
                }
                None if root_ready => {
                    return Err(Error::root_differs(
                        set,
                        format!("{} isn't in its record", pin.path),
                    ));
                }
                None => self.source_action(pin, &roots, cancel)?,
            };
            packages.push((pin.path.clone(), action));
        }
        Ok(Plan {
            set: set.clone(),
            packages,
            magisk_installed: magisk::is_installed(&self.home, &self.manifest.magisk)?,
            root_ready,
        })
    }

    fn source_action(
        &self,
        pin: &PackagePin,
        roots: &[SdkRoot],
        cancel: &Cancel,
    ) -> Result<PackageAction> {
        for root in roots {
            if root.set() == &self.manifest.set || root.record().state != RootState::Ready {
                continue;
            }
            if let Some(package) = root.record().packages.iter().find(|p| p.matches(pin))
                && package_damage(root, package, false, cancel)?.is_none()
            {
                return Ok(PackageAction::Link {
                    from: root.set().clone(),
                });
            }
        }
        if self.has_verified_zip(pin, cancel)? {
            return Ok(PackageAction::Extract);
        }
        Ok(PackageAction::Download)
    }

    fn zip_path(&self, pin: &PackagePin) -> Result<PathBuf> {
        Ok(self.home.checked_downloads_dir()?.join(pin.file_name()))
    }

    /// True when `downloads/` holds this pin's whole zip, under its final name or as a part
    /// that a kill left complete just before its rename. Either is hashed only when it has
    /// the pinned size, so the usual case costs nothing. This is a planning signal only:
    /// extraction hashes the zip again through the handle it reads.
    fn has_verified_zip(&self, pin: &PackagePin, cancel: &Cancel) -> Result<bool> {
        let zip = self.zip_path(pin)?;
        match fs::symlink_metadata(&zip) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                is_verified_file(&part_path(&zip), pin, cancel)
            }
            _ => is_verified_file(&zip, pin, cancel),
        }
    }

    /// Gives a complete part its final name. The planner hashed it, and extraction will hash
    /// it again, so this only runs for a plan that says `Extract`, under the home lock.
    fn promote_complete_part(&self, pin: &PackagePin) -> Result<()> {
        let zip = self.zip_path(pin)?;
        match fs::symlink_metadata(&zip) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fsutil::rename_with_retry(&part_path(&zip), &zip)
            }
            _ => Ok(()),
        }
    }

    /// Runs the license gate and provisions everything that is missing. Nothing but
    /// repository metadata is requested before each pending license is accepted, and an
    /// already complete set makes no request at all.
    pub fn install(
        &self,
        prompt: &mut dyn LicensePrompt,
        options: InstallOptions,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<InstallReport> {
        cancel.check()?;
        let _lock = self.home.lock()?;
        self.clear_staging()?;
        let plan = self.plan_cancellable(options.verify, cancel)?;
        let set = &self.manifest.set;
        let root = store::root_path(&self.home, set);
        if plan.is_complete() {
            for (path, _) in &plan.packages {
                on_event(Event::Reuse {
                    name: path.clone(),
                    detail: "installed from a verified zip".into(),
                });
            }
            on_event(Event::Reuse {
                name: format!("magisk {}", self.manifest.magisk.tag),
                detail: "sha256 ok".into(),
            });
            if !options.keep_downloads {
                self.delete_pinned_downloads(on_event);
            }
            on_event(Event::Done { downloads: 0 });
            return Ok(InstallReport {
                set: set.clone(),
                root,
                downloads: 0,
            });
        }

        self.check_path_length()?;
        self.run_gate(&plan, prompt, on_event, cancel)?;

        let mut record = SdkRoot::open(&self.home, set)?
            .map(|r| r.record().clone())
            .unwrap_or_else(|| RootRecord {
                schema: 1,
                set: set.clone(),
                state: RootState::Installing,
                packages: Vec::new(),
            });
        // In this order, so that every root directory MTPlayer makes has a record.
        store::write_record(&self.home, &record)?;
        fs::create_dir_all(&root).at(&root)?;

        let mut downloads = 0;
        for (pin, (_, action)) in self.manifest.packages.iter().zip(&plan.packages) {
            cancel.check()?;
            let installed = match action {
                PackageAction::Reuse => {
                    on_event(Event::Reuse {
                        name: pin.path.clone(),
                        detail: "installed from a verified zip".into(),
                    });
                    continue;
                }
                PackageAction::Link { from } => {
                    self.clear_package(&mut record, pin)?;
                    match self.link_package(pin, from, on_event, cancel) {
                        Ok(installed) => installed,
                        // A source damaged in a way its size can't show would otherwise
                        // block this set for good, so it falls back to Google's zip.
                        Err(e @ (Error::CompareFailed { .. } | Error::RootDamaged { .. })) => {
                            on_event(Event::Warning {
                                message: format!(
                                    "{} can't be linked from {from}: {e}; using its zip instead",
                                    pin.path
                                ),
                            });
                            self.fetch_after_failed_link(
                                pin,
                                prompt,
                                on_event,
                                cancel,
                                &mut downloads,
                            )?
                        }
                        Err(e) => return Err(e),
                    }
                }
                PackageAction::Extract => {
                    self.clear_package(&mut record, pin)?;
                    self.extract_kept_zip(pin, on_event, cancel)?
                }
                PackageAction::Download => {
                    self.clear_package(&mut record, pin)?;
                    self.download_package(pin, on_event, cancel, &mut downloads)?
                }
            };
            record.packages.push(installed);
            store::write_record(&self.home, &record)?;
        }

        if !plan.magisk_installed {
            cancel.check()?;
            let transferred = magisk::install_magisk(
                &self.home,
                &self.manifest.magisk,
                &self.client,
                on_event,
                cancel,
            )?;
            if transferred {
                downloads += 1;
            }
        } else {
            on_event(Event::Reuse {
                name: format!("magisk {}", self.manifest.magisk.tag),
                detail: "sha256 ok".into(),
            });
        }

        let platform_tools = root.join(store::PLATFORM_TOOLS_DIR);
        fsutil::ensure_no_reparse_point(self.home.path(), &platform_tools)?;
        fs::create_dir_all(&platform_tools).at(&platform_tools)?;
        record.state = RootState::Ready;
        store::write_record(&self.home, &record)?;
        if !options.keep_downloads {
            self.delete_pinned_downloads(on_event);
        }
        on_event(Event::Done { downloads });
        Ok(InstallReport {
            set: set.clone(),
            root,
            downloads,
        })
    }

    /// Refuses a home so deep that Google's binaries couldn't reach their own files, before
    /// anything is downloaded.
    fn check_path_length(&self) -> Result<()> {
        self.check_path_fits(
            store::root_path(&self.home, &self.manifest.set),
            MAX_PACKAGE_PATH_TAIL,
        )
    }

    /// Refuses a path `tail` characters long below `base` that Google's binaries couldn't
    /// open. Extraction would still succeed, because Rust uses long paths, so without this
    /// the failure would only appear when the emulator runs.
    fn check_path_fits(&self, base: PathBuf, tail: usize) -> Result<()> {
        let base = std::path::absolute(&base).unwrap_or(base);
        let length = base.as_os_str().encode_wide().count() + 1 + tail;
        if length >= WINDOWS_MAX_PATH {
            return Err(Error::HomePathTooLong {
                home: self.home.path().to_path_buf(),
                limit: WINDOWS_MAX_PATH,
            });
        }
        Ok(())
    }

    fn reviewed_google_text(&self) -> Option<Sha1Hex> {
        self.manifest.licenses[ANDROID_SDK_LICENSE]
            .reviewed_text_sha1
            .clone()
    }

    /// The pins whose planned action `wanted` picks.
    fn pins_where(&self, plan: &Plan, wanted: fn(&PackageAction) -> bool) -> Vec<&PackagePin> {
        self.manifest
            .packages
            .iter()
            .zip(&plan.packages)
            .filter(|(_, (_, action))| wanted(action))
            .map(|(pin, _)| pin)
            .collect()
    }

    /// Asks for every pending license. Every package that isn't already in the root is held to
    /// the stable-only rule against Google's XML. A kept zip or another root's files need no
    /// transfer, so a pin the XML no longer lists doesn't stop them, but one it now lists under
    /// another channel or license does, and they still read the live text, so that a changed
    /// one is asked again. Only when the machine can't reach the XML does an accepted record
    /// stand in for those comparisons, and the event says so.
    fn run_gate(
        &self,
        plan: &Plan,
        prompt: &mut dyn LicensePrompt,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<()> {
        let google_pending = plan
            .packages
            .iter()
            .any(|(_, a)| *a != PackageAction::Reuse);
        let transfers = self.pins_where(plan, |action| *action == PackageAction::Download);
        let present = self.pins_where(plan, |action| {
            matches!(action, PackageAction::Extract | PackageAction::Link { .. })
        });
        let magisk_pending = !plan.magisk_installed;

        // Fails before any request when no one can answer, so a script without a terminal
        // gets its instructions without touching the network.
        let store = self.home.licenses();
        let pending = [
            (ANDROID_SDK_LICENSE, google_pending),
            (GPL_3, magisk_pending),
        ];
        for (id, _) in pending.iter().filter(|(_, is_pending)| *is_pending) {
            if store.current(id).is_none() && !prompt.available(id) {
                return Err(Error::LicenseNotAccepted { id: id.to_string() });
            }
        }

        if !transfers.is_empty() || (google_pending && store.current(ANDROID_SDK_LICENSE).is_none())
        {
            self.gate_on_every_xml(&transfers, &present, prompt, on_event, cancel)?;
        } else if google_pending {
            self.gate_on_reachable_xmls(&present, prompt, on_event, cancel)?;
        }
        if magisk_pending {
            self.gate_one(&gpl3_text(), None, prompt, on_event)?;
        }
        Ok(())
    }

    /// Every distinct repository XML the pins name.
    fn pinned_xmls(&self) -> BTreeSet<&str> {
        self.manifest
            .packages
            .iter()
            .map(|pin| pin.repository_xml.as_str())
            .collect()
    }

    fn fetch_repository(&self, xml: &str, cancel: &Cancel) -> Result<Repository> {
        let text = self
            .client
            .get_text_cancellable(xml, MAX_XML_BYTES, cancel)?;
        Repository::parse(xml, &text)
    }

    /// The gate when a package has to be transferred or no acceptance is on record, so that
    /// any XML that can't be read stops the install.
    fn gate_on_every_xml(
        &self,
        transfers: &[&PackagePin],
        present: &[&PackagePin],
        prompt: &mut dyn LicensePrompt,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<()> {
        let mut repositories = BTreeMap::new();
        for xml in self.pinned_xmls() {
            repositories.insert(xml, self.fetch_repository(xml, cancel)?);
        }
        let text = check_repositories(&repositories, transfers, present)?;
        self.gate_one(
            &text,
            self.reviewed_google_text().as_ref(),
            prompt,
            on_event,
        )
    }

    /// The gate when only local files are needed and an acceptance is on record. Whatever XML
    /// can be read still counts, so a text changed in one of them is asked again. The record
    /// stands alone only for a machine that reached none of them: a server that answered with
    /// an error is not an offline machine, so with nothing read that error stops the install.
    fn gate_on_reachable_xmls(
        &self,
        present: &[&PackagePin],
        prompt: &mut dyn LicensePrompt,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<()> {
        let mut repositories = BTreeMap::new();
        let mut failures = Vec::new();
        for xml in self.pinned_xmls() {
            match self.fetch_repository(xml, cancel) {
                Ok(repository) => {
                    repositories.insert(xml, repository);
                }
                Err(
                    e @ (Error::Network { .. }
                    | Error::Http { .. }
                    | Error::BadResponse { .. }
                    | Error::Xml(_)),
                ) => failures.push(e),
                Err(e) => return Err(e),
            }
        }
        if repositories.is_empty() {
            return match failures
                .into_iter()
                .find(|e| !matches!(e, Error::Network { .. }))
            {
                Some(error) => Err(error),
                None => {
                    on_event(Event::License {
                        id: ANDROID_SDK_LICENSE.to_string(),
                        state: "accepted, live text not checked: offline".into(),
                    });
                    Ok(())
                }
            };
        }
        for failure in &failures {
            on_event(Event::Warning {
                message: format!(
                    "{failure}; the license text and pins of that XML were not checked"
                ),
            });
        }
        let text = check_repositories(&repositories, &[], present)?;
        self.gate_one(
            &text,
            self.reviewed_google_text().as_ref(),
            prompt,
            on_event,
        )
    }

    fn gate_one(
        &self,
        text: &LicenseText,
        reviewed: Option<&Sha1Hex>,
        prompt: &mut dyn LicensePrompt,
        on_event: &mut dyn FnMut(Event),
    ) -> Result<()> {
        let store = self.home.licenses();
        if let LicenseState::Accepted(_) = store.state(text) {
            on_event(Event::License {
                id: text.id().to_string(),
                state: "accepted".into(),
            });
            return Ok(());
        }
        match prompt.decide(text, reviewed)? {
            Decision::Accept { authority, via } => {
                store.record(text, authority, via)?;
                on_event(Event::License {
                    id: text.id().to_string(),
                    state: format!("recorded, text {}", text.sha1().prefix()),
                });
                Ok(())
            }
            Decision::Decline => Err(Error::LicenseNotAccepted {
                id: text.id().to_string(),
            }),
        }
    }

    /// Reads a license's text for a person to review: Google's from the pinned XML, the
    /// GPL's from this release.
    pub fn license_text(&self, id: &str) -> Result<LicenseText> {
        match id {
            GPL_3 => Ok(gpl3_text()),
            ANDROID_SDK_LICENSE => {
                let xml = self
                    .manifest
                    .packages
                    .first()
                    .map(|p| p.repository_xml.as_str())
                    .ok_or_else(|| Error::Manifest("no package pins".into()))?;
                let text = self.client.get_text(xml, MAX_XML_BYTES)?;
                Repository::parse(xml, &text)?.license_text(id)
            }
            other => Err(Error::InvalidName(other.to_string())),
        }
    }

    fn package_target(&self, pin: &PackagePin) -> PathBuf {
        store::root_path(&self.home, &self.manifest.set).join(pin.install_dir().replace('/', "\\"))
    }

    /// Drops whatever a crashed run left for this package. Only reached for a root that
    /// isn't `ready`, so no instance can be using these files.
    fn clear_package(&self, record: &mut RootRecord, pin: &PackagePin) -> Result<()> {
        fsutil::remove_tree(self.home.path(), &self.package_target(pin))?;
        record.packages.retain(|p| p.path != pin.path);
        store::write_record(&self.home, record)
    }

    fn move_into_root(&self, tree: &Path, pin: &PackagePin) -> Result<PathBuf> {
        let target = self.package_target(pin);
        fsutil::ensure_no_reparse_point(self.home.path(), &target)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).at(parent)?;
        }
        fsutil::rename_with_retry(tree, &target)?;
        Ok(target)
    }

    /// Downloads and installs one package. A pinned URL that is gone gets the withdrawn-pin
    /// error, even when the XML still lists it.
    fn download_package(
        &self,
        pin: &PackagePin,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
        downloads: &mut u32,
    ) -> Result<InstalledPackage> {
        let downloads_dir = self.home.checked_downloads_dir()?;
        fs::create_dir_all(&downloads_dir).at(&downloads_dir)?;
        self.home.note_owned_download(pin.file_name())?;
        let downloaded = self
            .client
            .download(
                &DownloadRequest {
                    url: pin.url.clone(),
                    dest: downloads_dir.join(pin.file_name()),
                    size: pin.size,
                    digest: Digest::Sha1(pin.sha1.clone()),
                },
                on_event,
                cancel,
            )
            .map_err(|e| e.pin_gone(&pin.path, &pin.revision))?;
        if !downloaded.reused {
            *downloads += 1;
        }
        self.extract_package(pin, &downloaded.path, on_event, cancel)
    }

    fn extract_kept_zip(
        &self,
        pin: &PackagePin,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<InstalledPackage> {
        self.promote_complete_part(pin)?;
        on_event(Event::Reuse {
            name: pin.file_name().to_string(),
            detail: "sha1 ok".into(),
        });
        self.extract_package(pin, &self.zip_path(pin)?, on_event, cancel)
    }

    /// A link was planned, so the gate never checked this pin as a transfer. A kept zip gets
    /// the same check as a planned one, and a transfer needs both the pin check and the license
    /// check first.
    fn fetch_after_failed_link(
        &self,
        pin: &PackagePin,
        prompt: &mut dyn LicensePrompt,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
        downloads: &mut u32,
    ) -> Result<InstalledPackage> {
        if self.has_verified_zip(pin, cancel)? {
            self.gate_on_reachable_xmls(&[pin], prompt, on_event, cancel)?;
            return self.extract_kept_zip(pin, on_event, cancel);
        }
        self.gate_on_every_xml(&[pin], &[], prompt, on_event, cancel)?;
        self.download_package(pin, on_event, cancel, downloads)
    }

    /// Extracts into a new empty directory, compares against the zip, and only then moves
    /// the result into the root. A failed compare keeps the zip for inspection. The zip stays
    /// open with writers barred from the hash to the move: the record will vouch for the
    /// pinned hash, and link reuse copies what it vouches for into later sets.
    fn extract_package(
        &self,
        pin: &PackagePin,
        zip_path: &Path,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<InstalledPackage> {
        on_event(Event::Extract {
            name: pin.path.clone(),
        });
        let zip = fsutil::open_denying_writes(zip_path)?;
        let digest = Digest::Sha1(pin.sha1.clone());
        let actual = digest.compute_reader_cancellable(&zip, zip_path, cancel)?;
        if actual != digest.expected() {
            return Err(Error::HashMismatch {
                name: pin.file_name().to_string(),
                expected: digest.expected().to_string(),
                actual,
            });
        }
        let plan = archive::scan(&zip, self.limits)?;
        self.check_path_fits(self.package_target(pin), plan.longest_relative)?;
        let staging_parent = self.home.staging_dir();
        fs::create_dir_all(&staging_parent).at(&staging_parent)?;
        let staging = fsutil::new_empty_dir(&staging_parent, &slug(&pin.path))?;
        let result = (|| {
            let unpack = staging.join("x");
            archive::extract_into_new_dir(&zip, &plan, &unpack, cancel)?;
            let tree = unpack.join(&plan.top_dir);
            let compared = archive::compare_with_zip(&plan, &tree, cancel)?;
            on_event(Event::Compare {
                name: pin.path.clone(),
                files: compared.files.len(),
            });
            check_package_identity(pin, &tree)?;
            let target = self.move_into_root(&tree, pin)?;
            on_event(Event::Install {
                name: format!("{} {}", pin.path, pin.revision),
                dest: target.display().to_string(),
            });
            Ok(InstalledPackage {
                path: pin.path.clone(),
                revision: pin.revision.clone(),
                zip_sha1: pin.sha1.clone(),
                install_dir: pin.install_dir(),
                installed_at: fsutil::now(),
                dirs: compared.dirs,
                files: compared.files,
            })
        })();
        let _ = fsutil::remove_tree(self.home.path(), &staging);
        result
    }

    /// Reuses a package from another root: hard links where the volume allows them, byte
    /// copies otherwise, every file verified against the source root's record.
    fn link_package(
        &self,
        pin: &PackagePin,
        from: &SetId,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<InstalledPackage> {
        let source_root = SdkRoot::open(&self.home, from)?
            .ok_or_else(|| Error::root_damaged(from, "its record disappeared".into()))?;
        let source = source_root
            .record()
            .packages
            .iter()
            .find(|p| p.matches(pin))
            .ok_or_else(|| Error::root_damaged(from, format!("{} left its record", pin.path)))?;
        on_event(Event::Reuse {
            name: pin.path.clone(),
            detail: format!("linked from {from}"),
        });

        let staging = fsutil::new_empty_dir(&self.home.staging_dir(), &slug(&pin.path))?;
        let result = (|| {
            let tree = staging.join("x");
            fs::create_dir(&tree).at(&tree)?;
            for dir in &source.dirs {
                let path = tree.join(dir);
                fs::create_dir_all(&path).at(&path)?;
            }
            let source_dir = source_root.path().join(&source.install_dir);
            for file in &source.files {
                cancel.check()?;
                let to = tree.join(&file.path);
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent).at(parent)?;
                }
                let from_file = source_dir.join(&file.path);
                if fs::hard_link(&from_file, &to).is_err() {
                    fs::copy(&from_file, &to).at(&to)?;
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&to)
                        .and_then(|file| file.sync_all())
                        .at(&to)?;
                }
            }
            let mut report = CheckReport::default();
            store::check_files(
                &tree,
                &source.install_dir,
                &source.files,
                true,
                &mut report,
                cancel,
            )?;
            if !report.ok() {
                let path = report
                    .missing
                    .iter()
                    .chain(&report.modified)
                    .next()
                    .cloned()
                    .unwrap_or_default();
                return Err(Error::CompareFailed {
                    path,
                    reason: "differs from the source root's record".into(),
                });
            }
            on_event(Event::Compare {
                name: pin.path.clone(),
                files: source.files.len(),
            });
            let target = self.move_into_root(&tree, pin)?;
            on_event(Event::Install {
                name: format!("{} {}", pin.path, pin.revision),
                dest: target.display().to_string(),
            });
            let mut installed = source.clone();
            installed.installed_at = fsutil::now();
            Ok(installed)
        })();
        let _ = fsutil::remove_tree(self.home.path(), &staging);
        result
    }

    /// Deletes what a crashed run left in `staging/`. The home may be a folder that already
    /// held a `staging` of someone else's, so only directories this crate named are touched.
    fn clear_staging(&self) -> Result<()> {
        let staging = self.home.staging_dir();
        let entries = match fs::read_dir(&staging) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).at(staging),
        };
        for entry in entries {
            let entry = entry.at(&staging)?;
            let name = entry.file_name();
            if entry.file_type().at(entry.path())?.is_dir()
                && fsutil::is_generated_dir_name(&name.to_string_lossy())
            {
                fsutil::remove_tree(self.home.path(), &entry.path())?;
            }
        }
        fsutil::remove_dir_if_empty(&staging);
        Ok(())
    }

    fn pinned_download_names(&self) -> impl Iterator<Item = &str> {
        self.manifest
            .packages
            .iter()
            .map(|p| p.file_name())
            .chain([self.manifest.magisk.asset.as_str()])
    }

    /// The set is already `ready` when this runs, so a file that can't be deleted is a
    /// warning and not a failed install.
    fn delete_pinned_downloads(&self, on_event: &mut dyn FnMut(Event)) {
        let dir = match self.home.checked_downloads_dir() {
            Ok(dir) => dir,
            Err(e) => {
                on_event(Event::Warning {
                    message: format!("the downloads can't be cleaned up: {e}"),
                });
                return;
            }
        };
        for name in self.pinned_download_names() {
            let path = dir.join(name);
            match fsutil::remove_file_with_retry(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => on_event(Event::Warning {
                    message: format!("{} can't be deleted: {e}", path.display()),
                }),
            }
        }
        if let Err(e) = self.home.prune_owned_downloads() {
            on_event(Event::Warning {
                message: format!("the list of downloads can't be updated: {e}"),
            });
        }
    }

    /// The pinned names plus the ones the ledger lists: these, and nothing else, are
    /// MTPlayer's, because the home may be a folder that already held a `downloads` of
    /// someone else's.
    fn download_names(&self, owned: Vec<String>) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = owned.into_iter().collect();
        names.extend(self.pinned_download_names().map(str::to_string));
        names
    }

    /// Deletes every zip and partial file MTPlayer put in `downloads/`, and returns whether
    /// any went. A file that can't be deleted stays listed for the next attempt.
    fn remove_downloads(&self) -> Result<bool> {
        let dir = self.home.checked_downloads_dir()?;
        let mut removed = false;
        let mut failure = None;
        for name in self.download_names(self.home.owned_downloads()?) {
            for path in [dir.join(&name), dir.join(format!("{name}.part"))] {
                if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
                    match fsutil::remove_file_with_retry(&path) {
                        Ok(()) => removed = true,
                        Err(source) => {
                            failure.get_or_insert(Error::Io { path, source });
                        }
                    }
                }
            }
        }
        self.home.prune_owned_downloads()?;
        fsutil::remove_dir_if_empty(&dir);
        match failure {
            Some(error) => Err(error),
            None => Ok(removed),
        }
    }

    /// Reports the home's state without any network request or lock. `deep` hashes every
    /// file and lists each difference.
    pub fn status(&self, deep: bool) -> Result<Status> {
        let mut licenses = Vec::new();
        let store = self.home.licenses();
        // Each license: the live text this release holds, and the text it was reviewed with.
        let known = [
            (ANDROID_SDK_LICENSE, None, self.reviewed_google_text()),
            (GPL_3, Some(gpl3_text().sha1().clone()), None),
        ];
        for (id, live, reviewed) in known {
            licenses.push(match store.current(id) {
                None => LicenseStatus {
                    id: id.to_string(),
                    state: "not-accepted".to_string(),
                    accepted_at: None,
                    text_sha1: None,
                    authority: None,
                    differs_from_reviewed: false,
                },
                Some(record) => {
                    let changed = live.as_ref().is_some_and(|h| *h != record.text_sha1);
                    LicenseStatus {
                        id: id.to_string(),
                        state: if changed { "text-changed" } else { "accepted" }.to_string(),
                        differs_from_reviewed: reviewed
                            .as_ref()
                            .is_some_and(|h| *h != record.text_sha1),
                        accepted_at: Some(record.accepted_at),
                        text_sha1: Some(record.text_sha1.to_string()),
                        authority: Some(format!("{:?}", record.authority).to_lowercase()),
                    }
                }
            });
        }

        let mut ok = true;
        let mut roots = Vec::new();
        let (good, damaged) = self.home.roots_with_damaged()?;
        for root in good {
            let check = if deep {
                root.deep_check()?
            } else {
                root.quick_check()?
            };
            ok &= check.ok();
            roots.push(RootStatus {
                set: root.set().to_string(),
                path: root.path().to_path_buf(),
                state: format!("{:?}", root.record().state).to_lowercase(),
                current: root.set() == &self.manifest.set,
                packages: root
                    .record()
                    .packages
                    .iter()
                    .map(|p| format!("{} {}", p.path, p.revision))
                    .collect(),
                leases: root.leases()?,
                notices: root.notice_files(),
                check,
                error: None,
            });
        }
        for root in damaged {
            ok = false;
            roots.push(RootStatus {
                current: root.set == self.manifest.set,
                state: "unreadable".to_string(),
                packages: Vec::new(),
                check: CheckReport::default(),
                leases: store::leases_of(&self.home, &root.set)?,
                notices: Vec::new(),
                error: Some(root.error),
                set: root.set.to_string(),
                path: root.path,
            });
        }
        roots.sort_by(|a, b| a.set.cmp(&b.set));

        let pin = &self.manifest.magisk;
        let present = magisk::binary_path(&self.home, &pin.tag).exists();
        let hash_ok = magisk::is_installed(&self.home, pin)?;
        ok &= !present || hash_ok;

        Ok(Status {
            home: self.home.path().to_path_buf(),
            set: self.manifest.set.to_string(),
            licenses,
            roots,
            magisk: MagiskStatus {
                tag: pin.tag.clone(),
                present,
                hash_ok,
            },
            pending_downloads: self.pending_downloads()?,
            ok,
        })
    }

    /// A damaged ledger can't stop `status`: the pinned names are still reported.
    fn pending_downloads(&self) -> Result<Vec<PendingDownload>> {
        let dir = self.home.checked_downloads_dir()?;
        let mut pending = Vec::new();
        for name in self.download_names(self.home.owned_downloads().unwrap_or_default()) {
            for file in [format!("{name}.part"), name] {
                let path = dir.join(&file);
                // A file can finish or be cleaned up during this listing, since `status`
                // takes no lock.
                match fs::symlink_metadata(&path) {
                    Ok(meta) if meta.is_file() => pending.push(PendingDownload {
                        name: file,
                        bytes: meta.len(),
                    }),
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e).at(path),
                }
            }
        }
        pending.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(pending)
    }

    /// Removes a root, unused data or the downloads. A leased root is refused unless `force`.
    pub fn remove(&self, target: RemoveTarget, force: bool) -> Result<Removed> {
        let _lock = self.home.lock()?;
        let mut removed = Removed::default();
        match target {
            RemoveTarget::Set(set) => {
                // The record's existence is what counts, not whether it can be read: a
                // damaged record must not block removing its root. A directory with no
                // record is not one MTPlayer made, so it isn't MTPlayer's to delete.
                if fs::symlink_metadata(store::record_path(&self.home, &set)).is_ok() {
                    store::remove_root(&self.home, &set, force)?;
                    removed.roots.push(set.to_string());
                } else if fs::symlink_metadata(store::root_path(&self.home, &set)).is_ok() {
                    return Err(Error::NotOurs {
                        path: store::root_path(&self.home, &set),
                    });
                }
            }
            RemoveTarget::Unused => {
                let (good, damaged) = self.home.roots_with_damaged()?;
                let sets = good
                    .iter()
                    .map(|root| root.set().clone())
                    .chain(damaged.into_iter().map(|root| root.set))
                    .filter(|set| *set != self.manifest.set);
                for set in sets {
                    match store::remove_root(&self.home, &set, false) {
                        Ok(()) => removed.roots.push(set.to_string()),
                        Err(Error::RootInUse { .. }) => removed.leased.push(set.to_string()),
                        Err(e) => return Err(e),
                    }
                }
                removed.roots.sort();
                removed.leased.sort();
                self.remove_old_magisk(&mut removed)?;
            }
            RemoveTarget::Downloads => {
                removed.downloads = self.remove_downloads()?;
            }
        }
        Ok(removed)
    }

    /// Removes the Magisk tags the manifest no longer names. Only a directory with its own
    /// record is MTPlayer's to delete, as with roots; anything else is reported and left.
    fn remove_old_magisk(&self, removed: &mut Removed) -> Result<()> {
        let dir = self.home.magisk_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e).at(dir),
        };
        for entry in entries {
            let entry = entry.at(&dir)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !entry.file_type().at(entry.path())?.is_dir() || name == self.manifest.magisk.tag {
                continue;
            }
            let record_path = dir.join(format!("{name}.json"));
            let ours = matches!(
                fsutil::read_json::<MagiskRecord>(&record_path),
                Ok(Some(record)) if record.tag == name
            );
            if !ours {
                removed.skipped.push(name);
                continue;
            }
            fsutil::remove_tree(self.home.path(), &entry.path())?;
            fs::remove_file(&record_path).at(&record_path)?;
            removed.magisk_tags.push(name);
        }
        removed.magisk_tags.sort();
        removed.skipped.sort();
        Ok(())
    }
}

/// Holds `present` and `transfers` to what the XMLs say and returns the license text they
/// agree on. Each transfer's XML must have been read. A present pin the XML no longer lists
/// may still install from its local copy, but it must not have moved off the stable channel
/// or license, and one whose XML couldn't be read goes unchecked.
fn check_repositories(
    repositories: &BTreeMap<&str, Repository>,
    transfers: &[&PackagePin],
    present: &[&PackagePin],
) -> Result<LicenseText> {
    for pin in transfers {
        let live =
            repositories[pin.repository_xml.as_str()].find_archive(&pin.path, &pin.revision)?;
        check_pin(pin, &live)?;
    }
    for pin in present {
        let Some(repository) = repositories.get(pin.repository_xml.as_str()) else {
            continue;
        };
        match repository.find_archive(&pin.path, &pin.revision) {
            Ok(live) => check_pin(pin, &live)?,
            Err(Error::PinNotInRepository { .. }) => {}
            Err(e) => return Err(e),
        }
    }
    let mut found: Option<LicenseText> = None;
    for repository in repositories.values() {
        let text = repository.license_text(ANDROID_SDK_LICENSE)?;
        match &found {
            Some(first) if first.sha1() != text.sha1() => {
                return Err(Error::PinMismatch {
                    path: ANDROID_SDK_LICENSE.to_string(),
                    field: "text in each repository XML".to_string(),
                    pinned: first.sha1().to_string(),
                    live: text.sha1().to_string(),
                });
            }
            Some(_) => {}
            None => found = Some(text),
        }
    }
    found.ok_or_else(|| Error::Manifest("no repository XML to read the license from".into()))
}

/// True when `path` is a regular file with this pin's size and SHA-1. A file of another size
/// costs no hashing.
fn is_verified_file(path: &Path, pin: &PackagePin, cancel: &Cancel) -> Result<bool> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() == pin.size => {}
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e).at(path),
    }
    let digest = Digest::Sha1(pin.sha1.clone());
    Ok(digest.compute_cancellable(path, cancel)? == digest.expected())
}

/// A description of what is wrong with an installed package, or `None`.
fn package_damage(
    root: &SdkRoot,
    package: &InstalledPackage,
    deep: bool,
    cancel: &Cancel,
) -> Result<Option<String>> {
    if root.is_behind_link() {
        return Ok(Some(
            "a link or junction stands in for the root".to_string(),
        ));
    }
    let mut report = CheckReport::default();
    store::check_files(
        &root.path().join(&package.install_dir),
        &package.install_dir,
        &package.files,
        deep,
        &mut report,
        cancel,
    )?;
    if let Some(path) = report.missing.first() {
        return Ok(Some(format!("missing: {path}")));
    }
    if let Some(path) = report.modified.first() {
        return Ok(Some(format!("modified: {path}")));
    }
    Ok(None)
}

/// The probe file exists and `source.properties` names the pinned revision.
fn check_package_identity(pin: &PackagePin, tree: &Path) -> Result<()> {
    let probe = tree.join(&pin.probe);
    if !probe.is_file() {
        return Err(Error::CompareFailed {
            path: pin.probe.clone(),
            reason: "the package's expected file is missing".into(),
        });
    }
    let properties = tree.join("source.properties");
    let text = fs::read_to_string(&properties).at(&properties)?;
    let revision = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("Pkg.Revision=").map(str::trim));
    if revision != Some(pin.revision.as_str()) {
        return Err(Error::CompareFailed {
            path: "source.properties".to_string(),
            reason: format!(
                "names revision {:?}, not {}",
                revision.unwrap_or("none"),
                pin.revision
            ),
        });
    }
    Ok(())
}
