//! SDK roots and their records. A root is never modified once its record says `ready`,
//! because an instance's qcow2 overlays and snapshots point at its files. A root holds
//! Google's packages and one empty `platform-tools` directory that the emulator requires, and
//! nothing else of MTPlayer's, so an extra file there always means someone else's.

use std::collections::BTreeSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::archive::{FileRecord, relative_slash};
use crate::error::{Error, IoContext, Result};
use crate::event::Cancel;
use crate::fsutil;
use crate::hash::{Sha1Hex, sha256_file_cancellable};
use crate::home::Home;
use crate::manifest::{PackagePin, SetId};

/// The emulator refuses an SDK root without this directory, and its contents don't matter to
/// it. It stays empty: adb.exe is never installed, because it would write a key to the
/// user's profile folder.
pub(crate) const PLATFORM_TOOLS_DIR: &str = "platform-tools";

/// Whether a root may be launched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RootState {
    Installing,
    Ready,
}

/// One package as installed in a root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledPackage {
    pub path: String,
    pub revision: String,
    pub zip_sha1: Sha1Hex,
    pub install_dir: String,
    pub installed_at: String,
    pub dirs: Vec<String>,
    pub files: Vec<FileRecord>,
}

impl InstalledPackage {
    /// True when this package came from exactly this pin.
    pub fn matches(&self, pin: &PackagePin) -> bool {
        self.path == pin.path && self.revision == pin.revision && self.zip_sha1 == pin.sha1
    }
}

/// `sdk/<set>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootRecord {
    pub schema: u32,
    pub set: SetId,
    pub state: RootState,
    pub packages: Vec<InstalledPackage>,
}

/// What a check found. Extra files are a warning: they don't change Google's files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CheckReport {
    pub files: usize,
    pub bytes: u64,
    pub missing: Vec<String>,
    pub modified: Vec<String>,
    pub extra: Vec<String>,
}

impl CheckReport {
    /// True when every recorded file is present and unchanged.
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.modified.is_empty()
    }
}

/// A `LICENSE` or `NOTICE` file that ships in a package. They are never removed or hidden.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub package: String,
    pub path: PathBuf,
    pub size: u64,
}

/// A lease file's content.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lease {
    pub owner: String,
    pub created_at: String,
    pub note: String,
}

/// A root with a record.
#[derive(Clone, Debug)]
pub struct SdkRoot {
    home: Home,
    path: PathBuf,
    record: RootRecord,
}

pub(crate) fn record_path(home: &Home, set: &SetId) -> PathBuf {
    home.sdk_dir().join(format!("{set}.json"))
}

fn leases_dir(home: &Home, set: &SetId) -> PathBuf {
    home.sdk_dir().join(format!("{set}.leases"))
}

pub(crate) fn root_path(home: &Home, set: &SetId) -> PathBuf {
    home.sdk_dir().join(set.as_str())
}

pub(crate) fn write_record(home: &Home, record: &RootRecord) -> Result<()> {
    fsutil::write_json_atomic(&record_path(home, &record.set), record)
}

/// True when the directory the emulator requires exists as a real directory, not a link.
pub(crate) fn has_platform_tools(root: &Path) -> bool {
    fs::symlink_metadata(root.join(PLATFORM_TOOLS_DIR)).is_ok_and(|meta| meta.is_dir())
}

/// A path as a record stores it: forward slashes, relative, with every component legal.
fn check_record_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') {
        return Err(format!("{value:?} is not a relative slash-separated path"));
    }
    for component in value.split('/') {
        fsutil::check_windows_component(component)
            .map_err(|reason| format!("{value:?} has {reason}"))?;
    }
    Ok(())
}

/// A record is input like any other file in the home, and link reuse, the checks and
/// `remove` all build paths from it, so nothing it says is used before this passes.
fn check_record(record: &RootRecord, set: &SetId) -> Result<(), String> {
    if record.schema != 1 {
        return Err(format!("unknown schema {}", record.schema));
    }
    if record.set != *set {
        return Err(format!("it claims set {}", record.set));
    }
    for package in &record.packages {
        check_record_path(&package.install_dir)?;
        if package.install_dir != package.path.replace(';', "/") {
            return Err(format!(
                "{} isn't installed under {}",
                package.path, package.install_dir
            ));
        }
        for dir in &package.dirs {
            check_record_path(dir)?;
        }
        for file in &package.files {
            check_record_path(&file.path)?;
        }
    }
    Ok(())
}

