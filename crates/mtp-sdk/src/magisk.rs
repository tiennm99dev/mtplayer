//! The Magisk binary, used only for `resetprop`. It is fetched on the user's machine from
//! Magisk's GitHub release and extracted by exact member name.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::archive::extract_member;
use crate::download::DownloadRequest;
use crate::error::{Error, IoContext, Result};
use crate::event::{Cancel, Event};
use crate::fsutil;
use crate::hash::{Digest, Sha256Hex, sha256_file};
use crate::home::Home;
use crate::manifest::MagiskPin;
use crate::source::Client;

/// `tools/magisk/<tag>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagiskRecord {
    pub schema: u32,
    pub tag: String,
    pub binary_sha256: Sha256Hex,
    pub installed_at: String,
}

/// Where a tag's binary lives.
pub fn binary_path(home: &Home, tag: &str) -> PathBuf {
    home.magisk_dir().join(tag).join("magisk")
}

/// True when the pinned binary exists with the pinned hash. It is always hashed, since
/// at 10 MB that costs nothing.
pub fn is_installed(home: &Home, pin: &MagiskPin) -> Result<bool> {
    match sha256_file(&binary_path(home, &pin.tag)) {
        Ok(hash) => Ok(hash == pin.binary_sha256),
        Err(e) if e.is_not_found() => Ok(false),
        Err(e) => Err(e),
    }
}

/// Downloads the pinned APK if needed and installs its binary. Returns whether a transfer
/// happened.
pub fn install_magisk(
    home: &Home,
    pin: &MagiskPin,
    client: &Client,
    on_event: &mut dyn FnMut(Event),
    cancel: &Cancel,
) -> Result<bool> {
    let downloads = home.checked_downloads_dir()?;
    fs::create_dir_all(&downloads).at(&downloads)?;
    home.note_owned_download(&pin.asset)?;
    let digest = Digest::Sha256(pin.sha256.clone());
    let downloaded = client
        .download(
            &DownloadRequest {
                url: pin.url.clone(),
                dest: downloads.join(&pin.asset),
                size: pin.size,
                digest: digest.clone(),
            },
            on_event,
            cancel,
        )
        .map_err(|e| e.pin_gone(&pin.repo, &pin.tag))?;

    on_event(Event::Extract {
        name: pin.member.clone(),
    });
    let apk = fsutil::open_denying_writes(&downloaded.path)?;
    let actual = digest.compute_reader_cancellable(&apk, &downloaded.path, cancel)?;
    if actual != digest.expected() {
        return Err(Error::HashMismatch {
            name: pin.asset.clone(),
            expected: digest.expected().to_string(),
            actual,
        });
    }
    let staging = fsutil::new_empty_dir(&home.staging_dir(), "magisk")?;
    let result = (|| {
        let binary = extract_member(&apk, &pin.member, &staging.join("magisk"))?;
        let actual = sha256_file(&binary)?;
        if actual != pin.binary_sha256 {
            return Err(Error::HashMismatch {
                name: pin.member.clone(),
                expected: pin.binary_sha256.to_string(),
                actual: actual.to_string(),
            });
        }
        let target = home.magisk_dir().join(&pin.tag);
        fsutil::ensure_no_reparse_point(home.path(), &target)?;
        fsutil::remove_tree(home.path(), &target)?;
        fs::create_dir_all(home.magisk_dir()).at(home.magisk_dir())?;
        // The record goes first, because `remove --unused` only deletes a tag that has one. A
        // record without a binary is harmless: `is_installed` hashes the binary, so a later
        // install just repeats the rename.
        fsutil::write_json_atomic(
            &home.magisk_dir().join(format!("{}.json", pin.tag)),
            &MagiskRecord {
                schema: 1,
                tag: pin.tag.clone(),
                binary_sha256: pin.binary_sha256.clone(),
                installed_at: fsutil::now(),
            },
        )?;
        fsutil::rename_with_retry(&staging, &target)?;
        on_event(Event::Install {
            name: format!("magisk {}", pin.tag),
            dest: target.display().to_string(),
        });
        Ok(())
    })();
    if result.is_err() {
        let _ = fsutil::remove_tree(home.path(), &staging);
    }
    result.map(|()| !downloaded.reused)
}
