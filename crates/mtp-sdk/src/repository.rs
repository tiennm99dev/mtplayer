//! Reads Google's repository XML. The document is untrusted input, so DTDs stay disabled
//! (roxmltree's default) and everything needed is copied out into plain values.

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::hash::{Sha1Hex, sha1_bytes};
use crate::license::{ANDROID_SDK_LICENSE, LicenseText};
use crate::manifest::{PackagePin, STABLE_CHANNEL};

/// One archive of a package, for the Windows x64 host, with an absolute URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteArchive {
    pub path: String,
    pub revision: String,
    pub url: String,
    pub size: u64,
    pub sha1: Sha1Hex,
    pub channel: String,
    pub license: String,
}

#[derive(Debug)]
struct RemotePackage {
    path: String,
    revision: String,
    is_preview: bool,
    channel: String,
    license: String,
    archive: Option<RawArchive>,
    /// Why this entry can't be used. It stops only a lookup of this package, because one
    /// broken entry among hundreds MTPlayer never asks for says nothing about the pins.
    defect: Option<String>,
}

#[derive(Debug)]
struct RawArchive {
    url: String,
    size: u64,
    checksum: String,
    checksum_type: String,
}

/// A parsed repository XML.
#[derive(Debug)]
pub struct Repository {
    xml_url: String,
    licenses: BTreeMap<String, String>,
    channels: BTreeMap<String, String>,
    packages: Vec<RemotePackage>,
}

impl Repository {
    /// Parses a document fetched from `xml_url`, which relative archive URLs are joined to.
    pub fn parse(xml_url: &str, text: &str) -> Result<Self> {
        let doc = roxmltree::Document::parse(text).map_err(|e| Error::Xml(e.to_string()))?;
        let mut licenses = BTreeMap::new();
        let mut channels = BTreeMap::new();
        let mut packages = Vec::new();
        for node in doc.root_element().children().filter(|n| n.is_element()) {
            match node.tag_name().name() {
                "license" => {
                    if let Some(id) = node.attribute("id") {
                        // All descendant text, untrimmed, is what the acceptance hash covers.
                        let body: String = node
                            .descendants()
                            .filter(|n| n.is_text())
                            .filter_map(|n| n.text())
                            .collect();
                        licenses.insert(id.to_string(), body);
                    }
                }
                "channel" => {
                    if let (Some(id), Some(name)) = (node.attribute("id"), node.text()) {
                        channels.insert(id.to_string(), name.trim().to_string());
                    }
                }
                "remotePackage" => packages.extend(parse_package(node)),
                _ => {}
            }
        }
        if packages.is_empty() || licenses.is_empty() {
            return Err(Error::Xml(format!(
                "{} has no packages or no licenses; has Google changed the format?",
                crate::source::display_url(xml_url)
            )));
        }
        Ok(Self {
            xml_url: xml_url.to_string(),
            licenses,
            channels,
            packages,
        })
    }

    fn is_stable(&self, package: &RemotePackage) -> bool {
        !package.is_preview
            && package.channel == STABLE_CHANNEL
            && package.license == ANDROID_SDK_LICENSE
            && self.channels.get(STABLE_CHANNEL).map(String::as_str) == Some("stable")
    }

    fn remote_archive(&self, package: &RemotePackage) -> Result<Option<RemoteArchive>> {
        let Some(raw) = &package.archive else {
            return Ok(None);
        };
        if !raw.checksum_type.is_empty() && raw.checksum_type != "sha1" {
            return Err(Error::Xml(format!(
                "{} {} uses checksum type {}, which this release doesn't understand",
                package.path, package.revision, raw.checksum_type
            )));
        }
        let sha1 = Sha1Hex::parse(raw.checksum.trim())
            .map_err(|_| Error::Xml(format!("{} has a malformed checksum", package.path)))?;
        Ok(Some(RemoteArchive {
            path: package.path.clone(),
            revision: package.revision.clone(),
            url: self.absolute_url(&raw.url),
            size: raw.size,
            sha1,
            channel: package.channel.clone(),
            license: package.license.clone(),
        }))
    }

    fn absolute_url(&self, url: &str) -> String {
        if url.contains("://") {
            return url.to_string();
        }
        let base = self.xml_url.rfind('/').map_or("", |i| &self.xml_url[..=i]);
        format!("{base}{url}")
    }

    /// Finds the stable Windows x64 archive of a pinned revision. A revision that exists
    /// only outside channel-0 or under another license is `NotStable`, not "missing".
    /// A malformed entry with this path and revision is an error, and so is one with no
    /// revision when nothing else answers, since it could be the one asked for.
    pub fn find_archive(&self, path: &str, revision: &str) -> Result<RemoteArchive> {
        let mut seen: Option<&RemotePackage> = None;
        let mut unidentified: Option<&String> = None;
        for package in self.packages.iter().filter(|p| p.path == path) {
            if package.revision.is_empty() {
                unidentified = package.defect.as_ref();
                continue;
            }
            if package.revision != revision {
                continue;
            }
            if let Some(defect) = &package.defect {
                return Err(Error::Xml(defect.clone()));
            }
            if package.archive.is_none() {
                continue;
            }
            if self.is_stable(package)
                && let Some(archive) = self.remote_archive(package)?
            {
                return Ok(archive);
            }
            seen = Some(package);
        }
        if let Some(defect) = unidentified {
            return Err(Error::Xml(defect.clone()));
        }
        match seen {
            Some(p) => Err(Error::NotStable {
                path: path.to_string(),
                channel: p.channel.clone(),
                license: p.license.clone(),
            }),
            None => Err(Error::PinNotInRepository {
                path: path.to_string(),
                revision: revision.to_string(),
                xml: crate::source::display_url(&self.xml_url),
            }),
        }
    }