/// Checks recorded files below `base`, by size alone or also by SHA-256. Findings are named
/// `<install_dir>/<file>`, relative to the root.
pub(crate) fn check_files(
    base: &Path,
    install_dir: &str,
    files: &[FileRecord],
    deep: bool,
    report: &mut CheckReport,
    cancel: &Cancel,
) -> Result<()> {
    for file in files {
        cancel.check()?;
        let path = base.join(&file.path);
        let label = format!("{install_dir}/{}", file.path);
        report.files += 1;
        match fs::metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => report.missing.push(label),
            Err(e) => return Err(e).at(path),
            Ok(meta) if meta.len() != file.size => report.modified.push(label),
            Ok(meta) => {
                report.bytes += meta.len();
                if deep {
                    // The file can vanish between these two looks while another process
                    // installs or removes, and `status` takes no lock.
                    match sha256_file_cancellable(&path, cancel) {
                        Ok(hash) if hash != file.sha256 => report.modified.push(label),
                        Ok(_) => {}
                        Err(e) if e.is_not_found() => report.missing.push(label),
                        Err(e) => return Err(e),
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_owner(owner: &str) -> Result<()> {
    // A leading dot could look like a temporary file, which `leases_of` doesn't count, and
    // a device name can't be a file. Either would give a lease that never blocks a remove.
    let ok = owner.len() <= 64
        && owner.starts_with(|c: char| c.is_ascii_alphanumeric())
        && owner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        && fsutil::check_windows_component(owner).is_ok();
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidName(owner.to_string()))
    }
}

impl SdkRoot {
    /// Reads a root's record. `None` when it has no record, and an error when it can't be
    /// read or doesn't hold up.
    pub fn open(home: &Home, set: &SetId) -> Result<Option<Self>> {
        let path = record_path(home, set);
        let Some(record) = fsutil::read_json::<RootRecord>(&path)? else {
            return Ok(None);
        };
        check_record(&record, set).map_err(|reason| Error::RecordInvalid { path, reason })?;
        Ok(Some(Self {
            home: home.clone(),
            path: root_path(home, set),
            record,
        }))
    }

    /// The set id.
    pub fn set(&self) -> &SetId {
        &self.record.set
    }

    /// The value for `ANDROID_SDK_ROOT` and `ANDROID_HOME`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The record as read.
    pub fn record(&self) -> &RootRecord {
        &self.record
    }

    /// `emulator.exe` in this root.
    pub fn emulator_exe(&self) -> PathBuf {
        self.path.join("emulator").join("emulator.exe")
    }

    /// The installed system image's directory.
    pub fn system_image_dir(&self) -> Option<PathBuf> {
        self.image_dir().map(|dir| self.path.join(dir))
    }

    /// The value for an AVD's `image.sysdir.1`, relative to the root, as the emulator expects.
    pub fn image_sysdir_relative(&self) -> Option<String> {
        self.image_dir().map(|dir| format!("{dir}/"))
    }

    fn image_dir(&self) -> Option<&str> {
        self.record
            .packages
            .iter()
            .find(|p| p.path.starts_with("system-images;"))
            .map(|p| p.install_dir.as_str())
    }

    /// Every `LICENSE*` and `NOTICE*` file the packages ship, for the about screen.
    pub fn notice_files(&self) -> Vec<Notice> {
        let mut notices = Vec::new();
        for package in &self.record.packages {
            for file in &package.files {
                let leaf = file
                    .path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&file.path)
                    .to_ascii_uppercase();
                if leaf.starts_with("LICENSE") || leaf.starts_with("NOTICE") {
                    notices.push(Notice {
                        package: package.path.clone(),
                        path: self.path.join(&package.install_dir).join(&file.path),
                        size: file.size,
                    });
                }
            }
        }
        notices
    }

    /// Every recorded file exists with its recorded size; extra files are counted. Fast
    /// enough to run on every `status`, because it hashes nothing.
    pub fn quick_check(&self) -> Result<CheckReport> {
        self.check(false)
    }

    /// The quick check plus a SHA-256 of every file against the record.
    pub fn deep_check(&self) -> Result<CheckReport> {
        self.check(true)
    }

    /// True when the root, or a folder between it and the home, is a link or junction. Every
    /// file would then look recorded while living outside the home, and a launcher would run
    /// `emulator.exe` from there.
    pub(crate) fn is_behind_link(&self) -> bool {
        fsutil::ensure_no_reparse_point(self.home.path(), &self.path).is_err()
    }

    fn check(&self, deep: bool) -> Result<CheckReport> {
        let mut report = CheckReport::default();
        if self.is_behind_link() {
            report.modified.push(self.record.set.to_string());
            return Ok(report);
        }
        let mut known = BTreeSet::new();
        let no_cancel = Cancel::new();
        if self.record.state == RootState::Ready && !has_platform_tools(&self.path) {
            report.missing.push(PLATFORM_TOOLS_DIR.to_string());
        }
        for package in &self.record.packages {
            check_files(
                &self.path.join(&package.install_dir),
                &package.install_dir,
                &package.files,
                deep,
                &mut report,
                &no_cancel,
            )?;
            for file in &package.files {
                known.insert(format!("{}/{}", package.install_dir, file.path));
            }
        }
        if self.path.exists() {
            for item in walkdir::WalkDir::new(&self.path)
                .min_depth(1)
                .follow_links(false)
                .follow_root_links(false)
            {
                let item = match item {
                    Ok(item) => item,
                    // Another process is removing or finishing this root, and `status`
                    // takes no lock, so this is a finding and not a failure.
                    Err(e)
                        if e.io_error().map(std::io::Error::kind) == Some(ErrorKind::NotFound) =>
                    {
                        if let Some(path) = e.path() {
                            let relative = relative_slash(&self.path, path)?;
                            if known.contains(&relative) && !report.missing.contains(&relative) {
                                report.missing.push(relative);
                            }
                        }
                        continue;
                    }
                    Err(e) => {
                        return Err(Error::Io {
                            path: self.path.clone(),
                            source: e.into(),
                        });
                    }
                };
                let relative = relative_slash(&self.path, item.path())?;
                // A link or junction can stand in for a whole package directory and make
                // its files look recorded, so it is damage and not just an extra file.
                if item.file_type().is_symlink() {
                    report.modified.push(relative);
                    continue;
                }
                if item.file_type().is_dir() {
                    continue;
                }
                if !known.contains(&relative) {
                    report.extra.push(relative);
                }
            }
        }
        report.missing.sort();
        report.modified.sort();
        report.extra.sort();
        Ok(report)
    }

    /// Marks this root as used by `owner`, typically an instance. `mtp sdk remove` refuses a
    /// leased root. The record is read again here, because a remove may have retired the
    /// root since this value was opened.
    pub fn lease(&self, owner: &str, note: &str) -> Result<()> {
        check_owner(owner)?;
        let _guard = self.home.lease_lock()?;
        let ready = SdkRoot::open(&self.home, self.set())?
            .is_some_and(|root| root.record.state == RootState::Ready);
        if !ready {
            return Err(Error::RootNotReady {
                set: self.set().to_string(),
            });
        }
        fsutil::write_json_atomic(
            &leases_dir(&self.home, self.set()).join(owner),
            &Lease {
                owner: owner.to_string(),
                created_at: fsutil::now(),
                note: note.to_string(),
            },
        )
    }

    /// Ends `owner`'s lease. Ending a lease that doesn't exist is not an error.
    pub fn release(&self, owner: &str) -> Result<()> {
        check_owner(owner)?;
        let path = leases_dir(&self.home, self.set()).join(owner);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).at(path),
        }
    }

    /// The owners currently holding a lease, sorted.
    pub fn leases(&self) -> Result<Vec<String>> {
        leases_of(&self.home, self.set())
    }
}

