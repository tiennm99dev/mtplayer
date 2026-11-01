//! Safe extraction. Zips are untrusted input even after their hash passes, so every entry
//! is checked over the central directory before the first byte is written, and the result
//! is compared against the zip before it is moved into place.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zip::{CompressionMethod, ZipArchive};

use crate::error::{Error, IoContext, Result};
use crate::event::Cancel;
use crate::fsutil::{self, DISK_RESERVE};
use crate::hash::{Sha256Hex, hash_file};

const MAX_NAME_BYTES: usize = 1024;
/// A single extracted member can't be larger than this.
const MAX_MEMBER_BYTES: u64 = 256 << 20;
/// Names the archive in an error, since these functions take an open handle.
const ARCHIVE_LABEL: &str = "the archive";

/// Bomb limits. The observed ratio of Google's zips is about 2.5.
#[derive(Clone, Copy, Debug)]
pub struct ExtractLimits {
    pub max_entries: usize,
    pub max_ratio: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_entries: 50_000,
            max_ratio: 8,
        }
    }
}

/// One checked entry.
#[derive(Clone, Debug)]
pub struct PlannedEntry {
    index: usize,
    /// The path below the single top-level directory, without a trailing slash.
    relative: String,
    is_dir: bool,
    size: u64,
    crc32: u32,
}

/// The outcome of a successful scan.
#[derive(Clone, Debug)]
pub struct ArchivePlan {
    pub top_dir: String,
    pub total: u64,
    /// The longest path below the top-level directory, in the UTF-16 units Windows counts.
    pub longest_relative: usize,
    entries: Vec<PlannedEntry>,
}

/// A file as extracted, with the hash later checks use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    /// The path below the package's directory, with forward slashes.
    pub path: String,
    pub size: u64,
    pub sha256: Sha256Hex,
}

/// What the compare proved about a package directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compared {
    pub files: Vec<FileRecord>,
    /// Directories the zip names explicitly, so that a link-reuse can recreate empty ones.
    pub dirs: Vec<String>,
}

