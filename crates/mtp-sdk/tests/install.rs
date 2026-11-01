//! The whole install, status and remove contract against a loopback world.

mod support;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use mtp_sdk::download::part_path;
use mtp_sdk::hash::sha256_file;
use mtp_sdk::install::{InstallOptions, PackageAction};
use mtp_sdk::manifest::Manifest;
use mtp_sdk::{
    Authority, Cancel, Client, Decision, Error, Event, GPL_3, Home, Installer, LicensePrompt,
    LicenseText, PreAccepted, RemoveTarget, RootState, SdkRoot, SetId, SourcePolicy, Via,
    gpl3_text,
};
use support::fixtures::{
    EMULATOR_ZIP_PATH, IMAGE_XML_PATH, LICENSE_BODY, MAIN_XML_PATH, PackageSpec, World, noise,
    repository_xml, revision_xml, serve_set, world, zip_of,
};
use support::http::{FixtureServer, Route};
use support::temp_dir;
use tempfile::TempDir;

fn installer(dir: &TempDir, server: &FixtureServer, manifest: &Manifest) -> Installer {
    let client = Client::new(SourcePolicy::loopback(server.port()))
        .with_retries(1, Duration::from_millis(10));
    Installer::new(Home::new(dir.path()), manifest.clone(), client)
}

fn accept_all(world: &World) -> PreAccepted {
    PreAccepted::new(
        BTreeMap::from([
            (
                mtp_sdk::ANDROID_SDK_LICENSE.to_string(),
                world.license_sha1.clone(),
            ),
            (GPL_3.to_string(), gpl3_text().sha1().clone()),
        ]),
        Authority::Personal,
    )
}

fn run(
    installer: &Installer,
    prompt: &mut dyn LicensePrompt,
    options: InstallOptions,
) -> mtp_sdk::Result<(mtp_sdk::InstallReport, Vec<Event>)> {
    let mut events = Vec::new();
    let report = installer.install(prompt, options, &mut |e| events.push(e), &Cancel::new())?;
    Ok((report, events))
}

fn root_file(dir: &TempDir, set: &SetId, relative: &str) -> PathBuf {
    dir.path().join("sdk").join(set.as_str()).join(relative)
}

/// How many zips and partial files sit in `downloads/`, not counting the ledger.
fn kept_downloads(dir: &TempDir) -> usize {
    std::fs::read_dir(dir.path().join("downloads"))
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name() != "owned.json")
        .count()
}

/// A prompt that answers without any flag, as a GUI dialog would.
struct Dialog {
    answer: Decision,
    shown: Vec<String>,
}

impl LicensePrompt for Dialog {
    fn available(&self, _id: &str) -> bool {
        true
    }

    fn decide(
        &mut self,
        text: &LicenseText,
        _reviewed: Option<&mtp_sdk::hash::Sha1Hex>,
    ) -> mtp_sdk::Result<Decision> {
        self.shown.push(text.id().to_string());
        Ok(self.answer)
    }
}

#[test]
fn a_full_install_lands_in_google_layout_with_nothing_of_ours_in_the_root() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, events) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

    assert_eq!(report.downloads, 3);
    assert!(report.set.as_str().starts_with("37.2.13-36r2-"));
    assert!(root_file(&dir, &report.set, "emulator/emulator.exe").is_file());
    assert!(root_file(&dir, &report.set, "emulator/empty").is_dir());
    assert!(
        root_file(
            &dir,
            &report.set,
            "system-images/android-36/default/x86_64/system.img"
        )
        .is_file()
    );
    assert!(
        root_file(
            &dir,
            &report.set,
            "system-images/android-36/default/x86_64/NOTICE.txt"
        )
        .is_file()
    );
    assert!(dir.path().join("tools/magisk/v1.0/magisk").is_file());
    assert_eq!(
        sha256_file(&dir.path().join("tools/magisk/v1.0/magisk")).unwrap(),
        w.manifest.magisk.binary_sha256
    );

    let top: Vec<String> = std::fs::read_dir(root_file(&dir, &report.set, ""))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(top.len(), 3, "{top:?}");
    let platform_tools = root_file(&dir, &report.set, "platform-tools");
    assert_eq!(std::fs::read_dir(&platform_tools).unwrap().count(), 0);

    let home = Home::new(dir.path());
    let root = home.sdk_root(&report.set).unwrap().expect("a ready root");
    assert_eq!(root.record().state, RootState::Ready);
    assert_eq!(
        root.emulator_exe(),
        root_file(&dir, &report.set, "")
            .join("emulator")
            .join("emulator.exe")
    );
    assert_eq!(
        root.image_sysdir_relative().unwrap(),
        "system-images/android-36/default/x86_64/"
    );
    let deep = root.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
    assert_eq!(deep.files, 8);

    let notices = root.notice_files();
    assert_eq!(notices.len(), 2);
    assert!(notices.iter().all(|n| n.path.is_file()));

    let licenses = home.licenses();
    let record = licenses.current("android-sdk-license").unwrap();
    assert_eq!(record.via, Via::Flag);
    assert_eq!(record.text, LICENSE_BODY);
    assert!(licenses.current(GPL_3).is_some());

    // Downloads and staging are cleaned up, so the zips don't sit on a small disk.
    assert_eq!(
        std::fs::read_dir(dir.path().join("downloads"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("staging"))
            .unwrap()
            .count(),
        0
    );
    let names: Vec<_> = events.iter().map(|e| e.to_string()).collect();
    assert!(names.iter().any(|n| n.starts_with("license ")), "{names:?}");
    assert!(names.iter().any(|n| n.starts_with("compare ")), "{names:?}");
    assert_eq!(names.last().unwrap(), "done     downloads=3");
}

#[test]
fn a_second_install_makes_zero_requests() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    w.server.clear_requests();

    // Nothing is pending, so no acceptance is needed either.
    let mut no_flags = PreAccepted::new(BTreeMap::new(), Authority::Personal);
    let (report, events) = run(&inst, &mut no_flags, InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 0);
    assert_eq!(w.server.request_count(), 0);
    assert_eq!(events.last().unwrap().to_string(), "done     downloads=0");

    let (report, _) = run(
        &inst,
        &mut no_flags,
        InstallOptions {
            verify: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report.downloads, 0);
    assert_eq!(w.server.request_count(), 0);
}

#[test]
fn keep_downloads_keeps_the_zips_and_they_are_reused_after_a_remove() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let keep = InstallOptions {
        keep_downloads: true,
        ..Default::default()
    };
    run(&inst, &mut accept_all(&w), keep).unwrap();
    assert_eq!(kept_downloads(&dir), 3);

    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    let (report, _) = run(&inst, &mut accept_all(&w), keep).unwrap();
    assert_eq!(report.downloads, 0, "verified zips are reused");

    let removed = inst.remove(RemoveTarget::Downloads, false).unwrap();
    assert!(removed.downloads);
    assert!(!dir.path().join("downloads").exists());
}

#[test]
fn without_acceptance_nothing_downloads_and_not_even_metadata_is_requested() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let mut none = PreAccepted::new(BTreeMap::new(), Authority::Personal);
    let err = run(&inst, &mut none, InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::LicenseNotAccepted { .. }), "{err}");
    assert_eq!(err.exit_code(), 3);
    assert_eq!(w.server.request_count(), 0);
    assert!(!dir.path().join("sdk").exists());
}

#[test]
fn a_wrong_hash_fetches_only_metadata() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let mut wrong = PreAccepted::new(
        BTreeMap::from([
            (
                mtp_sdk::ANDROID_SDK_LICENSE.to_string(),
                mtp_sdk::hash::sha1_bytes(b"other text"),
            ),
            (GPL_3.to_string(), gpl3_text().sha1().clone()),
        ]),
        Authority::Personal,
    );
    let err = run(&inst, &mut wrong, InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::LicenseTextChanged { .. }), "{err}");
    assert!(
        !err.to_string().contains(w.license_sha1.as_str()),
        "the live hash comes only together with its text: {err}"
    );
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));
    assert!(!dir.path().join("downloads").exists());
    assert!(
        Home::new(dir.path())
            .licenses()
            .current("android-sdk-license")
            .is_none()
    );
}

#[test]
fn declining_stops_before_any_package_and_records_nothing() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let mut dialog = Dialog {
        answer: Decision::Decline,
        shown: Vec::new(),
    };
    let err = run(&inst, &mut dialog, InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::LicenseNotAccepted { .. }), "{err}");
    assert_eq!(dialog.shown, ["android-sdk-license"]);
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));
    assert!(
        Home::new(dir.path())
            .licenses()
            .current("android-sdk-license")
            .is_none()
    );
}

#[test]
fn a_dialog_sees_both_licenses_and_an_accepted_text_is_not_asked_again() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let answer = Decision::Accept {
        authority: Authority::Employer,
        via: Via::Gui,
    };
    let mut dialog = Dialog {
        answer,
        shown: Vec::new(),
    };
    run(&inst, &mut dialog, InstallOptions::default()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license", "gpl-3.0"]);
    let record = Home::new(dir.path())
        .licenses()
        .current("android-sdk-license")
        .unwrap();
    assert_eq!(
        (record.authority, record.via),
        (Authority::Employer, Via::Gui)
    );

    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    let mut dialog = Dialog {
        answer,
        shown: Vec::new(),
    };
    run(&inst, &mut dialog, InstallOptions::default()).unwrap();
    assert!(dialog.shown.is_empty(), "accepted texts need no new prompt");
}

/// Serves both repository XMLs with this license text and the world's pins.
fn serve_license_text(w: &World, license_text: &str) {
    serve_pins(w, license_text, "channel-0", "android-sdk-license");
}

