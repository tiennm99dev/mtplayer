//! The on-disk layout of an MTPlayer home. The library has no default home and reads no
//! environment variable: every caller names the path.
//!
//! ```text
//! <home>/mtp-sdk.lock              exclusive lock for install and remove
//! <home>/leases.lock               short lock around taking a lease or retiring a root
//! <home>/sdk/<set>/                ANDROID_SDK_ROOT of that set: Google's packages and the
//!                                  one empty platform-tools directory the emulator requires
//! <home>/sdk/<set>.json            root record
//! <home>/sdk/<set>.leases/<owner>  one file per instance using the root
//! <home>/staging/                  extractions, same volume as sdk/
//! <home>/downloads/                zips and partial files
//! <home>/downloads/owned.json      names of the files MTPlayer put there
//! <home>/licenses/<id>.json        acceptance records
//! <home>/tools/magisk/<tag>/magisk and <home>/tools/magisk/<tag>.json
//! ```

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::license::LicenseStore;
use crate::manifest::{SetId, is_safe_name};
use crate::store::{self, RootState, SdkRoot};

const LEASE_LOCK_ATTEMPTS: u32 = 40;
const LEASE_LOCK_BACKOFF: Duration = Duration::from_millis(50);

/// An MTPlayer home folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

/// Holds the home's exclusive lock until dropped.
#[derive(Debug)]
pub struct HomeLock {
    _file: File,
}

/// A root whose record exists but can't be read. It can still be listed and removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamagedRoot {
    pub set: SetId,
    pub path: PathBuf,
    pub error: String,
}

impl Home {
    /// A home at `path`. Nothing is created until something needs it.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }

    /// The home's path.
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// `<home>/sdk`.
    pub fn sdk_dir(&self) -> PathBuf {
        self.root.join("sdk")
    }

    /// `<home>/downloads`.
    pub fn downloads_dir(&self) -> PathBuf {
        self.root.join("downloads")
    }

    /// `<home>/downloads`, for anything that reads, writes or deletes through it. A link or
    /// junction there would point all of that at files outside the home, so it is refused.
    pub(crate) fn checked_downloads_dir(&self) -> Result<PathBuf> {
        let dir = self.downloads_dir();
        fsutil::ensure_no_reparse_point(&self.root, &dir)?;
        Ok(dir)
    }

    /// `<home>/staging`.
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// `<home>/licenses`.
    pub fn licenses_dir(&self) -> PathBuf {
        self.root.join("licenses")
    }

    /// `<home>/tools/magisk`.
    pub fn magisk_dir(&self) -> PathBuf {
        self.root.join("tools").join("magisk")
    }

    /// The license records of this home.
    pub fn licenses(&self) -> LicenseStore {
        LicenseStore::new(&self.licenses_dir())
    }

    fn owned_downloads_path(&self) -> Result<PathBuf> {
        Ok(self.checked_downloads_dir()?.join("owned.json"))
    }

    /// The files in `downloads/` that MTPlayer put there, including ones from an earlier
    /// release's pins, which no current manifest can name. The list is input like any other
    /// file in the home, so a name that could leave the folder is dropped.
    pub(crate) fn owned_downloads(&self) -> Result<Vec<String>> {
        let names: Vec<String> =
            fsutil::read_json(&self.owned_downloads_path()?)?.unwrap_or_default();
        Ok(names
            .into_iter()
            .filter(|name| is_safe_name(name) && name != "owned.json")
            .collect())
    }

    /// Records a file before its transfer starts, so that even a partial one can be removed
    /// after the pins move on.
    pub(crate) fn note_owned_download(&self, name: &str) -> Result<()> {
        let mut names = self.owned_downloads()?;
        if names.iter().any(|known| known == name) {
            return Ok(());
        }
        names.push(name.to_string());
        fsutil::write_json_atomic(&self.owned_downloads_path()?, &names)
    }