fn unsafe_entry(name: &str, reason: &str) -> Error {
    Error::UnsafeArchive {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}

fn open(zip: &File) -> Result<ZipArchive<BufReader<&File>>> {
    ZipArchive::new(BufReader::new(zip))
        .map_err(|e| unsafe_entry(ARCHIVE_LABEL, &format!("not a readable zip: {e}")))
}

/// Checks a whole entry name and returns its components. A directory's trailing slash is
/// removed first.
///
/// This is the only containment check, and it must stay a superset of the zip crate's
/// `enclosed_name`, which can't be called on untrusted names: version 9.2.1 panics when byte
/// 3 of a component falls inside a multi-byte character.
fn check_name(name: &str, is_dir: bool) -> Result<Vec<&str>> {
    if name.len() > MAX_NAME_BYTES {
        return Err(unsafe_entry(name, "a name over 1024 bytes"));
    }
    if name.contains('\0') || name.contains('\\') {
        return Err(unsafe_entry(name, "a NUL or a backslash"));
    }
    if name.starts_with('/') {
        return Err(unsafe_entry(name, "an absolute path"));
    }
    let trimmed = if is_dir {
        name.strip_suffix('/').unwrap_or(name)
    } else {
        name
    };
    let components: Vec<&str> = trimmed.split('/').collect();
    for component in &components {
        fsutil::check_windows_component(component).map_err(|reason| unsafe_entry(name, reason))?;
    }
    Ok(components)
}

/// Checks an archive over its central directory. Nothing is written. It takes an open handle
/// so that a caller can hash, scan and extract the same bytes.
pub fn scan(zip: &File, limits: ExtractLimits) -> Result<ArchivePlan> {
    let zip_size = zip.metadata().at(ARCHIVE_LABEL)?.len();
    let mut archive = open(zip)?;
    if archive.len() > limits.max_entries {
        return Err(unsafe_entry(
            ARCHIVE_LABEL,
            &format!(
                "{} entries, over the limit of {}",
                archive.len(),
                limits.max_entries
            ),
        ));
    }

    let mut entries = Vec::with_capacity(archive.len());
    let mut top_dir: Option<String> = None;
    let mut total = 0u64;
    let mut longest_relative = 0;
    // Lowercased path -> original spelling, because NTFS merges names that differ in case.
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut dirs: BTreeMap<String, String> = BTreeMap::new();
    let mut named_dirs = BTreeSet::new();

    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|e| {
            unsafe_entry(
                &format!("#{index}"),
                &format!("unreadable central directory: {e}"),
            )
        })?;
        let name = std::str::from_utf8(entry.name_raw())
            .map_err(|_| {
                unsafe_entry(
                    &String::from_utf8_lossy(entry.name_raw()),
                    "a name that isn't UTF-8",
                )
            })?
            .to_string();
        if entry.is_symlink() {
            return Err(unsafe_entry(&name, "a symbolic link"));
        }
        if !entry.is_dir() && !entry.is_file() {
            return Err(unsafe_entry(&name, "neither a file nor a directory"));
        }
        if entry.encrypted() {
            return Err(unsafe_entry(&name, "an encrypted entry"));
        }
        let is_dir = entry.is_dir();
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(unsafe_entry(
                &name,
                "a compression method other than stored or deflated",
            ));
        }
        let components = check_name(&name, is_dir)?;
        match &top_dir {
            None => top_dir = Some(components[0].to_string()),
            Some(top) if top == components[0] => {}
            Some(_) => return Err(unsafe_entry(&name, "more than one top-level directory")),
        }
        if components.len() == 1 && !is_dir {
            return Err(unsafe_entry(
                &name,
                "a file outside the top-level directory",
            ));
        }

        for depth in 1..components.len() {
            let ancestor = components[..depth].join("/");
            let key = ancestor.to_lowercase();
            match dirs.get(&key) {
                Some(spelling) if *spelling != ancestor => {
                    return Err(unsafe_entry(&name, "a duplicate differing only in case"));
                }
                Some(_) => {}
                None => {
                    dirs.insert(key, ancestor);
                }
            }
        }
        let path = components.join("/");
        let key = path.to_lowercase();
        if is_dir {
            if !named_dirs.insert(key.clone()) {
                return Err(unsafe_entry(&name, "a duplicate entry"));
            }
            match dirs.get(&key) {
                Some(spelling) if *spelling != path => {
                    return Err(unsafe_entry(&name, "a duplicate differing only in case"));
                }
                _ => {
                    dirs.insert(key, path.clone());
                }
            }
        } else if files.insert(key, path.clone()).is_some() {
            return Err(unsafe_entry(&name, "a duplicate entry"));
        }

        total = total.saturating_add(entry.size());
        let relative = components[1..].join("/");
        longest_relative = longest_relative.max(relative.encode_utf16().count());
        entries.push(PlannedEntry {
            index,
            relative,
            is_dir,
            size: if is_dir { 0 } else { entry.size() },
            crc32: entry.crc32(),
        });
    }

    if let Some(conflict) = files.keys().find(|file| dirs.contains_key(*file)) {
        return Err(unsafe_entry(
            &files[conflict],
            "a file whose path is also a directory",
        ));
    }
    let limit = zip_size.saturating_mul(limits.max_ratio);
    if total > limit {
        return Err(Error::ArchiveTooLarge {
            declared: total,
            limit,
        });
    }
    let top_dir = top_dir.ok_or_else(|| unsafe_entry(ARCHIVE_LABEL, "an empty archive"))?;
    Ok(ArchivePlan {
        top_dir,
        total,
        longest_relative,
        entries,
    })
}

/// Copies one entry to a brand-new file, stopping at the declared size. The zip crate
/// verifies the CRC-32 when the entry's stream ends, and a failure there is fatal.
fn copy_entry(reader: &mut impl Read, name: &str, size: u64, dest: &Path) -> Result<()> {
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .at(dest)?;
    let written = std::io::copy(&mut reader.take(size + 1), &mut out)
        .map_err(|e| unsafe_entry(name, &format!("corrupt data: {e}")))?;
    if written != size {
        return Err(unsafe_entry(
            name,
            "data that doesn't match its declared size",
        ));
    }
    // A crash after this point can't leave a file of the right size with a zeroed tail,
    // which a later size check would take for a good one.
    out.sync_all().at(dest)?;
    Ok(())
}

