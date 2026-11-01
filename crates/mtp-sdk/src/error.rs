//! Typed errors. Messages name scheme, host and path only, never a query string,
//! because GitHub's signed release URLs carry credentials there.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

/// A server can't make a download wait longer than this for one retry.
const MAX_RETRY_AFTER_SECS: u64 = 30;

/// Any failure in provisioning.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("another MTPlayer process holds {}", lock.display())]
    Busy { lock: PathBuf },
    #[error("license {id} is not accepted; nothing was downloaded")]
    LicenseNotAccepted { id: String },
    #[error(
        "the text of license {id} is not the one with sha1 {given}; read the current text and its hash with `mtp sdk license {id} --text`"
    )]
    LicenseTextChanged { id: String, given: String },
    #[error(
        "{path} {revision} is no longer listed in {xml}; installed roots keep working, but this one can't be installed"
    )]
    PinNotInRepository {
        path: String,
        revision: String,
        xml: String,
    },
    #[error(
        "{path} {revision} can no longer be downloaded from {url_host_path}; installed roots keep working, but this one can't be installed"
    )]
    PinWithdrawn {
        path: String,
        revision: String,
        url_host_path: String,
    },
    #[error("{path}: pinned {field} is {pinned}, but the repository XML says {live}; refusing")]
    PinMismatch {
        path: String,
        field: String,
        pinned: String,
        live: String,
    },
    #[error(
        "{path} is on {channel} under {license}, not a channel-0 android-sdk-license package; refusing. Installed roots keep working"
    )]
    NotStable {
        path: String,
        channel: String,
        license: String,
    },
    #[error("host {host} is not an allowed download source")]
    RedirectNotAllowed { host: String },
    #[error("{url} is not an allowed URL")]
    InsecureUrl { url: String },
    #[error("more than the allowed redirects from {host}")]
    TooManyRedirects { host: String },
    #[error("HTTP {status} from {host}{path}")]
    Http {
        status: u16,
        host: String,
        path: String,
        /// A numeric `Retry-After`, in seconds, when the server sent one.
        retry_after: Option<u64>,
    },
    #[error("bad response from {host}{path}: {reason}")]
    BadResponse {
        host: String,
        path: String,
        reason: String,
    },
    #[error("network error talking to {host}: {message}")]
    Network { host: String, message: String },
    #[error("{name}: expected {expected} bytes, got {actual}")]
    SizeMismatch {
        name: String,
        expected: u64,
        actual: u64,
    },
    #[error("{name}: expected hash {expected}, got {actual}; the file was rejected")]
    HashMismatch {
        name: String,
        expected: String,
        actual: String,
    },
    #[error("unsafe archive entry {name:?}: {reason}")]
    UnsafeArchive { name: String, reason: String },
    #[error("archive declares {declared} bytes, over the limit of {limit}")]
    ArchiveTooLarge { declared: u64, limit: u64 },
    #[error("not enough disk space: {needed} bytes needed, {free} free")]
    InsufficientDisk { needed: u64, free: u64 },
    #[error("post-extraction compare failed for {path}: {reason}")]
    CompareFailed { path: String, reason: String },
    #[error("SDK root {set} is in use by: {}", leases.join(", "))]
    RootInUse { set: String, leases: Vec<String> },
    #[error("SDK root {set} is damaged ({reason}); {remedy}")]
    RootDamaged {
        set: String,
        reason: String,
        remedy: String,
    },
    #[error("{}: not a valid root record: {reason}", path.display())]
    RecordInvalid { path: PathBuf, reason: String },
    #[error("SDK root {set} isn't ready, so it can't take a lease")]
    RootNotReady { set: String },
    #[error(
        "{} is too deep: Google's binaries need every path in an SDK root to stay under {limit} characters. Choose a shorter home with --home or MTPLAYER_HOME",
        home.display()
    )]
    HomePathTooLong { home: PathBuf, limit: usize },
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("invalid name {0:?}")]
    InvalidName(String),
    #[error("{} has no MTPlayer record, so it is not MTPlayer's to delete", path.display())]
    NotOurs { path: PathBuf },
    #[error("cancelled")]
    Cancelled,
    #[error("{name}: the transfer was interrupted after {received} bytes; the next run resumes it")]
    Interrupted { name: String, received: u64 },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("repository XML: {0}")]
    Xml(String),
    #[error("{}: {source}", path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