/// Serves both repository XMLs with this license text, listing the world's pins under this
/// channel and license.
fn serve_pins(w: &World, license_text: &str, channel: &str, license: &str) {
    for path in [MAIN_XML_PATH, IMAGE_XML_PATH] {
        let pin = w
            .manifest
            .packages
            .iter()
            .find(|p| p.repository_xml.ends_with(path))
            .unwrap();
        let xml = repository_xml(
            license_text,
            &[PackageSpec {
                path: &pin.path,
                revision_xml: &revision_xml(&pin.revision),
                channel,
                license,
                url: pin.url.rsplit('/').next().unwrap(),
                size: pin.size,
                sha1: pin.sha1.as_str(),
            }],
        );
        w.server.body(path, xml.into_bytes());
    }
}

#[test]
fn a_changed_license_text_asks_again_before_a_new_download() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();

    serve_license_text(&w, "Fixture license text, second edition\n");
    w.server.clear_requests();

    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::LicenseTextChanged { .. }), "{err}");
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));

    let mut dialog = Dialog {
        answer: Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        },
        shown: Vec::new(),
    };
    run(&inst, &mut dialog, InstallOptions::default()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license"]);
    assert!(
        dir.path()
            .join("licenses")
            .join(format!(
                "android-sdk-license.{}.json",
                w.license_sha1.prefix()
            ))
            .exists()
    );
}

#[test]
fn a_tampered_zip_is_rejected_before_extraction() {
    let w = world();
    let mut tampered = support::fixtures::package_zip("emulator", "emulator.exe", "37.2.13", 1);
    assert_eq!(tampered.len() as u64, w.manifest.packages[0].size);
    tampered[100] ^= 1;
    w.server.body(EMULATOR_ZIP_PATH, tampered);
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(kept_downloads(&dir), 0);
    assert!(!dir.path().join("staging").exists());
    let root = SdkRoot::open(&Home::new(dir.path()), &w.manifest.set)
        .unwrap()
        .unwrap();
    assert_eq!(root.record().state, RootState::Installing);
    assert!(
        Home::new(dir.path())
            .sdk_root(&w.manifest.set)
            .unwrap()
            .is_none()
    );
}

fn manifest_for_zip(w: &World, zip: Vec<u8>) -> Manifest {
    let mut manifest = w.manifest.clone();
    let sha1 = mtp_sdk::hash::sha1_bytes(&zip);
    manifest.packages[0].size = zip.len() as u64;
    manifest.packages[0].sha1 = sha1.clone();
    let xml = repository_xml(
        LICENSE_BODY,
        &[PackageSpec {
            path: "emulator",
            revision_xml: &revision_xml("37.2.13"),
            channel: "channel-0",
            license: "android-sdk-license",
            url: "emulator-fixture.zip",
            size: zip.len() as u64,
            sha1: sha1.as_str(),
        }],
    );
    w.server.body(MAIN_XML_PATH, xml.into_bytes());
    w.server.body(EMULATOR_ZIP_PATH, zip);
    manifest
}

#[test]
fn an_unsafe_zip_with_the_right_hash_is_refused_and_keeps_the_zip_for_inspection() {
    let w = world();
    let evil = zip_of(&[
        ("emulator/emulator.exe", b"x"),
        ("../evil", noise(10, 1).as_slice()),
    ]);
    let manifest = manifest_for_zip(&w, evil);
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
    assert!(dir.path().join("downloads/emulator-fixture.zip").is_file());
    assert!(!dir.path().join("evil").exists());
}

#[test]
fn a_package_that_names_another_revision_is_refused() {
    let w = world();
    let zip = support::fixtures::package_zip("emulator", "emulator.exe", "37.2.12", 1);
    let manifest = manifest_for_zip(&w, zip);
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(err.to_string().contains("source.properties"), "{err}");
    assert_eq!(
        std::fs::read_dir(dir.path().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_package_without_its_expected_file_is_refused() {
    let w = world();
    let zip = support::fixtures::package_zip("emulator", "other.exe", "37.2.13", 1);
    let manifest = manifest_for_zip(&w, zip);
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(err.to_string().contains("emulator.exe"), "{err}");
}

#[test]
fn a_pin_that_drifted_from_the_xml_is_refused_before_download() {
    let w = world();
    let mut manifest = w.manifest.clone();
    manifest.packages[0].sha1 = mtp_sdk::hash::Sha1Hex::parse(&"b".repeat(40)).unwrap();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::PinMismatch { .. }), "{err}");
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));
}

#[test]
fn a_withdrawn_pin_is_reported_and_an_installed_root_keeps_working() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

    // Google drops every package from its XML and the zip from its server.
    let empty = repository_xml(
        LICENSE_BODY,
        &[PackageSpec {
            path: "something-else",
            revision_xml: "<major>1</major>",
            channel: "channel-0",
            license: "android-sdk-license",
            url: "a.zip",
            size: 1,
            sha1: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    w.server.body(MAIN_XML_PATH, empty.clone().into_bytes());
    w.server.body(IMAGE_XML_PATH, empty.into_bytes());
    w.server.route(EMULATOR_ZIP_PATH, Route::Status(404));

    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 0, "the installed root still works");

    let other = temp_dir();
    let inst = installer(&other, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::PinNotInRepository { .. }), "{err}");
    assert_eq!(err.exit_code(), 5);
    assert!(err.to_string().contains("keep working"), "{err}");
}

