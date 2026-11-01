//! Proposes a new `packages/manifest.json` from the latest stable entries in Google's
//! repository XML and Magisk's latest GitHub release, for a person to review.
//!
//! By default it only prints the proposal. It writes only when given `--write`, which a
//! person runs after reading the differences, and the result is then reviewed as a git diff.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use mtp_sdk::archive::extract_member;
use mtp_sdk::download::DownloadRequest;
use mtp_sdk::hash::{Digest, Sha256Hex, sha256_bytes, sha256_file};
use mtp_sdk::license::gpl3_sha256;
use mtp_sdk::manifest::{MagiskPin, Manifest, PackagePin, SetId, is_safe_name};
use mtp_sdk::{ANDROID_SDK_LICENSE, Cancel, Client, Repository, SourcePolicy};
use serde::Deserialize;
use serde_json::Value;

const MAX_METADATA_BYTES: u64 = 16 << 20;

#[derive(Parser)]
#[command(
    name = "pin-update",
    about = "Proposes new pins for a person to review"
)]
struct Args {
    /// The manifest to compare with.
    #[arg(long, default_value = "packages/manifest.json")]
    manifest: PathBuf,
    /// Where a new Magisk APK is downloaded while its binary is hashed.
    #[arg(long, default_value = ".tmp/pin-update")]
    work_dir: PathBuf,
    /// Replace the manifest with the proposal. Run this only after reading the differences.
    #[arg(long)]
    write: bool,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

/// The asset the pin needs, from a stable release that publishes its hash.
#[derive(Debug, PartialEq, Eq)]
struct MagiskAsset {
    tag: String,
    asset: String,
    url: String,
    size: u64,
    sha256: Sha256Hex,
}

/// The response is untrusted input, because a TLS-inspecting proxy that the platform trusts
/// could forge it. The tag and the asset become paths under the work folder, and the URL
/// gets a request, so all three are checked here, before anything uses them.
fn select_magisk_asset(release: &Release, repo: &str) -> Result<MagiskAsset> {
    ensure!(
        !release.draft && !release.prerelease,
        "the latest Magisk release {} is not stable",
        release.tag_name
    );
    let name = format!("Magisk-{}.apk", release.tag_name);
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .with_context(|| format!("release {} has no {name}", release.tag_name))?;
    let digest = asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .with_context(|| format!("{name} has no sha256 digest"))?;
    ensure!(
        is_safe_name(&release.tag_name) && is_safe_name(&asset.name),
        "the release names an unsafe tag or asset: {:?}, {:?}",
        release.tag_name,
        asset.name
    );
    let expected_url = format!(
        "https://github.com/{repo}/releases/download/{}/{}",
        release.tag_name, asset.name
    );
    ensure!(
        asset.browser_download_url == expected_url,
        "{name} is served from {}, not {expected_url}",
        mtp_sdk::display_url(&asset.browser_download_url)
    );
    Ok(MagiskAsset {
        tag: release.tag_name.clone(),
        asset: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sha256: Sha256Hex::parse(digest)?,
    })
}

/// Picks the newest stable archive of a pinned package. The stable filter is the repository
/// reader's, so a preview revision can never be proposed.
fn propose_package(current: &PackagePin, repository: &Repository) -> Result<PackagePin> {
    let newest = repository
        .stable_candidates(&current.path)?
        .into_iter()
        .next()
        .with_context(|| {
            format!(
                "no channel-0 {ANDROID_SDK_LICENSE} entry for {}",
                current.path
            )
        })?;
    Ok(PackagePin {
        revision: newest.revision,
        channel: newest.channel,
        license: newest.license,
        url: newest.url,
        size: newest.size,
        sha1: newest.sha1,
        ..current.clone()
    })
}

/// One line per field that differs, as `path: old -> new`.
fn differences(old: &Value, new: &Value) -> Vec<String> {
    fn walk(path: &str, old: Option<&Value>, new: Option<&Value>, out: &mut Vec<String>) {
        match (old, new) {
            (Some(Value::Object(a)), Some(Value::Object(b))) => {
                let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
                keys.sort();
                keys.dedup();
                for key in keys {
                    walk(&format!("{path}.{key}"), a.get(key), b.get(key), out);
                }
            }
            (Some(Value::Array(a)), Some(Value::Array(b))) => {
                for i in 0..a.len().max(b.len()) {
                    walk(&format!("{path}[{i}]"), a.get(i), b.get(i), out);
                }
            }
            (a, b) if a == b => {}
            (a, b) => {
                let show = |v: Option<&Value>| v.map_or("(none)".to_string(), Value::to_string);
                out.push(format!(
                    "{}: {} -> {}",
                    path.trim_start_matches('.'),
                    show(a),
                    show(b)
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk("", Some(old), Some(new), &mut out);
    out
}

fn fetch_repositories(client: &Client, current: &Manifest) -> Result<Vec<(String, Repository)>> {
    let mut repositories: Vec<(String, Repository)> = Vec::new();
    for pin in &current.packages {
        if repositories
            .iter()
            .any(|(url, _)| *url == pin.repository_xml)
        {
            continue;
        }
        let text = client.get_text(&pin.repository_xml, MAX_METADATA_BYTES)?;
        let repository = Repository::parse(&pin.repository_xml, &text)?;
        repositories.push((pin.repository_xml.clone(), repository));
    }
    Ok(repositories)
}

/// Hashes the member of a newly pinned APK. An unchanged APK keeps its known binary hash, so
/// nothing is downloaded when the release hasn't moved.
fn binary_hash(
    client: &Client,
    current: &MagiskPin,
    asset: &MagiskAsset,
    work_dir: &Path,
) -> Result<Sha256Hex> {
    if asset.sha256 == current.sha256 {
        return Ok(current.binary_sha256.clone());
    }
    let dir = work_dir.join(&asset.tag);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let downloaded = client.download(
        &DownloadRequest {
            url: asset.url.clone(),
            dest: dir.join(&asset.asset),
            size: asset.size,
            digest: Digest::Sha256(asset.sha256.clone()),
        },
        &mut |event| {
            if !matches!(event, mtp_sdk::Event::Progress { .. }) {
                println!("{event}");
            }
        },
        &Cancel::new(),
    )?;
    let binary = dir.join("binary-to-hash");
    if binary.exists() {
        std::fs::remove_file(&binary).with_context(|| binary.display().to_string())?;
    }
    let apk = std::fs::File::open(&downloaded.path)
        .with_context(|| downloaded.path.display().to_string())?;
    extract_member(&apk, &current.member, &binary)?;
    Ok(sha256_file(&binary)?)
}

fn check_gpl_text(client: &Client, tag: &str) -> Result<()> {
    let url = format!("https://raw.githubusercontent.com/topjohnwu/Magisk/{tag}/LICENSE");
    let text = client.get_text(&url, MAX_METADATA_BYTES)?;
    let live = sha256_bytes(text.as_bytes());
    if live != gpl3_sha256() {
        bail!(
            "Magisk's LICENSE at {tag} has sha256 {live}, not the embedded text's {}. Review it, \
             then replace packages/licenses/gpl-3.0.txt byte for byte",
            gpl3_sha256()
        );
    }
    Ok(())
}

fn propose(client: &Client, current: &Manifest, work_dir: &Path, today: &str) -> Result<Manifest> {
    let repositories = fetch_repositories(client, current)?;
    let find = |url: &str| {
        repositories
            .iter()
            .find(|(u, _)| u == url)
            .map(|(_, r)| r)
            .context("repository XML not fetched")
    };

    let mut proposal = current.clone();
    proposal.packages = current
        .packages
        .iter()
        .map(|pin| propose_package(pin, find(&pin.repository_xml)?))
        .collect::<Result<_>>()?;
    proposal.set = SetId::derive(&proposal.packages)?;

    let first = repositories.first().context("no repository XML")?;
    let sha1 = first.1.license_text(ANDROID_SDK_LICENSE)?.sha1().clone();
    for (url, repository) in &repositories {
        let other = repository.license_text(ANDROID_SDK_LICENSE)?;
        ensure!(
            other.sha1() == &sha1,
            "{url} carries another {ANDROID_SDK_LICENSE} text ({}, not {sha1}); review before pinning",
            other.sha1()
        );
    }
    let license = proposal
        .licenses
        .get_mut(ANDROID_SDK_LICENSE)
        .context("no android-sdk-license entry")?;
    if license.reviewed_text_sha1.as_ref() != Some(&sha1) {
        println!(
            "note     the {ANDROID_SDK_LICENSE} text changed; read it before accepting this proposal"
        );
        license.reviewed_text_sha1 = Some(sha1);
        license.reviewed_on = Some(today.to_string());
    }

    let release_text = client.get_text(
        &format!(
            "https://api.github.com/repos/{}/releases/latest",
            current.magisk.repo
        ),
        MAX_METADATA_BYTES,
    )?;
    let release: Release =
        serde_json::from_str(&release_text).context("the GitHub release JSON")?;
    let asset = select_magisk_asset(&release, &current.magisk.repo)?;
    check_gpl_text(client, &asset.tag)?;
    proposal.magisk.binary_sha256 = binary_hash(client, &current.magisk, &asset, work_dir)?;
    proposal.magisk.tag = asset.tag;
    proposal.magisk.asset = asset.asset;
    proposal.magisk.url = asset.url;
    proposal.magisk.size = asset.size;
    proposal.magisk.sha256 = asset.sha256;

    proposal
        .validate(&SourcePolicy::production())
        .context("the proposal breaks a manifest rule; refusing it")?;
    Ok(proposal)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let policy = SourcePolicy::production();
    let current = Manifest::from_path(&args.manifest, &policy)?;
    let client = Client::new(SourcePolicy::pin_update());
    let today = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let proposal = propose(&client, &current, &args.work_dir, &today)?;

    let changes = differences(
        &serde_json::to_value(&current)?,
        &serde_json::to_value(&proposal)?,
    );
    if changes.is_empty() {
        println!(
            "no changes: {} matches the latest stable pins",
            args.manifest.display()
        );
        return Ok(());
    }
    println!("proposed changes to {}:", args.manifest.display());
    for change in &changes {
        println!("  {change}");
    }
    println!();
    print!("{}", proposal.to_json());
    if args.write {
        std::fs::write(&args.manifest, proposal.to_json())
            .with_context(|| args.manifest.display().to_string())?;
        println!(
            "wrote {}; review it with git diff before committing",
            args.manifest.display()
        );
    } else {
        println!("nothing was written; run again with --write after reviewing");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "topjohnwu/Magisk";
    const XML: &str = r#"<?xml version="1.0"?>
<r:repo xmlns:r="urn:x">
  <license id="android-sdk-license" type="text">abc</license>
  <channel id="channel-0">stable</channel>
  <channel id="channel-2">dev</channel>
  <remotePackage path="emulator">
    <revision><major>37</major><minor>2</minor><micro>13</micro></revision>
    <uses-license ref="android-sdk-license"/>
    <channelRef ref="channel-0"/>
    <archives><archive>
      <complete><size>1</size><checksum type="sha1">1111111111111111111111111111111111111111</checksum><url>old.zip</url></complete>
      <host-os>windows</host-os><host-arch>x64</host-arch>
    </archive></archives>
  </remotePackage>
  <remotePackage path="emulator">
    <revision><major>37</major><minor>10</minor><micro>0</micro></revision>
    <uses-license ref="android-sdk-license"/>
    <channelRef ref="channel-0"/>
    <archives><archive>
      <complete><size>2</size><checksum type="sha1">2222222222222222222222222222222222222222</checksum><url>new.zip</url></complete>
      <host-os>windows</host-os><host-arch>x64</host-arch>
    </archive></archives>
  </remotePackage>
  <remotePackage path="emulator">
    <revision><major>99</major><minor>0</minor><micro>0</micro></revision>
    <uses-license ref="android-sdk-license"/>
    <channelRef ref="channel-2"/>
    <archives><archive>
      <complete><size>3</size><checksum type="sha1">3333333333333333333333333333333333333333</checksum><url>canary.zip</url></complete>
      <host-os>windows</host-os><host-arch>x64</host-arch>
    </archive></archives>
  </remotePackage>
  <remotePackage path="only-preview">
    <revision><major>1</major></revision>
    <uses-license ref="android-sdk-preview-license"/>
    <channelRef ref="channel-0"/>
    <archives><archive>
      <complete><size>4</size><checksum type="sha1">4444444444444444444444444444444444444444</checksum><url>p.zip</url></complete>
    </archive></archives>
  </remotePackage>
</r:repo>"#;

    fn repository() -> Repository {
        Repository::parse(
            "https://dl.google.com/android/repository/repository2-3.xml",
            XML,
        )
        .unwrap()
    }

    fn pin(path: &str) -> PackagePin {
        let mut pin = Manifest::embedded().unwrap().packages.remove(0);
        pin.path = path.to_string();
        pin
    }

    #[test]
    fn the_newest_stable_revision_wins_numerically_and_a_canary_never_does() {
        let proposed = propose_package(&pin("emulator"), &repository()).unwrap();
        assert_eq!(proposed.revision, "37.10.0");
        assert_eq!(
            proposed.url,
            "https://dl.google.com/android/repository/new.zip"
        );
        assert_eq!(proposed.size, 2);
        assert_eq!(proposed.probe, "emulator.exe");
    }

    #[test]
    fn a_package_without_a_stable_entry_is_an_error() {
        assert!(propose_package(&pin("only-preview"), &repository()).is_err());
        assert!(propose_package(&pin("missing"), &repository()).is_err());
    }

    fn release(tag: &str, draft: bool, prerelease: bool, digest: Option<&str>) -> Release {
        Release {
            tag_name: tag.to_string(),
            draft,
            prerelease,
            assets: vec![ReleaseAsset {
                name: format!("Magisk-{tag}.apk"),
                size: 7,
                browser_download_url: format!(
                    "https://github.com/topjohnwu/Magisk/releases/download/{tag}/Magisk-{tag}.apk"
                ),
                digest: digest.map(str::to_string),
            }],
        }
    }

    #[test]
    fn a_stable_release_with_a_digest_is_selected() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let asset =
            select_magisk_asset(&release("v32.0", false, false, Some(&digest)), REPO).unwrap();
        assert_eq!(asset.tag, "v32.0");
        assert_eq!(asset.size, 7);
    }

    #[test]
    fn drafts_prereleases_and_missing_digests_are_refused() {
        let digest = format!("sha256:{}", "a".repeat(64));
        for (draft, prerelease, digest) in [
            (true, false, Some(digest.as_str())),
            (false, true, Some(digest.as_str())),
            (false, false, None),
            (false, false, Some("md5:abc")),
        ] {
            let release = release("v32.0", draft, prerelease, digest);
            assert!(select_magisk_asset(&release, REPO).is_err());
        }
    }

    #[test]
    fn a_tag_that_could_leave_the_work_folder_is_refused() {
        let digest = format!("sha256:{}", "a".repeat(64));
        for tag in ["..\\x", "../x", "..", "a/b", "C:\\x"] {
            let err =
                select_magisk_asset(&release(tag, false, false, Some(&digest)), REPO).unwrap_err();
            assert!(err.to_string().contains("unsafe"), "{tag}: {err}");
        }
    }

    #[test]
    fn a_download_url_anywhere_but_the_pinned_repo_is_refused() {
        let digest = format!("sha256:{}", "a".repeat(64));
        for url in [
            "https://evil.example/topjohnwu/Magisk/releases/download/v32.0/Magisk-v32.0.apk",
            "https://github.com/someone/Magisk/releases/download/v32.0/Magisk-v32.0.apk",
            "http://github.com/topjohnwu/Magisk/releases/download/v32.0/Magisk-v32.0.apk",
            "https://github.com/topjohnwu/Magisk/releases/download/v31.0/Magisk-v32.0.apk",
        ] {
            let mut forged = release("v32.0", false, false, Some(&digest));
            forged.assets[0].browser_download_url = url.to_string();
            let err = select_magisk_asset(&forged, REPO).unwrap_err();
            assert!(err.to_string().contains("served from"), "{url}: {err}");
        }
    }

    #[test]
    fn differences_name_each_changed_field() {
        let current = Manifest::embedded().unwrap();
        let mut changed = current.clone();
        changed.packages[0].size = 1;
        changed.magisk.tag = "v99".to_string();
        let lines = differences(
            &serde_json::to_value(&current).unwrap(),
            &serde_json::to_value(&changed).unwrap(),
        );
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("packages[0].size: 455344678 -> 1"))
        );
        assert!(lines.iter().any(|l| l.contains("magisk.tag")));
        assert!(
            differences(
                &serde_json::to_value(&current).unwrap(),
                &serde_json::to_value(&current).unwrap()
            )
            .is_empty()
        );
    }

    #[test]
    fn a_proposal_with_a_preview_channel_breaks_the_manifest_rules() {
        let mut manifest = Manifest::embedded().unwrap();
        manifest.packages[0].channel = "channel-2".to_string();
        assert!(manifest.validate(&SourcePolicy::production()).is_err());
    }
}