pub(crate) fn leases_of(home: &Home, set: &SetId) -> Result<Vec<String>> {
    let dir = leases_dir(home, set);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).at(dir),
    };
    let mut owners = Vec::new();
    for entry in entries {
        let name = entry.at(&dir)?.file_name().to_string_lossy().into_owned();
        if !name.starts_with(".tmp-") {
            owners.push(name);
        }
    }
    owners.sort();
    Ok(owners)
}

/// Removes a root. A leased root is refused unless `force`. The record is marked `installing`
/// first, under the lease lock, so that a crash halfway leaves a root nothing launches and
/// no new lease can appear once the leases have been checked.
pub(crate) fn remove_root(home: &Home, set: &SetId, force: bool) -> Result<()> {
    {
        let _guard = home.lease_lock()?;
        let leases = leases_of(home, set)?;
        if !leases.is_empty() && !force {
            return Err(Error::RootInUse {
                set: set.to_string(),
                leases,
            });
        }
        // An unreadable record must not block removing the root it describes.
        if let Ok(Some(root)) = SdkRoot::open(home, set) {
            let mut record = root.record;
            record.state = RootState::Installing;
            write_record(home, &record)?;
        }
    }
    fsutil::remove_tree(home.path(), &root_path(home, set))?;
    fsutil::remove_tree(home.path(), &leases_dir(home, set))?;
    let record = record_path(home, set);
    match fs::remove_file(&record) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).at(record),
    }
}