#[test]
fn a_crashed_install_resumes_without_repeating_finished_packages() {
    let w = world();
    let dir = temp_dir();
    let image_pin = &w.manifest.packages[1];
    let image_path = image_pin.url[w.server.url("").len()..].to_string();
    let good_image = support::fixtures::package_zip("x86_64", "system.img", "2", 10);
    assert_eq!(good_image.len() as u64, image_pin.size);
    w.server.route(&image_path, Route::Status(500));

    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Http { status: 500, .. }), "{err}");
    let home = Home::new(dir.path());
    let root = SdkRoot::open(&home, &w.manifest.set).unwrap().unwrap();
    assert_eq!(root.record().state, RootState::Installing);
    assert_eq!(root.record().packages.len(), 1);
    assert!(
        home.sdk_root(&w.manifest.set).unwrap().is_none(),
        "never launch an unfinished root"
    );

    // A crash can also leave an unrecorded directory and a stale staging folder.
    let stray = root_file(
        &dir,
        &w.manifest.set,
        "system-images/android-36/default/x86_64",
    );
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("left-behind"), b"x").unwrap();
    std::fs::create_dir_all(dir.path().join("staging/emulator-1a-2b-3c/x")).unwrap();

    w.server.body(&image_path, good_image);
    w.server.clear_requests();
    let plan = inst.plan(false).unwrap();
    assert_eq!(plan.packages[0].1, PackageAction::Reuse);
    assert_eq!(plan.packages[1].1, PackageAction::Download);

    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert_eq!(
        report.downloads, 2,
        "the image and Magisk, not the emulator"
    );
    assert!(
        !w.server
            .requests()
            .iter()
            .any(|r| r.path == EMULATOR_ZIP_PATH)
    );
    let root = home.sdk_root(&w.manifest.set).unwrap().unwrap();
    let deep = root.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
    assert_eq!(
        std::fs::read_dir(dir.path().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_damaged_ready_root_is_never_repaired_in_place() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let exe = root_file(&dir, &report.set, "emulator/emulator.exe");
    let mut bytes = std::fs::read(&exe).unwrap();
    bytes[5] ^= 1;
    std::fs::write(&exe, &bytes).unwrap();

    // Same size: the quick check can't see it, and only the deep check does.
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let err = run(
        &inst,
        &mut accept_all(&w),
        InstallOptions {
            verify: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
    assert!(err.to_string().contains("mtp sdk remove"), "{err}");
    let deep = home
        .sdk_root(&report.set)
        .unwrap()
        .unwrap()
        .deep_check()
        .unwrap();
    assert_eq!(deep.modified, ["emulator/emulator.exe"]);

    // A missing file makes the root unusable, and the quick check sees that.
    std::fs::remove_file(&exe).unwrap();
    assert!(home.sdk_root(&report.set).unwrap().is_none());
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
}

#[test]
fn a_missing_magisk_binary_is_fetched_again_without_touching_the_root() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let magisk = dir.path().join("tools/magisk/v1.0/magisk");
    std::fs::write(&magisk, b"tampered").unwrap();
    w.server.clear_requests();

    let (again, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert_eq!(again.downloads, 1);
    assert_eq!(
        sha256_file(&magisk).unwrap(),
        w.manifest.magisk.binary_sha256
    );
    assert!(
        w.server.requests().iter().all(|r| r.path.ends_with(".apk")),
        "no Google request"
    );
    assert_eq!(report.set, again.set);
}

#[test]
fn a_tampered_apk_member_is_refused() {
    let w = world();
    let mut manifest = w.manifest.clone();
    manifest.magisk.binary_sha256 = mtp_sdk::hash::sha256_bytes(b"something else");
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert!(!dir.path().join("tools/magisk/v1.0").exists());
}

#[test]
fn a_lease_blocks_remove_and_force_overrides_it() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let root = home.sdk_root(&report.set).unwrap().unwrap();
    root.lease("instance-a", "first instance").unwrap();
    root.lease("instance-b", "").unwrap();
    assert_eq!(root.leases().unwrap(), ["instance-a", "instance-b"]);
    assert!(root.lease("../evil", "").is_err());

    let err = inst
        .remove(RemoveTarget::Set(report.set.clone()), false)
        .unwrap_err();
    assert!(
        matches!(&err, Error::RootInUse { leases, .. } if leases.len() == 2),
        "{err}"
    );
    assert!(err.to_string().contains("instance-a"), "{err}");
    assert!(root.path().is_dir());

    root.release("instance-a").unwrap();
    root.release("instance-a").unwrap();
    assert_eq!(root.leases().unwrap(), ["instance-b"]);

    let removed = inst
        .remove(RemoveTarget::Set(report.set.clone()), true)
        .unwrap();
    assert_eq!(removed.roots, [report.set.to_string()]);
    assert!(!root.path().exists());
    assert!(!dir.path().join(format!("sdk/{}.json", report.set)).exists());
    assert!(
        !dir.path()
            .join(format!("sdk/{}.leases", report.set))
            .exists()
    );
    assert!(home.sdk_root(&report.set).unwrap().is_none());
}

#[test]
fn a_second_set_shares_an_unchanged_package_with_zero_downloads_of_it() {
    let w = world();
    let dir = temp_dir();
    let first = installer(&dir, &w.server, &w.manifest);
    let (first_report, _) = run(&first, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let next_manifest = serve_set(&w.server, "3", 20);
    assert!(next_manifest.set.as_str().starts_with("37.2.13-36r3-"));
    let next = installer(&dir, &w.server, &next_manifest);
    let plan = next.plan(false).unwrap();
    assert_eq!(
        plan.packages[0].1,
        PackageAction::Link {
            from: first_report.set.clone()
        }
    );
    assert_eq!(plan.packages[1].1, PackageAction::Download);
    w.server.clear_requests();

    let (report, events) = run(&next, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 1, "only the new image");
    assert!(
        !w.server
            .requests()
            .iter()
            .any(|r| r.path == EMULATOR_ZIP_PATH)
    );
    assert!(
        events
            .iter()
            .any(|e| e.to_string().contains("linked from 37.2.13-36r2"))
    );

    let home = Home::new(dir.path());
    let old = home
        .sdk_root(&first_report.set)
        .unwrap()
        .expect("the previous set stays");
    let new = home
        .sdk_root(&report.set)
        .unwrap()
        .expect("the new set is ready");
    assert_eq!(new.deep_check().unwrap().modified.len(), 0);
    assert!(
        new.path().join("emulator/empty").is_dir(),
        "empty directories are kept too"
    );
    assert_eq!(
        sha256_file(&old.emulator_exe()).unwrap(),
        sha256_file(&new.emulator_exe()).unwrap()
    );

    // Removing the old root leaves the shared files of the new one intact.
    next.remove(RemoveTarget::Set(first_report.set.clone()), false)
        .unwrap();
    assert!(!old.path().exists());
    let deep = new.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
}

/// Writes `tools/magisk/<file_tag>.json` the way an install would, naming `tag`.
fn write_magisk_record(home: &Home, file_tag: &str, tag: &str) {
    let record = serde_json::json!({
        "schema": 1,
        "tag": tag,
        "binary_sha256": "a".repeat(64),
        "installed_at": "2026-11-01T00:00:00Z",
    });
    std::fs::write(
        home.magisk_dir().join(format!("{file_tag}.json")),
        record.to_string(),
    )
    .unwrap();
}

#[test]
fn remove_unused_keeps_the_current_and_leased_sets() {
    let w = world();
    let dir = temp_dir();
    let old_inst = installer(&dir, &w.server, &w.manifest);
    let (old_report, _) = run(&old_inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let leased_manifest = serve_set(&w.server, "3", 20);
    let leased_inst = installer(&dir, &w.server, &leased_manifest);
    let (leased_report, _) =
        run(&leased_inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let current_manifest = serve_set(&w.server, "4", 30);
    let current = installer(&dir, &w.server, &current_manifest);
    let (current_report, _) =
        run(&current, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let home = Home::new(dir.path());
    home.sdk_root(&leased_report.set)
        .unwrap()
        .unwrap()
        .lease("instance-a", "")
        .unwrap();
    // A Magisk version no manifest names any more.
    let old_tag = home.magisk_dir().join("v0.9");
    std::fs::create_dir_all(&old_tag).unwrap();
    std::fs::write(old_tag.join("magisk"), b"old").unwrap();
    write_magisk_record(&home, "v0.9", "v0.9");

    let removed = current.remove(RemoveTarget::Unused, false).unwrap();
    assert_eq!(removed.roots, [old_report.set.to_string()]);
    assert_eq!(removed.leased, [leased_report.set.to_string()]);
    assert_eq!(removed.magisk_tags, ["v0.9"]);
    assert!(!old_tag.exists());
    assert!(home.magisk_dir().join("v1.0/magisk").is_file());
    assert!(home.sdk_root(&current_report.set).unwrap().is_some());
    assert!(home.sdk_root(&leased_report.set).unwrap().is_some());
    assert!(home.sdk_root(&old_report.set).unwrap().is_none());
}

#[test]
fn the_home_lock_excludes_a_second_install() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let _held = Home::new(dir.path()).lock().unwrap();
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Busy { .. }), "{err}");
    assert_eq!(err.exit_code(), 4);
    assert!(err.to_string().contains("mtp-sdk.lock"), "{err}");
    assert_eq!(w.server.request_count(), 0);
}

#[test]
fn status_reports_every_part_and_takes_no_lock_or_network() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let empty = inst.status(false).unwrap();
    assert!(empty.ok);
    assert!(empty.roots.is_empty());
    assert_eq!(empty.licenses[0].state, "not-accepted");
    assert!(!empty.magisk.present);
    assert!(!dir.path().exists() || std::fs::read_dir(dir.path()).unwrap().next().is_none());

    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    w.server.clear_requests();
    let _held = Home::new(dir.path()).lock().unwrap();

    let status = inst.status(true).unwrap();
    assert_eq!(w.server.request_count(), 0);
    assert!(status.ok);
    assert_eq!(status.licenses[0].state, "accepted");
    assert_eq!(status.licenses[1].state, "accepted");
    assert_eq!(status.roots.len(), 1);
    let root = &status.roots[0];
    assert!(root.current);
    assert_eq!(root.state, "ready");
    assert_eq!(root.packages.len(), 2);
    assert_eq!(root.notices.len(), 2);
    assert!(status.magisk.hash_ok);
    assert!(status.pending_downloads.is_empty());

    std::fs::write(root.path.join("emulator/extra.txt"), b"x").unwrap();
    let status = inst.status(false).unwrap();
    assert!(status.ok, "an extra file is a warning");
    assert_eq!(status.roots[0].check.extra, ["emulator/extra.txt"]);

    std::fs::remove_file(root.path.join("emulator/NOTICE.txt")).unwrap();
    let status = inst.status(false).unwrap();
    assert!(!status.ok);
    assert_eq!(status.roots[0].check.missing, ["emulator/NOTICE.txt"]);

    std::fs::write(dir.path().join("tools/magisk/v1.0/magisk"), b"tampered").unwrap();
    let status = inst.status(false).unwrap();
    assert!(status.magisk.present && !status.magisk.hash_ok);
}

#[test]
fn status_lists_partial_downloads() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    std::fs::create_dir_all(dir.path().join("downloads")).unwrap();
    std::fs::write(
        dir.path().join("downloads/emulator-fixture.zip.part"),
        b"12345",
    )
    .unwrap();
    let status = inst.status(false).unwrap();
    assert_eq!(
        status.pending_downloads[0].name,
        "emulator-fixture.zip.part"
    );
    assert_eq!(status.pending_downloads[0].bytes, 5);
}

#[test]
fn a_license_text_is_available_for_review() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let text = inst.license_text("android-sdk-license").unwrap();
    assert_eq!(*text.sha1(), w.license_sha1);
    assert_eq!(inst.license_text(GPL_3).unwrap(), gpl3_text());
    assert!(inst.license_text("nope").is_err());
}

fn no_flags() -> PreAccepted {
    PreAccepted::new(BTreeMap::new(), Authority::Personal)
}

fn keep_zips() -> InstallOptions {
    InstallOptions {
        keep_downloads: true,
        ..Default::default()
    }
}

fn flip_a_byte(path: &std::path::Path) {
    let mut bytes = std::fs::read(path).unwrap();
    bytes[5] ^= 1;
    std::fs::write(path, &bytes).unwrap();
}

fn warnings(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Warning { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

/// Leaves the set `installing` with the emulator recorded and the image not, as a crash does.
fn crash_before_the_image(w: &World, inst: &Installer) -> String {
    let image_path = w.manifest.packages[1].url[w.server.url("").len()..].to_string();
    w.server.route(&image_path, Route::Status(500));
    let err = run(inst, &mut accept_all(w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Http { status: 500, .. }), "{err}");
    image_path
}

#[test]
fn what_is_not_ours_survives_a_failed_gate_and_removing_downloads() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let downloads = dir.path().join("downloads");
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("user.txt"), b"mine").unwrap();
    std::fs::write(downloads.join("emulator-fixture.zip.part"), b"12").unwrap();
    std::fs::create_dir_all(staging.join("keep")).unwrap();
    std::fs::write(staging.join("keep/file.txt"), b"mine").unwrap();
    std::fs::create_dir_all(staging.join("emulator-1a-2b-3c")).unwrap();

    let err = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::LicenseNotAccepted { .. }), "{err}");
    assert!(staging.join("keep/file.txt").is_file());
    assert!(!staging.join("emulator-1a-2b-3c").exists());

    let removed = inst.remove(RemoveTarget::Downloads, false).unwrap();
    assert!(removed.downloads);
    assert!(downloads.join("user.txt").is_file());
    assert!(!downloads.join("emulator-fixture.zip.part").exists());

    let removed = inst.remove(RemoveTarget::Downloads, false).unwrap();
    assert!(!removed.downloads, "nothing pinned was left to remove");
    assert!(downloads.join("user.txt").is_file());
}

