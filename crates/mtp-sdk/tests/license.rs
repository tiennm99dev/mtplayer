//! The license gate's records and states.

mod support;

use std::collections::BTreeMap;

use mtp_sdk::hash::{Sha1Hex, sha1_bytes};
use mtp_sdk::{
    Authority, Decision, Error, LicensePrompt, LicenseState, LicenseStore, LicenseText,
    PreAccepted, Repository, Via, gpl3_text,
};
use support::fixtures::repository_xml;
use support::temp_dir;

const XML_URL: &str = "https://dl.google.com/android/repository/repository2-3.xml";

fn text_of(body: &str) -> LicenseText {
    let xml = repository_xml(
        body,
        &[support::fixtures::PackageSpec {
            path: "emulator",
            revision_xml: "<major>1</major>",
            channel: "channel-0",
            license: "android-sdk-license",
            url: "a.zip",
            size: 1,
            sha1: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }],
    );
    Repository::parse(XML_URL, &xml)
        .unwrap()
        .license_text("android-sdk-license")
        .unwrap()
}

fn pre_accepted(id: &str, sha1: &Sha1Hex, authority: Authority) -> PreAccepted {
    PreAccepted::new(BTreeMap::from([(id.to_string(), sha1.clone())]), authority)
}

#[test]
fn a_new_store_has_nothing_accepted() {
    let dir = temp_dir();
    let store = LicenseStore::new(dir.path());
    assert_eq!(store.state(&text_of("Terms")), LicenseState::NotAccepted);
    assert!(store.current("android-sdk-license").is_none());
}

#[test]
fn a_record_holds_the_hash_the_time_the_authority_and_the_whole_text() {
    let dir = temp_dir();
    let store = LicenseStore::new(dir.path());
    let text = text_of("Terms and Conditions\n");
    let record = store
        .record(&text, Authority::Employer, Via::Prompt)
        .unwrap();

    assert_eq!(record.text_sha1, sha1_bytes(b"Terms and Conditions\n"));
    assert_eq!(record.text, "Terms and Conditions\n");
    assert_eq!(record.text_source, XML_URL);
    assert_eq!(record.authority, Authority::Employer);
    assert_eq!(record.via, Via::Prompt);
    assert!(record.accepted_at.ends_with('Z'), "{}", record.accepted_at);
    assert_eq!(record.mtplayer_version, env!("CARGO_PKG_VERSION"));

    let file = std::fs::read_to_string(dir.path().join("android-sdk-license.json")).unwrap();
    assert!(file.ends_with('\n'));
    assert!(!file.contains('\r'));
    let json: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(json["license_id"], "android-sdk-license");
    assert_eq!(json["authority"], "employer");
    assert_eq!(json["via"], "prompt");

    assert!(matches!(store.state(&text), LicenseState::Accepted(_)));
}

#[test]
fn a_changed_text_asks_again_and_the_old_record_is_kept() {
    let dir = temp_dir();
    let store = LicenseStore::new(dir.path());
    let old = text_of("Old terms");
    store
        .record(&old, Authority::Personal, Via::Prompt)
        .unwrap();

    let new = text_of("New terms");
    assert!(matches!(store.state(&new), LicenseState::TextChanged(_)));
    store.record(&new, Authority::Personal, Via::Flag).unwrap();
    assert!(matches!(store.state(&new), LicenseState::Accepted(_)));
    assert!(matches!(store.state(&old), LicenseState::TextChanged(_)));

    let kept = dir
        .path()
        .join(format!("android-sdk-license.{}.json", old.sha1().prefix()));
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(kept).unwrap()).unwrap();
    assert_eq!(json["text"], "Old terms");
}

#[test]
fn recording_the_same_text_again_leaves_no_history_file() {
    let dir = temp_dir();
    let store = LicenseStore::new(dir.path());
    let text = text_of("Terms");
    store
        .record(&text, Authority::Personal, Via::Prompt)
        .unwrap();
    store
        .record(&text, Authority::Employer, Via::Prompt)
        .unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_damaged_or_hand_edited_record_counts_as_not_accepted() {
    let dir = temp_dir();
    let store = LicenseStore::new(dir.path());
    let text = text_of("Terms");
    store
        .record(&text, Authority::Personal, Via::Prompt)
        .unwrap();
    let path = dir.path().join("android-sdk-license.json");

    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"text\": \"Terms\"", "\"text\": \"Other\"");
    std::fs::write(&path, edited).unwrap();
    assert_eq!(store.state(&text), LicenseState::NotAccepted);

    std::fs::write(&path, "not json").unwrap();
    assert_eq!(store.state(&text), LicenseState::NotAccepted);
}

#[test]
fn the_right_hash_accepts_with_the_stated_authority() {
    let text = text_of("Terms");
    let mut prompt = pre_accepted("android-sdk-license", text.sha1(), Authority::Employer);
    assert!(prompt.available("android-sdk-license"));
    assert_eq!(
        prompt.decide(&text, None).unwrap(),
        Decision::Accept {
            authority: Authority::Employer,
            via: Via::Flag
        }
    );
}

#[test]
fn a_wrong_hash_is_refused_naming_only_the_hash_given() {
    let text = text_of("Terms");
    let stale = sha1_bytes(b"Older terms");
    let mut prompt = pre_accepted("android-sdk-license", &stale, Authority::Personal);
    let err = prompt.decide(&text, None).unwrap_err();
    assert!(matches!(err, Error::LicenseTextChanged { .. }), "{err}");
    assert_eq!(err.exit_code(), 3);
    let message = err.to_string();
    assert!(message.contains(stale.as_str()), "{message}");
    assert!(
        !message.contains(text.sha1().as_str()),
        "the live hash comes only together with its text: {message}"
    );
    assert!(message.contains("--text"), "{message}");
}

#[test]
fn an_unlisted_license_is_not_accepted() {
    let text = text_of("Terms");
    let mut prompt = pre_accepted("gpl-3.0", text.sha1(), Authority::Personal);
    assert!(!prompt.available("android-sdk-license"));
    let err = prompt.decide(&text, None).unwrap_err();
    assert!(matches!(err, Error::LicenseNotAccepted { .. }), "{err}");
}

#[test]
fn the_gpl_text_is_magisks_license_byte_for_byte() {
    let text = gpl3_text();
    assert_eq!(text.id(), "gpl-3.0");
    assert_eq!(text.text().len(), 35_141);
    assert_eq!(
        mtp_sdk::license::gpl3_sha256().as_str(),
        "589ed823e9a84c56feb95ac58e7cf384626b9cbf4fda2a907bc36e103de1bad2"
    );
}
