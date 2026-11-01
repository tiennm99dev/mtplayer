//! File-system helpers: atomic records, retries around antivirus locks, confined deletes.

use std::fs;
use std::io::{self, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_READ};

use crate::error::{Error, IoContext, Result};
use crate::event::Cancel;

/// Free space always left on the volume after a download or an extraction.
pub const DISK_RESERVE: u64 = 256 << 20;

const RESERVED_DEVICE_NAMES: [&str; 7] =
    ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "CLOCK$"];
const MAX_COMPONENT_UTF16: usize = 255;
const ATTEMPTS: u32 = 8;
const BACKOFF: Duration = Duration::from_millis(250);
const CANCEL_POLL: Duration = Duration::from_millis(100);

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// Waits between retries. A server can ask for many seconds, and the daemon's cancel must not
/// wait for them.
pub(crate) fn sleep_unless_cancelled(duration: Duration, cancel: &Cancel) -> Result<()> {
    let deadline = Instant::now() + duration;
    loop {
        cancel.check()?;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(CANCEL_POLL));
    }
}

/// Runs `action`, retrying for a short while because Windows Defender briefly holds
/// freshly extracted executables open.
fn with_retry(mut action: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match action() {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(e),
            Err(_) if attempt + 1 < ATTEMPTS => {
                attempt += 1;
                std::thread::sleep(BACKOFF);
            }
            Err(e) => return Err(e),
        }
    }
}

/// Renames within a volume.
pub(crate) fn rename_with_retry(from: &Path, to: &Path) -> Result<()> {
    with_retry(|| fs::rename(from, to)).at(to)
}

/// Deletes one file. Fails with `NotFound` when it isn't there.
pub(crate) fn remove_file_with_retry(path: &Path) -> io::Result<()> {
    with_retry(|| fs::remove_file(path))
}

/// Removes a directory only if nothing is left in it. A directory that stays is not an
/// error, because whatever is in it isn't MTPlayer's to delete.
pub(crate) fn remove_dir_if_empty(path: &Path) {
    let _ = fs::remove_dir(path);
}

/// True for a name `new_empty_dir` produces: `<prefix>-<hex>-<hex>-<hex>`.
pub(crate) fn is_generated_dir_name(name: &str) -> bool {
    let is_hex = |part: Option<&str>| {
        part.is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let mut parts = name.rsplitn(4, '-');
    is_hex(parts.next())
        && is_hex(parts.next())
        && is_hex(parts.next())
        && parts.next().is_some_and(|prefix| !prefix.is_empty())
}

/// Applies the rules for one path component. They follow what NTFS would otherwise do
/// silently: a colon opens an alternate data stream, a trailing dot or space is dropped,
/// and a device name opens the device instead of a file.
pub(crate) fn check_windows_component(component: &str) -> Result<(), &'static str> {
    if component.is_empty() || component == "." || component == ".." {
        return Err("an empty, . or .. path component");
    }
    if component
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return Err("a character Windows forbids in a file name");
    }
    if component.ends_with('.') || component.ends_with(' ') {
        return Err("a component ending in a dot or space");
    }
    if component.encode_utf16().count() > MAX_COMPONENT_UTF16 {
        return Err("a path component over 255 UTF-16 units");
    }
    // Win32 drops spaces before the extension, so `CON .txt` still opens the console.
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let numbered = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
        });
    if RESERVED_DEVICE_NAMES.contains(&stem.as_str()) || numbered {
        return Err("a reserved device name");
    }
    Ok(())
}

/// Creates a new, empty, uniquely named directory under `parent`.
pub(crate) fn new_empty_dir(parent: &Path, prefix: &str) -> Result<PathBuf> {
    fs::create_dir_all(parent).at(parent)?;
    let path = parent.join(format!("{prefix}-{}", unique_suffix()));
    fs::create_dir(&path).at(&path)?;
    Ok(path)
}

/// Writes pretty JSON through a temporary file and a rename, so that a reader without the
/// home lock never sees a partial record.
pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent).at(parent)?;
    let mut text = serde_json::to_string_pretty(value).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })?;
    text.push('\n');
    let temp = parent.join(format!(".tmp-{}", unique_suffix()));
    let mut file = fs::File::create(&temp).at(&temp)?;
    file.write_all(text.as_bytes()).at(&temp)?;
    file.sync_all().at(&temp)?;
    drop(file);
    rename_with_retry(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// Reads a JSON record, returning `None` when the file doesn't exist.
pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).at(path),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| Error::Json {
            path: path.to_path_buf(),
            source,
        })
}

/// Fails unless the volume holding `dir` has `needed` bytes free. `dir` lies `levels` below
/// the home, on the same volume, and the question goes to the home: the Windows call takes
/// no long path, and only the home's length is kept short.
pub(crate) fn ensure_free(dir: &Path, levels: usize, needed: u64) -> Result<()> {
    let probe = dir
        .ancestors()
        .nth(levels)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(dir);
    let free = crate::windows::free_bytes(probe).at(probe)?;
    if free < needed {
        return Err(Error::InsufficientDisk { needed, free });
    }
    Ok(())
}

/// True when `path` is strictly inside `base` and takes no `..` step below it.
pub(crate) fn is_inside(base: &Path, path: &Path) -> bool {
    path.strip_prefix(base).is_ok_and(|below| {
        below.components().next().is_some()
            && below
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
    })
}