#[test]
fn remove_can_not_reach_another_roots_leases_or_record() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let root = home.sdk_root(&report.set).unwrap().unwrap();
    root.lease("instance-a", "").unwrap();

    for suffix in ["leases", "json", "LEASES"] {
        assert!(SetId::parse(&format!("{}.{suffix}", report.set)).is_err());
    }
    assert_eq!(root.leases().unwrap(), ["instance-a"]);
    assert!(home.sdk_root(&report.set).unwrap().is_some());
}

#[test]
fn a_directory_without_a_record_is_not_removed() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let stranger = dir.path().join("sdk/9.9.9-1r1");
    std::fs::create_dir_all(&stranger).unwrap();
    std::fs::write(stranger.join("file.txt"), b"mine").unwrap();

    let set = SetId::parse("9.9.9-1r1").unwrap();
    let err = inst.remove(RemoveTarget::Set(set), true).unwrap_err();
    assert!(matches!(err, Error::NotOurs { .. }), "{err}");
    assert!(stranger.join("file.txt").is_file());
}

#[test]
fn a_lease_that_could_never_be_counted_is_refused() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    for owner in [".tmp-x", ".hidden", "NUL", "com1.txt", "-dash", "", "a."] {
        let err = root.lease(owner, "").unwrap_err();
        assert!(matches!(err, Error::InvalidName(_)), "{owner}: {err}");
    }
    assert!(root.leases().unwrap().is_empty());
    root.lease("instance.a_1", "").unwrap();
    assert_eq!(root.leases().unwrap(), ["instance.a_1"]);
}

#[test]
fn a_resumed_install_extracts_again_what_a_crash_altered() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let image_path = crash_before_the_image(&w, &inst);

    // The right size with other bytes, as a crash before a flush can leave behind.
    let exe = root_file(&dir, &w.manifest.set, "emulator/emulator.exe");
    flip_a_byte(&exe);
    w.server.body(
        &image_path,
        support::fixtures::package_zip("x86_64", "system.img", "2", 10),
    );

    let (report, events) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Reuse { name, .. } if name == "emulator")),
        "{events:?}"
    );
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    let deep = root.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
}

#[test]
fn a_kept_zip_installs_on_a_machine_that_reaches_no_xml_and_says_the_text_was_not_checked() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();

    let plan = inst.plan(false).unwrap();
    assert_eq!(plan.packages[0].1, PackageAction::Extract);
    assert_eq!(plan.packages[1].1, PackageAction::Extract);

    // With the server gone, every connection is refused.
    drop(w.server);
    let (report, events) = run(&inst, &mut no_flags(), keep_zips()).unwrap();
    assert_eq!(report.downloads, 0);
    assert!(
        events.contains(&Event::License {
            id: "android-sdk-license".into(),
            state: "accepted, live text not checked: offline".into()
        }),
        "{events:?}"
    );
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    assert!(root.deep_check().unwrap().ok());
}

#[test]
fn a_kept_zip_with_no_acceptance_on_record_still_shows_the_text() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    std::fs::remove_file(dir.path().join("licenses/android-sdk-license.json")).unwrap();
    w.server.clear_requests();

    let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
    assert!(matches!(err, Error::LicenseNotAccepted { .. }), "{err}");
    assert_eq!(w.server.request_count(), 0);

    let mut dialog = Dialog {
        answer: Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        },
        shown: Vec::new(),
    };
    run(&inst, &mut dialog, keep_zips()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license"]);
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));
}

#[test]
fn a_second_set_needing_only_local_files_installs_with_the_server_gone() {
    let w = world();
    let dir = temp_dir();
    let first = installer(&dir, &w.server, &w.manifest);
    let (first_report, _) = run(&first, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let next_manifest = serve_set(&w.server, "3", 20);
    std::fs::create_dir_all(dir.path().join("downloads")).unwrap();
    std::fs::write(
        dir.path().join("downloads/x86_64-r3.zip"),
        support::fixtures::package_zip("x86_64", "system.img", "3", 20),
    )
    .unwrap();
    let next = installer(&dir, &w.server, &next_manifest);
    let plan = next.plan(false).unwrap();
    assert_eq!(
        plan.packages[0].1,
        PackageAction::Link {
            from: first_report.set.clone()
        }
    );
    assert_eq!(plan.packages[1].1, PackageAction::Extract);

    drop(w.server);
    let (report, events) = run(&next, &mut no_flags(), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 0);
    assert!(
        events.contains(&Event::License {
            id: "android-sdk-license".into(),
            state: "accepted, live text not checked: offline".into()
        }),
        "{events:?}"
    );
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    let deep = root.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
}

#[test]
fn an_unreadable_record_blocks_nothing_but_its_own_set() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let other = dir.path().join("sdk/1.0.0-1r1");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("file.bin"), b"x").unwrap();
    std::fs::write(dir.path().join("sdk/1.0.0-1r1.json"), b"{\"schema\": 1,").unwrap();

    let status = inst.status(false).unwrap();
    assert!(!status.ok);
    assert_eq!(status.roots[0].state, "unreadable");
    assert!(status.roots[0].error.is_some());
    assert!(!status.roots[0].current);

    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let status = inst.status(false).unwrap();
    assert_eq!(status.roots.len(), 2);
    assert!(status.roots.iter().any(|r| r.state == "ready" && r.current));

    let removed = inst
        .remove(RemoveTarget::Set(SetId::parse("1.0.0-1r1").unwrap()), false)
        .unwrap();
    assert_eq!(removed.roots, ["1.0.0-1r1"]);
    assert!(!other.exists());
    assert!(!dir.path().join("sdk/1.0.0-1r1.json").exists());
    assert!(inst.status(false).unwrap().ok);

    // Damage to the manifest's own set is reported, and `remove` is the way out.
    let record = dir.path().join(format!("sdk/{}.json", report.set));
    std::fs::write(&record, b"{").unwrap();
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
    assert!(err.to_string().contains("mtp sdk remove"), "{err}");
    inst.remove(RemoveTarget::Set(report.set.clone()), false)
        .unwrap();
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
}

#[test]
fn remove_unused_also_clears_an_unreadable_root_unless_it_is_leased() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    for set in ["1.0.0-1r1", "2.0.0-1r1"] {
        std::fs::create_dir_all(dir.path().join("sdk").join(set)).unwrap();
        std::fs::write(dir.path().join(format!("sdk/{set}.json")), b"{").unwrap();
    }
    std::fs::create_dir_all(dir.path().join("sdk/2.0.0-1r1.leases")).unwrap();
    std::fs::write(dir.path().join("sdk/2.0.0-1r1.leases/instance-a"), b"{}").unwrap();

    let removed = inst.remove(RemoveTarget::Unused, false).unwrap();
    assert_eq!(removed.roots, ["1.0.0-1r1"]);
    assert_eq!(removed.leased, ["2.0.0-1r1"]);
    assert!(dir.path().join("sdk/2.0.0-1r1").is_dir());
}

#[test]
fn a_root_that_is_not_ready_takes_no_lease() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    crash_before_the_image(&w, &inst);
    let home = Home::new(dir.path());
    let root = SdkRoot::open(&home, &w.manifest.set).unwrap().unwrap();
    let err = root.lease("instance-a", "").unwrap_err();
    assert!(matches!(err, Error::RootNotReady { .. }), "{err}");
    assert_eq!(err.exit_code(), 1);
    assert!(root.leases().unwrap().is_empty());
}

#[test]
fn a_lease_taken_through_a_stale_handle_after_a_remove_is_refused() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    inst.remove(RemoveTarget::Set(report.set.clone()), false)
        .unwrap();
    let err = root.lease("instance-a", "").unwrap_err();
    assert!(matches!(err, Error::RootNotReady { .. }), "{err}");
    assert!(
        !dir.path()
            .join(format!("sdk/{}.leases", report.set))
            .exists()
    );
}

#[test]
fn a_damaged_link_source_falls_back_to_a_download() {
    let w = world();
    let dir = temp_dir();
    let first = installer(&dir, &w.server, &w.manifest);
    let (first_report, _) = run(&first, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let source_exe = root_file(&dir, &first_report.set, "emulator/emulator.exe");
    flip_a_byte(&source_exe);

    let next_manifest = serve_set(&w.server, "3", 20);
    let next = installer(&dir, &w.server, &next_manifest);
    assert!(matches!(
        next.plan(false).unwrap().packages[0].1,
        PackageAction::Link { .. }
    ));
    w.server.clear_requests();

    let (report, events) = run(&next, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 2, "the emulator and the new image");
    assert!(
        w.server
            .requests()
            .iter()
            .any(|r| r.path == EMULATOR_ZIP_PATH)
    );
    let warned = warnings(&events);
    assert!(
        warned.iter().any(|m| m.contains("37.2.13-36r2")),
        "{warned:?}"
    );
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    let deep = root.deep_check().unwrap();
    assert!(deep.ok() && deep.extra.is_empty(), "{deep:?}");
    assert_ne!(
        sha256_file(&source_exe).unwrap(),
        sha256_file(&root.emulator_exe()).unwrap()
    );
}

#[test]
fn a_home_too_deep_for_googles_binaries_is_refused_before_any_request() {
    let w = world();
    let dir = temp_dir();
    let deep = dir
        .path()
        .join("a".repeat(100))
        .join("b".repeat(100))
        .join("c");
    let client = Client::new(SourcePolicy::loopback(w.server.port()));
    let inst = Installer::new(Home::new(&deep), w.manifest.clone(), client);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(
        matches!(err, Error::HomePathTooLong { limit: 260, .. }),
        "{err}"
    );
    assert_eq!(w.server.request_count(), 0);
    assert!(!deep.join("downloads").exists());
}

#[test]
fn status_calls_a_consistent_google_record_accepted_whatever_was_reviewed() {
    let w = world();
    let dir = temp_dir();
    let mut manifest = w.manifest.clone();
    manifest
        .licenses
        .get_mut("android-sdk-license")
        .unwrap()
        .reviewed_text_sha1 = Some(mtp_sdk::hash::sha1_bytes(b"a text reviewed earlier"));
    let inst = installer(&dir, &w.server, &manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let status = inst.status(false).unwrap();
    assert_eq!(status.licenses[0].state, "accepted");
    assert!(status.licenses[0].differs_from_reviewed);
    assert_eq!(status.licenses[1].state, "accepted");
    assert!(!status.licenses[1].differs_from_reviewed);

    let plain = installer(&dir, &w.server, &w.manifest);
    assert!(!plain.status(false).unwrap().licenses[0].differs_from_reviewed);
}

#[test]
fn a_no_op_install_removes_a_leftover_zip_and_only_warns_when_it_can_not() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    let downloads = dir.path().join("downloads");

    let (_, events) = run(&inst, &mut no_flags(), keep_zips()).unwrap();
    assert!(warnings(&events).is_empty());
    assert_eq!(kept_downloads(&dir), 3);

    let (report, _) = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 0);
    assert_eq!(std::fs::read_dir(&downloads).unwrap().count(), 0);

    // A directory in the zip's place can't be deleted as a file. The set is ready anyway.
    std::fs::create_dir(downloads.join("emulator-fixture.zip")).unwrap();
    let (_, events) = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap();
    assert!(
        warnings(&events)
            .iter()
            .any(|m| m.contains("emulator-fixture.zip")),
        "{events:?}"
    );
    assert_eq!(events.last().unwrap().to_string(), "done     downloads=0");
}

