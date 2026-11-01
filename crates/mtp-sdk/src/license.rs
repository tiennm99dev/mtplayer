//! The license gate.
//!
//! Records are MTPlayer's own, not Google's format, and Google's tools don't read them.
//! The SHA-1 detects a changed text and nothing more.
//!
//! No function accepts an id or a hash alone: recording an acceptance takes a `LicenseText`,
//! which only this crate can build, from the live XML or the embedded GPL text. Every
//! caller therefore holds the exact text it accepts.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsutil;
use crate::hash::{Sha1Hex, Sha256Hex, sha1_bytes, sha256_bytes};

/// The id of Google's stable license.
pub const ANDROID_SDK_LICENSE: &str = "android-sdk-license";
/// The id of Magisk's license.
pub const GPL_3: &str = "gpl-3.0";

const GPL_TEXT: &str = include_str!("../../../packages/licenses/gpl-3.0.txt");
const GPL_SOURCE: &str = "the LICENSE file of https://github.com/topjohnwu/Magisk";

/// The exact text of a license, and where it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LicenseText {
    id: String,
    text: String,
    sha1: Sha1Hex,
    source: String,
}

impl LicenseText {
    pub(crate) fn new(id: &str, text: String, sha1: Sha1Hex, source: String) -> Self {
        Self {
            id: id.to_string(),
            text,
            sha1,
            source,
        }
    }

    /// The license id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The whole text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The SHA-1 of the text's UTF-8 bytes.
    pub fn sha1(&self) -> &Sha1Hex {
        &self.sha1
    }

    /// Where the text was read from.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// Magisk's GPL-3.0 text, byte for byte. Its APK carries no license file and the GPL
/// permits verbatim copies, so the repository keeps one for the gate.
pub fn gpl3_text() -> LicenseText {
    LicenseText::new(
        GPL_3,
        GPL_TEXT.to_string(),
        sha1_bytes(GPL_TEXT.as_bytes()),
        GPL_SOURCE.to_string(),
    )
}

/// The SHA-256 of the embedded GPL text.
pub fn gpl3_sha256() -> Sha256Hex {
    sha256_bytes(GPL_TEXT.as_bytes())
}

/// On whose behalf the license was accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Authority {
    Personal,
    /// The user confirmed they have the authority to bind their employer (§2.4).
    Employer,
    /// An acknowledgement; no employer question was asked.
    NotStated,
}

/// How the acceptance was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Prompt,
    Flag,
    Gui,
}

/// What a prompt decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Accept { authority: Authority, via: Via },
    Decline,
}

/// A way to ask for an acceptance: the CLI's prompt, a GUI dialog, or `PreAccepted`.
pub trait LicensePrompt {
    /// False when this prompt can't decide `id` in this run, which stops the install
    /// before any network request.
    fn available(&self, id: &str) -> bool;

    /// Shows `text` and returns the decision. `reviewed` is the hash reviewed for this
    /// MTPlayer release, when one exists.
    fn decide(&mut self, text: &LicenseText, reviewed: Option<&Sha1Hex>) -> Result<Decision>;
}

/// The non-interactive path: acceptance of one exact text version per license. A flag can't
/// prove a human read the text. It enforces specificity, an explicit authority statement
/// and an auditable record.
#[derive(Clone, Debug)]
pub struct PreAccepted {
    accepted: BTreeMap<String, Sha1Hex>,
    authority: Authority,
}

impl PreAccepted {
    /// Accepts each listed `(id, sha1)` pair on behalf of `authority`.
    pub fn new(accepted: BTreeMap<String, Sha1Hex>, authority: Authority) -> Self {
        Self {
            accepted,
            authority,
        }
    }
}

impl LicensePrompt for PreAccepted {
    fn available(&self, id: &str) -> bool {
        self.accepted.contains_key(id)
    }

    fn decide(&mut self, text: &LicenseText, _reviewed: Option<&Sha1Hex>) -> Result<Decision> {
        let Some(given) = self.accepted.get(text.id()) else {
            return Err(Error::LicenseNotAccepted {
                id: text.id().to_string(),
            });
        };
        if given != text.sha1() {
            return Err(Error::LicenseTextChanged {
                id: text.id().to_string(),
                given: given.to_string(),
            });
        }
        Ok(Decision::Accept {
            authority: self.authority,
            via: Via::Flag,
        })
    }
}

/// An acceptance record: `licenses/<id>.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseRecord {
    pub schema: u32,
    pub license_id: String,
    pub text_sha1: Sha1Hex,
    pub text_source: String,
    pub accepted_at: String,
    pub authority: Authority,
    pub via: Via,
    pub mtplayer_version: String,
    pub text: String,
}

/// A license's state against one text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LicenseState {
    Accepted(Box<LicenseRecord>),
    NotAccepted,
    TextChanged(Box<LicenseRecord>),
}

/// The acceptance records of one home.
#[derive(Clone, Debug)]
pub struct LicenseStore {
    dir: PathBuf,
}

impl LicenseStore {
    /// A store over `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// The current record. An unreadable or inconsistent record counts as no record, so a
    /// damaged file can only make the gate ask again.
    pub fn current(&self, id: &str) -> Option<LicenseRecord> {
        let record: LicenseRecord = fsutil::read_json(&self.path(id)).ok().flatten()?;
        let consistent = record.schema == 1
            && record.license_id == id
            && sha1_bytes(record.text.as_bytes()) == record.text_sha1;
        consistent.then_some(record)
    }

    /// Compares the current record with a live text.
    pub fn state(&self, text: &LicenseText) -> LicenseState {
        match self.current(text.id()) {
            None => LicenseState::NotAccepted,
            Some(record) if record.text_sha1 == *text.sha1() => {
                LicenseState::Accepted(Box::new(record))
            }
            Some(record) => LicenseState::TextChanged(Box::new(record)),
        }
    }

    /// Records an acceptance of exactly this text. A previous record for another text is
    /// kept beside it as `<id>.<first 8 of its sha1>.json`.
    pub fn record(
        &self,
        text: &LicenseText,
        authority: Authority,
        via: Via,
    ) -> Result<LicenseRecord> {
        fs::create_dir_all(&self.dir).at(&self.dir)?;
        if let Some(previous) = self.current(text.id())
            && previous.text_sha1 != *text.sha1()
        {
            let kept = self.dir.join(format!(
                "{}.{}.json",
                text.id(),
                previous.text_sha1.prefix()
            ));
            fsutil::rename_with_retry(&self.path(text.id()), &kept)?;
        }
        let record = LicenseRecord {
            schema: 1,
            license_id: text.id().to_string(),
            text_sha1: text.sha1().clone(),
            text_source: text.source().to_string(),
            accepted_at: fsutil::now(),
            authority,
            via,
            mtplayer_version: env!("CARGO_PKG_VERSION").to_string(),
            text: text.text().to_string(),
        };
        fsutil::write_json_atomic(&self.path(text.id()), &record)?;
        Ok(record)
    }
}
