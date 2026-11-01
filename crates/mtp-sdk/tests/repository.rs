//! The repository-XML reader against documents shaped like Google's.

mod support;

use mtp_sdk::hash::sha1_bytes;
use mtp_sdk::manifest::Manifest;
use mtp_sdk::{Error, Repository, SourcePolicy, check_pin};
use support::fixtures::{PackageSpec, repository_xml, revision_xml};

const XML_URL: &str = "https://dl.google.com/android/repository/sys-img/android/sys-img2-3.xml";
const SHA1_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn spec<'a>(
    path: &'a str,
    revision: &'a str,
    channel: &'a str,
    license: &'a str,
) -> PackageSpec<'a> {
    PackageSpec {
        path,
        revision_xml: revision,
        channel,
        license,
        url: "a.zip",
        size: 5,
        sha1: SHA1_A,
    }
}

fn parse(packages: &[PackageSpec<'_>]) -> Repository {
    Repository::parse(XML_URL, &repository_xml("Terms", packages)).unwrap()
}

#[test]
fn finds_the_windows_x64_archive_and_joins_its_relative_url() {
    let rev = revision_xml("37.2.13");
    let repo = parse(&[spec("emulator", &rev, "channel-0", "android-sdk-license")]);
    let archive = repo.find_archive("emulator", "37.2.13").unwrap();
    assert_eq!(
        archive.url,
        "https://dl.google.com/android/repository/sys-img/android/a.zip"
    );
    assert_eq!(archive.size, 5);
    assert_eq!(archive.sha1.as_str(), SHA1_A);
    assert_eq!(archive.channel, "channel-0");
}

#[test]
fn an_archive_without_host_elements_counts_as_windows() {
    let xml = r#"<r>
      <license id="android-sdk-license">T</license>
      <channel id="channel-0">stable</channel>
      <remotePackage path="system-images;android-36;default;x86_64">
        <revision><major>2</major></revision>
        <uses-license ref="android-sdk-license"/>
        <channelRef ref="channel-0"/>
        <archives><archive><complete><size>9</size>
          <checksum type="sha1">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</checksum><url>x.zip</url>
        </complete></archive></archives>
      </remotePackage></r>"#;
    let repo = Repository::parse(XML_URL, xml).unwrap();
    assert_eq!(
        repo.find_archive("system-images;android-36;default;x86_64", "2")
            .unwrap()
            .size,
        9
    );
}

#[test]
fn a_preview_channel_entry_is_not_stable() {
    let rev = revision_xml("38.0.0");
    let repo = parse(&[spec("emulator", &rev, "channel-2", "android-sdk-license")]);
    let err = repo.find_archive("emulator", "38.0.0").unwrap_err();
    assert!(matches!(err, Error::NotStable { .. }), "{err}");
    assert_eq!(err.exit_code(), 5);
    assert!(repo.stable_candidates("emulator").unwrap().is_empty());
}

#[test]
fn another_license_is_not_stable() {
    let rev = revision_xml("38.0.0");
    let repo = parse(&[spec(
        "emulator",
        &rev,
        "channel-0",
        "android-sdk-preview-license",
    )]);
    let err = repo.find_archive("emulator", "38.0.0").unwrap_err();
    assert!(matches!(err, Error::NotStable { .. }), "{err}");
}

#[test]
fn a_revision_marked_preview_is_not_stable_even_on_channel_0() {
    let rev = format!("{}<preview>1</preview>", revision_xml("38.0.0"));
    let repo = parse(&[spec("emulator", &rev, "channel-0", "android-sdk-license")]);
    assert!(matches!(
        repo.find_archive("emulator", "38.0.0"),
        Err(Error::NotStable { .. })
    ));
    assert!(repo.stable_candidates("emulator").unwrap().is_empty());
}

#[test]
fn a_document_that_renames_channel_0_has_no_stable_entries() {
    let xml = repository_xml(
        "T",
        &[spec(
            "emulator",
            &revision_xml("1.0.0"),
            "channel-0",
            "android-sdk-license",
        )],
    )
    .replace(
        r#"<channel id="channel-0">stable</channel>"#,
        r#"<channel id="channel-0">canary</channel>"#,
    );
    let repo = Repository::parse(XML_URL, &xml).unwrap();
    assert!(matches!(
        repo.find_archive("emulator", "1.0.0"),
        Err(Error::NotStable { .. })
    ));
}

#[test]
fn a_withdrawn_pin_is_reported_clearly() {
    let rev = revision_xml("37.2.13");
    let repo = parse(&[spec("emulator", &rev, "channel-0", "android-sdk-license")]);
    let err = repo.find_archive("emulator", "37.2.12").unwrap_err();
    assert!(matches!(err, Error::PinNotInRepository { .. }), "{err}");
    assert_eq!(err.exit_code(), 5);
    assert!(err.to_string().contains("keep working"), "{err}");
}

#[test]
fn any_drift_from_the_pin_is_refused() {
    let mut pin = Manifest::embedded().unwrap().packages.remove(0);
    pin.url = "https://dl.google.com/android/repository/sys-img/android/a.zip".to_string();
    pin.revision = "37.2.13".to_string();
    pin.size = 5;
    pin.sha1 = mtp_sdk::hash::Sha1Hex::parse(SHA1_A).unwrap();
    let rev = revision_xml("37.2.13");
    let repo = parse(&[spec("emulator", &rev, "channel-0", "android-sdk-license")]);
    let live = repo.find_archive("emulator", "37.2.13").unwrap();
    check_pin(&pin, &live).unwrap();

    let mut wrong = pin.clone();
    wrong.size = 6;
    let err = check_pin(&wrong, &live).unwrap_err();
    assert!(
        matches!(&err, Error::PinMismatch { field, .. } if field == "size"),
        "{err}"
    );

    let mut wrong = pin.clone();
    wrong.sha1 = mtp_sdk::hash::Sha1Hex::parse(&"b".repeat(40)).unwrap();
    let err = check_pin(&wrong, &live).unwrap_err();
    assert!(
        matches!(&err, Error::PinMismatch { field, .. } if field == "sha1"),
        "{err}"
    );

    let mut wrong = pin;
    wrong.url = "https://dl.google.com/android/repository/other.zip".to_string();
    let err = check_pin(&wrong, &live).unwrap_err();
    assert!(
        matches!(&err, Error::PinMismatch { field, .. } if field == "url"),
        "{err}"
    );
}

#[test]
fn a_checksum_type_this_release_does_not_know_is_a_clear_error() {
    let xml = repository_xml(
        "T",
        &[spec(
            "emulator",
            &revision_xml("1.0.0"),
            "channel-0",
            "android-sdk-license",
        )],
    )
    .replace(
        &format!(r#"<checksum type="sha1">{SHA1_A}"#),
        &format!(r#"<checksum type="sha256">{SHA1_A}"#),
    );
    let repo = Repository::parse(XML_URL, &xml).unwrap();
    let err = repo.find_archive("emulator", "1.0.0").unwrap_err();
    assert!(err.to_string().contains("sha256"), "{err}");
}

#[test]
fn the_license_hash_covers_the_untrimmed_text() {
    // A value computed outside this code: `printf 'abc' | sha1sum`.
    let xml = |body: &str| {
        repository_xml(
            body,
            &[spec(
                "e",
                "<major>1</major>",
                "channel-0",
                "android-sdk-license",
            )],
        )
    };
    let repo = Repository::parse(XML_URL, &xml("abc")).unwrap();
    let text = repo.license_text("android-sdk-license").unwrap();
    assert_eq!(
        text.sha1().as_str(),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    assert_eq!(text.text(), "abc");
    assert_eq!(text.source(), XML_URL);

    // Surrounding whitespace counts, entities decode, and CRLF normalizes as XML requires.
    let repo = Repository::parse(XML_URL, &xml(" a &amp; b\r\nc\n")).unwrap();
    let text = repo.license_text("android-sdk-license").unwrap();
    assert_eq!(text.text(), " a & b\nc\n");
    assert_eq!(*text.sha1(), sha1_bytes(b" a & b\nc\n"));
}

#[test]
fn a_missing_license_is_a_clear_error() {
    let repo = parse(&[spec(
        "e",
        "<major>1</major>",
        "channel-0",
        "android-sdk-license",
    )]);
    assert!(repo.license_text("nope").is_err());
}

#[test]
fn a_changed_format_fails_with_a_message() {
    for text in [
        "<not-closed>",
        "<r></r>",
        "<r><license id='x'>T</license></r>",
        "no xml here",
    ] {
        assert!(
            matches!(Repository::parse(XML_URL, text), Err(Error::Xml(_))),
            "{text}"
        );
    }
}

#[test]
fn a_dtd_is_refused() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x "boom">]><r><license id="a">&x;</license></r>"#;
    assert!(matches!(
        Repository::parse(XML_URL, xml),
        Err(Error::Xml(_))
    ));
}

#[test]
fn the_embedded_manifest_keeps_its_urls_under_the_production_prefix() {
    let manifest = Manifest::embedded().unwrap();
    let prefix = SourcePolicy::production().google_prefix();
    assert!(manifest.packages.iter().all(|p| p.url.starts_with(&prefix)));
}

#[test]
fn a_malformed_entry_stops_only_a_lookup_of_that_package() {
    let rev = revision_xml("37.2.13");
    let xml = repository_xml(
        "T",
        &[spec("emulator", &rev, "channel-0", "android-sdk-license")],
    )
    .replace(
        "</sdk:sdk-repository>",
        r#"<remotePackage path="no-checksum">
             <revision><major>1</major></revision>
             <archives><archive><complete><size>9</size><url>x.zip</url></complete></archive></archives>
           </remotePackage>
           <remotePackage path="no-size">
             <revision><major>1</major></revision>
             <archives><archive><complete>
               <checksum type="sha1">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</checksum><url>x.zip</url>
             </complete></archive></archives>
           </remotePackage>
           <remotePackage path="no-revision">
             <archives><archive><complete><size>9</size>
               <checksum type="sha1">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</checksum><url>x.zip</url>
             </complete></archive></archives>
           </remotePackage>
           <remotePackage><revision><major>1</major></revision></remotePackage>
         </sdk:sdk-repository>"#,
    );
    let repo = Repository::parse(XML_URL, &xml).unwrap();

    assert_eq!(repo.find_archive("emulator", "37.2.13").unwrap().size, 5);
    assert_eq!(repo.stable_candidates("emulator").unwrap().len(), 1);

    for (path, needle) in [
        ("no-checksum", "checksum"),
        ("no-size", "size"),
        ("no-revision", "revision"),
    ] {
        for err in [
            repo.find_archive(path, "1").unwrap_err(),
            repo.stable_candidates(path).unwrap_err(),
        ] {
            assert!(matches!(err, Error::Xml(_)), "{path}: {err}");
            assert!(err.to_string().contains(needle), "{path}: {err}");
        }
    }

    // Another revision of a broken package is simply not listed.
    assert!(matches!(
        repo.find_archive("no-checksum", "2"),
        Err(Error::PinNotInRepository { .. })
    ));
}

#[test]
fn a_stray_entry_with_no_revision_does_not_hide_the_pin_beside_it() {
    let rev = revision_xml("37.2.13");
    let xml = repository_xml(
        "T",
        &[spec("emulator", &rev, "channel-0", "android-sdk-license")],
    )
    .replace(
        "</sdk:sdk-repository>",
        r#"<remotePackage path="emulator"></remotePackage></sdk:sdk-repository>"#,
    );
    let repo = Repository::parse(XML_URL, &xml).unwrap();
    assert_eq!(repo.find_archive("emulator", "37.2.13").unwrap().size, 5);
    assert!(matches!(
        repo.find_archive("emulator", "38.0.0"),
        Err(Error::Xml(_))
    ));
}

#[test]
fn a_document_of_nothing_but_unnamed_packages_is_still_a_changed_format() {
    let xml = r#"<r><license id="a">T</license><remotePackage/></r>"#;
    assert!(matches!(
        Repository::parse(XML_URL, xml),
        Err(Error::Xml(_))
    ));
}
