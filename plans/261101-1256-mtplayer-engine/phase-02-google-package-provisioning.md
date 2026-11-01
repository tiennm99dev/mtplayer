---
phase: 2
title: "Google package provisioning"
status: in-review
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
  - before any package download, show the full license text embedded in the
    repository XML, and Magisk's license. The repository XML, a metadata
    file, is fetched to obtain the text, and no package downloads before
    acceptance;
  - the terminal prompt, and later the GUI dialog, asks the user to confirm
    that, when installing for work, they may accept the license for their
    employer (§2.4). The prompt has no skip option. A script without a terminal
    instead names the exact text with `--accept <id>=<sha1>` and states
    `--on-behalf-of personal|employer`, which has no default;
  - store MTPlayer's own acceptance record for each license: the SHA-1 of
    the license text in the repository XML, used to detect text changes, the
    acceptance timestamp, and a copy of the accepted text. This is not
    Google's acceptance format, and Google's tools are not expected to
    recognize it; the emulator never reads the `licenses` folder;
  - when the text changes, its hash changes, so the gate asks again. This
    holds for a package installed from a kept zip or another root too. The
    only exception is a machine that can't reach the XML: an existing
    acceptance then stands, and the install says the live text wasn't checked;
  - nothing downloads without an acceptance.
- **Notices:** keep the `LICENSE` and `NOTICE` files that ship in each
  package. `mtp sdk status --notices` lists them, and Phase 6 shows them in the
  GUI's about screen. Never remove or hide them (§3.8).
- **Download:**
  - from dl.google.com and Magisk's GitHub release (served through
    release-assets.githubusercontent.com or objects.githubusercontent.com)
    only, over HTTPS;
  - resumable, with the hash checked before extraction;
  - each zip extracted into its own empty directory, then moved into place;
  - an existing, verified copy is reused, so updates never re-download
    unchanged packages. A kept file that fails its check is discarded with a
    warning and downloaded again;
  - zips are deleted after a finished install unless `--keep-downloads` is
    given.
- **Layout:** one private SDK root per set at `<home>\sdk\<set>`, in the layout
  Phase 1 proved the emulator accepts, plus the empty `platform-tools` directory
  the emulator requires. The home is `--home`, then `MTPLAYER_HOME`, then
  `%LOCALAPPDATA%\MTPlayer`. A set id is
  `<emulator revision>-<api level>r<image revision>-<zip hashes>`, with the
  first seven digits of each pinned zip's SHA-1, for example
  `37.2.13-36r2-4d7268a-829c076`, so any new pin lands in a new root.
  `ANDROID_SDK_ROOT` names the instance's set for every emulator process.
  MTPlayer never uses or writes the user's own Android SDK.
- **Updates:** a new pin is a new MTPlayer release. The previous version
  stays installed until no instance leases it. If Google withdraws a pinned
  zip, existing copies keep working and the downloader reports the missing
  pin clearly.
- **No modification:** files are used exactly as downloaded. A check after
  extraction compares every file against the zip.

## Tasks & Steps

1. Write the repository-XML reader that finds a package's archive for
   Windows x64, its URL, size and hash, and its license text.
2. Write the `mtp-sdk` crate: license gate, download, verify, extract,
   install and remove.
3. Add `mtp sdk status|install|remove|license` to the CLI, so Phase 4 can
   provision before creating instances.
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
- `mtp sdk` writes nothing outside its home folder, which is
  `%LOCALAPPDATA%\MTPlayer` by default.
- The repo and the release artifacts contain no Google or Magisk binary.

## Verification status

The phase stays in review until the check below is done. The
[acceptance report](../reports/phase-02-261101-2313-google-package-provisioning.md)
holds the evidence for each result.

| Item | Result | Evidence |
| --- | --- | --- |
| Install asks for the license | Partly proven | The prompt was never driven end to end, because the user chose to accept by flags. |
| Install downloads the pinned packages | Passed | Three downloads, 1.31 GB, in 58 s, with exit 0. This machine has the user's own Android SDK, which showed no change, and no `ANDROID_*` variable is set. |
| Boot test against the result | Passed | `sys.boot_completed=1` after 64.7 s from the product root. |
| A second run downloads nothing | Passed | `downloads=0`, also with proxy variables pointed at a closed port. |
| A tampered zip is rejected before extraction | Passed | The bad file was rejected, and no package directory or staging folder existed. |
| Nothing is written outside the home | Passed | A snapshot of C: and D: shows no MTPlayer write outside it. The GPU driver's own caches and the registry are covered in the report. |
| No Google or Magisk binary in the repo | Passed for the repo | No release exists yet, so Phase 7 repeats this check on its artifacts. |

**The check left for the user** takes about a minute in a real terminal, because
the prompt requires both stdin and stdout to be terminals.

```text
cargo run -q -p mtp-cli -- sdk install --home D:\tiennm99dev\mtplayer\.tmp\mtp-prompt
```

The Android SDK License text comes first. Type `I accept` and answer `N` to the
employer question, so that the GPL-3.0 text follows. Then type anything other
than `I acknowledge`. Both texts must have been shown, the exit code in
`$LASTEXITCODE` must be 3, and `.tmp\mtp-prompt\downloads` must hold no zip or
`.part` file. Declining at the first question also exits with 3 and downloads
nothing, but it never reaches the GPL text. Delete `.tmp\mtp-prompt` afterwards,
then set this phase to done.

## Risks

- Google can change the repository XML format or withdraw old zips. The
  reader fails with a clear message, and verified copies keep working.
- The license text can change between revisions. The gate shows the text for
  the exact revision being installed and asks again when it changes.
