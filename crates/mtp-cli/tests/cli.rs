//! Runs the `mtp` binary without a terminal and without any network request. Every home is
//! a temporary folder under the repository's `.tmp`, never the real MTPlayer folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn temp_home() -> TempDir {
    let base = workspace().join(".tmp").join("test");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

fn mtp(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mtp"))
        .arg("--home")
        .arg(home)
        .arg("--manifest")
        .arg(workspace().join("packages").join("manifest.json"))
        .args(args)
        .env_remove("MTPLAYER_HOME")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn is_empty_dir(path: &Path) -> bool {
    std::fs::read_dir(path).unwrap().next().is_none()
}

#[test]
fn status_on_an_empty_home_succeeds_and_creates_nothing() {
    let home = temp_home();
    let output = mtp(home.path(), &["sdk", "status"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("(--home)"), "{text}");
    assert!(text.contains("set 37.2.13-36r2-4d7268a-829c076"), "{text}");
    assert!(
        text.contains("license  android-sdk-license not accepted"),
        "{text}"
    );
    assert!(text.contains("license  gpl-3.0 not accepted"), "{text}");
    assert!(text.contains("root     none installed"), "{text}");
    assert!(text.contains("magisk   v31.1 not installed"), "{text}");
    assert!(is_empty_dir(home.path()));
}

#[test]
fn status_json_can_be_parsed() {
    let home = temp_home();
    let output = mtp(home.path(), &["sdk", "status", "--json"]);
    assert_eq!(output.status.code(), Some(0));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["set"], "37.2.13-36r2-4d7268a-829c076");
    assert_eq!(value["ok"], true);
}

#[test]
fn install_without_a_terminal_or_acceptance_exits_3_and_downloads_nothing() {
    let home = temp_home();
    let output = mtp(home.path(), &["sdk", "install"]);
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains("license android-sdk-license is not accepted"),
        "{message}"
    );
    assert!(
        message.contains("--accept android-sdk-license=<sha1>"),
        "{message}"
    );
    assert!(!home.path().join("downloads").exists());
    assert!(!home.path().join("sdk").exists());
    assert!(!home.path().join("licenses").exists());
}

#[test]
fn accept_for_only_one_license_still_refuses_before_any_request() {
    let home = temp_home();
    let accept = format!("gpl-3.0={}", mtp_sdk::gpl3_text().sha1());
    let output = mtp(
        home.path(),
        &[
            "sdk",
            "install",
            "--accept",
            &accept,
            "--on-behalf-of",
            "personal",
        ],
    );
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));
    assert!(stderr(&output).contains("android-sdk-license"));
    assert!(!home.path().join("downloads").exists());
}