    /// Forgets the listed files that are gone. The list itself goes with the last of them,
    /// so that a clean `downloads/` is an empty one.
    pub(crate) fn prune_owned_downloads(&self) -> Result<()> {
        let dir = self.checked_downloads_dir()?;
        let names = self.owned_downloads()?;
        let kept: Vec<&String> = names
            .iter()
            .filter(|name| {
                let exists = |path: PathBuf| fs::symlink_metadata(path).is_ok();
                exists(dir.join(name)) || exists(dir.join(format!("{name}.part")))
            })
            .collect();
        let path = self.owned_downloads_path()?;
        if !kept.is_empty() {
            return fsutil::write_json_atomic(&path, &kept);
        }
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).at(path),
        }
    }

    /// `<home>/mtp-sdk.lock`.
    pub fn lock_path(&self) -> PathBuf {
        self.root.join("mtp-sdk.lock")
    }

    /// Takes the exclusive lock install and remove need. `status` takes none, because
    /// records change only through an atomic rename.
    pub fn lock(&self) -> Result<HomeLock> {
        let path = self.lock_path();
        let file = self.open_lock_file(&path)?;
        match file.try_lock() {
            Ok(()) => Ok(HomeLock { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::Busy { lock: path }),
            Err(TryLockError::Error(e)) => Err(e).at(path),
        }
    }

    /// Serializes taking a lease with retiring a root, so that a lease can't appear between
    /// a remove's check and its delete. The home lock can't do this, because an install
    /// holds it for minutes; this one is held only for a few small writes, so a short wait
    /// is enough.
    pub(crate) fn lease_lock(&self) -> Result<HomeLock> {
        let path = self.root.join("leases.lock");
        let file = self.open_lock_file(&path)?;
        for _ in 0..LEASE_LOCK_ATTEMPTS {
            match file.try_lock() {
                Ok(()) => return Ok(HomeLock { _file: file }),
                Err(TryLockError::WouldBlock) => std::thread::sleep(LEASE_LOCK_BACKOFF),
                Err(TryLockError::Error(e)) => return Err(e).at(path),
            }
        }
        Err(Error::Busy { lock: path })
    }

    fn open_lock_file(&self, path: &Path) -> Result<File> {
        fs::create_dir_all(&self.root).at(&self.root)?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .at(path)
    }

    /// The bytes free on the home's volume. The home must exist.
    pub fn free_bytes(&self) -> Result<u64> {
        crate::windows::free_bytes(&self.root).at(&self.root)
    }

    /// The root a launcher may use: only a `ready` record whose files pass the quick check.
    pub fn sdk_root(&self, set: &SetId) -> Result<Option<SdkRoot>> {
        let Some(root) = SdkRoot::open(self, set)? else {
            return Ok(None);
        };
        let usable = root.record().state == RootState::Ready && root.quick_check()?.ok();
        Ok(usable.then_some(root))
    }

    /// Every root with a readable record, in any state.
    pub fn roots(&self) -> Result<Vec<SdkRoot>> {
        Ok(self.roots_with_damaged()?.0)
    }

    /// Every root with a record, in any state, split by whether the record can be read. One
    /// damaged record must not stop anyone from working with the others, or from removing it.
    pub fn roots_with_damaged(&self) -> Result<(Vec<SdkRoot>, Vec<DamagedRoot>)> {
        let dir = self.sdk_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), Vec::new()));
            }
            Err(e) => return Err(e).at(dir),
        };
        let mut roots = Vec::new();
        let mut damaged = Vec::new();
        for entry in entries {
            let name = entry.at(&dir)?.file_name().to_string_lossy().into_owned();
            let Some(set) = name
                .strip_suffix(".json")
                .and_then(|s| SetId::parse(s).ok())
            else {
                continue;
            };
            match SdkRoot::open(self, &set) {
                Ok(Some(root)) => roots.push(root),
                Ok(None) => {}
                Err(e) => damaged.push(DamagedRoot {
                    path: store::root_path(self, &set),
                    set,
                    error: e.to_string(),
                }),
            }
        }
        roots.sort_by(|a, b| a.set().cmp(b.set()));
        damaged.sort_by(|a, b| a.set.cmp(&b.set));
        Ok((roots, damaged))
    }
}