#[test]
fn status_reports_files_that_vanished_instead_of_failing() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    std::fs::remove_file(root_file(&dir, &report.set, "emulator/emulator.exe")).unwrap();
    std::fs::remove_file(dir.path().join("tools/magisk/v1.0/magisk")).unwrap();

    let status = inst.status(true).unwrap();
    assert!(!status.ok);
    assert_eq!(status.roots[0].check.missing, ["emulator/emulator.exe"]);
    assert!(!status.magisk.present && !status.magisk.hash_ok);
}

#[test]
fn a_pinned_zip_that_is_gone_but_still_listed_is_a_withdrawn_pin() {
    for status in [404, 410] {
        let w = world();
        let dir = temp_dir();
        w.server.route(EMULATOR_ZIP_PATH, Route::Status(status));
        let inst = installer(&dir, &w.server, &w.manifest);
        let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
        assert!(matches!(err, Error::PinWithdrawn { .. }), "{err}");
        assert_eq!(err.exit_code(), 5);
        assert!(err.to_string().contains("keep working"), "{err}");
    }
}

#[test]
fn a_magisk_asset_that_is_gone_is_a_withdrawn_pin() {
    let w = world();
    let dir = temp_dir();
    let asset = w.manifest.magisk.url[w.server.url("").len()..].to_string();
    w.server.route(&asset, Route::Status(404));
    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::PinWithdrawn { .. }), "{err}");
    assert_eq!(err.exit_code(), 5);
    assert!(err.to_string().contains("topjohnwu/Magisk v1.0"), "{err}");
}

#[test]
fn a_server_error_on_a_pinned_zip_stays_a_plain_http_error() {
    let w = world();
    let dir = temp_dir();
    w.server.route(EMULATOR_ZIP_PATH, Route::Status(503));
    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Http { status: 503, .. }), "{err}");
    assert_eq!(err.exit_code(), 1);
}

#[test]
fn a_ready_root_without_the_directory_the_emulator_requires_is_refused() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let root = home.sdk_root(&report.set).unwrap().unwrap();
    let check = root.quick_check().unwrap();
    assert!(check.ok() && check.extra.is_empty(), "{check:?}");

    std::fs::remove_dir(root.path().join("platform-tools")).unwrap();
    let check = root.quick_check().unwrap();
    assert!(!check.ok());
    assert_eq!(check.missing, ["platform-tools"]);
    assert!(home.sdk_root(&report.set).unwrap().is_none());
    assert!(!inst.status(false).unwrap().ok);

    let err = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
    assert!(err.to_string().contains("platform-tools"), "{err}");
}

#[test]
fn a_root_linked_from_another_set_has_the_required_directory_too() {
    let w = world();
    let dir = temp_dir();
    run(
        &installer(&dir, &w.server, &w.manifest),
        &mut accept_all(&w),
        InstallOptions::default(),
    )
    .unwrap();
    let next_manifest = serve_set(&w.server, "3", 20);
    let next = installer(&dir, &w.server, &next_manifest);
    let (report, _) = run(&next, &mut accept_all(&w), InstallOptions::default()).unwrap();
    assert!(root_file(&dir, &report.set, "platform-tools").is_dir());
}

/// Makes `link` a junction to `target`, which needs no special rights.
fn junction(link: &std::path::Path, target: &std::path::Path) {
    let output = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(std::path::absolute(link).unwrap())
        .arg(std::path::absolute(target).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

fn assert_refused_as_a_link(err: &Error) {
    match err {
        Error::Io { source, .. } => {
            assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
            assert!(source.to_string().contains("junction"), "{err}");
        }
        other => panic!("expected the link to be refused, got {other}"),
    }
}

#[test]
fn a_resumed_install_neither_deletes_nor_writes_through_a_junction_in_the_root() {
    let w = world();
    let dir = temp_dir();
    let outside = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let image_path = crash_before_the_image(&w, &inst);
    w.server.body(
        &image_path,
        support::fixtures::package_zip("x86_64", "system.img", "2", 10),
    );

    let keep = outside.path().join("android-36/default/x86_64/keep.txt");
    std::fs::create_dir_all(keep.parent().unwrap()).unwrap();
    std::fs::write(&keep, b"mine").unwrap();
    junction(
        &root_file(&dir, &w.manifest.set, "system-images"),
        outside.path(),
    );

    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert_refused_as_a_link(&err);
    assert_eq!(std::fs::read(&keep).unwrap(), b"mine");
    assert!(!keep.with_file_name("system.img").exists());

    // Removing the root takes the link away and leaves what it pointed at.
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    assert!(!root_file(&dir, &w.manifest.set, "system-images").exists());
    assert_eq!(std::fs::read(&keep).unwrap(), b"mine");
}

#[test]
fn a_junction_on_the_way_to_the_magisk_folder_is_refused() {
    let w = world();
    let dir = temp_dir();
    let outside = temp_dir();
    junction(&dir.path().join("tools"), outside.path());
    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert_refused_as_a_link(&err);
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn a_junction_in_a_ready_root_makes_the_root_unusable() {
    let w = world();
    let dir = temp_dir();
    let outside = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    junction(
        &root_file(&dir, &report.set, "emulator/linked"),
        outside.path(),
    );

    let home = Home::new(dir.path());
    assert!(home.sdk_root(&report.set).unwrap().is_none());
    let root = SdkRoot::open(&home, &report.set).unwrap().unwrap();
    let check = root.quick_check().unwrap();
    assert_eq!(check.modified, ["emulator/linked"]);
    assert!(check.extra.is_empty(), "{check:?}");
}

/// Answers like a dialog, but first runs `meanwhile`, standing in for whatever else happens
/// on the machine while a person reads the license.
struct SlowDialog<F: FnMut()> {
    meanwhile: F,
}

impl<F: FnMut()> LicensePrompt for SlowDialog<F> {
    fn available(&self, _id: &str) -> bool {
        true
    }

    fn decide(
        &mut self,
        _text: &LicenseText,
        _reviewed: Option<&mtp_sdk::hash::Sha1Hex>,
    ) -> mtp_sdk::Result<Decision> {
        (self.meanwhile)();
        Ok(Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        })
    }
}

#[test]
fn a_kept_zip_swapped_after_planning_is_refused_and_nothing_is_recorded() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    std::fs::remove_file(dir.path().join("licenses/android-sdk-license.json")).unwrap();
    let zip = dir.path().join("downloads/emulator-fixture.zip");
    assert_eq!(
        inst.plan(false).unwrap().packages[0].1,
        PackageAction::Extract
    );

    let mut dialog = SlowDialog {
        meanwhile: || flip_a_byte(&zip),
    };
    let err = run(&inst, &mut dialog, keep_zips()).unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    let root = SdkRoot::open(&Home::new(dir.path()), &w.manifest.set)
        .unwrap()
        .unwrap();
    assert_eq!(root.record().state, RootState::Installing);
    assert!(root.record().packages.is_empty());
    assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
}

#[test]
fn no_one_can_change_a_zip_while_it_is_being_extracted() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let zip = dir.path().join("downloads/emulator-fixture.zip");
    let mut checked = false;
    inst.install(
        &mut accept_all(&w),
        keep_zips(),
        &mut |event| {
            if matches!(&event, Event::Compare { name, .. } if name == "emulator") {
                assert!(std::fs::OpenOptions::new().write(true).open(&zip).is_err());
                assert!(std::fs::remove_file(&zip).is_err());
                checked = true;
            }
        },
        &Cancel::new(),
    )
    .unwrap();
    assert!(checked);
    assert!(zip.is_file());
}