#[test]
fn accept_without_an_authority_is_a_usage_error() {
    let home = temp_home();
    let accept = format!("android-sdk-license={}", "a".repeat(40));
    let output = mtp(home.path(), &["sdk", "install", "--accept", &accept]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("--on-behalf-of"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_authority_without_accept_is_a_usage_error() {
    let home = temp_home();
    let output = mtp(
        home.path(),
        &["sdk", "install", "--on-behalf-of", "employer"],
    );
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
}

#[test]
fn malformed_and_unknown_accept_values_are_usage_errors() {
    let home = temp_home();
    for value in ["android-sdk-license", "android-sdk-license=abc"] {
        let output = mtp(
            home.path(),
            &[
                "sdk",
                "install",
                "--accept",
                value,
                "--on-behalf-of",
                "personal",
            ],
        );
        assert_eq!(output.status.code(), Some(2), "{value}");
    }
    let unknown = format!("some-other-license={}", "a".repeat(40));
    let output = mtp(
        home.path(),
        &[
            "sdk",
            "install",
            "--accept",
            &unknown,
            "--on-behalf-of",
            "personal",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn there_is_no_way_to_skip_the_gate() {
    let home = temp_home();
    for flag in ["--yes", "--accept-all", "-y"] {
        let output = mtp(home.path(), &["sdk", "install", flag]);
        assert_eq!(output.status.code(), Some(2), "{flag}");
    }
}

#[test]
fn remove_needs_exactly_one_target() {
    let home = temp_home();
    assert_eq!(mtp(home.path(), &["sdk", "remove"]).status.code(), Some(2));
    assert_eq!(
        mtp(home.path(), &["sdk", "remove", "--unused", "--downloads"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        mtp(home.path(), &["sdk", "remove", "../evil"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn remove_of_a_set_that_is_not_there_reports_nothing_to_do() {
    let home = temp_home();
    let output = mtp(
        home.path(),
        &["sdk", "remove", "37.2.13-36r2-4d7268a-829c076"],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("nothing to remove"));
}

#[test]
fn a_held_lock_exits_4() {
    let home = temp_home();
    let _lock = mtp_sdk::Home::new(home.path()).lock().unwrap();
    let output = mtp(home.path(), &["sdk", "remove", "--unused"]);
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    assert!(stderr(&output).contains("mtp-sdk.lock"));
}

#[test]
fn the_gpl_text_and_hash_come_from_this_release() {
    let home = temp_home();
    let output = mtp(home.path(), &["sdk", "license", "gpl-3.0", "--text"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains(mtp_sdk::gpl3_text().sha1().as_str()));
    assert!(text.contains("GNU GENERAL PUBLIC LICENSE"));

    let short = stdout(&mtp(home.path(), &["sdk", "license", "gpl-3.0"]));
    assert!(short.contains("sha1"));
    assert!(!short.contains("GNU GENERAL PUBLIC LICENSE"));
}

fn has_a_sha1(text: &str) -> bool {
    text.split(|c: char| !c.is_ascii_hexdigit())
        .any(|token| token.len() == 40)
}

#[test]
fn the_hash_comes_only_together_with_the_text_it_stands_for() {
    let home = temp_home();
    let short = stdout(&mtp(home.path(), &["sdk", "license", "gpl-3.0"]));
    assert!(!has_a_sha1(&short), "{short}");
    assert!(short.contains("--text"), "{short}");

    let full = stdout(&mtp(home.path(), &["sdk", "license", "gpl-3.0", "--text"]));
    assert!(has_a_sha1(&full));
    assert!(full.contains("GNU GENERAL PUBLIC LICENSE"));
}

#[test]
fn a_set_id_naming_another_roots_leases_is_a_usage_error() {
    let home = temp_home();
    let set = "37.2.13-36r2-4d7268a-829c076";
    let leases = home.path().join("sdk").join(format!("{set}.leases"));
    std::fs::create_dir_all(&leases).unwrap();
    std::fs::write(leases.join("instance-a"), b"{}").unwrap();
    for set in [format!("{set}.leases"), format!("{set}.json")] {
        let set = set.as_str();
        let output = mtp(home.path(), &["sdk", "remove", set]);
        assert_eq!(output.status.code(), Some(2), "{set}");
    }
    assert!(leases.join("instance-a").is_file());
}

#[test]
fn an_acknowledgement_with_no_authority_shows_as_not_stated() {
    let home = temp_home();
    mtp_sdk::Home::new(home.path())
        .licenses()
        .record(
            &mtp_sdk::gpl3_text(),
            mtp_sdk::Authority::NotStated,
            mtp_sdk::Via::Prompt,
        )
        .unwrap();
    let output = mtp(home.path(), &["sdk", "status"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(stdout(&output).contains("authority not stated"));

    let json = mtp(home.path(), &["sdk", "status", "--json"]);
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["licenses"][1]["authority"], "notstated");
    assert_eq!(value["licenses"][1]["state"], "accepted");
}

#[test]
fn an_unreadable_root_is_listed_and_can_be_removed() {
    let home = temp_home();
    let sdk = home.path().join("sdk");
    std::fs::create_dir_all(sdk.join("1.0.0-1r1")).unwrap();
    std::fs::write(sdk.join("1.0.0-1r1.json"), b"{").unwrap();

    let output = mtp(home.path(), &["sdk", "status"]);
    assert_eq!(output.status.code(), Some(1), "a check failed");
    let text = stdout(&output);
    assert!(text.contains("1.0.0-1r1 unreadable"), "{text}");
    assert!(text.contains("mtp sdk remove 1.0.0-1r1"), "{text}");

    let output = mtp(home.path(), &["sdk", "remove", "1.0.0-1r1"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(!sdk.join("1.0.0-1r1").exists());
    assert_eq!(mtp(home.path(), &["sdk", "status"]).status.code(), Some(0));
}

#[test]
fn an_unknown_license_is_a_usage_error() {
    let home = temp_home();
    let output = mtp(home.path(), &["sdk", "license", "nope"]);
    assert_eq!(output.status.code(), Some(2));
}

/// Records every pinned Google package as installed, so that only the GPL's gate is left and
/// the run needs no network.
fn write_ready_root(home: &Path) {
    let manifest = mtp_sdk::Manifest::embedded().unwrap();
    let packages: Vec<serde_json::Value> = manifest
        .packages
        .iter()
        .map(|pin| {
            serde_json::json!({
                "path": pin.path,
                "revision": pin.revision,
                "zip_sha1": pin.sha1.as_str(),
                "install_dir": pin.install_dir(),
                "installed_at": "2026-11-01T00:00:00Z",
                "dirs": [],
                "files": [],
            })
        })
        .collect();
    let record = serde_json::json!({
        "schema": 1,
        "set": manifest.set.as_str(),
        "state": "ready",
        "packages": packages,
    });
    let sdk = home.join("sdk");
    std::fs::create_dir_all(sdk.join(manifest.set.as_str()).join("platform-tools")).unwrap();
    std::fs::write(
        sdk.join(format!("{}.json", manifest.set.as_str())),
        record.to_string(),
    )
    .unwrap();
}

#[test]
fn a_wrong_accept_never_reveals_the_live_hash() {
    let home = temp_home();
    write_ready_root(home.path());
    let wrong = "a".repeat(40);
    let accept_google = format!("android-sdk-license={wrong}");
    let accept_gpl = format!("gpl-3.0={wrong}");
    let output = mtp(
        home.path(),
        &[
            "sdk",
            "install",
            "--accept",
            &accept_google,
            "--accept",
            &accept_gpl,
            "--on-behalf-of",
            "personal",
        ],
    );
    assert_eq!(output.status.code(), Some(3), "{}", stderr(&output));

    let combined = format!("{}{}", stdout(&output), stderr(&output));
    assert!(
        !combined.contains(mtp_sdk::gpl3_text().sha1().as_str()),
        "{combined}"
    );
    assert!(
        combined.contains("mtp sdk license gpl-3.0 --text"),
        "{combined}"
    );
    assert!(!home.path().join("downloads").exists());
    assert!(!home.path().join("licenses").exists());
}