/// The crate's result type.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// The process exit code for this error: 3 for the license gate, 4 for a held lock,
    /// 5 for a pin that is no longer offered as stable or no longer served, and 1 for
    /// everything else.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::LicenseNotAccepted { .. } | Error::LicenseTextChanged { .. } => 3,
            Error::Busy { .. } => 4,
            Error::PinNotInRepository { .. }
            | Error::PinWithdrawn { .. }
            | Error::NotStable { .. } => 5,
            _ => 1,
        }
    }

    /// Files of this root are missing or changed, and the way out is to remove it.
    pub(crate) fn root_damaged(set: impl fmt::Display, reason: String) -> Self {
        Error::RootDamaged {
            set: set.to_string(),
            reason,
            remedy: format!("remove it with `mtp sdk remove {set}`, then install again"),
        }
    }

    /// The set id names every pinned zip's hash, so a ready root that disagrees with the
    /// manifest means one of the two was edited. Nothing says the root itself is bad, and it
    /// may be leased, so this gives no advice to remove it.
    pub(crate) fn root_differs(set: impl fmt::Display, reason: String) -> Self {
        Error::RootDamaged {
            set: set.to_string(),
            reason,
            remedy: "its record or this manifest was edited, so the root was left as it is"
                .to_string(),
        }
    }

    /// True for a failure that a later attempt can cure: a dropped connection, or a status
    /// that says the server can't answer right now.
    pub(crate) fn is_transient(&self) -> bool {
        matches!(
            self,
            Error::Interrupted { .. }
                | Error::Network { .. }
                | Error::Http {
                    status: 408 | 429 | 500..=599,
                    ..
                }
        )
    }

    /// How long to wait before the next attempt: what the server asked for, within reason.
    pub(crate) fn retry_delay(&self, backoff: Duration) -> Duration {
        match self {
            Error::Http {
                retry_after: Some(seconds),
                ..
            } => Duration::from_secs((*seconds).min(MAX_RETRY_AFTER_SECS)),
            _ => backoff,
        }
    }

    /// True for an I/O error saying a file doesn't exist.
    pub(crate) fn is_not_found(&self) -> bool {
        matches!(self, Error::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound)
    }

    /// Turns the "gone" statuses of a pinned file into the withdrawn-pin error, which names
    /// the package and says what still works. Any other error passes through.
    pub(crate) fn pin_gone(self, pin_path: &str, revision: &str) -> Self {
        match self {
            Error::Http {
                status: 404 | 410,
                host,
                path,
                ..
            } => Error::PinWithdrawn {
                path: pin_path.to_string(),
                revision: revision.to_string(),
                url_host_path: format!("{host}{path}"),
            },
            other => other,
        }
    }
}

/// Attaches the path to an I/O error.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| Error::Io {
            path: path.into(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16, retry_after: Option<u64>) -> Error {
        Error::Http {
            status,
            host: "h".into(),
            path: "/p".into(),
            retry_after,
        }
    }

    #[test]
    fn only_statuses_that_can_pass_are_retried() {
        for status in [408, 429, 500, 503, 599] {
            assert!(http(status, None).is_transient(), "{status}");
        }
        for status in [400, 403, 404, 410, 416] {
            assert!(!http(status, None).is_transient(), "{status}");
        }
    }

    #[test]
    fn a_server_chooses_the_delay_only_within_the_cap() {
        let backoff = Duration::from_secs(2);
        assert_eq!(http(503, None).retry_delay(backoff), backoff);
        assert_eq!(
            http(503, Some(5)).retry_delay(backoff),
            Duration::from_secs(5)
        );
        assert_eq!(
            http(503, Some(86_400)).retry_delay(backoff),
            Duration::from_secs(30)
        );
    }
}
