//! Resumable downloads, checked against the pin before anything else touches the file.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};
use crate::event::{Cancel, Event};
use crate::fsutil::{self, DISK_RESERVE};
use crate::hash::Digest;
use crate::source::{Client, display_url, host_and_path, retry_after};

const READ_BUFFER: usize = 256 * 1024;

/// One pinned file to fetch.
#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub url: String,
    /// Where the verified file ends up. Its parent directory must exist.
    pub dest: PathBuf,
    pub size: u64,
    pub digest: Digest,
}

/// The verified file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    /// True when no response body was read: a verified copy, or a complete part left by an
    /// earlier run, was already there.
    pub reused: bool,
}

/// The partial file beside a download's final name.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

fn file_len(path: &Path) -> Result<u64> {
    match fs::metadata(path) {
        Ok(meta) => Ok(meta.len()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e).at(path),
    }
}

fn delete_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).at(path),
    }
}

/// Checks `Content-Range: bytes <start>-<end>/<total>` against the part and the pin.
fn content_range_matches(value: Option<&str>, start: u64, size: u64) -> bool {
    let expected = format!("bytes {start}-{}/{size}", size.saturating_sub(1));
    value.is_some_and(|v| v.trim() == expected)
}

impl Client {
    /// Fetches a pinned file. A verified copy is reused without a request. An interrupted
    /// transfer keeps its part, is retried here, and is resumed by a later run; the retry
    /// budget counts attempts in a row that gained nothing, so a link that keeps dropping
    /// still finishes a large file. A file that fails its hash is deleted and reported before
    /// anything can extract it.
    pub fn download(
        &self,
        request: &DownloadRequest,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<Downloaded> {
        let name = request
            .dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let dest = &request.dest;
        if file_len(dest)? == request.size
            && request.digest.compute_cancellable(dest, cancel)? == request.digest.expected()
        {
            on_event(Event::Reuse {
                name,
                detail: format!("{} ok", request.digest.algorithm()),
            });
            return Ok(Downloaded {
                path: dest.clone(),
                reused: true,
            });
        }
        if file_len(dest)? > 0 {
            on_event(Event::Warning {
                message: format!(
                    "{name}: the kept file failed its {} check and was discarded",
                    request.digest.algorithm()
                ),
            });
        }
        delete_if_present(dest)?;

        let part = part_path(dest);
        let mut attempt = 0;
        let mut transferred = false;
        loop {
            cancel.check()?;
            let before = file_len(&part)?;
            match self.transfer(request, &name, &part, on_event, cancel) {
                Ok(read_a_body) => {
                    transferred |= read_a_body;
                    break;
                }
                Err(e) if e.is_transient() => {
                    if file_len(&part)? > before {
                        attempt = 0;
                        transferred = true;
                    }
                    if attempt >= self.retries {
                        return Err(e);
                    }
                    attempt += 1;
                    on_event(Event::Retry {
                        name: name.clone(),
                        reason: e.to_string(),
                    });
                    fsutil::sleep_unless_cancelled(e.retry_delay(self.backoff), cancel)?;
                }
                Err(e) => return Err(e),
            }
        }

        let actual_len = file_len(&part)?;
        if actual_len != request.size {
            delete_if_present(&part)?;
            return Err(Error::SizeMismatch {
                name,
                expected: request.size,
                actual: actual_len,
            });
        }
        let actual = request.digest.compute_cancellable(&part, cancel)?;
        if actual != request.digest.expected() {
            delete_if_present(&part)?;
            return Err(Error::HashMismatch {
                name,
                expected: request.digest.expected().to_string(),
                actual,
            });
        }
        fsutil::rename_with_retry(&part, dest)?;
        on_event(Event::Verified {
            name,
            algorithm: request.digest.algorithm().to_string(),
        });
        Ok(Downloaded {
            path: dest.clone(),
            reused: !transferred,
        })
    }

    /// Makes the part hold every pinned byte, or fails leaving a resumable part. Returns
    /// whether a response body was read: a part that is already complete needs no request.
    fn transfer(
        &self,
        request: &DownloadRequest,
        name: &str,
        part: &Path,
        on_event: &mut dyn FnMut(Event),
        cancel: &Cancel,
    ) -> Result<bool> {
        let size = request.size;
        let mut have = file_len(part)?;
        if have == size {
            return Ok(false); // Complete already; the caller hashes it.
        }
        if have > size {
            delete_if_present(part)?;
            have = 0;
        }
        let parent = part.parent().unwrap_or(Path::new("."));
        fsutil::ensure_free(parent, 1, (size - have).saturating_add(DISK_RESERVE))?;

        on_event(Event::Download {
            name: name.to_string(),
            source: display_url(&request.url),
            resume_from: (have > 0).then_some(have),
        });
        let range = (have > 0).then_some(have);
        let (response, final_url) = self.send(&request.url, range)?;
        let (host, path) = host_and_path(&final_url);
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        match status {
            206 if range.is_some() => {
                if !content_range_matches(header("content-range").as_deref(), have, size) {
                    // The server would send other bytes than the ones this part needs.
                    delete_if_present(part)?;
                    return Err(Error::Interrupted {
                        name: name.to_string(),
                        received: 0,
                    });
                }
            }
            200 => {
                // A wrong file fails here, before hundreds of megabytes are transferred.
                let length = header("content-length").and_then(|v| v.parse::<u64>().ok());
                if length != Some(size) {
                    return Err(Error::SizeMismatch {
                        name: name.to_string(),
                        expected: size,
                        actual: length.unwrap_or(0),
                    });
                }
                delete_if_present(part)?;
                have = 0;
            }
            416 => {
                delete_if_present(part)?;
                return Err(Error::Interrupted {
                    name: name.to_string(),
                    received: 0,
                });
            }
            _ => {
                return Err(Error::Http {
                    status,
                    host,
                    path,
                    retry_after: retry_after(&response),
                });
            }
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(part)
            .at(part)?;
        let mut writer = BufWriter::with_capacity(READ_BUFFER, file);
        let mut body = response.into_body();
        let mut reader = body.as_reader();
        let mut buffer = vec![0u8; READ_BUFFER];
        let mut received = have;
        let outcome = loop {
            if let Err(e) = cancel.check() {
                break Err(e);
            }
            let read = match reader.read(&mut buffer) {
                Ok(0) => break Ok(()),
                Ok(read) => read,
                Err(_) => {
                    break Err(Error::Interrupted {
                        name: name.to_string(),
                        received,
                    });
                }
            };
            if received + read as u64 > size {
                break Err(Error::SizeMismatch {
                    name: name.to_string(),
                    expected: size,
                    actual: received + read as u64,
                });
            }
            writer.write_all(&buffer[..read]).at(part)?;
            received += read as u64;
            on_event(Event::Progress {
                name: name.to_string(),
                received,
                total: size,
            });
        };
        writer.flush().at(part)?;
        drop(writer);
        if matches!(outcome, Err(Error::SizeMismatch { .. })) {
            delete_if_present(part)?;
        }
        outcome?;
        if received < size {
            return Err(Error::Interrupted {
                name: name.to_string(),
                received,
            });
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_must_name_this_part_and_the_pinned_size() {
        assert!(content_range_matches(Some("bytes 5-9/10"), 5, 10));
        assert!(!content_range_matches(Some("bytes 4-9/10"), 5, 10));
        assert!(!content_range_matches(Some("bytes 5-9/11"), 5, 10));
        assert!(!content_range_matches(Some("bytes 5-8/10"), 5, 10));
        assert!(!content_range_matches(None, 5, 10));
    }
}
