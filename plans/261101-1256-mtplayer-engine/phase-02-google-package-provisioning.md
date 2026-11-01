---
phase: 2
title: "Google package provisioning"
status: pending
priority: P1
effort: "2-3 weeks"
dependencies: [1]
---

# Phase 2: Google package provisioning

## Goal

Get Google's emulator and Android 16 image, and the Magisk binary used for
`resetprop`, onto a user's machine legally and reproducibly. MTPlayer never
ships them. The user accepts each license, then MTPlayer downloads the pinned
versions, checks them and stores them in its own folder.

## Requirements

- **Pins** in `packages/manifest.json`, in the repo: for each package, the
  SDK path (for example `system-images;android-36;default;x86_64`), the
  revision, the URL, the size and the hash taken from Google's repository
  XML. Magisk is pinned by release tag and SHA-256. The manifest holds only
  metadata, never a Google file.
- **Stable only:** the pin tool and the downloader refuse any package that
  isn't on channel-0 (stable), or whose `uses-license` isn't
  `android-sdk-license`.
- **License gate:**
  - before the first download, show the full license text embedded in the
    repository XML, and Magisk's license;
  - the dialog asks the user to confirm that, when installing for work, they
    may accept the license for their employer (§2.4);
  - store each acceptance in Google's format, as the SHA-1 of the exact
    license text, on its own line in `<sdk>/licenses/<license-id>`, so
    Google's own tools recognize it. Record a timestamp in the store as
    well;
  - when the text changes, its hash changes, so the gate asks again;
  - nothing downloads without an acceptance.
- **Notices:** keep the `LICENSE` and `NOTICE` files that ship in each
  package, and show them in the GUI's about screen. Never remove or hide
  them (§3.8).
- **Download:**
  - from dl.google.com and Magisk's GitHub release only, over HTTPS;
  - resumable, with the hash checked before extraction;
  - each zip extracted into its own empty directory, then moved into place;
  - an existing, verified copy is reused, so updates never re-download
    unchanged packages.
- **Layout:** a private SDK root at `%LOCALAPPDATA%\MTPlayer\sdk`, in the
  layout Phase 1 proved the emulator accepts. `ANDROID_SDK_ROOT` points
  there for every emulator process. MTPlayer never uses or writes the user's
  own Android SDK.
- **Updates:** a new pin is a new MTPlayer release. The previous version
  stays installed until no instance uses it. If Google withdraws a pinned
  zip, existing copies keep working and the downloader reports the missing
  pin clearly.
- **No modification:** files are used exactly as downloaded. A check after
  extraction compares every file against the zip.

## Tasks & Steps

1. Write the repository-XML reader that finds a package's archive for
   Windows x64, its URL, size and hash, and its license text.
2. Write the `mtp-sdk` crate: license gate, download, verify, extract,
   install and remove.
3. Add `mtp sdk status|install|remove` to the CLI, so Phase 4 can provision
   before creating instances.
4. Write `tools/pin-update` to propose a new manifest from the latest
   repository XML, for a person to review.
5. Write integration tests against the real dl.google.com endpoints,
   including a resumed download and a corrupted file that fails the hash
   check.

## Verification

- On a machine with no Android SDK, `mtp sdk install` asks for the license,
  downloads the pinned packages, and the Phase 1 boot test passes against
  the result.
- A second run downloads nothing.
- A tampered zip is rejected before extraction.
- Nothing is written outside `%LOCALAPPDATA%\MTPlayer` or the user's temp
  folder.
- The repo and the release artifacts contain no Google or Magisk binary.

## Risks

- Google can change the repository XML format or withdraw old zips. The
  reader fails with a clear message, and verified copies keep working.
- The license text can change between revisions. The gate shows the text for
  the exact revision being installed and asks again when it changes.
