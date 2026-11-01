# MTPlayer
Try to create something that can replace existing android emulator for Windows

## Installing Google's packages

MTPlayer ships no Google or Magisk file. `mtp sdk install` shows each license,
then downloads the versions pinned in
[`packages/manifest.json`](packages/manifest.json), checks them against their
pinned hashes, and stores them unmodified. `mtp sdk status` reports what is
installed, and `mtp sdk remove` removes a set, unused sets or the downloads.

### Commands

```text
mtp sdk status [--verify] [--notices] [--json]
mtp sdk license <id> [--text]
mtp sdk install [--verify] [--keep-downloads] [--accept <id>=<sha1> --on-behalf-of personal|employer]
mtp sdk remove <set> [--force]
mtp sdk remove --unused
mtp sdk remove --downloads
```

`mtp sdk status` makes no network request. Its `--verify` flag hashes every file,
and `--notices` lists the `LICENSE` and `NOTICE` files the packages ship. A second
`mtp sdk install` downloads nothing, and a kept zip that fails its check is discarded
with a warning and downloaded again.

`mtp sdk remove <set>` refuses a root that an instance leases, unless you pass
`--force`. It also refuses a folder that has no MTPlayer record. `--unused` removes
every root that is neither current nor leased, along with old Magisk versions. No
`remove` form deletes your license records or the current Magisk binary.

The exit code is 0 on success, 2 for a usage error, 3 when a license isn't accepted,
4 when another MTPlayer process holds the lock, 5 when a pin is no longer offered as
stable or no longer served, and 1 for anything else.

### Where files go

Everything stays in one home folder. It is the `--home` flag if given, then the
`MTPLAYER_HOME` environment variable, then `%LOCALAPPDATA%\MTPlayer`. MTPlayer
never uses or writes your own Android SDK.

```text
<home>/
  mtp-sdk.lock, leases.lock    locks for install, remove and leases
  sdk/<set>/                   ANDROID_SDK_ROOT of one set of Google's packages
  sdk/<set>.json               what that root holds, with each file's size and hash
  sdk/<set>.leases/<owner>     one file for each instance that uses the root
  staging/                     extractions in progress
  downloads/                   zips and partial files; zips go after a finished install
  licenses/<id>.json           your acceptance records
  tools/magisk/<tag>/magisk    the Magisk binary, used only for resetprop
```

A set id such as `37.2.13-36r2-4d7268a-829c076` names the emulator revision, the
Android API level and image revision, and the start of each pinned zip's SHA-1.
A new pin is therefore always a new root, and a root is never changed once it is
ready, because instances point at its files. A root holds Google's packages and
one empty `platform-tools` folder, which the emulator requires. `adb.exe` is not
installed.

### Where downloads come from

MTPlayer only talks to these hosts, over HTTPS, and it checks every redirect
against the same list. No setting can widen it.

- `dl.google.com` for Google's packages and their repository XML
- `github.com` for Magisk's release, which redirects to
  `release-assets.githubusercontent.com` or `objects.githubusercontent.com`

### The license gate

The repository XML is fetched first, because it holds the Android SDK License
text. No package is downloaded until you have accepted that text and acknowledged
Magisk's GPL-3.0. In a terminal, `mtp sdk install` prints each full text and asks
you to answer. It also asks whether you are installing for an employer, and if so
to confirm that you have the authority to bind it.

A script without a terminal has to name exactly what it accepts:

```text
mtp sdk license android-sdk-license --text
mtp sdk license gpl-3.0 --text
mtp sdk install --accept android-sdk-license=<sha1> --accept gpl-3.0=<sha1> --on-behalf-of personal
```

`mtp sdk license <id> --text` is the only way to get a hash, and it prints the hash
with the text. `--on-behalf-of` takes `personal` or `employer`, and has no default. The hash
must match the live text, so a changed text stops the install until it is accepted
again. A flag proves which text was meant and who it was accepted for, and it leaves
a record of both, but it can't prove that a person read the text.

When a package can be installed from a kept zip or another root, the live text is
still read if the machine can reach it. Offline, an existing acceptance stands, and
the install says the live text wasn't checked.

Google's packages are licensed for developing and testing apps (Android SDK License
section 3.1). MTPlayer's identity and spoofing features are for testing your own apps
and must not be used to get around third-party anti-fraud or attestation systems
(section 4.4).
