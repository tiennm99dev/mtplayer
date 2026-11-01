//! Shared test helpers. Each test binary uses only part of this.
#![allow(dead_code)]

pub mod fixtures;
pub mod http;

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// A temporary folder under the repository's `.tmp`, so tests stay on the bulk disk (NTFS,
/// where hard links work) and never touch the real MTPlayer folder.
pub fn temp_dir() -> TempDir {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".tmp")
        .join("test");
    std::fs::create_dir_all(&base).unwrap();
    tempfile::tempdir_in(base).unwrap()
}

pub fn path(dir: &TempDir, name: &str) -> PathBuf {
    dir.path().join(name)
}
