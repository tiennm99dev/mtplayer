//! The pins in `packages/manifest.json`: metadata only, never a Google file.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::hash::{Sha1Hex, Sha256Hex};
use crate::license::{ANDROID_SDK_LICENSE, GPL_3};
use crate::source::SourcePolicy;

const EMBEDDED: &str = include_str!("../../../packages/manifest.json");
/// Google's license gate only covers stable packages under this channel.
pub const STABLE_CHANNEL: &str = "channel-0";
/// The longest path below an SDK root that any pinned package needs, with a margin. The
/// measured maximum in emulator 37.2.13 and the Android 36 image is 112 characters.
pub const MAX_PACKAGE_PATH_TAIL: usize = 120;
const SET_ID_HASH_DIGITS: usize = 7;

/// The id of one set of Google packages, and the name of its SDK root.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SetId(String);

impl SetId {
    /// Validates a set id. It becomes a directory name, so it can't hold a separator and
    /// can't end in a dot, which Windows would drop. It also can't end in `.json` or
    /// `.leases`, because `<set>.json` and `<set>.leases` are another root's own files.
    pub fn parse(value: &str) -> Result<Self> {
        let mut chars = value.chars();
        let first_ok = chars.next().is_some_and(|c| c.is_ascii_digit());
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        let lower = value.to_ascii_lowercase();
        let names_a_sibling = lower.ends_with(".json") || lower.ends_with(".leases");
        if first_ok && rest_ok && value.len() <= 64 && !value.ends_with('.') && !names_a_sibling {
            Ok(Self(value.to_string()))
        } else {
            Err(Error::InvalidName(value.to_string()))
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Derives `<emulator revision>-<api level>r<image revision>-<zip hashes>` from the pins,
    /// where each zip hash is the first seven digits of a pinned SHA-1, in manifest order. The
    /// hashes are what make any new Google pin land in a new root and never replace files in
    /// use, even a different image or a re-uploaded zip at the same revision.
    pub fn derive(packages: &[PackagePin]) -> Result<Self> {
        let emulator = packages
            .iter()
            .find(|p| p.path == "emulator")
            .ok_or_else(|| Error::Manifest("no emulator pin".into()))?;
        let image = packages
            .iter()
            .find(|p| p.path.starts_with("system-images;"))
            .ok_or_else(|| Error::Manifest("no system image pin".into()))?;
        let api = image
            .path
            .split(';')
            .nth(1)
            .and_then(|s| s.strip_prefix("android-"))
            .ok_or_else(|| Error::Manifest(format!("no API level in {}", image.path)))?;
        let mut id = format!("{}-{}r{}", emulator.revision, api, image.revision);
        for pin in packages {
            id.push('-');
            id.push_str(&pin.sha1.as_str()[..SET_ID_HASH_DIGITS]);
        }
        Self::parse(&id)
    }
}

impl fmt::Display for SetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for SetId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::parse(&value)
    }
}

impl From<SetId> for String {
    fn from(value: SetId) -> String {
        value.0
    }
}

/// One license named by the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LicensePin {
    /// The hash of the text reviewed for this release. Only used to tell an interactive
    /// user that the live text differs; the text itself always comes from Google's XML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_text_sha1: Option<Sha1Hex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_sha256: Option<Sha256Hex>,
}

/// One pinned Google package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackagePin {
    pub path: String,
    pub revision: String,
    pub channel: String,
    pub license: String,
    pub repository_xml: String,
    pub url: String,
    pub size: u64,
    pub sha1: Sha1Hex,
    pub probe: String,
}

impl PackagePin {
    /// The package's directory under an SDK root, in Google's layout.
    pub fn install_dir(&self) -> String {
        self.path.replace(';', "/")
    }

    /// The name the zip is stored under while downloading.
    pub fn file_name(&self) -> &str {
        url_leaf(&self.url)
    }
}

/// The pinned Magisk release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MagiskPin {
    pub repo: String,
    pub tag: String,
    pub license: String,
    pub asset: String,
    pub url: String,
    pub size: u64,
    pub sha256: Sha256Hex,
    pub member: String,
    pub binary_sha256: Sha256Hex,
}

/// The whole manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub set: SetId,
    pub licenses: BTreeMap<String, LicensePin>,
    pub packages: Vec<PackagePin>,
    pub magisk: MagiskPin,
}

