# Stock emulator gates, 2026-11-01

This report closes the Phase 1 spike of the
[MTPlayer engine plan](../261101-1256-mtplayer-engine/plan.md) on miti-b550
(Windows 11 Pro 10.0.26300, Ryzen 7 5800X, 32 GB RAM, RTX 3060, WHPX). It
covers the [Phase 1](../261101-1256-mtplayer-engine/phase-01-feasibility-spike-and-gates.md)
tasks 1 to 4 and 7, and the reviewer's go/no-go for task 8.

## Outcome

Google's stock emulator 37.2.13 and the unmodified
`system-images;android-36;default;x86_64` image meet acceptance criteria 1, 2
and 4 on miti-b550 within the limits listed below. Everything ran from a
private SDK folder under `.tmp/`, used only runtime features, and changed no
Google file. The Fable review gives **GO for Phase 2** with no code blocker.
Phase 4 should wait until the two pending tasks are done: the stock baseline
benchmark (task 5) and the ABI check of the work APKs (task 6). The user
answered the reviewer's six questions in session 5; see
[Decisions (session 5)](#decisions-session-5).

The stock stack has these hard limits:

- The console auth token is one file per Windows user, not one per instance.
- `adb.exe` writes its key into `%USERPROFILE%\.android`.
- `-prop` cannot set `ro.*` properties.
- `-android-serialno` crashes or hangs the emulator.
- The IMEI is fixed at `867400022047199`.
- The eth0 MAC address is fixed unless it is changed in RAM as root.

None of these limits blocks the plan. The product talks to the emulator over
gRPC instead of the console, ships its own adb client instead of `adb.exe`,
uses `resetprop` for the identity properties, and lists the IMEI as fixed.

All evidence paths below are under `.tmp/evidence/`. That folder is
git-ignored and exists only on miti-b550.

## Pins

All Google packages are channel-0 and under `android-sdk-license`. The pin
file is [`spike/packages.pin.json`](../../spike/packages.pin.json).

| Package | Revision | Zip | Size (bytes) | SHA-1 |
| --- | --- | --- | --- | --- |
| `emulator` | 37.2.13 | `emulator-windows_x64-16740729.zip` | 455,344,678 | `4d7268aa7787c640ad892844e6c9a5f159e91ad3` |
| `platform-tools` (spike only) | 37.0.1 | `platform-tools_r37.0.1-win.zip` | 8,044,989 | `e03e78b1d80b396f1c3358e31251cb31740e1110` |
| `system-images;android-36;default;x86_64` | 2 | `sys-img/android/x86_64-36_r02.zip` | 844,217,077 | `829c076e8ff448a336097ae25a355b495ba36e2c` |
| `build-tools;36.1.0` (test app only) | 36.1.0 | `build-tools_r36.1_windows.zip` | 59,366,211 | `95f048ae758dad90617975fe3f787f038b03338c` |
| `platforms;android-36` (test app only) | 2 | `platform-36_r02.zip` | 65,878,410 | `2c1a80dd4d9f7d0e6dd336ec603d9b5c55a6f576` |

The Magisk pin is release `v31.1` (published 2026-10-25, not a prerelease)
under GPL-3.0. Its asset `Magisk-v31.1.apk` is 10,679,283 bytes with SHA-256
`89cefac0a318afa39fb3880c45c87b036c05bc1ebf02a7a715b7f8d7c26ac6e7`; GitHub
publishes only a SHA-256 digest. The spike extracts `lib/x86_64/libmagisk.so`
from it as the `magisk` binary, with SHA-256
`19e7c1d37f9261828091377657f6d637f4ce6920fbf571374d08dfefc91ebd6f`.

Google's XML also lists emulator 37.3.6. It is on channel-2 under the preview
license, so the filter rejected it.

The image is a userdebug build: `ro.build.type=userdebug`,
`ro.build.tags=test-keys`, build ID `BE2A.250530.026.D1`, Android 16 (SDK 36),
security patch 2025-07-05. Its `source.properties` requires emulator 35.4.9 or
newer, which 37.2.13 meets.

**Correction to the license-acceptance hash.** The pin stores
`efa68a6b3c661d18699d5c026771d5911cdc2f83`, which is the SHA-1 of the raw
license text node in Google's XML. The provision stage said this differs from
the well-known `24333f8a…` because Google revised the text. That was wrong:
the text was not revised and is still dated January 16, 2019, and Android
Studio wrote `24333f8a…` for the same text on this machine on 2026-10-24.
Nobody has reproduced how Google gets from this text to its hash, so the
spike's value is a detector for license-text changes only, and Google's tools
do not recognize it. The emulator never reads the `licenses` folder, so
Phase 1 is not affected. The user decided that Phase 2 keeps MTPlayer's own
acceptance record and makes no `sdkmanager` interop claim (decision 5).

## Checks and proofs

### Task 1: package pin and layout

| Check | Result | Evidence |
| --- | --- | --- |
| Pinned downloads verified against the XML (URL, size, SHA-1, channel-0, license) before extraction | pass | `fetch-packages-run1.log` (3 downloads) |
| A second run downloads nothing and does not touch the network | pass | `fetch-packages-run2.log`, `fetch-magisk-run2.log` |
| Each zip is extracted into its own new empty folder, then moved into the standard SDK layout | pass | `provision-summary.txt` |
| The emulator needs no `package.xml`, no `platforms/` folder and no `avdmanager` | pass | `boot-stage-summary.txt` |
| The reviewer compared every installed file with its zip entry for all five packages (11,931 entries) | pass, 0 differences | the reviewer's `review0-*` files |

The spike scripts for this task are `spike/fetch-common.ps1`,
`spike/fetch-packages.ps1` and `spike/fetch-magisk.ps1`. Downloads use
System32 `curl.exe` and extraction uses System32 `tar.exe`.

### Task 2: boot and isolation

| Check | Result | Evidence |
| --- | --- | --- |
| A hand-written AVD (`spike/new-avd.ps1`) boots from `.tmp/avd` | pass | `boot-stage-summary.txt` |
| `sys.boot_completed=1`, `adb root` gives uid 0, `ro.build.type=userdebug`, a screenshot works | pass | `spike0-adb-checks.txt`, `spike0-boot.png` |
| The host GPU renders the guest (`-gpu host`) | pass | `spike0-gles.txt` |
| Console, adb, gRPC and adb-server ports listen on `127.0.0.1` and `::1` only | pass | `spike0-netstat-attempt2.txt` |
| The console auth token is per instance | fail (stock limit) | `spike0-console-auth.txt` |
| gRPC token auth works and the token is per instance | pass | `spike0-grpc-token-per-instance.txt`, `spike0-grpc-auth-attempt2.txt` |
| Nothing is written to `%USERPROFILE%\.android` | fail (stock `adb.exe` only) | `user-profile-steady-state.txt` |
| MuMu is untouched | partial | `adb-scan-incident.txt`, `spike0-adb-devices.txt` |
| `spike/run-emulator.ps1` works end to end | pass | `run-emulator-e2e-attempt2.txt` |
| No spike process is left running | pass | final process and port checks |

The console token fails because emulator 37.2.13 on Windows always reads it
from `%USERPROFILE%\.emulator_console_auth_token`. The path comes from
`SHGetFolderPathW(CSIDL_PROFILE)` in `System.cpp`, and no flag or environment
variable moves it. Console auth is still enforced: without the token only
`avd name` answers. The token value was never printed.

The gRPC token is generated fresh for each emulator process (88 characters).
Two consecutive processes got different tokens. A call without the token, or
with a wrong one, gets gRPC status 16 (unauthenticated); the right token gets
status 0.

The profile-folder check fails only because stock `adb.exe` created
`adbkey` and `adbkey.pub` in `%USERPROFILE%\.android` once, at 17:10:43
(`adb_utils.cpp` uses `CSIDL_PROFILE`). The emulator itself wrote its files
only into `.tmp/home/<instance>`, and nothing new appeared in the profile
folder after that. The product does not run `adb.exe`. The stock emulator also
created `%USERPROFILE%\.emulator_console_auth_token` at 17:12:20, for the
reason above.

MuMu is marked partial because of one incident in attempt 1. The first adb
server found MuMu's port 5555 by its automatic scan and listed it, but no
command was sent to it. Setting `ADB_EMU=0` stopped the scan, and from then on
`adb devices -l` listed only the spike's own instances.

Two operational findings came out of this task:

- A Quick Boot snapshot saved within seconds of boot is broken. The restored
  guest hangs at "Phone is starting" and adb shows it offline. The daemon must
  never save a snapshot until the guest has settled for about 90 seconds.
- The emulator and the adb server inherit the caller's stdio handles, so a
  pipe on `run-emulator.ps1` stays open until the instance stops. The daemon
  must spawn them with explicit handles.

### Task 3: identity proofs

A small test app, `dev.mtplayer.spiketest`, reads `Build.*`,
`Build.getSerial()` and `ANDROID_ID`. `spike/make-test-app.ps1` builds it from
`spike/test-app/` with the pinned build-tools and platform and a host JDK
(Temurin 25). The identity scripts are `spike/apply-identity.ps1`,
`spike/read-identity.ps1`, `spike/clone-instance.ps1` and
`spike/identity.sample.json`. Evidence is under `.tmp/evidence/identity/`.

| Proof | Result | Evidence |
| --- | --- | --- |
| `-prop ro.*` for model, brand, device, fingerprint or serial | fail (stock limit) | `A1-prop-flags.txt` |
| `-android-serialno` | fail (stock bug) | `A3a` to `A3d` |
| `-qemu -append androidboot.serialno=…` | pass | `A2-qemu-append-serial-icc-mac.txt` |
| `-append-userspace-opt` (bootconfig) | partial | `A4-append-userspace-opt.txt` |
| `magisk resetprop` plus a framework restart, read by the test app | pass | `B-app-before.txt`, `B-app-after.txt` |
| Applied identity survives Quick Boot and is gone after a cold boot | pass | `B-snapshot-persistence.txt` |
| The app's `ANDROID_ID` is its own per-app SSAID | pass | `B-app-before.txt` |
| A fresh AVD gets a new `android_id` | pass | `spike2-fresh.json` |
| A cloned AVD gets a new `android_id` and a new per-app SSAID after a reset | pass | `C-app-ssaid-clone.txt`, `clone-spike1-to-spike3.json` |
| Phone number via `-phone-number` | pass | `D-console-phonenumber.txt` |
| IMSI and ICCID via a per-instance `-icc-profile` copy | pass | identity summary |
| IMEI | fail (fixed) | identity summary |
| MAC address | partial | `D-mac-runtime-ip-link.txt` |

The details behind these results are as follows.

- `-prop` accepts only `qemu.*` names on the host side, and the guest's
  `qemu-props` service renames them to `vendor.*`, which SELinux denies.
- `-android-serialno` made the emulator die or hang at gfxstream start-up in 3
  of 3 boots. The same arguments without the flag boot in about 21 seconds.
- `-qemu -append androidboot.serialno=MTP-SPIKE-0001` set both `ro.serialno`
  and `ro.boot.serialno`.
- `-append-userspace-opt` sets only `ro.boot.*`, and `ro.product.*` is not
  derived from it.
- After `resetprop` and a framework restart, all eight values the app read
  (MODEL, BRAND, DEVICE, MANUFACTURER, PRODUCT, FINGERPRINT, ID and
  `getSerial()`) matched `identity.sample.json`. 40 properties were applied
  with no mismatch. `getSerial()` needed `appops set <pkg>
  READ_DEVICE_IDENTIFIERS allow`. The framework restart took 7.1, 8.2 and 14.5
  seconds in three runs, and 13 to 17 seconds with four instances running.
- The applied identity was still there after a 3.4-second Quick Boot from a
  snapshot saved 100 seconds after the restart. A `-no-snapshot` cold boot
  (25.2 seconds) brought back the stock values. Phase 4 can therefore apply
  the identity once, let the guest settle, run `adb shell sync`, save the
  snapshot, and skip the restart on later starts.
- A plain copy of an AVD keeps the source's `android_id`, per-app SSAID and
  SSAID user key. The reset that changed all three is: delete the secure
  `android_id` setting, stop the framework, delete
  `/data/system/users/0/settings_ssaid.xml`, stop the emulator, and cold start
  with no network (`-feature -VirtioWifi` plus `restrict=on`). After it,
  `android_id` went from `d9c5d677a83f07b2` to `9f60ac64f2ad3f8e` and the
  app's SSAID from `597aaa74bcfc4bd5` to `0d662b0139149f0f`. The earlier
  recipe in phase-01 and phase-04 (delete the SSAID file and `adb reboot`)
  was incomplete, and `adb reboot` froze emulator 37.2.13. Both phase files
  now use the recipe above.
- `-phone-number 15551230001` gave the line number `+15551230001`. A
  per-instance copy of the ICC profile gave IMSI `310260123450001` and ICCID
  `89860318640220130001`; the copy lives in the AVD folder, and `.tmp/sdk` was
  not edited.
- The IMEI is the string literal `867400022047199` in the modem simulator's
  `HandleGetIMEI` (`misc_service.cpp`), compiled into the emulator. No flag or
  setter changes it.
- The wlan0 MAC address already differs per console port, while eth0 is fixed
  at `52:54:00:12:34:56`. `ip link set` as root changes both, in RAM only.
  Apps on API 30 and later cannot read MAC addresses anyway.

### Task 4: multi-instance

`spike/multi-instance.ps1 -Fresh` reproduces this task from an empty `.tmp`
and exited 0 in the final run. Its summary is
`multi/multi-instance-20261101-203352.json` and `.txt`.

| Check | Result |
| --- | --- |
| Four AVDs `m0` to `m3`, each with 2 GB RAM and 2 cores | pass |
| Each AVD has its own folder, qcow2 overlays and Quick Boot snapshot, and all share one read-only image | pass |
| No instance writes into `.tmp/sdk` (14,888 entries compared before and after: 0 added, 0 removed, 0 changed) | pass |
| All four start at once on their own console, adb and gRPC ports | pass |
| Each instance can be controlled on its own (a different `font_scale` read back from each) | pass |
| Each instance's screenshot shows its own state (four different SHA-256 values) | pass |
| `m0` and `m1` get different identities from `apply-identity.ps1`, and keep them after Quick Boot | pass |
| All eight stops (two rounds of four) are graceful, and no process or port is left | pass |
| The user profile and MuMu are untouched | pass |

The ports were: `m0` 5630/5631 with gRPC 8615, `m1` 5640/5641 with gRPC
8620, `m2` 5650/5651 with gRPC 8625, and `m3` 5660/5661 with gRPC 8630. Each
AVD takes about 3.3 GB on disk and holds no copy of the system, vendor,
ramdisk or kernel image.

Two script bugs were found and fixed during this task. `stop-instance.ps1`
counted the shared `netsimd` network daemon as the first instance's child and
force-killed it while the others still used it. `apply-identity.ps1` wrote
`device_name` just before the framework stop, so SettingsProvider lost it; it
now writes it after the restart and reads it back.

## Boot times

| Measurement | Result |
| --- | --- |
| First boot of a new AVD, one instance | 60.3 s (66 s in the reviewer's re-run) |
| First boot of four new AVDs at once | 92.1 to 93.0 s each |
| Cold boot of an initialized AVD (`-no-snapshot`), three runs | 21.6, 23.9 and 23.8 s (median 23.8 s) |
| Quick Boot, one instance | 3.4 to 6.6 s |
| Quick Boot, four at once | 5.4 to 6.9 s |
| Framework restart after `resetprop` | 7.1 to 17.2 s |

`spike/measure-cold-boot.ps1` measures cold boots and
`spike/record-host-state.ps1` records the host state for each run
(`host-state-*.txt`). Every run had the hypervisor present
(`HypervisorPresent: True`), but no WSL distro and no Docker were running.
The Ubuntu and docker-desktop distros are installed but were stopped.

## Multi-instance resource use

| State | Physical RAM used | Commit charge | CPU idle |
| --- | --- | --- | --- |
| Baseline (user's own apps, MuMu running) | 17.5 of 32.7 GB | 32.89 of 42.93 GB | 84.2% |
| Four instances just booted | 29.5 GB | 36.98 GB | 24.6% (settling) |
| Four instances at steady state, 100 s after identity | 29.8 GB (+12.2 GB) | 37.95 GB (+5.06 GB) | 81.8% |
| Four instances after a Quick Boot | 21.3 GB (+3.8 GB) | 38.57 GB | 78.5% |

Each emulator tree used about 3.4 GB of working set and 1.4 GB of private
memory at steady state. The commit charge rose by only about 1.27 GB per
instance because guest RAM is backed by each AVD's snapshot file
(`ram.img`, 2560 MB). After a Quick Boot, physical use drops because guest
pages load only as they are used. Peak commit was 38.57 GB, about 90% of the
system-managed 42.93 GB limit, because this host's baseline commit is already
about 33 GB. The daemon should check commit headroom before it starts an
instance.

## Identity: what changes and what stays fixed

These values change per instance:

- model, brand, device, manufacturer, product, fingerprint and build ID,
  through `resetprop` and a framework restart;
- `ro.serialno` and `Build.getSerial()`, through `-qemu -append
  androidboot.serialno=…` or `resetprop`;
- the device name (`settings put global device_name`);
- `android_id` and each app's SSAID, new on a fresh AVD and new on a clone
  after the reset recipe above;
- the phone number, through `-phone-number`;
- the IMSI and ICCID, through a per-instance `-icc-profile` copy;
- the wlan0 MAC address, which differs per console port without any action.

These values stay fixed, or change only in RAM:

- the IMEI, which is always `867400022047199`;
- the eth0 MAC address, `52:54:00:12:34:56`, which `ip link set` as root can
  change until the next boot;
- every `ro.*` property set by `resetprop`, which lasts until the next cold
  boot but is kept in a Quick Boot snapshot.

## Fable review

The review ran on Claude Fable 5.1 and was advisory only. Its condensed
findings are below.

- **Verdict: GO for Phase 2**, with no code blocker. Phase 2 is the downloader
  and license gate, which the product needs whatever criterion 3 shows.
  Phase 4 should wait for tasks 5 and 6, as the plan already intends. Nothing
  found calls for the engine fork or a different image.
- **Hard rules held.** The reviewer checked git status and `.gitignore`, the
  byte-level match of every installed Google file against its zip, the launch
  flags of every recorded instance, the channel and license filter, the
  absence of `C:\mtp`, the isolated `ANDROID_*` variables and adb port, and
  that no token value appears in any file. The only exceptions are the two
  profile files written by stock binaries, which were already reported.
- **The reviewer's own boot** of a fresh AVD through `run-emulator.ps1` on
  port 5680 reached `sys.boot_completed=1` in 66 seconds, with every port on
  loopback, and stopped gracefully with no process left.
- **One stage claim was wrong:** the license-acceptance hash, described in the
  Pins section above.
- **One gap was not flagged by the stages:** criterion 1 asks for boot times
  with WSL2 and Docker running, and neither was running during any measured
  boot.
- **Criterion 1:** Quick Boot passes. Cold boot passes for an initialized
  instance (23.8 seconds) and is a marginal fail if it means the first boot
  of a new AVD (60.3 to 66 seconds). tiennm5-pc was not tested.
- **Criterion 2:** four instances on miti-b550 pass. The two instances on
  tiennm5-pc were not tested.
- **Criterion 4:** a new Android ID on clone passes with the corrected reset
  recipe. Build fields and serial per instance pass. The phone number, IMSI
  and ICCID pass. The IMEI is fixed and the MAC address is partial; the
  criterion already allows listing them as fixed.
- **Security wording:** the plan's earlier requirement of a console token per
  instance cannot be met. The daemon does not need the console, because a
  graceful shutdown
  (`setVmState` with `SHUTDOWN`) and snapshot save and load are available over
  the per-instance gRPC token. The daemon should warn when the per-user token
  file is empty, because an empty file turns console auth off.
- **Script findings**, all low or informational. The six script findings were
  fixed in session 5 in `spike/*.ps1`; the crashpad item feeds Phase 4:
  - `fetch-packages.ps1` called its hash "Google's format" (medium, a claim
    rather than a safety issue); it now calls it a license-text change
    detector;
  - `start-instance.ps1` treated a reused PID as a running instance; it now
    requires the process to run from `.tmp/sdk` (`Test-OurPid` in
    `spike-env.ps1`);
  - `stop-instance.ps1` dereferenced a null path on a protected process; it
    now refuses with a message;
  - `multi-instance.ps1` used English performance-counter names; it now reads
    `Win32_PerfFormattedData_PerfOS_Memory` and
    `Win32_PerfRawData_PerfOS_Processor`;
  - `fetch-packages.ps1` did not say that running it accepts the license on
    this machine; it now prints a one-line acceptance notice;
  - `make-test-app.ps1` held a literal keystore password; it now generates a
    random one into `.tmp/tools/test-app/spike.keystore.pass`;
  - `crashpad_handler.exe` runs even with `-no-metrics`, so Phase 4 must
    confirm that no crash report is uploaded without consent.
- **What to avoid:** hard-coding Google's hash, working around the per-user
  console token, saving a snapshot within about 90 seconds of a boot or a
  restart, `-android-serialno`, `-prop` for `ro.*`, inheriting stdio into the
  emulator, treating the shared `netsimd` as one instance's child, and
  counting a four-at-once first boot as a cold boot.

## Pending items

- **Task 5, the stock baseline benchmark** (pending, any session can run it;
  it records the host state for each run with `spike/record-host-state.ps1`),
  runs on the plain stock emulator and
  needs no reboot. Criterion 3 is unmeasured until then.
- **Task 6, the ABI check** (user), needs the user's work APKs.
- **Re-measure with WSL2 and Docker running** (user). Start the Ubuntu WSL2
  distro and Docker, then run `spike/measure-cold-boot.ps1 spike0 5600` (three
  runs, about three minutes) before Phase 4 starts.
- **tiennm5-pc** (user) was not available, so the parts of criteria 1 and 2
  for that host are deferred.
- **Housekeeping** (user). `.tmp/avd` holds about 28.7 GB (`spike0` to `spike3` and
  `m0` to `m3`). The `adbkey` files in `%USERPROFILE%\.android` are safe to
  delete if the user does not otherwise use adb.

The reviewer asked the user six questions:

1. **How should "cold boot" be defined?** The reviewer recommends that cold
   boot means starting an initialized instance without a snapshot (target
   60 seconds, measured 23.8 seconds), and that first boot gets its own
   target, such as 90 seconds for one instance, so the daemon staggers
   creation.
2. **Should the Phase 1 numbers be accepted without WSL2 and Docker
   running?** The reviewer recommends accepting the Hyper-V-present result now
   and re-measuring once with WSL2 and Docker up before Phase 4 starts.
3. **What goes in the README's fixed list?** The reviewer recommends listing
   the IMEI and the eth0 MAC address as fixed and offering the runtime MAC
   change as an optional RAM-only feature.
4. **How should the security constraint read?** The reviewer recommends that
   gRPC requires a per-instance token, that the daemon never uses the console,
   and that it warns when the per-user console token file is empty.
5. **What should Phase 2 commit to for license acceptance?** The reviewer
   recommends that Phase 2 keeps its own acceptance record and drops the
   `sdkmanager` interop claim, optionally after a half-day attempt to
   reproduce Google's hash with the real `sdkmanager` on a clean SDK folder.
6. **Should Phase 2 start now?** The reviewer recommends starting Phase 2 now
   and finishing tasks 5 and 6 before Phase 4 starts.

The reviewer also listed plan wording fixes that follow from these decisions,
for `plan.md` and the phase 1, 2 and 4 files. Among them, the Phase 4 port
scheme used console port 5554 and adb port 5555, which collide with MuMu's
port 5555. All of these fixes have been applied.

## Decisions (session 5)

The user decided the six questions on 2026-11-01. The full wording is in the
plan's [session 5 log](../261101-1256-mtplayer-engine/plan.md#session-5-2026-11-01-phase-1-review-decisions).

1. **Cold boot** means starting an initialized instance without a snapshot,
   with a target of 60 seconds (measured median 23.8 seconds). The first boot
   of a new instance is reported separately, with a target of 90 seconds for
   one instance, and the daemon staggers creating several.
2. **WSL2 and Docker:** the Hyper-V-present result is accepted as the Phase 1
   gate. The user re-measures once with the Ubuntu WSL2 distro and Docker up
   before Phase 4 starts.
3. **IMEI and MAC:** criterion 4 stays as written. The README lists the IMEI
   and the eth0 MAC address as fixed, and MTPlayer offers an optional
   RAM-only MAC change. The supported per-instance set is phone number, IMSI
   and ICCID, model, brand, device, fingerprint, serial and Android ID.
4. **Tokens:** gRPC requires a per-instance token. The daemon never uses the
   console; it stops instances and saves and loads snapshots over gRPC, and
   warns if the per-user console token file is empty. The console port still
   binds to `127.0.0.1` only.
5. **License acceptance:** Phase 2 stores MTPlayer's own acceptance record
   (the SHA-1 of the license text as a change detector, a timestamp, and a
   copy of the text). The claim that Google's tools recognize it is dropped.
6. **MuMu:** the MuMu comparison is skipped, so no reboot with Hyper-V off is
   needed. Task 5 becomes a stock baseline benchmark, and criterion 3 now
   requires MTPlayer-managed instances to stay within 10% of a plain
   stock-emulator launch on the same host. MuMu stays installed until
   criterion 3 passes on both hosts and the user confirms the switch.