#[test]
fn an_apk_swapped_before_extraction_is_refused() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let apk = dir.path().join("downloads/Magisk-v1.0.apk");
    let err = inst
        .install(
            &mut accept_all(&w),
            keep_zips(),
            &mut |event| {
                if matches!(&event, Event::Extract { name } if name.ends_with("libmagisk.so")) {
                    flip_a_byte(&apk);
                }
            },
            &Cancel::new(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert!(!dir.path().join("tools/magisk/v1.0").exists());
}

/// Rewrites a root's record through `edit`, as anything with write access to the home could.
fn edit_record(dir: &TempDir, set: &SetId, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = dir.path().join(format!("sdk/{set}.json"));
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(&path, value.to_string()).unwrap();
}

#[test]
fn a_record_whose_paths_could_escape_is_treated_as_damaged() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let good = std::fs::read(dir.path().join(format!("sdk/{}.json", report.set))).unwrap();
    for bad in [
        "../../escaped",
        "..\\escaped",
        "/rooted",
        "C:/drive",
        "//server/share",
        "a//b",
        "",
        "a:stream",
        "NUL",
    ] {
        edit_record(&dir, &report.set, |record| {
            record["packages"][0]["dirs"] = serde_json::json!([bad]);
        });
        let err = SdkRoot::open(&home, &report.set).unwrap_err();
        assert!(matches!(err, Error::RecordInvalid { .. }), "{bad}: {err}");
        edit_record(&dir, &report.set, |record| {
            record["packages"][0]["dirs"] = serde_json::json!([]);
            record["packages"][0]["files"][0]["path"] = serde_json::json!(bad);
        });
        let err = SdkRoot::open(&home, &report.set).unwrap_err();
        assert!(matches!(err, Error::RecordInvalid { .. }), "{bad}: {err}");
        std::fs::write(dir.path().join(format!("sdk/{}.json", report.set)), &good).unwrap();
    }
    edit_record(&dir, &report.set, |record| {
        record["packages"][0]["install_dir"] = serde_json::json!("system-images");
    });
    let err = SdkRoot::open(&home, &report.set).unwrap_err();
    assert!(matches!(err, Error::RecordInvalid { .. }), "{err}");
    std::fs::write(dir.path().join(format!("sdk/{}.json", report.set)), &good).unwrap();
    edit_record(&dir, &report.set, |record| {
        record["schema"] = serde_json::json!(2);
    });
    let err = SdkRoot::open(&home, &report.set).unwrap_err();
    assert!(matches!(err, Error::RecordInvalid { .. }), "{err}");
}

#[test]
fn a_second_set_does_not_link_from_a_record_that_could_escape() {
    let w = world();
    let dir = temp_dir();
    let first = installer(&dir, &w.server, &w.manifest);
    let (first_report, _) = run(&first, &mut accept_all(&w), InstallOptions::default()).unwrap();
    edit_record(&dir, &first_report.set, |record| {
        record["packages"][0]["dirs"] = serde_json::json!(["../../escaped"]);
    });

    let next_manifest = serve_set(&w.server, "3", 20);
    let next = installer(&dir, &w.server, &next_manifest);
    assert_eq!(
        next.plan(false).unwrap().packages[0].1,
        PackageAction::Download
    );
    let (report, _) = run(&next, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let status = next.status(false).unwrap();
    let damaged = status
        .roots
        .iter()
        .find(|root| root.set == first_report.set.as_str())
        .unwrap();
    assert_eq!(damaged.state, "unreadable");
    assert!(!status.ok);
    assert!(
        Home::new(dir.path())
            .sdk_root(&report.set)
            .unwrap()
            .is_some()
    );
    for escaped in ["../escaped", "../../escaped", "sdk/escaped", "escaped"] {
        assert!(!dir.path().join(escaped).exists(), "{escaped}");
    }
}

#[test]
fn a_record_claiming_another_set_is_damaged_and_removes_only_its_own_root() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let sdk = dir.path().join("sdk");
    std::fs::create_dir_all(sdk.join("1.0.0-1r1")).unwrap();
    std::fs::create_dir_all(sdk.join("2.0.0-1r1")).unwrap();
    std::fs::write(sdk.join("2.0.0-1r1/keep.txt"), b"mine").unwrap();
    std::fs::write(
        sdk.join("1.0.0-1r1.json"),
        r#"{"schema":1,"set":"2.0.0-1r1","state":"ready","packages":[]}"#,
    )
    .unwrap();

    let status = inst.status(false).unwrap();
    assert_eq!(status.roots.len(), 1);
    assert_eq!(status.roots[0].set, "1.0.0-1r1");
    assert_eq!(status.roots[0].state, "unreadable");

    let removed = inst.remove(RemoveTarget::Unused, false).unwrap();
    assert_eq!(removed.roots, ["1.0.0-1r1"]);
    assert!(!sdk.join("1.0.0-1r1").exists());
    assert!(!sdk.join("1.0.0-1r1.json").exists());
    assert!(sdk.join("2.0.0-1r1/keep.txt").is_file());
}

#[test]
fn a_ready_root_that_disagrees_with_the_manifest_gets_no_advice_to_remove_it() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let mut edited = w.manifest.clone();
    edited.packages[0].sha1 = mtp_sdk::hash::Sha1Hex::parse(&"b".repeat(40)).unwrap();
    let err = installer(&dir, &w.server, &edited).plan(false).unwrap_err();
    assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
    assert!(err.to_string().contains("another pin"), "{err}");
    assert!(!err.to_string().contains("mtp sdk remove"), "{err}");

    edit_record(&dir, &report.set, |record| {
        record["packages"].as_array_mut().unwrap().remove(0);
    });
    let err = inst.plan(false).unwrap_err();
    assert!(err.to_string().contains("isn't in its record"), "{err}");
    assert!(!err.to_string().contains("mtp sdk remove"), "{err}");
}

#[test]
fn remove_unused_deletes_an_old_magisk_tag_only_with_its_record() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();
    let home = Home::new(dir.path());
    let magisk = home.magisk_dir();

    std::fs::create_dir_all(magisk.join("my-notes")).unwrap();
    std::fs::write(magisk.join("my-notes/keep.txt"), b"mine").unwrap();
    std::fs::create_dir_all(magisk.join("v0.9")).unwrap();
    std::fs::write(magisk.join("v0.9/magisk"), b"old").unwrap();
    write_magisk_record(&home, "v0.9", "v0.9");
    // A record naming another tag says nothing about this directory.
    std::fs::create_dir_all(magisk.join("v0.8")).unwrap();
    std::fs::write(magisk.join("v0.8/keep.txt"), b"mine").unwrap();
    write_magisk_record(&home, "v0.8", "v0.7");
    // Neither does one that can't be read.
    std::fs::create_dir_all(magisk.join("v0.6")).unwrap();
    std::fs::write(magisk.join("v0.6.json"), b"{").unwrap();

    let removed = inst.remove(RemoveTarget::Unused, false).unwrap();
    assert_eq!(removed.magisk_tags, ["v0.9"]);
    assert_eq!(removed.skipped, ["my-notes", "v0.6", "v0.8"]);
    assert!(!magisk.join("v0.9").exists());
    assert!(!magisk.join("v0.9.json").exists());
    assert!(magisk.join("my-notes/keep.txt").is_file());
    assert!(magisk.join("v0.8/keep.txt").is_file());
    assert!(magisk.join("v1.0/magisk").is_file());
    assert!(magisk.join("v1.0.json").is_file());
}

#[test]
fn a_complete_part_left_by_a_kill_installs_with_no_package_request() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    let downloads = dir.path().join("downloads");
    for pin in &w.manifest.packages {
        let zip = downloads.join(pin.file_name());
        std::fs::rename(&zip, part_path(&zip)).unwrap();
    }
    w.server.clear_requests();

    let plan = inst.plan(false).unwrap();
    assert_eq!(plan.packages[0].1, PackageAction::Extract);
    assert_eq!(plan.packages[1].1, PackageAction::Extract);
    assert!(
        part_path(&downloads.join(w.manifest.packages[0].file_name())).exists(),
        "planning changes nothing on disk"
    );

    let (report, _) = run(&inst, &mut no_flags(), keep_zips()).unwrap();
    assert_eq!(report.downloads, 0);
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));
    let root = Home::new(dir.path())
        .sdk_root(&report.set)
        .unwrap()
        .unwrap();
    assert!(root.deep_check().unwrap().ok());
    for pin in &w.manifest.packages {
        assert!(downloads.join(pin.file_name()).is_file());
    }
}

#[test]
fn a_part_with_wrong_bytes_is_not_taken_for_a_kept_zip() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let pin = &w.manifest.packages[0];
    std::fs::write(
        part_path(&downloads.join(pin.file_name())),
        vec![0u8; pin.size as usize],
    )
    .unwrap();
    assert_eq!(
        inst.plan(false).unwrap().packages[0].1,
        PackageAction::Download
    );
}

#[test]
fn removing_downloads_covers_earlier_pins_in_the_ledger_and_leaves_the_rest() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("old-image.zip"), b"zip").unwrap();
    std::fs::write(downloads.join("older-image.zip.part"), b"part").unwrap();
    std::fs::write(downloads.join("stranger.zip"), b"mine").unwrap();
    let outside = dir.path().join("outside.zip");
    std::fs::write(&outside, b"mine").unwrap();
    std::fs::write(
        downloads.join("owned.json"),
        r#"["old-image.zip", "older-image.zip", "..\\outside.zip", "../outside.zip"]"#,
    )
    .unwrap();

    let pending: Vec<String> = inst
        .status(false)
        .unwrap()
        .pending_downloads
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(pending, ["old-image.zip", "older-image.zip.part"]);

    let removed = inst.remove(RemoveTarget::Downloads, false).unwrap();
    assert!(removed.downloads);
    assert!(!downloads.join("old-image.zip").exists());
    assert!(!downloads.join("older-image.zip.part").exists());
    assert!(!downloads.join("owned.json").exists());
    assert!(downloads.join("stranger.zip").is_file());
    assert!(outside.is_file());
    assert!(inst.status(false).unwrap().pending_downloads.is_empty());
}