impl Manifest {
    /// The manifest compiled into this release. A new pin is a new MTPlayer release.
    pub fn embedded() -> Result<Self> {
        Self::parse(EMBEDDED, &SourcePolicy::production())
    }

    /// Reads a development or test manifest. The same rules apply as for the embedded one.
    pub fn from_path(path: &Path, policy: &SourcePolicy) -> Result<Self> {
        let text = std::fs::read_to_string(path).at(path)?;
        Self::parse(&text, policy)
    }

    /// Parses and validates a manifest against the URLs that `policy` allows.
    pub fn parse(text: &str, policy: &SourcePolicy) -> Result<Self> {
        let manifest: Manifest =
            serde_json::from_str(text).map_err(|e| Error::Manifest(e.to_string()))?;
        manifest.validate(policy)?;
        Ok(manifest)
    }

    /// The text that goes into `packages/manifest.json`: two-space JSON, LF, final newline.
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a manifest always serializes");
        text.push('\n');
        text
    }

    /// Checks every rule a manifest must meet before anything uses it.
    pub fn validate(&self, policy: &SourcePolicy) -> Result<()> {
        let bad = |message: String| Err(Error::Manifest(message));
        if self.schema != 1 {
            return bad(format!("unknown schema {}", self.schema));
        }
        if !self.licenses.contains_key(ANDROID_SDK_LICENSE) || !self.licenses.contains_key(GPL_3) {
            return bad("both licenses must be named".into());
        }
        let google = policy.google_prefix();
        for pin in &self.packages {
            if pin.channel != STABLE_CHANNEL || pin.license != ANDROID_SDK_LICENSE {
                return Err(Error::NotStable {
                    path: pin.path.clone(),
                    channel: pin.channel.clone(),
                    license: pin.license.clone(),
                });
            }
            if !pin.path.split(';').all(is_safe_name) {
                return bad(format!("unsafe package path {}", pin.path));
            }
            if pin.revision.is_empty() || !is_safe_name(&pin.revision) {
                return bad(format!("unsafe revision {}", pin.revision));
            }
            if !is_safe_name(&pin.probe) {
                return bad(format!("unsafe probe {}", pin.probe));
            }
            for url in [&pin.url, &pin.repository_xml] {
                if !url.starts_with(&google) || !is_safe_name(url_leaf(url)) {
                    return bad(format!("{url} is not under {google}"));
                }
            }
        }
        if self.packages.iter().enumerate().any(|(i, a)| {
            self.packages[..i]
                .iter()
                .any(|b| a.path == b.path || a.install_dir().eq_ignore_ascii_case(&b.install_dir()))
        }) {
            return bad("a package is pinned twice".into());
        }

        let derived = SetId::derive(&self.packages)?;
        if derived != self.set {
            return bad(format!(
                "set is {}, but the pins derive {derived}",
                self.set
            ));
        }

        let magisk = &self.magisk;
        if magisk.repo != "topjohnwu/Magisk" {
            return bad(format!("unexpected Magisk repo {}", magisk.repo));
        }
        if magisk.license != GPL_3 {
            return bad(format!("unexpected Magisk license {}", magisk.license));
        }
        if !is_safe_name(&magisk.tag) || !is_safe_name(&magisk.asset) {
            return bad("unsafe Magisk tag or asset".into());
        }
        let prefix = policy.github_release_prefix(&magisk.repo, &magisk.tag);
        if magisk.url != format!("{prefix}{}", magisk.asset) {
            return bad(format!("{} is not under {prefix}", magisk.url));
        }
        if magisk.member.is_empty()
            || !magisk.member.split('/').all(is_safe_name)
            || magisk.member.contains(';')
        {
            return bad(format!("unsafe Magisk member {}", magisk.member));
        }
        let gpl = &self.licenses[GPL_3];
        match &gpl.text_sha256 {
            Some(hash) if *hash == crate::license::gpl3_sha256() => Ok(()),
            _ => bad("the GPL-3.0 hash does not match the embedded text".into()),
        }
    }
}

/// The last path segment of a URL.
pub(crate) fn url_leaf(url: &str) -> &str {
    let no_query = url.split(['?', '#']).next().unwrap_or(url);
    no_query.rsplit('/').next().unwrap_or(no_query)
}