/// Fails unless `path` lies inside `base` with no link or junction between the two. The
/// lexical check can't see one, and the system resolves it in any component but the last,
/// so without this a delete or a move would act on whatever the link points at.
pub(crate) fn ensure_no_reparse_point(base: &Path, path: &Path) -> Result<()> {
    let refused = |reason: &str| {
        Err(Error::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, reason),
        })
    };
    if !is_inside(base, path) {
        return refused("outside the MTPlayer home");
    }
    for ancestor in path.ancestors().take_while(|ancestor| *ancestor != base) {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return refused("a link or junction below the MTPlayer home");
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).at(ancestor),
        }
    }
    Ok(())
}

/// Opens a file for reading and bars anyone from writing, renaming or deleting it while the
/// handle lives, so that a hash taken through it stays true for later reads. The retry is
/// for the antivirus scan a freshly written file attracts.
pub(crate) fn open_denying_writes(path: &Path) -> Result<fs::File> {
    let mut file = None;
    with_retry(|| {
        file = Some(
            fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .open(path)?,
        );
        Ok(())
    })
    .at(path)?;
    Ok(file.expect("the retry returned Ok only after opening"))
}

/// Deletes a tree that MTPlayer created. Refuses any path outside `base` or behind a link,
/// clears read-only attributes that block a delete, and doesn't follow links out of the tree.
pub(crate) fn remove_tree(base: &Path, path: &Path) -> Result<()> {
    ensure_no_reparse_point(base, path)?;
    if fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    let result = with_retry(|| match fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            clear_read_only(path);
            Err(e)
        }
        other => other,
    });
    match result {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e).at(path),
        _ => Ok(()),
    }
}

fn clear_read_only(path: &Path) {
    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if let Ok(meta) = entry.path().symlink_metadata()
            && meta.permissions().readonly()
        {
            let mut permissions = meta.permissions();
            // On Windows this only clears the read-only attribute.
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(entry.path(), permissions);
        }
    }
}

/// A timestamp for records, in RFC 3339 UTC.
pub(crate) fn now() -> String {
    jiff::Timestamp::now()
        .round(jiff::Unit::Second)
        .map_or_else(|_| jiff::Timestamp::now().to_string(), |t| t.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_wait_ends_at_once_when_cancelled_and_after_its_time_otherwise() {
        let cancel = Cancel::new();
        let started = Instant::now();
        sleep_unless_cancelled(Duration::from_millis(150), &cancel).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(150));

        cancel.cancel();
        let started = Instant::now();
        let err = sleep_unless_cancelled(Duration::from_secs(60), &cancel).unwrap_err();
        assert!(matches!(err, Error::Cancelled), "{err}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn only_paths_strictly_below_the_base_count_as_inside() {
        let base = Path::new("D:\\a\\..\\home");
        assert!(is_inside(base, &base.join("sdk").join("x")));
        assert!(!is_inside(base, base));
        assert!(!is_inside(base, Path::new("D:\\other")));
        assert!(!is_inside(base, &base.join("..").join("other")));
        assert!(!is_inside(
            base,
            &base.join("sdk").join("..").join("..").join("x")
        ));
    }

    #[test]
    fn free_space_is_asked_at_the_home_however_deep_the_directory_is() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/test");
        fs::create_dir_all(&base).unwrap();
        let home = tempfile::tempdir_in(base).unwrap();
        let name = "d".repeat(120);
        let deep = home.path().join(&name).join(&name).join(&name);
        fs::create_dir_all(&deep).unwrap();
        assert!(deep.as_os_str().len() > 300);
        ensure_free(&deep, 3, 1).unwrap();
        assert!(matches!(
            ensure_free(&deep, 3, u64::MAX),
            Err(Error::InsufficientDisk { .. })
        ));
    }

    #[test]
    fn only_names_new_empty_dir_produces_count_as_generated() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/test");
        let made = new_empty_dir(&base, "system-images-android-36").unwrap();
        let name = made.file_name().unwrap().to_str().unwrap().to_string();
        fs::remove_dir(&made).unwrap();
        assert!(is_generated_dir_name(&name), "{name}");
        for other in [
            "keep",
            "old",
            "a-1-2",
            "-1-2-3",
            "keep-1-2-xyz",
            "my-own-folder",
        ] {
            assert!(!is_generated_dir_name(other), "{other}");
        }
    }

    #[test]
    fn device_names_and_ntfs_tricks_are_refused_in_any_case() {
        for bad in [
            "NUL",
            "con",
            "Com1",
            "lpt9.txt",
            "aux.tar.gz",
            "a:b",
            "a.",
            "a ",
            "..",
        ] {
            assert!(check_windows_component(bad).is_err(), "{bad}");
        }
        for good in ["instance-a", "COM0", "console", "a.b"] {
            assert!(check_windows_component(good).is_ok(), "{good}");
        }
    }

    #[test]
    fn remove_tree_refuses_anything_outside_the_base() {
        let base = Path::new("D:\\no-such-home");
        assert!(remove_tree(base, Path::new("D:\\no-such-other")).is_err());
        assert!(remove_tree(base, base).is_err());
    }
}