#[test]
fn every_transfer_is_in_the_ledger_before_it_starts() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    w.server.route(EMULATOR_ZIP_PATH, Route::Status(503));
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    let ledger = std::fs::read_to_string(dir.path().join("downloads/owned.json")).unwrap();
    assert!(ledger.contains("emulator-fixture.zip"), "{ledger}");
}

#[test]
fn a_damaged_ledger_does_not_stop_status_and_is_not_overwritten() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let ledger = dir.path().join("downloads/owned.json");
    std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
    std::fs::write(&ledger, b"someone else's file").unwrap();

    assert!(inst.status(false).unwrap().pending_downloads.is_empty());
    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::Json { .. }), "{err}");
    let err = inst.remove(RemoveTarget::Downloads, false).unwrap_err();
    assert!(matches!(err, Error::Json { .. }), "{err}");
    assert_eq!(std::fs::read(&ledger).unwrap(), b"someone else's file");
}

/// Runs an install that cancels itself on the first event `trigger` picks.
fn run_cancelling_on(
    installer: &Installer,
    w: &World,
    trigger: impl Fn(&Event) -> bool,
) -> mtp_sdk::Result<mtp_sdk::InstallReport> {
    let cancel = Cancel::new();
    let flag = cancel.clone();
    installer.install(
        &mut accept_all(w),
        InstallOptions::default(),
        &mut |event| {
            if trigger(&event) {
                flag.cancel();
            }
        },
        &cancel,
    )
}

#[test]
fn a_cancel_as_extraction_starts_lands_nothing_and_records_nothing() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    let err = run_cancelling_on(
        &inst,
        &w,
        |event| matches!(event, Event::Extract { name } if name == "emulator"),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");

    let root = SdkRoot::open(&Home::new(dir.path()), &w.manifest.set)
        .unwrap()
        .unwrap();
    assert!(root.record().packages.is_empty());
    assert_eq!(root.record().state, RootState::Installing);
    assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
    assert!(
        !dir.path().join("staging").exists(),
        "the hash of the zip stops before anything is unpacked"
    );
}

