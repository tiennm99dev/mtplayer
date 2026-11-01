//! Tests against the real dl.google.com and GitHub. They are ignored by default, and their
//! transfers stay small: the 8 MB platform-tools zip, which the product itself doesn't pin,
//! and a 64 KiB range of the Magisk asset. Run them with
//! `cargo test -p mtp-sdk --test live -- --ignored`.

mod support;

use mtp_sdk::download::{DownloadRequest, part_path};
use mtp_sdk::hash::{Digest, Sha1Hex};
use mtp_sdk::manifest::Manifest;
use mtp_sdk::{
    ANDROID_SDK_LICENSE, Cancel, Client, Error, Event, Repository, SourcePolicy, check_pin,
};
use support::temp_dir;

const PLATFORM_TOOLS_URL: &str =
    "https://dl.google.com/android/repository/platform-tools_r37.0.1-win.zip";
const PLATFORM_TOOLS_SIZE: u64 = 8_044_989;
const PLATFORM_TOOLS_SHA1: &str = "e03e78b1d80b396f1c3358e31251cb31740e1110";
const MAX_XML: u64 = 16 << 20;

fn client() -> Client {
    Client::new(SourcePolicy::production())
}

fn platform_tools(dest: &std::path::Path) -> DownloadRequest {
    DownloadRequest {
        url: PLATFORM_TOOLS_URL.to_string(),
        dest: dest.to_path_buf(),
        size: PLATFORM_TOOLS_SIZE,
        digest: Digest::Sha1(Sha1Hex::parse(PLATFORM_TOOLS_SHA1).unwrap()),
    }
}

#[test]
#[ignore = "talks to dl.google.com"]
fn every_pin_still_matches_google_and_the_license_hash_is_the_reviewed_one() {
    let manifest = Manifest::embedded().unwrap();
    let client = client();
    let reviewed = manifest.licenses[ANDROID_SDK_LICENSE]
        .reviewed_text_sha1
        .clone()
        .unwrap();
    for pin in &manifest.packages {
        let text = client.get_text(&pin.repository_xml, MAX_XML).unwrap();
        let repository = Repository::parse(&pin.repository_xml, &text).unwrap();
        let live = repository.find_archive(&pin.path, &pin.revision).unwrap();
        check_pin(pin, &live).unwrap();
        let license = repository.license_text(ANDROID_SDK_LICENSE).unwrap();
        assert_eq!(*license.sha1(), reviewed, "{}", pin.repository_xml);
    }
}

#[test]
#[ignore = "downloads the 8 MB platform-tools zip from dl.google.com"]
fn a_cancelled_download_resumes_with_a_ranged_request_and_verifies() {
    let dir = temp_dir();
    let dest = dir.path().join("platform-tools.zip");
    let request = platform_tools(&dest);
    let client = client();

    let cancel = Cancel::new();
    let trigger = cancel.clone();
    let err = client
        .download(
            &request,
            &mut move |event| {
                if matches!(event, Event::Progress { received, .. } if received >= 2 << 20) {
                    trigger.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let kept = std::fs::metadata(part_path(&dest)).unwrap().len();
    assert!(
        (2 << 20..PLATFORM_TOOLS_SIZE).contains(&kept),
        "kept {kept}"
    );

    // The server must really answer the range with a 206 for the pinned file.
    let mut events = Vec::new();
    let done = client
        .download(&request, &mut |event| events.push(event), &Cancel::new())
        .unwrap();
    assert!(!done.reused);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Download { resume_from: Some(from), .. } if *from == kept)),
        "{events:?}"
    );
    let first_progress = events.iter().find_map(|e| match e {
        Event::Progress { received, .. } => Some(*received),
        _ => None,
    });
    assert!(
        first_progress.unwrap() > kept,
        "the transfer continued from the part"
    );
    assert_eq!(std::fs::metadata(&dest).unwrap().len(), PLATFORM_TOOLS_SIZE);
    assert!(events.iter().any(|e| matches!(e, Event::Verified { .. })));

    // A verified copy needs no request.
    let again = client
        .download(&request, &mut |_| {}, &Cancel::new())
        .unwrap();
    assert!(again.reused);
}

#[test]
#[ignore = "downloads the 8 MB platform-tools zip from dl.google.com"]
fn a_corrupted_download_fails_the_hash_check_before_any_extraction() {
    let dir = temp_dir();
    let dest = dir.path().join("platform-tools.zip");
    let request = platform_tools(&dest);
    let client = client();
    client
        .download(&request, &mut |_| {}, &Cancel::new())
        .unwrap();

    // Flip one byte and put the file back as a complete part, as a run would find it after
    // the disk or a third party altered it.
    let mut bytes = std::fs::read(&dest).unwrap();
    bytes[1_000_000] ^= 0x01;
    std::fs::write(part_path(&dest), &bytes).unwrap();
    std::fs::remove_file(&dest).unwrap();

    let err = client
        .download(&request, &mut |_| {}, &Cancel::new())
        .unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert!(err.to_string().contains(PLATFORM_TOOLS_SHA1), "{err}");
    assert!(!dest.exists(), "a rejected file can't reach extraction");
    assert!(!part_path(&dest).exists());
}

#[test]
#[ignore = "reads 64 KiB of the Magisk asset from GitHub"]
fn the_magisk_redirect_chain_ends_on_an_allowed_host() {
    let manifest = Manifest::embedded().unwrap();
    let dir = temp_dir();
    let dest = dir.path().join(&manifest.magisk.asset);
    // A part one 64 KiB block short of complete asks for only the last block, so the chain
    // is walked for real and the production policy checks each hop.
    let bytes = vec![0u8; (manifest.magisk.size - 64 * 1024) as usize];
    std::fs::write(part_path(&dest), &bytes).unwrap();

    let request = DownloadRequest {
        url: manifest.magisk.url.clone(),
        dest: dest.clone(),
        size: manifest.magisk.size,
        digest: Digest::Sha256(manifest.magisk.sha256.clone()),
    };
    let mut events = Vec::new();
    let err = client()
        .download(&request, &mut |event| events.push(event), &Cancel::new())
        .unwrap_err();
    // The zeros fail the hash, which proves the last block arrived through every hop.
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    let last = events.iter().rev().find_map(|e| match e {
        Event::Progress { received, .. } => Some(*received),
        _ => None,
    });
    assert_eq!(last, Some(manifest.magisk.size));
    let mut printed = String::new();
    for event in &events {
        printed.push_str(&event.to_string());
    }
    assert!(
        !printed.contains('?'),
        "signed URLs never reach output: {printed}"
    );
}
