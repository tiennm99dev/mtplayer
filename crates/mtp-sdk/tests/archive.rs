//! Every zip rule has a fixture that breaks it.

mod support;

use std::fs::File;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use mtp_sdk::archive::{self, ArchivePlan, Compared, ExtractLimits};
use mtp_sdk::{Cancel, Error};
use support::fixtures::{deflated, noise, package_zip, stored, zip_of};
use support::temp_dir;
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

fn scan(zip: &Path, limits: ExtractLimits) -> mtp_sdk::Result<ArchivePlan> {
    archive::scan(&File::open(zip).unwrap(), limits)
}

fn extract_into_new_dir(zip: &Path, plan: &ArchivePlan, dest: &Path) -> mtp_sdk::Result<()> {
    archive::extract_into_new_dir(&File::open(zip).unwrap(), plan, dest, &Cancel::new())
}

fn compare_with_zip(plan: &ArchivePlan, dir: &Path) -> mtp_sdk::Result<Compared> {
    archive::compare_with_zip(plan, dir, &Cancel::new())
}

fn extract_member(zip: &Path, member: &str, dest: &Path) -> mtp_sdk::Result<PathBuf> {
    archive::extract_member(&File::open(zip).unwrap(), member, dest)
}

fn write_zip(dir: &TempDir, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join("fixture.zip");
    std::fs::write(&path, bytes).unwrap();
    path
}

fn scan_bytes(bytes: &[u8]) -> mtp_sdk::Result<mtp_sdk::archive::ArchivePlan> {
    let dir = temp_dir();
    let path = write_zip(&dir, bytes);
    scan(
        &path,
        ExtractLimits {
            max_entries: 100,
            max_ratio: 1000,
        },
    )
}

fn assert_unsafe(names: &[&str]) {
    for name in names {
        let bytes = zip_of(&[(name, b"data")]);
        let err = scan_bytes(&bytes).unwrap_err();
        assert!(matches!(err, Error::UnsafeArchive { .. }), "{name}: {err}");
    }
}

#[test]
fn a_well_formed_package_scans_with_its_top_directory() {
    let plan = scan_bytes(&package_zip("emulator", "emulator.exe", "37.2.13", 1)).unwrap();
    assert_eq!(plan.top_dir, "emulator");
    assert!(plan.total > 90_000);
}

#[test]
fn traversal_and_absolute_names_are_refused() {
    assert_unsafe(&[
        "../evil",
        "top/../../evil",
        "/abs/evil",
        "top/./x",
        "top//x",
    ]);
}

#[test]
fn backslashes_and_drive_paths_are_refused() {
    assert_unsafe(&["top\\evil", "top\\..\\..\\evil", "C:/evil", "C:\\evil"]);
}

#[test]
fn names_windows_would_change_silently_are_refused() {
    assert_unsafe(&[
        "top/foo:bar",
        "top/file.txt:stream",
        "top/a<b",
        "top/a>b",
        "top/a\"b",
        "top/a|b",
        "top/a?b",
        "top/a*b",
        "top/trailing.",
        "top/trailing ",
        "top/ctrl\u{1}x",
    ]);
}

#[test]
fn reserved_device_names_are_refused_with_or_without_an_extension() {
    assert_unsafe(&[
        "top/CON",
        "top/con.txt",
        "top/PRN.log",
        "top/AUX",
        "top/nul.tar.gz",
        "top/COM1",
        "top/com9.txt",
        "top/LPT1",
        "top/lpt9.dat",
        "CON.txt",
    ]);
}