#[test]
fn a_cancel_during_a_link_lands_nothing_and_records_nothing() {
    let w = world();
    let dir = temp_dir();
    run(
        &installer(&dir, &w.server, &w.manifest),
        &mut accept_all(&w),
        InstallOptions::default(),
    )
    .unwrap();
    let next_manifest = serve_set(&w.server, "3", 20);
    let next = installer(&dir, &w.server, &next_manifest);
    let err = run_cancelling_on(
        &next,
        &w,
        |event| matches!(event, Event::Reuse { detail, .. } if detail.starts_with("linked from")),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");

    let root = SdkRoot::open(&Home::new(dir.path()), &next_manifest.set)
        .unwrap()
        .unwrap();
    assert!(root.record().packages.is_empty());
    assert!(!root_file(&dir, &next_manifest.set, "emulator").exists());
    assert_eq!(
        std::fs::read_dir(dir.path().join("staging"))
            .unwrap()
            .count(),
        0
    );
}

/// Serves both repository XMLs with this license text and only an unrelated package.
fn serve_xml_without_the_pins(w: &World, license_text: &str) {
    let xml = repository_xml(
        license_text,
        &[PackageSpec {
            path: "something-else",
            revision_xml: "<major>1</major>",
            channel: "channel-0",
            license: "android-sdk-license",
            url: "a.zip",
            size: 1,
            sha1: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    w.server.body(MAIN_XML_PATH, xml.clone().into_bytes());
    w.server.body(IMAGE_XML_PATH, xml.into_bytes());
}

#[test]
fn a_changed_license_text_asks_again_before_a_kept_zip_is_used() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    serve_license_text(&w, "Fixture license text, second edition\n");
    w.server.clear_requests();

    let mut dialog = Dialog {
        answer: Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        },
        shown: Vec::new(),
    };
    let (report, _) = run(&inst, &mut dialog, keep_zips()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license"]);
    assert_eq!(report.downloads, 0);
    assert!(w.server.requests().iter().all(|r| r.path.ends_with(".xml")));

    // Refusing to look at it stops the install too.
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    serve_license_text(&w, "Fixture license text, third edition\n");
    let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
    assert_eq!(err.exit_code(), 3, "{err}");
}

#[test]
fn a_changed_license_text_asks_again_before_another_roots_files_are_linked() {
    let w = world();
    let dir = temp_dir();
    run(
        &installer(&dir, &w.server, &w.manifest),
        &mut accept_all(&w),
        keep_zips(),
    )
    .unwrap();
    // Only the link is left to do once the image's zip is there too.
    let next_manifest = serve_set(&w.server, "3", 20);
    std::fs::write(
        dir.path().join("downloads/x86_64-r3.zip"),
        support::fixtures::package_zip("x86_64", "system.img", "3", 20),
    )
    .unwrap();
    let next = installer(&dir, &w.server, &next_manifest);
    assert!(matches!(
        next.plan(false).unwrap().packages[0].1,
        PackageAction::Link { .. }
    ));
    serve_xml_without_the_pins(&w, "Fixture license text, second edition\n");

    let mut dialog = Dialog {
        answer: Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        },
        shown: Vec::new(),
    };
    run(&next, &mut dialog, InstallOptions::default()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license"]);
}

#[test]
fn a_kept_zip_installs_when_the_xml_no_longer_lists_its_pin() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    serve_xml_without_the_pins(&w, LICENSE_BODY);

    let (report, events) = run(&inst, &mut no_flags(), keep_zips()).unwrap();
    assert_eq!(report.downloads, 0);
    assert!(
        events.contains(&Event::License {
            id: "android-sdk-license".into(),
            state: "accepted".into()
        }),
        "{events:?}"
    );
}

/// A kept zip's world after its set was removed, with the acceptance still on record.
fn removed_set_with_kept_zips(w: &World, dir: &TempDir) -> Installer {
    let inst = installer(dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(w), keep_zips()).unwrap();
    inst.remove(RemoveTarget::Set(w.manifest.set.clone()), false)
        .unwrap();
    w.server.clear_requests();
    inst
}

fn accept_dialog() -> Dialog {
    Dialog {
        answer: Decision::Accept {
            authority: Authority::Personal,
            via: Via::Gui,
        },
        shown: Vec::new(),
    }
}

#[test]
fn a_text_changed_in_the_one_xml_that_answered_is_still_asked_about() {
    let w = world();
    let dir = temp_dir();
    let inst = removed_set_with_kept_zips(&w, &dir);
    serve_license_text(&w, "Fixture license text, second edition\n");
    w.server.route(IMAGE_XML_PATH, Route::Status(503));

    let err = run(&inst, &mut accept_all(&w), keep_zips()).unwrap_err();
    assert!(matches!(err, Error::LicenseTextChanged { .. }), "{err}");

    let mut dialog = accept_dialog();
    let (report, events) = run(&inst, &mut dialog, keep_zips()).unwrap();
    assert_eq!(dialog.shown, ["android-sdk-license"]);
    assert_eq!(report.downloads, 0);
    let warned = warnings(&events);
    assert!(
        warned
            .iter()
            .any(|m| m.contains("503") && m.contains("127.0.0.1") && !m.contains("offline")),
        "{warned:?}"
    );
    assert!(
        !events.iter().any(|e| e.to_string().contains("offline")),
        "{events:?}"
    );
}

#[test]
fn a_server_that_answers_with_errors_is_not_an_offline_machine() {
    for status in [404, 503] {
        let w = world();
        let dir = temp_dir();
        let inst = removed_set_with_kept_zips(&w, &dir);
        w.server.route(MAIN_XML_PATH, Route::Status(status));
        w.server.route(IMAGE_XML_PATH, Route::Status(status));

        let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
        assert!(matches!(err, Error::Http { .. }), "{err}");
        assert_eq!(err.exit_code(), 1);
        assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
    }
}

#[test]
fn an_xml_that_is_not_xml_is_not_an_offline_machine() {
    let w = world();
    let dir = temp_dir();
    let inst = removed_set_with_kept_zips(&w, &dir);
    w.server
        .body(MAIN_XML_PATH, b"<html>a captive portal</html>".to_vec());
    w.server
        .body(IMAGE_XML_PATH, b"<html>a captive portal</html>".to_vec());

    let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
    assert!(matches!(err, Error::Xml(_)), "{err}");
    assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
}

#[test]
fn a_kept_zip_whose_pin_left_the_stable_channel_or_license_is_refused() {
    for (channel, license) in [
        ("channel-2", "android-sdk-license"),
        ("channel-0", "android-sdk-preview-license"),
    ] {
        let w = world();
        let dir = temp_dir();
        let inst = removed_set_with_kept_zips(&w, &dir);
        serve_pins(&w, LICENSE_BODY, channel, license);

        let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
        assert!(matches!(err, Error::NotStable { .. }), "{err}");
        assert_eq!(err.exit_code(), 5);
        assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
    }
}

#[test]
fn a_kept_zip_whose_pin_drifted_from_the_xml_is_refused() {
    let w = world();
    let dir = temp_dir();
    let inst = removed_set_with_kept_zips(&w, &dir);
    let pin = &w.manifest.packages[0];
    let xml = repository_xml(
        LICENSE_BODY,
        &[PackageSpec {
            path: &pin.path,
            revision_xml: &revision_xml(&pin.revision),
            channel: "channel-0",
            license: "android-sdk-license",
            url: pin.url.rsplit('/').next().unwrap(),
            size: pin.size + 1,
            sha1: pin.sha1.as_str(),
        }],
    );
    w.server.body(MAIN_XML_PATH, xml.into_bytes());

    let err = run(&inst, &mut no_flags(), keep_zips()).unwrap_err();
    assert!(
        matches!(&err, Error::PinMismatch { field, .. } if field == "size"),
        "{err}"
    );
    assert!(!root_file(&dir, &w.manifest.set, "emulator").exists());
}

#[test]
fn a_link_whose_pin_left_the_stable_channel_is_refused() {
    let w = world();
    let dir = temp_dir();
    run(
        &installer(&dir, &w.server, &w.manifest),
        &mut accept_all(&w),
        InstallOptions::default(),
    )
    .unwrap();
    let next_manifest = serve_set(&w.server, "3", 20);
    std::fs::write(
        dir.path().join("downloads/x86_64-r3.zip"),
        support::fixtures::package_zip("x86_64", "system.img", "3", 20),
    )
    .unwrap();
    let next = installer(&dir, &w.server, &next_manifest);
    assert!(matches!(
        next.plan(false).unwrap().packages[0].1,
        PackageAction::Link { .. }
    ));
    serve_pins(&w, LICENSE_BODY, "channel-2", "android-sdk-license");

    let err = run(&next, &mut no_flags(), InstallOptions::default()).unwrap_err();
    assert!(matches!(err, Error::NotStable { .. }), "{err}");
    assert!(!root_file(&dir, &next_manifest.set, "emulator").exists());
}

#[test]
fn a_kept_zip_used_after_a_failed_link_is_held_to_the_stable_rule_too() {
    let w = world();
    let dir = temp_dir();
    let first = installer(&dir, &w.server, &w.manifest);
    let (first_report, _) = run(&first, &mut accept_all(&w), keep_zips()).unwrap();
    flip_a_byte(&root_file(&dir, &first_report.set, "emulator/emulator.exe"));
    let next_manifest = serve_set(&w.server, "3", 20);
    let next = installer(&dir, &w.server, &next_manifest);
    assert!(matches!(
        next.plan(false).unwrap().packages[0].1,
        PackageAction::Link { .. }
    ));

    // The pin moves after the gate looked at it, and the link then fails on the damaged source.
    let err = next
        .install(
            &mut accept_all(&w),
            keep_zips(),
            &mut |event| {
                if matches!(event, Event::License { .. }) {
                    serve_pins(&w, LICENSE_BODY, "channel-2", "android-sdk-license");
                }
            },
            &Cancel::new(),
        )
        .unwrap_err();
    assert!(matches!(err, Error::NotStable { .. }), "{err}");
    assert!(!root_file(&dir, &next_manifest.set, "emulator").exists());
}

#[test]
fn a_junction_in_place_of_downloads_stops_everything_that_would_go_through_it() {
    let w = world();
    let dir = temp_dir();
    let outside = temp_dir();
    let asset = outside.path().join(&w.manifest.magisk.asset);
    let ledger = outside.path().join("owned.json");
    std::fs::write(&asset, b"mine").unwrap();
    std::fs::write(&ledger, format!("[\"{}\"]", w.manifest.magisk.asset)).unwrap();
    junction(&dir.path().join("downloads"), outside.path());
    let inst = installer(&dir, &w.server, &w.manifest);

    assert_refused_as_a_link(&inst.remove(RemoveTarget::Downloads, false).unwrap_err());
    assert_refused_as_a_link(&inst.status(false).unwrap_err());
    assert_refused_as_a_link(&run(&inst, &mut accept_all(&w), keep_zips()).unwrap_err());

    assert_eq!(w.server.request_count(), 0);
    assert_eq!(std::fs::read(&asset).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 2);
}

#[test]
fn a_junction_in_place_of_downloads_is_a_warning_once_the_set_is_ready() {
    let w = world();
    let dir = temp_dir();
    let outside = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

    let asset = outside.path().join(&w.manifest.magisk.asset);
    let ledger = outside.path().join("owned.json");
    std::fs::write(&asset, b"mine").unwrap();
    std::fs::write(&ledger, format!("[\"{}\"]", w.manifest.magisk.asset)).unwrap();
    let downloads = dir.path().join("downloads");
    std::fs::remove_dir(&downloads).unwrap();
    junction(&downloads, outside.path());

    let (report, events) = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap();
    assert_eq!(report.downloads, 0);
    let warned = warnings(&events);
    assert!(warned.iter().any(|m| m.contains("junction")), "{warned:?}");
    assert_eq!(std::fs::read(&asset).unwrap(), b"mine");
    assert!(ledger.is_file());

    // Magisk writes through the same folder, and the ready root doesn't excuse that.
    std::fs::remove_file(dir.path().join("tools/magisk/v1.0/magisk")).unwrap();
    let err = run(&inst, &mut no_flags(), InstallOptions::default()).unwrap_err();
    assert_refused_as_a_link(&err);
    assert_eq!(std::fs::read(&asset).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 2);
}

/// Moves a directory, retrying because an antivirus scan briefly holds fresh files open.
fn move_dir(from: &std::path::Path, to: &std::path::Path) {
    for _ in 0..40 {
        if std::fs::rename(from, to).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    std::fs::rename(from, to).unwrap();
}

#[test]
fn a_junction_in_place_of_the_root_or_the_sdk_folder_makes_the_root_unusable() {
    for replace_sdk in [false, true] {
        let w = world();
        let dir = temp_dir();
        let outside = temp_dir();
        let inst = installer(&dir, &w.server, &w.manifest);
        let (report, _) = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap();

        let sdk = dir.path().join("sdk");
        let link = if replace_sdk {
            sdk.clone()
        } else {
            sdk.join(report.set.as_str())
        };
        let moved = outside.path().join("moved");
        move_dir(&link, &moved);
        junction(&link, &moved);

        let home = Home::new(dir.path());
        let root = SdkRoot::open(&home, &report.set).unwrap().unwrap();
        assert!(!root.quick_check().unwrap().ok());
        assert!(!root.deep_check().unwrap().ok());
        assert!(home.sdk_root(&report.set).unwrap().is_none());

        let err = inst.plan(false).unwrap_err();
        assert!(matches!(err, Error::RootDamaged { .. }), "{err}");
        assert!(err.to_string().contains("junction"), "{err}");
        assert!(!inst.status(false).unwrap().ok);

        // Another set doesn't link from it either.
        let next_manifest = serve_set(&w.server, "3", 20);
        let next = installer(&dir, &w.server, &next_manifest);
        assert_eq!(
            next.plan(false).unwrap().packages[0].1,
            PackageAction::Download
        );
    }
}

#[test]
fn a_cancel_stops_the_hash_of_a_kept_zip_and_nothing_is_extracted() {
    let w = world();
    let payload = noise(64 << 20, 5);
    let zip = zip_of(&[
        ("emulator/source.properties", b"Pkg.Revision=37.2.13\n"),
        ("emulator/emulator.exe", payload.as_slice()),
    ]);
    let manifest = manifest_for_zip(&w, zip.clone());
    let dir = temp_dir();
    let downloads = dir.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join(manifest.packages[0].file_name()), zip).unwrap();
    let inst = installer(&dir, &w.server, &manifest);
    let cancelled = Cancel::new();
    cancelled.cancel();

    // Only the plan's hash of the zip can see this flag.
    let err = inst.plan_cancellable(false, &cancelled).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert_eq!(
        inst.plan(false).unwrap().packages[0].1,
        PackageAction::Extract
    );
    w.server.clear_requests();

    let started = Instant::now();
    let err = inst
        .install(
            &mut accept_all(&w),
            InstallOptions::default(),
            &mut |_| {},
            &cancelled,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(w.server.request_count(), 0);
    assert!(!dir.path().join("sdk").exists());
    assert!(!dir.path().join("staging").exists());
}

#[test]
fn a_cancel_stops_the_hash_of_files_in_a_resumed_root() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    crash_before_the_image(&w, &inst);
    let cancelled = Cancel::new();
    cancelled.cancel();
    let err = inst.plan_cancellable(false, &cancelled).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
}

#[test]
fn a_package_path_too_long_for_googles_binaries_is_refused_before_extraction() {
    let w = world();
    let long_entry = format!("emulator/{}", "p".repeat(200));
    let zip = zip_of(&[(long_entry.as_str(), noise(1024, 3).as_slice())]);
    let manifest = manifest_for_zip(&w, zip);
    let dir = temp_dir();
    let home = dir.path().join("h".repeat(30));
    let client = Client::new(SourcePolicy::loopback(w.server.port()))
        .with_retries(1, Duration::from_millis(10));
    let inst = Installer::new(Home::new(&home), manifest, client);

    let err = run(&inst, &mut accept_all(&w), InstallOptions::default()).unwrap_err();
    assert!(
        matches!(err, Error::HomePathTooLong { limit: 260, .. }),
        "{err}"
    );
    assert!(
        home.join("downloads/emulator-fixture.zip").is_file(),
        "the estimate before the download let this home through"
    );
    assert!(!home.join("staging").exists());
}

#[test]
fn a_complete_apk_part_counts_as_no_transfer() {
    let w = world();
    let dir = temp_dir();
    let inst = installer(&dir, &w.server, &w.manifest);
    run(&inst, &mut accept_all(&w), keep_zips()).unwrap();
    let apk = dir.path().join("downloads/Magisk-v1.0.apk");
    std::fs::rename(&apk, part_path(&apk)).unwrap();
    std::fs::remove_file(dir.path().join("tools/magisk/v1.0/magisk")).unwrap();
    w.server.clear_requests();

    let (report, events) = run(&inst, &mut no_flags(), keep_zips()).unwrap();
    assert_eq!(report.downloads, 0);
    assert_eq!(w.server.request_count(), 0);
    assert_eq!(events.last().unwrap().to_string(), "done     downloads=0");
    assert!(dir.path().join("tools/magisk/v1.0/magisk").is_file());
}
