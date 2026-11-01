# Licenses

This is the first version of MTPlayer's license inventory. It lists every
component MTPlayer uses, its license, and whether MTPlayer ships it or the
user downloads it.

## MTPlayer

MTPlayer's own code is licensed under the [Apache License 2.0](LICENSE).

## Downloaded by the user, not shipped

MTPlayer's repository, installer and releases contain none of these
components. Each user's machine downloads them from their official source.
Google's packages are downloaded only after that user accepts the
[Android SDK License](https://developer.android.com/studio/terms) themselves.
MTPlayer never modifies these files; it uses only their runtime features.
The terms of use that follow from the Android SDK License are stated in
[the license gate section of the README](README.md#the-license-gate).

| Component | Version pinned in packages/manifest.json | License | Source |
| --- | --- | --- | --- |
| Android Emulator (`emulator`) | 37.2.13, channel-0 | Android SDK License | dl.google.com |
| Android 16 system image (`system-images;android-36;default;x86_64`) | revision 2, channel-0 | Android SDK License | dl.google.com |
| Android SDK Platform-Tools (`platform-tools`) | spike only, not pinned (the spike uses 37.0.1, channel-0) | Android SDK License | dl.google.com |
| Android SDK Build-Tools (`build-tools;36.1.0`) | spike only, not pinned | Android SDK License | dl.google.com |
| Android SDK Platform (`platforms;android-36`) | spike only, not pinned | Android SDK License | dl.google.com |
| Magisk (only its `magisk` binary is used, for `resetprop`) | v31.1 | GPL-3.0 | github.com/topjohnwu/Magisk releases |

The emulator package and the system image each carry their own `NOTICE.txt`
file. Those files list the open-source components inside them and their
licenses, which include Apache-2.0, BSD, GPL and LGPL. The notices stay with
the downloaded packages, and MTPlayer does not remove or change them.

The spike downloads the three packages marked "spike only" to boot and test a
small app. The product does not download or use them. Each SDK root holds an
empty `platform-tools` folder, because the emulator refuses a root without one,
but it never holds a file from that package.

The current pinned set is `37.2.13-36r2-4d7268a-829c076`. The exact URLs, sizes and
hashes are in [`packages/manifest.json`](packages/manifest.json). The spike keeps its own pins
in [`spike/packages.pin.json`](spike/packages.pin.json).

Magisk's APK carries no license file, so the repository keeps
[`packages/licenses/gpl-3.0.txt`](packages/licenses/gpl-3.0.txt) for the gate to
show. It is a verbatim copy of Magisk's `LICENSE` at the pinned tag, with SHA-256
`589ed823e9a84c56feb95ac58e7cf384626b9cbf4fda2a907bc36e103de1bad2`. The GPL-3.0
permits verbatim copies of its text, and keeping one doesn't place MTPlayer under
that license.

## Rust dependencies

`mtp.exe` and `pin-update.exe` compile in 85 third-party Rust crates on Windows x64.
This summary comes from `cargo metadata`, and it counts different spellings of the
same terms together.

| License terms | Crates |
| --- | --- |
| MIT or Apache-2.0 | 64 |
| Unlicense or MIT | 9 |
| MIT | 4 |
| ISC (`rustls-webpki`, `untrusted`) | 2 |
| Apache-2.0 or ISC or MIT (`rustls`) | 1 |
| Apache-2.0 and ISC (`ring`) | 1 |
| (MIT or Apache-2.0) and Unicode-3.0 (`unicode-ident`) | 1 |
| BSD-3-Clause (`subtle`) | 1 |
| CDLA-Permissive-2.0 (`webpki-roots`) | 1 |
| Zlib (`zlib-rs`) | 1 |

All of these terms are compatible with MTPlayer's Apache-2.0 license, and none of them
carries a copyleft obligation. Five more crates are resolved only for tests and build
scripts, and each of those is under MIT or Apache-2.0. A release has to carry the
notices these licenses require.

The list of crates, versions and licenses is in
[`packages/cargo-licenses.md`](packages/cargo-licenses.md). That file is generated
from `Cargo.lock` by [`packages/cargo-licenses.ps1`](packages/cargo-licenses.ps1),
so regenerate it whenever the lock file changes.

## Bundled later

These components are planned to ship with MTPlayer's installer in a later
phase. Their license notices will ship with them.

| Component | License |
| --- | --- |
| bundletool | Apache-2.0 |
| OpenJDK runtime, for bundletool | GPLv2 with the Classpath Exception |

## Trademarks

Android is a trademark of Google LLC. MTPlayer is not affiliated with or
endorsed by Google, and the product name does not include "Android".