    /// Every stable Windows x64 archive of a package, newest revision first.
    pub fn stable_candidates(&self, path: &str) -> Result<Vec<RemoteArchive>> {
        let mut found = Vec::new();
        for package in self.packages.iter().filter(|p| p.path == path) {
            if let Some(defect) = &package.defect {
                return Err(Error::Xml(defect.clone()));
            }
            if self.is_stable(package)
                && let Some(archive) = self.remote_archive(package)?
            {
                found.push(archive);
            }
        }
        found.sort_by_key(|a| std::cmp::Reverse(revision_key(&a.revision)));
        Ok(found)
    }

    /// The license text this document carries for `id`.
    pub fn license_text(&self, id: &str) -> Result<LicenseText> {
        let text = self.licenses.get(id).ok_or_else(|| {
            Error::Xml(format!(
                "{} has no license {id}",
                crate::source::display_url(&self.xml_url)
            ))
        })?;
        Ok(LicenseText::new(
            id,
            text.clone(),
            sha1_bytes(text.as_bytes()),
            crate::source::display_url(&self.xml_url),
        ))
    }
}

/// Compares a pin with what the XML says now and refuses any drift.
pub fn check_pin(pin: &PackagePin, live: &RemoteArchive) -> Result<()> {
    let compare = |field: &str, pinned: &str, value: &str| {
        if pinned == value {
            Ok(())
        } else {
            Err(Error::PinMismatch {
                path: pin.path.clone(),
                field: field.to_string(),
                pinned: pinned.to_string(),
                live: value.to_string(),
            })
        }
    };
    compare("url", &pin.url, &live.url)?;
    compare("size", &pin.size.to_string(), &live.size.to_string())?;
    compare("sha1", pin.sha1.as_str(), live.sha1.as_str())?;
    compare("channel", &pin.channel, &live.channel)?;
    compare("license", &pin.license, &live.license)
}

fn revision_key(revision: &str) -> Vec<u64> {
    revision
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .chain(std::iter::repeat(0))
        .take(4)
        .collect()
}

fn child<'a, 'b>(node: roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}

fn child_text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|n| n.text()).map(str::trim)
}

/// Reads one entry. A malformed one is kept with its defect, for a lookup of it to report. An
/// entry with no path can't be asked for by anyone, so it is `None`.
fn parse_package(node: roxmltree::Node<'_, '_>) -> Option<RemotePackage> {
    let path = node.attribute("path")?.to_string();
    let revision_node = child(node, "revision");
    let revision = revision_node
        .map(|revision| {
            ["major", "minor", "micro"]
                .iter()
                .filter_map(|part| child_text(revision, part))
                .collect::<Vec<_>>()
                .join(".")
        })
        .unwrap_or_default();
    let mut defect = revision
        .is_empty()
        .then(|| format!("{path} has no revision"));
    let reference = |name: &str| {
        child(node, name)
            .and_then(|n| n.attribute("ref"))
            .unwrap_or("none")
            .to_string()
    };
    let archive = match child(node, "archives") {
        Some(archives) => archives
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "archive")
            .find(|a| {
                child_text(*a, "host-os").is_none_or(|os| os == "windows")
                    && child_text(*a, "host-arch").is_none_or(|arch| arch == "x64")
            })
            .and_then(|a| match parse_archive(&path, a) {
                Ok(archive) => Some(archive),
                Err(reason) => {
                    defect.get_or_insert(reason);
                    None
                }
            }),
        None => None,
    };
    Some(RemotePackage {
        is_preview: revision_node.is_some_and(|revision| child(revision, "preview").is_some()),
        channel: reference("channelRef"),
        license: reference("uses-license"),
        path,
        revision,
        archive,
        defect,
    })
}

fn parse_archive(path: &str, node: roxmltree::Node<'_, '_>) -> Result<RawArchive, String> {
    let missing = |what: &str| format!("{path}: archive has no {what}");
    let complete = child(node, "complete").ok_or_else(|| missing("complete element"))?;
    let checksum = child(complete, "checksum").ok_or_else(|| missing("checksum"))?;
    let size = child_text(complete, "size")
        .ok_or_else(|| missing("size"))?
        .parse()
        .map_err(|_| missing("valid size"))?;
    Ok(RawArchive {
        url: child_text(complete, "url")
            .ok_or_else(|| missing("url"))?
            .to_string(),
        size,
        checksum: checksum.text().unwrap_or_default().to_string(),
        checksum_type: checksum.attribute("type").unwrap_or_default().to_string(),
    })
}