/// True for a single path component that is safe on Windows.
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded_text() -> String {
        EMBEDDED.replace("\r\n", "\n")
    }

    #[test]
    fn embedded_manifest_is_valid_and_pins_only_what_the_product_needs() {
        let m = Manifest::embedded().unwrap();
        assert_eq!(m.set.as_str(), "37.2.13-36r2-4d7268a-829c076");
        let paths: Vec<_> = m.packages.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            paths,
            ["emulator", "system-images;android-36;default;x86_64"]
        );
        assert_eq!(m.packages[0].install_dir(), "emulator");
        assert_eq!(
            m.packages[1].install_dir(),
            "system-images/android-36/default/x86_64"
        );
    }

    #[test]
    fn the_writer_reproduces_the_committed_file_byte_for_byte() {
        assert_eq!(Manifest::embedded().unwrap().to_json(), embedded_text());
    }

    fn with(from: &str, to: &str) -> Result<Manifest> {
        let text = embedded_text();
        assert!(text.contains(from), "fixture text {from} not found");
        Manifest::parse(&text.replace(from, to), &SourcePolicy::production())
    }

    #[test]
    fn a_preview_channel_is_refused() {
        let err = with(r#""channel": "channel-0""#, r#""channel": "channel-2""#).unwrap_err();
        assert!(matches!(err, Error::NotStable { .. }), "{err}");
    }

    #[test]
    fn another_package_license_is_refused() {
        let err = with(
            r#""license": "android-sdk-license""#,
            r#""license": "android-sdk-preview-license""#,
        )
        .unwrap_err();
        assert!(matches!(err, Error::NotStable { .. }), "{err}");
    }

    #[test]
    fn a_set_that_does_not_match_the_pins_is_refused() {
        for set in ["37.2.13-36r3-4d7268a-829c076", "37.2.13-36r2"] {
            let err = with(
                r#""set": "37.2.13-36r2-4d7268a-829c076""#,
                &format!(r#""set": "{set}""#),
            )
            .unwrap_err();
            assert!(err.to_string().contains("derive"), "{err}");
        }
    }

    #[test]
    fn pins_that_differ_only_in_a_zip_hash_derive_different_sets() {
        let first = Manifest::embedded().unwrap().packages;
        for changed in 0..first.len() {
            let mut second = first.clone();
            second[changed].sha1 = Sha1Hex::parse(&"f".repeat(40)).unwrap();
            assert_ne!(
                SetId::derive(&first).unwrap(),
                SetId::derive(&second).unwrap()
            );
        }
    }

    #[test]
    fn urls_outside_the_allowed_sources_are_refused() {
        for (from, to) in [
            (
                "https://dl.google.com/android/repository/emulator",
                "http://dl.google.com/android/repository/emulator",
            ),
            (
                "https://dl.google.com/android/repository/emulator",
                "https://evil.example/android/repository/emulator",
            ),
            (
                "https://dl.google.com/android/repository/sys-img",
                "https://dl.google.com/other/sys-img",
            ),
            (
                "https://github.com/topjohnwu/Magisk/releases/download/v31.1/",
                "https://github.com/someone/Magisk/releases/download/v31.1/",
            ),
        ] {
            assert!(matches!(with(from, to), Err(Error::Manifest(_))), "{to}");
        }
    }

    #[test]
    fn traversal_in_a_package_path_is_refused() {
        let err = with(
            r#""path": "system-images;android-36;default;x86_64""#,
            r#""path": "system-images;android-36;..;x86_64""#,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Manifest(_)), "{err}");
    }

    #[test]
    fn a_bad_hex_length_is_refused() {
        assert!(with("4d7268aa7787c640ad892844e6c9a5f159e91ad3", "4d72").is_err());
    }

    #[test]
    fn unknown_fields_are_refused() {
        assert!(with(r#""schema": 1,"#, r#""schema": 1, "extra": true,"#).is_err());
    }

    #[test]
    fn set_id_rules() {
        assert!(SetId::parse("37.2.13-36r2-4d7268a-829c076").is_ok());
        for bad in ["", "a1", "1/2", "1\\2", "1.", &"1".repeat(65), "1 2"] {
            assert!(SetId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_set_id_can_not_name_another_roots_record_or_leases() {
        for bad in [
            "37.2.13-36r2.json",
            "37.2.13-36r2.leases",
            "37.2.13-36r2.JSON",
            "37.2.13-36r2.Leases",
        ] {
            assert!(SetId::parse(bad).is_err(), "{bad}");
        }
    }
}