#[test]
fn names_that_merely_resemble_device_names_are_allowed() {
    let names: [&str; 5] = [
        "top/CONSOLE",
        "top/COM0.txt",
        "top/LPT.dat",
        "top/nullable",
        "top/aux2",
    ];
    for name in names {
        let bytes = zip_of(&[(name, noise(100, 1).as_slice())]);
        scan_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn a_nul_is_refused() {
    let bytes = zip_of(&[("top/evil\0.txt", b"data")]);
    assert!(matches!(
        scan_bytes(&bytes),
        Err(Error::UnsafeArchive { .. })
    ));
}

#[test]
fn an_overlong_name_is_refused() {
    let name = format!("top/{}", "a".repeat(1100));
    assert_unsafe(&[&name]);
}

#[test]
fn a_component_over_the_ntfs_limit_is_refused_in_utf16_units() {
    // Both fit the byte limit on the whole name. Three-byte characters show that the
    // component limit counts UTF-16 units, as NTFS does.
    let ascii = format!("top/{}", "a".repeat(256));
    let wide = format!("top/{}", "\u{20ac}".repeat(256));
    assert_unsafe(&[&ascii, &wide]);

    let longest = format!("top/{}", "\u{20ac}".repeat(255));
    scan_bytes(&zip_of(&[(&longest, b"data".as_slice())])).unwrap();
}

#[test]
fn every_device_name_spelling_windows_honours_is_refused() {
    assert_unsafe(&[
        "top/CONIN$",
        "top/conout$.txt",
        "top/CLOCK$",
        "top/COM\u{b9}",
        "top/com\u{b2}.txt",
        "top/LPT\u{b3}",
        // Win32 drops the spaces before the extension, so this still opens the console.
        "top/CON .txt",
        "top/com1 .log",
    ]);
}

/// Names whose byte 3 falls inside a multi-byte character, in the shapes that reach a
/// device-name check: a component, a stem before an extension, and the top directory.
const MULTI_BYTE_NAMES: [&str; 5] = [
    "top/ab\u{20ac}x",
    "top/ab\u{20ac}x.txt",
    "top/a\u{e9}\u{e9}/data.bin",
    "top/COM\u{e9}",
    "ab\u{20ac}x/data.bin",
];

#[test]
fn a_multi_byte_character_across_byte_three_scans_without_a_panic() {
    for name in MULTI_BYTE_NAMES {
        let bytes = zip_of(&[(name, b"data".as_slice())]);
        scan_bytes(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn a_multi_byte_name_extracts_and_compares_exactly() {
    for name in MULTI_BYTE_NAMES {
        let dir = temp_dir();
        let bytes = zip_of(&[(name, noise(100, 1).as_slice())]);
        let (_, plan, tree) = extract(&dir, &bytes);
        let relative = name.split_once('/').unwrap().1;
        assert!(tree.join(relative).is_file(), "{name}");
        let compared = compare_with_zip(&plan, &tree).unwrap();
        assert_eq!(compared.files[0].path, relative);
    }
}

#[test]
fn a_name_that_is_not_utf8_is_refused() {
    let mut bytes = zip_of(&[("top/aXb", b"data")]);
    // Rewrite the name in the local and central headers to an invalid UTF-8 byte.
    let pattern = b"top/aXb";
    let mut at = 0;
    while let Some(found) = bytes[at..]
        .windows(pattern.len())
        .position(|w| w == pattern)
    {
        bytes[at + found + 5] = 0xFF;
        at += found + pattern.len();
    }
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
}

#[test]
fn symlinks_are_refused() {
    // `add_symlink` is the writer's way to set the link mode; `unix_permissions` masks it off.
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("top/link", "../../outside", SimpleFileOptions::default())
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
}

#[test]
fn case_insensitive_duplicates_are_refused() {
    let bytes = zip_of(&[("top/a.txt", b"one"), ("top/A.txt", b"two")]);
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("duplicate"), "{err}");

    let bytes = zip_of(&[("top/Dir/a", b"one"), ("top/dir/b", b"two")]);
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("case"), "{err}");
}

#[test]
fn a_file_whose_path_is_also_a_directory_is_refused() {
    let bytes = zip_of(&[("top/a", b"file"), ("top/a/b", b"inner")]);
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("also a directory"), "{err}");
}

#[test]
fn more_than_one_top_directory_is_refused() {
    let bytes = zip_of(&[("one/a", b"1"), ("two/b", b"2")]);
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("top-level"), "{err}");
}

#[test]
fn a_file_at_the_top_is_refused() {
    let bytes = zip_of(&[("a.txt", b"1")]);
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(err.to_string().contains("top-level"), "{err}");
}

#[test]
fn an_empty_or_unreadable_archive_is_refused() {
    let writer = ZipWriter::new(Cursor::new(Vec::new()));
    let empty = writer.finish().unwrap().into_inner();
    assert!(matches!(
        scan_bytes(&empty),
        Err(Error::UnsafeArchive { .. })
    ));
    assert!(matches!(
        scan_bytes(b"not a zip at all"),
        Err(Error::UnsafeArchive { .. })
    ));
}

#[test]
fn too_many_entries_are_refused() {
    let dir = temp_dir();
    let names: Vec<String> = (0..11).map(|i| format!("top/{i}")).collect();
    let entries: Vec<(&str, &[u8])> = names
        .iter()
        .map(|n| (n.as_str(), b"x".as_slice()))
        .collect();
    let path = write_zip(&dir, &zip_of(&entries));
    let err = scan(
        &path,
        ExtractLimits {
            max_entries: 10,
            max_ratio: 1000,
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("over the limit"), "{err}");
}

#[test]
fn a_zip_bomb_trips_the_ratio_limit_before_anything_is_written() {
    let dir = temp_dir();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("top/zeros", deflated()).unwrap();
    writer.write_all(&vec![0u8; 64 << 20]).unwrap();
    let path = write_zip(&dir, &writer.finish().unwrap().into_inner());
    let err = scan(&path, ExtractLimits::default()).unwrap_err();
    assert!(matches!(err, Error::ArchiveTooLarge { .. }), "{err}");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn an_unsupported_compression_method_is_refused() {
    let mut bytes = zip_of(&[("top/a", b"data"), ("top/b", b"other")]);
    // Set the central directory's compression method of each entry to 12 (bzip2).
    let signature = [0x50, 0x4B, 0x01, 0x02];
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(4).position(|w| w == signature) {
        bytes[at + found + 10] = 12;
        bytes[at + found + 11] = 0;
        at += found + 4;
    }
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
}

#[test]
fn an_encrypted_entry_is_refused() {
    let mut bytes = zip_of(&[("top/a", b"data")]);
    // Set the central directory's encryption flag.
    let signature = [0x50, 0x4B, 0x01, 0x02];
    let at = bytes.windows(4).position(|w| w == signature).unwrap();
    bytes[at + 8] |= 1;
    let err = scan_bytes(&bytes).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
}

fn extract(dir: &TempDir, bytes: &[u8]) -> (PathBuf, mtp_sdk::archive::ArchivePlan, PathBuf) {
    let zip = write_zip(dir, bytes);
    let plan = scan(&zip, ExtractLimits::default()).unwrap();
    let dest = dir.path().join("x");
    extract_into_new_dir(&zip, &plan, &dest).unwrap();
    let tree = dest.join(&plan.top_dir);
    (zip, plan, tree)
}

#[test]
fn extraction_produces_exactly_the_zip_and_the_compare_records_it() {
    let dir = temp_dir();
    let (_, plan, tree) = extract(&dir, &package_zip("emulator", "emulator.exe", "37.2.13", 1));
    assert!(tree.join("empty").is_dir());
    assert_eq!(
        std::fs::read(tree.join("NOTICE.txt")).unwrap(),
        b"fixture notices\n"
    );
    let compared = compare_with_zip(&plan, &tree).unwrap();
    let names: Vec<&str> = compared.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        names,
        [
            "NOTICE.txt",
            "emulator.exe",
            "lib/data.bin",
            "source.properties"
        ]
    );
    assert_eq!(compared.dirs, ["empty"]);
    let notice = compared
        .files
        .iter()
        .find(|f| f.path == "NOTICE.txt")
        .unwrap();
    assert_eq!(
        notice.sha256,
        mtp_sdk::hash::sha256_bytes(b"fixture notices\n")
    );
}

#[test]
fn extraction_refuses_a_directory_that_already_exists() {
    let dir = temp_dir();
    let zip = write_zip(&dir, &package_zip("emulator", "emulator.exe", "1", 1));
    let plan = scan(&zip, ExtractLimits::default()).unwrap();
    let dest = dir.path().join("x");
    std::fs::create_dir(&dest).unwrap();
    assert!(matches!(
        extract_into_new_dir(&zip, &plan, &dest),
        Err(Error::Io { .. })
    ));
}

fn compare_failure(mutate: impl Fn(&Path)) -> Error {
    let dir = temp_dir();
    let (_, plan, tree) = extract(&dir, &package_zip("emulator", "emulator.exe", "1", 1));
    mutate(&tree);
    compare_with_zip(&plan, &tree).unwrap_err()
}

#[test]
fn a_set_cancel_flag_stops_extraction_and_the_compare() {
    let dir = temp_dir();
    let (zip, plan, tree) = extract(&dir, &package_zip("emulator", "emulator.exe", "1", 1));
    let cancel = Cancel::new();
    cancel.cancel();
    let err = archive::extract_into_new_dir(
        &File::open(&zip).unwrap(),
        &plan,
        &dir.path().join("y"),
        &cancel,
    )
    .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let err = archive::compare_with_zip(&plan, &tree, &cancel).unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
}

#[test]
fn compare_finds_changed_bytes_of_the_same_size() {
    let err = compare_failure(|tree| {
        let path = tree.join("emulator.exe");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[10] ^= 1;
        std::fs::write(&path, bytes).unwrap();
    });
    assert!(err.to_string().contains("different bytes"), "{err}");
    assert!(matches!(err, Error::CompareFailed { .. }));
}

#[test]
fn compare_finds_a_changed_size() {
    let err = compare_failure(|tree| std::fs::write(tree.join("NOTICE.txt"), b"x").unwrap());
    assert!(err.to_string().contains("size"), "{err}");
}

#[test]
fn compare_finds_a_missing_file() {
    let err = compare_failure(|tree| std::fs::remove_file(tree.join("lib/data.bin")).unwrap());
    assert!(err.to_string().contains("not on disk"), "{err}");
}

#[test]
fn compare_finds_an_extra_file_and_an_extra_directory() {
    let err = compare_failure(|tree| std::fs::write(tree.join("extra.txt"), b"x").unwrap());
    assert!(err.to_string().contains("isn't in the zip"), "{err}");
    let err = compare_failure(|tree| std::fs::create_dir(tree.join("extra")).unwrap());
    assert!(err.to_string().contains("isn't in the zip"), "{err}");
}

#[test]
fn entry_data_that_fails_its_crc_is_fatal() {
    let dir = temp_dir();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let payload = noise(4096, 3);
    writer.start_file("top/a", stored()).unwrap();
    writer.write_all(&payload).unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    // Flip a data byte in the stored payload; both headers still declare the old CRC.
    let at = bytes.windows(16).position(|w| w == &payload[..16]).unwrap();
    bytes[at + 100] ^= 0xFF;
    let zip = write_zip(&dir, &bytes);
    let plan = scan(&zip, ExtractLimits::default()).unwrap();
    let err = extract_into_new_dir(&zip, &plan, &dir.path().join("x")).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
}

#[test]
fn a_deflated_entry_longer_than_declared_is_refused() {
    let dir = temp_dir();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file("top/a", deflated()).unwrap();
    writer.write_all(&noise(4096, 4)).unwrap();
    let mut bytes = writer.finish().unwrap().into_inner();
    // Shrink the declared uncompressed size in both headers.
    let (local, central) = (
        bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4B, 0x03, 0x04])
            .unwrap(),
        bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4B, 0x01, 0x02])
            .unwrap(),
    );
    bytes[local + 22..local + 26].copy_from_slice(&100u32.to_le_bytes());
    bytes[central + 24..central + 28].copy_from_slice(&100u32.to_le_bytes());
    let zip = write_zip(&dir, &bytes);
    let plan = scan(&zip, ExtractLimits::default()).unwrap();
    let err = extract_into_new_dir(&zip, &plan, &dir.path().join("x")).unwrap_err();
    assert!(matches!(err, Error::UnsafeArchive { .. }), "{err}");
}

