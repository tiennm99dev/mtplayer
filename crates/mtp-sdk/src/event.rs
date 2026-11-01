//! Progress events and cancellation, shared by the CLI's printer and a future GUI.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Error, Result};

const MIB: u64 = 1 << 20;

/// Something the installer did or is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    License {
        id: String,
        state: String,
    },
    Reuse {
        name: String,
        detail: String,
    },
    Download {
        name: String,
        source: String,
        resume_from: Option<u64>,
    },
    Progress {
        name: String,
        received: u64,
        total: u64,
    },
    Retry {
        name: String,
        reason: String,
    },
    Verified {
        name: String,
        algorithm: String,
    },
    Extract {
        name: String,
    },
    Compare {
        name: String,
        files: usize,
    },
    Install {
        name: String,
        dest: String,
    },
    /// Something went wrong that doesn't stop the install.
    Warning {
        message: String,
    },
    Done {
        downloads: u32,
    },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::License { id, state } => write!(f, "license  {id} {state}"),
            Event::Reuse { name, detail } => write!(f, "reuse    {name} ({detail})"),
            Event::Download {
                name,
                source,
                resume_from: None,
            } => {
                write!(f, "download {name} from {source}")
            }
            Event::Download {
                name,
                source,
                resume_from: Some(from),
            } => {
                write!(
                    f,
                    "download {name} from {source}, resume from {} MiB",
                    from / MIB
                )
            }
            Event::Progress {
                name,
                received,
                total,
            } => {
                write!(
                    f,
                    "progress {name} {} of {} MiB",
                    received / MIB,
                    total / MIB
                )
            }
            Event::Retry { name, reason } => write!(f, "retry    {name}: {reason}"),
            Event::Verified { name, algorithm } => write!(f, "verified {name} ({algorithm} ok)"),
            Event::Extract { name } => write!(f, "extract  {name}"),
            Event::Compare { name, files } => write!(f, "compare  {name} ({files} files match)"),
            Event::Install { name, dest } => write!(f, "install  {name} -> {dest}"),
            Event::Warning { message } => write!(f, "warning  {message}"),
            Event::Done { downloads } => write!(f, "done     downloads={downloads}"),
        }
    }
}

/// A flag that stops a transfer. A cancelled download keeps its partial file for resuming.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// A flag that isn't set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks the running operation to stop at its next check.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.0.load(Ordering::SeqCst) {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    }
}