/// Extracts a scanned archive into `dest`, which must not exist yet. The result is
/// `dest/<top_dir>/...`.
pub fn extract_into_new_dir(
    zip: &File,
    plan: &ArchivePlan,
    dest: &Path,
    cancel: &Cancel,
) -> Result<()> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).at(parent)?;
    // `dest` is `<home>/staging/<new dir>/x`.
    fsutil::ensure_free(parent, 2, plan.total.saturating_add(DISK_RESERVE))?;
    fs::create_dir(dest).at(dest)?;

    let root = dest.join(&plan.top_dir);
    fs::create_dir(&root).at(&root)?;
    let mut archive = open(zip)?;
    for planned in &plan.entries {
        cancel.check()?;
        if planned.relative.is_empty() {
            continue;
        }
        let target = root.join(&planned.relative);
        if planned.is_dir {
            fs::create_dir_all(&target).at(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).at(parent)?;
        }
        let mut entry = archive
            .by_index(planned.index)
            .map_err(|e| unsafe_entry(&planned.relative, &format!("unreadable entry: {e}")))?;
        copy_entry(&mut entry, &planned.relative, planned.size, &target)?;
    }
    Ok(())
}

/// Compares a package directory with the zip's central directory: every file maps to an
/// entry with equal size and CRC-32, no entry is missing and nothing extra exists.
pub fn compare_with_zip(plan: &ArchivePlan, dir: &Path, cancel: &Cancel) -> Result<Compared> {
    let failed = |path: &str, reason: &str| Error::CompareFailed {
        path: path.to_string(),
        reason: reason.to_string(),
    };
    let mut expected_files: BTreeMap<&str, &PlannedEntry> = BTreeMap::new();
    let mut known_dirs: BTreeSet<String> = BTreeSet::new();
    let mut named_dirs = Vec::new();
    for entry in plan.entries.iter().filter(|e| !e.relative.is_empty()) {
        let mut ancestors: Vec<&str> = entry.relative.split('/').collect();
        ancestors.pop();
        for depth in 1..=ancestors.len() {
            known_dirs.insert(ancestors[..depth].join("/"));
        }
        if entry.is_dir {
            known_dirs.insert(entry.relative.clone());
            named_dirs.push(entry.relative.clone());
        } else {
            expected_files.insert(&entry.relative, entry);
        }
    }

    let mut files = Vec::with_capacity(expected_files.len());
    for item in walkdir::WalkDir::new(dir).min_depth(1).follow_links(false) {
        cancel.check()?;
        let item = item.map_err(|e| failed(&dir.display().to_string(), &e.to_string()))?;
        let relative = relative_slash(dir, item.path())?;
        if item.file_type().is_dir() {
            if !known_dirs.contains(&relative) {
                return Err(failed(&relative, "a directory that isn't in the zip"));
            }
            continue;
        }
        if !item.file_type().is_file() {
            return Err(failed(&relative, "not a regular file"));
        }
        let Some(entry) = expected_files.remove(relative.as_str()) else {
            return Err(failed(&relative, "a file that isn't in the zip"));
        };
        let hash = hash_file(item.path())?;
        if hash.len != entry.size {
            return Err(failed(&relative, "a different size than the zip"));
        }
        if hash.crc32 != entry.crc32 {
            return Err(failed(&relative, "different bytes than the zip"));
        }
        files.push(FileRecord {
            path: relative,
            size: hash.len,
            sha256: hash.sha256,
        });
    }
    if let Some(missing) = expected_files.keys().next() {
        return Err(failed(missing, "in the zip but not on disk"));
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Compared {
        files,
        dirs: named_dirs,
    })
}

/// A path below `base`, with forward slashes.
pub(crate) fn relative_slash(base: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(base).map_err(|_| Error::CompareFailed {
        path: path.display().to_string(),
        reason: "outside the directory being compared".into(),
    })?;
    let parts: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    Ok(parts.join("/"))
}

/// Extracts exactly one named file into `dest`, which must be a new path.
pub fn extract_member(zip: &File, member: &str, dest: &Path) -> Result<PathBuf> {
    let mut archive = open(zip)?;
    let index = archive
        .index_for_name(member)
        .ok_or_else(|| unsafe_entry(member, "not in the archive"))?;
    let mut entry = archive
        .by_index(index)
        .map_err(|e| unsafe_entry(member, &format!("unreadable entry: {e}")))?;
    if entry.is_symlink() || !entry.is_file() || entry.encrypted() {
        return Err(unsafe_entry(member, "not a plain file"));
    }
    if entry.size() > MAX_MEMBER_BYTES {
        return Err(Error::ArchiveTooLarge {
            declared: entry.size(),
            limit: MAX_MEMBER_BYTES,
        });
    }
    let size = entry.size();
    copy_entry(&mut entry, member, size, dest)?;
    Ok(dest.to_path_buf())
}