#[test]
fn one_member_is_extracted_by_exact_name() {
    let dir = temp_dir();
    let zip = write_zip(
        &dir,
        &zip_of(&[("a/b.so", b"wanted"), ("a/c.so", b"other")]),
    );
    let dest = dir.path().join("out");
    extract_member(&zip, "a/b.so", &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"wanted");

    assert!(
        extract_member(&zip, "a/b.so", &dest).is_err(),
        "must not overwrite"
    );
    assert!(extract_member(&zip, "a/nope.so", &dir.path().join("o2")).is_err());
}

#[test]
fn a_symlink_member_is_refused() {
    let dir = temp_dir();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("lib/x", "../../outside", SimpleFileOptions::default())
        .unwrap();
    let zip = write_zip(&dir, &writer.finish().unwrap().into_inner());
    assert!(extract_member(&zip, "lib/x", &dir.path().join("out")).is_err());
}

#[test]
fn a_scan_reports_the_longest_path_below_the_top_directory_in_utf16_units() {
    let plan = scan_bytes(&zip_of(&[
        ("top/a", b"x"),
        ("top/abc\u{1F600}/d", b"x"),
        ("top/\u{e9}", b"x"),
    ]))
    .unwrap();

    assert_eq!(plan.longest_relative, 7);
}
