//! Fixture zips, repository XML and manifests.

use std::io::{Cursor, Write};

use mtp_sdk::hash::{Sha1Hex, sha1_bytes, sha256_bytes};
use mtp_sdk::license::gpl3_sha256;
use mtp_sdk::manifest::{LicensePin, MagiskPin, Manifest, PackagePin, SetId};
use mtp_sdk::{ANDROID_SDK_LICENSE, GPL_3, SourcePolicy};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::http::FixtureServer;

/// Bytes that don't compress, so a fixture stays under the ratio limit like a real package.
pub fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state as u8
        })
        .collect()
}

pub fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

pub fn stored() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Stored)
}

/// A zip of `(name, bytes)` files, alternating deflate and stored.
pub fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (index, (name, bytes)) in entries.iter().enumerate() {
        let options = if index % 2 == 0 { deflated() } else { stored() };
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// A package zip: one top-level directory holding `source.properties`, the probe, a notice
/// file, an empty directory and some payload.
pub fn package_zip(top: &str, probe: &str, revision: &str, seed: u32) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let properties = format!("Pkg.Desc=Fixture\nPkg.Revision={revision}\n");
    let payload = noise(64 * 1024, seed);
    writer.add_directory(format!("{top}/"), deflated()).unwrap();
    writer
        .add_directory(format!("{top}/empty/"), deflated())
        .unwrap();
    writer
        .start_file(format!("{top}/source.properties"), deflated())
        .unwrap();
    writer.write_all(properties.as_bytes()).unwrap();
    writer
        .start_file(format!("{top}/{probe}"), stored())
        .unwrap();
    writer.write_all(&payload).unwrap();
    writer
        .start_file(format!("{top}/NOTICE.txt"), deflated())
        .unwrap();
    writer.write_all(b"fixture notices\n").unwrap();
    writer
        .start_file(format!("{top}/lib/data.bin"), deflated())
        .unwrap();
    writer.write_all(&noise(32 * 1024, seed + 1)).unwrap();
    writer.finish().unwrap().into_inner()
}

/// A fake APK: not one top-level directory, as with a real one, and only one member is used.
pub fn fake_apk(member: &str, binary: &[u8]) -> Vec<u8> {
    zip_of(&[
        ("AndroidManifest.xml", b"<manifest/>"),
        (member, binary),
        ("lib/arm64-v8a/libmagisk.so", b"not this one"),
    ])
}

pub const LICENSE_BODY: &str = "Fixture license text\n\nSection 2.4\n";

pub struct PackageSpec<'a> {
    pub path: &'a str,
    pub revision_xml: &'a str,
    pub channel: &'a str,
    pub license: &'a str,
    pub url: &'a str,
    pub size: u64,
    pub sha1: &'a str,
}

