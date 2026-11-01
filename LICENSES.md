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

| Component | Version pinned in the spike | License | Source |
| --- | --- | --- | --- |
| Android Emulator (`emulator`) | 37.2.13, channel-0 | Android SDK License | dl.google.com |
| Android 16 system image (`system-images;android-36;default;x86_64`) | revision 2, channel-0 | Android SDK License | dl.google.com |
| Android SDK Platform-Tools (`platform-tools`), used by the spike only | 37.0.1, channel-0 | Android SDK License | dl.google.com |
| Magisk (only its `magisk` binary is used, for `resetprop`) | v31.1 | GPL-3.0 | github.com/topjohnwu/Magisk releases |

The emulator package and the system image each carry their own `NOTICE.txt`
file. Those files list the open-source components inside them and their
licenses, which include Apache-2.0, BSD, GPL and LGPL. The notices stay with
the downloaded packages, and MTPlayer does not remove or change them.

The spike also downloads `build-tools;36.1.0` and `platforms;android-36`
(both channel-0, Android SDK License) to build a small test app. The product
does not use them.

The exact URLs, sizes and hashes are in
[`spike/packages.pin.json`](spike/packages.pin.json).

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