pub fn repository_xml(license_body: &str, packages: &[PackageSpec<'_>]) -> String {
    let mut text = format!(
        r#"<?xml version='1.0' encoding='utf-8'?>
<sdk:sdk-repository xmlns:sdk="urn:sdk" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <license id="android-sdk-license" type="text">{license_body}</license>
  <license id="android-sdk-preview-license" type="text">Preview terms</license>
  <channel id="channel-0">stable</channel>
  <channel id="channel-1">beta</channel>
  <channel id="channel-2">dev</channel>
  <channel id="channel-3">canary</channel>
"#
    );
    for package in packages {
        text.push_str(&format!(
            r#"  <remotePackage path="{path}">
    <type-details xsi:type="generic:genericDetailsType"/>
    <revision>{revision}</revision>
    <display-name>Fixture</display-name>
    <uses-license ref="{license}"/>
    <channelRef ref="{channel}"/>
    <archives>
      <archive>
        <complete><size>999</size><checksum type="sha1">1111111111111111111111111111111111111111</checksum><url>wrong-linux.zip</url></complete>
        <host-os>linux</host-os><host-arch>x64</host-arch>
      </archive>
      <archive>
        <complete><size>{size}</size><checksum type="sha1">{sha1}</checksum><url>{url}</url></complete>
        <host-os>windows</host-os><host-arch>x64</host-arch>
      </archive>
    </archives>
  </remotePackage>
"#,
            path = package.path,
            revision = package.revision_xml,
            license = package.license,
            channel = package.channel,
            size = package.size,
            sha1 = package.sha1,
            url = package.url,
        ));
    }
    text.push_str("</sdk:sdk-repository>\n");
    text
}

/// A full fake world on one fixture server: an emulator, a system image, a Magisk release
/// and the XML that lists them.
pub struct World {
    pub server: FixtureServer,
    pub manifest: Manifest,
    pub license_sha1: Sha1Hex,
}

pub const EMULATOR_ZIP_PATH: &str = "/android/repository/emulator-fixture.zip";
pub const IMAGE_ZIP_PATH: &str = "/android/repository/sys-img/android/x86_64-fixture.zip";
pub const MAIN_XML_PATH: &str = "/android/repository/repository2-3.xml";
pub const IMAGE_XML_PATH: &str = "/android/repository/sys-img/android/sys-img2-3.xml";
pub const MAGISK_MEMBER: &str = "lib/x86_64/libmagisk.so";

pub fn revision_xml(revision: &str) -> String {
    let names = ["major", "minor", "micro"];
    revision
        .split('.')
        .zip(names)
        .map(|(value, name)| format!("<{name}>{value}</{name}>"))
        .collect()
}

pub fn world() -> World {
    world_with_image("2", 10)
}

/// Serves a world with this image revision. The emulator zip stays the same, so two worlds
/// on one server can share it.
pub fn world_with_image(image_revision: &str, seed: u32) -> World {
    let server = FixtureServer::start();
    let manifest = serve_set(&server, image_revision, seed);
    World {
        server,
        manifest,
        license_sha1: sha1_bytes(LICENSE_BODY.as_bytes()),
    }
}

/// Registers a set on `server` and returns its manifest.
pub fn serve_set(server: &FixtureServer, image_revision: &str, seed: u32) -> Manifest {
    let emulator = package_zip("emulator", "emulator.exe", "37.2.13", 1);
    let image_path = format!("/android/repository/sys-img/android/x86_64-r{image_revision}.zip");
    let image = package_zip("x86_64", "system.img", image_revision, seed);
    let binary = noise(4096, 77);
    let apk = fake_apk(MAGISK_MEMBER, &binary);

    let emulator_sha1 = sha1_bytes(&emulator);
    let image_sha1 = sha1_bytes(&image);
    let main_xml = repository_xml(
        LICENSE_BODY,
        &[PackageSpec {
            path: "emulator",
            revision_xml: &revision_xml("37.2.13"),
            channel: "channel-0",
            license: ANDROID_SDK_LICENSE,
            url: "emulator-fixture.zip",
            size: emulator.len() as u64,
            sha1: emulator_sha1.as_str(),
        }],
    );
    let image_xml = repository_xml(
        LICENSE_BODY,
        &[PackageSpec {
            path: "system-images;android-36;default;x86_64",
            revision_xml: &revision_xml(image_revision),
            channel: "channel-0",
            license: ANDROID_SDK_LICENSE,
            url: image_path.rsplit('/').next().unwrap(),
            size: image.len() as u64,
            sha1: image_sha1.as_str(),
        }],
    );

    let policy = SourcePolicy::loopback(server.port());
    let google = policy.google_prefix();
    let magisk_prefix = policy.github_release_prefix("topjohnwu/Magisk", "v1.0");
    let magisk_url = format!("{magisk_prefix}Magisk-v1.0.apk");
    let packages = vec![
        PackagePin {
            path: "emulator".to_string(),
            revision: "37.2.13".to_string(),
            channel: "channel-0".to_string(),
            license: ANDROID_SDK_LICENSE.to_string(),
            repository_xml: format!("{google}repository2-3.xml"),
            url: format!("{google}emulator-fixture.zip"),
            size: emulator.len() as u64,
            sha1: emulator_sha1,
            probe: "emulator.exe".to_string(),
        },
        PackagePin {
            path: "system-images;android-36;default;x86_64".to_string(),
            revision: image_revision.to_string(),
            channel: "channel-0".to_string(),
            license: ANDROID_SDK_LICENSE.to_string(),
            repository_xml: format!("{google}sys-img/android/sys-img2-3.xml"),
            url: server.url(&image_path),
            size: image.len() as u64,
            sha1: image_sha1,
            probe: "system.img".to_string(),
        },
    ];
    server.body(EMULATOR_ZIP_PATH, emulator);
    server.body(&image_path, image);
    server.body(MAIN_XML_PATH, main_xml.into_bytes());
    server.body(IMAGE_XML_PATH, image_xml.into_bytes());
    server.body(&magisk_url[server.url("").len()..], apk.clone());

    let manifest = Manifest {
        schema: 1,
        set: SetId::derive(&packages).unwrap(),
        licenses: [
            (
                ANDROID_SDK_LICENSE.to_string(),
                LicensePin {
                    reviewed_text_sha1: Some(sha1_bytes(LICENSE_BODY.as_bytes())),
                    reviewed_on: Some("2026-11-01".to_string()),
                    file: None,
                    text_sha256: None,
                },
            ),
            (
                GPL_3.to_string(),
                LicensePin {
                    reviewed_text_sha1: None,
                    reviewed_on: None,
                    file: Some("packages/licenses/gpl-3.0.txt".to_string()),
                    text_sha256: Some(gpl3_sha256()),
                },
            ),
        ]
        .into_iter()
        .collect(),
        packages,
        magisk: MagiskPin {
            repo: "topjohnwu/Magisk".to_string(),
            tag: "v1.0".to_string(),
            license: GPL_3.to_string(),
            asset: "Magisk-v1.0.apk".to_string(),
            url: magisk_url,
            size: apk.len() as u64,
            sha256: sha256_bytes(&apk),
            member: MAGISK_MEMBER.to_string(),
            binary_sha256: sha256_bytes(&binary),
        },
    };
    manifest.validate(&policy).unwrap();
    manifest
}
