---
title: "MTPlayer: Android emulator manager for Windows on Google's emulator"
description: "A free, commercially usable Windows Android emulator for app testing, built as a manager, testing-tools and identity layer over Google's stock emulator and Android 16 SDK image, to replace MuMuPlayer, LDPlayer and BlueStacks."
status: pending
priority: P2
effort: "17-25 full-time weeks plus 25% slack (21-31 weeks); about 10-15 months at half time"
branch: main
tags: [android, emulator, windows, whpx, rust, tauri, research]
blockedBy: []
blocks: []
created: 2026-11-01
---

# MTPlayer: Android emulator manager for Windows on Google's emulator

## Outcome

MTPlayer is a Windows Android emulator for testing the user's apps, in
[tiennm99dev/mtplayer](https://github.com/tiennm99dev/mtplayer). It replaces
MuMuPlayer, LDPlayer and BlueStacks on miti-b550 and the work PC
(`tiennm5-pc`), and runs with Hyper-V, WSL2 and Docker turned on, which MuMu
12 cannot do.

MTPlayer's own code is the instance manager, the testing tools and the
identity layer. The engine and the guest image are Google's stock emulator
and Android 16 SDK system image. Each user downloads them through MTPlayer
after accepting the Android SDK License themselves. MTPlayer never
redistributes or modifies them, so work use stays within that license: §3.1
allows use "solely to develop applications for compatible implementations of
Android", and testing apps is part of that.

## Decisions taken

- **2026-11-01, user: required features** are a multi-instance manager, app
  testing tools, and root plus device spoofing. Keymapping and macros are not
  required.
- **2026-11-01, user: this plan replaces the MTPlayer MVP plan of
  2026-08-27.** The repo name stays `mtplayer`.
- **2026-11-01, user: v1 runs x86_64 apps only.** Installing an ARM-only APK
  is refused with a clear message. Own ARM translation is a separate plan
  after v1.
- **2026-11-01, user: Android 16** (API 36).
- **2026-11-01, user (session 4): no source builds,** neither AOSP nor the
  engine. This reverses the first decision, "build an emulator engine, not
  just a launcher". The reason: no public ci.android.com build has ever
  published a goldfish emulator image, so every license-clean image or engine
  needs a source build, and the user dropped those.
- **2026-11-01, user (session 4): engine** is Google's stock prebuilt
  emulator (classic engine, WHPX). An own engine fork becomes a later plan,
  started only if the user funds a cloud build
  ([Phase 3](phase-03-engine-fork.md)).
- **2026-11-01, user (session 4): image** is
  `system-images;android-36;default;x86_64`: an AOSP userdebug build without
  GMS, so `adb root` works.
- **2026-11-01, user (session 4): no image changes.** Nothing Google ships is
  patched, repacked or re-signed, because §3.4 of the SDK License forbids
  modifying it. Root is `adb root`. Identity is set at runtime: emulator flags
  and boot properties, and Magisk's `resetprop` run as root from `/data`.
  There is no Magisk ramdisk patch.
- **2026-11-01, user (session 4): MTPlayer downloads the packages.** On
  first run it shows the Android SDK License. After the user accepts, it
  fetches the pinned emulator and image zips from dl.google.com and checks
  them against Google's repository XML. It needs no Android Studio or
  `sdkmanager`.
- **Licenses:** MTPlayer's own code stays Apache-2.0. It distributes no GPL
  engine, kernel or Magisk binary. Magisk is downloaded from its GitHub
  release on the user's machine, the same way as Google's packages.
- **2026-11-01, user (session 5): Phase 1 review decisions.** Cold boot is
  defined, first boot gets its own target, the console is unused, the license
  record is MTPlayer's own, and the MuMu comparison is dropped. See the
  [session 5 log](#session-5-2026-11-01-phase-1-review-decisions).

## Constraints

- **License rules**, from the
  [license research](../reports/research-261101-1652-android-sdk-license-for-mtplayer.md).
  Every phase keeps all four:
  1. **Never ship Google's files.** The repo, the installer and the releases
     contain no emulator, system image or `adb`. Each user's machine
     downloads them after that user accepts the license.
  2. **Never change Google's files.** Use only runtime features: `adb root`,
     emulator flags, `resetprop` in RAM, and qcow2 overlays.
  3. **Stable packages only:** channel-0 packages whose `uses-license` is
     `android-sdk-license`. Dev and canary builds fall under the stricter
     preview license.
  4. **App testing only** (§3.1). Spoofing is for testing the user's own
     apps, not for getting around third-party anti-fraud or attestation
     systems (§4.4).
- **Hypervisor:** WHPX only. No kernel driver (no HAXM, VirtualBox or AEHD), so
  it coexists with Hyper-V, WSL2 and Docker.
- **No Google software in MTPlayer's repo or releases.** Google's emulator
  and image reach a machine only through MTPlayer's downloader, after that
  user accepts the license. No Google Play or GMS. No proprietary ARM
  translator (libndk_translation, libhoudini).
- **No source builds** of AOSP or the engine.
- **Hardware targets:**
  - miti-b550: Ryzen 7 5800X, 32 GB RAM, RTX 3060.
  - tiennm5-pc: i5-9600K, 16 GB RAM, GTX 1660.
- **Disk:** development work files go in the repo's git-ignored `.tmp/`
  folder on D:, which has 56 GB free. Nothing goes in `C:\mtp`.
- **Code location:** public repo `tiennm99dev/mtplayer`. Company code never
  goes there.
- **Security:**
  - every console, adb and gRPC port binds to `127.0.0.1` only, because
    instances run with root adb;
  - gRPC requires a per-instance token (`-grpc-use-token`) that only the
    daemon holds. The daemon never uses the emulator console: it stops
    instances and saves and loads snapshots over gRPC. The stock console
    token is one file per Windows user
    (`%USERPROFILE%\.emulator_console_auth_token`), so the daemon warns if
    that file is empty, because an empty file turns console auth off;
  - the guest is treated as untrusted input, including file paths and
    logcat text;
  - downloads are checked against pinned hashes before use;
  - the README states the residual threat: any local process on the host has
    root on every instance.

## Non-goals

- Keymapping, gamepad, macros, or a gaming-oriented UI.
- Google Play Store, Play Billing or Play Integrity testing. Those stay on real
  devices.
- macOS or Linux hosts.
- Building, forking or patching the engine or the image in v1.
- Magisk root (`su` for apps) in v1, because it needs a ramdisk patch.
- ARM translation in v1.

## Acceptance criteria

1. With Hyper-V, WSL2 and Docker running, an instance boots on both target
   hosts:
   - cold boot ≤ 60 s. A cold boot starts an existing, initialized instance
     without a snapshot;
   - Quick Boot ≤ 10 s;
   - first boot of a new instance ≤ 90 s for one instance, reported
     separately from cold boot. The daemon staggers creating several
     instances, because first boots run slower when they overlap.
2. **Instance counts:**
   - four persistent instances run at once on miti-b550;
   - two run on tiennm5-pc, with 2 GB guest RAM each and WSL2 capped at 4 GB
     (`.wslconfig`), and the commit charge stays below physical RAM;
   - each instance has its own adb port.
3. **Performance against the stock baseline.** Phase 1 measures its
   benchmark set (GPU score, app cold start, guest `fio` 4K random, cold boot
   and Quick Boot) on a plain stock-emulator launch and records it as
   MTPlayer's baseline. On each host, MTPlayer-managed instances stay within
   10% of a plain stock-emulator launch on the same host, for every benchmark
   in the set.
4. **Cloning and identity:**
   - a cloned instance gets a new Android ID;
   - model, brand, device, fingerprint and `ro.serialno` can be set per
     instance, and `Build.*` in an app shows the set values;
   - phone number, IMEI, IMSI and MAC address change per instance where
     Phase 1 proves the stock emulator allows it. Any that it doesn't are
     listed in the README as fixed.
5. Root: `adb root` works on every instance. No Google file is modified.
6. From the CLI and the GUI, you can:
   - install APKs, split APKs, AABs and XAPKs (APKs plus OBB);
   - read logcat;
   - take screenshots and screen recordings;
   - push and pull files;
   - set GPS location, network speed and latency, battery and sensors;
   - copy input to several instances at once (best-effort, documented as
     such).
7. Installing an ARM-only APK fails at once with a message naming the missing
   ABI. The limitation is in the README.
8. Install and uninstall on tiennm5-pc with no admin steps beyond turning on
   the WHPX Windows feature. Uninstall leaves nothing behind, including in the
   user's default Android SDK home folder. Two files there are written by
   Google's stock binaries, not by MTPlayer's code: the emulator writes
   `%USERPROFILE%\.emulator_console_auth_token` and `adb` writes
   `%USERPROFILE%\.android\adbkey*`. Uninstall removes only the files that
   MTPlayer created. The GPU driver also refreshes its own caches in place while
   an instance renders with `-gpu host` (on NVIDIA,
   `%LOCALAPPDATA%\NVIDIA\GLCache` and `C:\ProgramData\NVIDIA Corporation\Drs`),
   as it does for any OpenGL program; those files are the driver's and are
   outside uninstall.
9. **Licenses:**
   - `LICENSES.md` lists every component's license, and states which ones the
     user downloads rather than MTPlayer shipping them;
   - no release contains a Google or GPL binary.

## Phases

| # | Phase | Status | Effort | Depends on |
| --- | --- | --- | --- | --- |
| 1 | [Feasibility spike and gates](phase-01-feasibility-spike-and-gates.md) | in progress: tasks 1-4 and 7 done, go for Phase 2, review decisions taken (session 5); task 5 (stock baseline benchmark) and task 6 (ABI check) pending before Phase 4 | 2-3 weeks | none |
| 2 | [Google package provisioning](phase-02-google-package-provisioning.md) | in review: every check passed against the real endpoints; the user's terminal check of the interactive license prompt is pending (session 6) | 2-3 weeks | 1 |
| 3 | [Own engine fork](phase-03-engine-fork.md) | deferred | later plan | user-funded cloud build |
| 4 | [Instance daemon and CLI](phase-04-instance-daemon-and-cli.md) | pending | 4-6 weeks | 2 |
| 5 | [App testing tools](phase-05-app-testing-tools.md) | pending | 3-4 weeks | 4 |
| 6 | [Manager GUI](phase-06-manager-gui.md) | pending | 4-6 weeks | 4, 5 |
| 7 | [Packaging, performance and rollout](phase-07-packaging-performance-and-rollout.md) | pending | 2-3 weeks | 5, 6 |

For one developer, the phases run in order: 1, 2, 4, 5, 6, 7. They sum to
17-25 full-time weeks. With 25% slack for integration and rework, that is
21-31 weeks, or about 10-15 months at half time.

## Architecture

```text
 Tauri 2 manager GUI ──┐
                       ├──> instance daemon (Rust, gRPC + CLI)
 CLI / CI scripts ─────┘         │  store: per-instance AVD dir + qcow2 overlays + own snapshot
                                 │  (shared read-only system image; ephemeral CI mode: -read-only)
                                 │  downloader: Google SDK packages + Magisk, after license accept
                                 ▼
                    stock emulator.exe per instance (Google prebuilt, WHPX, token auth)
                       │ -gpu host (gfxstream) ──> host GPU driver (ANGLE fallback)
                       │ gRPC control + adb port (mtp-adb client, no adb.exe)
                       ▼
                    guest: system-images;android-36;default;x86_64 (userdebug, unmodified)
                       identity at runtime: emulator flags and boot properties,
                       resetprop as root from /data, then a framework restart
                       (x86_64 apps only in v1)
```

## Key risks

- **Runtime identity may not reach apps.** `Build.*` fields are read when
  zygote starts, so `resetprop` must run before the framework starts or be
  followed by a framework restart. Phase 1 proves it before anything else
  depends on it. If it fails, criterion 4 shrinks and the user decides.
- **The stock emulator may fix some hardware identifiers** (IMEI, IMSI, MAC)
  with no flag to change them, and v1 can't patch the engine. Phase 1 found
  the IMEI and the eth0 MAC address fixed; criterion 4 already allows for
  that, and the README lists them.
- **Google can change or withdraw packages.** Old zips on dl.google.com have
  stayed available so far, but nothing promises it. MTPlayer pins exact
  versions and hashes, and keeps working with the copies already downloaded.
- **License reading.** The plan relies on §3.1 (use to develop apps) and on
  never modifying or redistributing Google's files. §3.2 also forbids using
  the SDK "to develop another SDK". MTPlayer is a test tool, not an SDK, but
  the user should confirm this reading for work use.
- **WHPX VM exits cost more than the kernel drivers MuMu and LDPlayer use.**
  The user chose not to benchmark MuMu (session 5), so this risk is accepted.
  The Phase 1 stock baseline benchmark records what WHPX gives before the big
  build-out starts, and criterion 3 keeps MTPlayer within 10% of it.
- **GPU drivers break in vendor-specific ways.** Phase 7 keeps a list of
  known-good drivers.

## Research

- [Earlier ChatGPT findings on licensing and Hyper-V](../reports/research-261101-1241-android-emulator-prior-chatgpt-findings.md)
- [Engine stack: VMM, images, graphics](../reports/researcher-261101-1241-android-emulator-engine-stack.md)
- [Feature layer: ARM, root, spoofing, tools, GMS, multi-instance, GUI](../reports/researcher-261101-1241-android-emulator-feature-layer.md)
- [Fable review of this plan, 2026-11-01](../reports/review-261101-1408-mtplayer-engine-plan.md)
- [Image and engine gates, 2026-11-01](../reports/spike-261101-1629-image-and-engine-gates.md)
- [License research: Google sources and community precedent](../reports/research-261101-1652-android-sdk-license-for-mtplayer.md)
- [Stock emulator gates (Phase 1 spike), 2026-11-01](../reports/spike-261101-1700-stock-emulator-gates.md)
- [Phase 2 acceptance, 2026-11-02](../reports/phase-02-261101-2313-google-package-provisioning.md)
- [Android SDK License](https://developer.android.com/studio/terms), §3.1,
  §3.2, §3.4 and §3.5.
- The superseded MTPlayer MVP plan and its research stay in git history
  (commit `6bda00d`).

## Validation Log

### Session 1, 2026-11-01

- ARM path: the user asked whether we can write our own translator. Yes, as
  an ARM64 front end for Berberis, but it is a large effort. The user chose
  a separate later track; v1 is x86_64 only.
- Repo: the user chose `tiennm99dev/mtplayer`, which already existed. This
  plan replaces its 2026-08-27 plan, and the repo name stays.
- Android version: Android 16 confirmed.

### Session 2, 2026-11-01

- Disk: no AOSP build. The plan took pinned prebuilt CI images with a
  host-side repack, and its own ADB client, from the MVP plan.
  Superseded by session 4: those CI images don't exist.

### Session 3, 2026-11-01: Fable review

The user applied every Critical, High and Medium finding of the
[review](../reports/review-261101-1408-mtplayer-engine-plan.md). Session 4
made most of them moot, because they were about the engine fork and the
image repack. These still apply: two instance modes with a per-instance
snapshot (C2), the Android ID clone flow (H3), per-instance AVD directories
and environment variables, no Job Object memory cap, the benchmark APK ABI
check, token auth and the residual-threat note, and the WSL2 memory cap.

### Session 4, 2026-11-01: image re-plan

- Evidence from the cook workflow's image gate: no public ci.android.com
  branch has ever published a goldfish `sdk_phone64_x86_64` image, at any API
  level (41 build IDs and every `sdk_phone*` target checked). Google's
  android-36 and android-37 images exist only on dl.google.com, under the
  Android SDK License.
- The engine-recipe research found that the next-generation engine hands
  every AVD below API 37 to the classic engine, and gives it no host GPU
  (`trampoline.cc`, `gpu_device.cc` in `emu-main-next`). With Android 16 the
  stock classic engine is what runs anyway.
- The user dropped all source builds, kept Android 16, chose the stock
  emulator with an own fork deferred, no image changes, the `default` image
  tag, and MTPlayer's own downloader.
- The user moved working files to `.tmp/` in the repo and had `C:\mtp`
  deleted.
- Phases changed: 1 (gates now prove runtime identity on the stock stack), 2
  (rewritten as package provisioning), 3 (deferred), 4 (identity and root
  are applied at runtime), 5 (no JRE change), 7 (installer bundles no engine).
  Acceptance criteria 4, 5 and 9 changed.
- License research (Google sources and community precedent, on Fable):
  the design holds. The user had the four license rules added to the
  constraints, and the stable-only filter, Google-format acceptance, the
  employer confirmation and kept notices added to Phase 2. Session 5
  replaced the Google-format acceptance with MTPlayer's own record.

### Session 5, 2026-11-01: Phase 1 review decisions

The user answered the six questions of the Fable review in the
[stock emulator gates report](../reports/spike-261101-1700-stock-emulator-gates.md).

1. **Cold boot.** In criterion 1, "cold boot ≤ 60 s" means starting an
   existing, initialized instance without a snapshot (measured median
   23.8 s). The first boot of a new instance is reported separately, with its
   own target of ≤ 90 s for one instance (measured 60.3 to 66 s, and about
   92 s each when four are created at once). The daemon staggers creating
   several instances.
2. **WSL2 and Docker.** Hyper-V present is accepted as the Phase 1 gate now.
   Before Phase 4 starts, the user re-measures once with the Ubuntu WSL2
   distro and Docker up (`spike/measure-cold-boot.ps1`, about 3 minutes).
3. **IMEI and MAC.** Criterion 4 stays as written. The README lists the IMEI
   (fixed at the emulator's compiled-in value) and the eth0 MAC address as
   fixed. MTPlayer offers an optional, RAM-only runtime MAC change
   (`ip link set` as root). The supported per-instance set is phone number,
   IMSI and ICCID, model, brand, device, fingerprint, serial and Android ID.
4. **Tokens.** gRPC requires a per-instance token, which the spike proved
   (`-grpc-use-token`). The daemon never uses the emulator console. It stops
   instances and saves and loads snapshots over gRPC (`EmulatorController`
   `setVmState` with `SHUTDOWN`; `SnapshotService` `SaveSnapshot` and
   `LoadSnapshot`). It warns if `%USERPROFILE%\.emulator_console_auth_token`
   is empty, because that turns console auth off. The console port still
   binds to `127.0.0.1` only.
5. **License acceptance.** Phase 2 stores MTPlayer's own acceptance record:
   the SHA-1 of the XML license text as a text-change detector, a timestamp,
   and a copy of the text. The plan no longer claims that Google's tools
   recognize it.
6. **MuMu.** The MuMu comparison is skipped entirely, with no reboot with
   Hyper-V off. Criterion 3 no longer compares with MuMu: the Phase 1
   benchmark set (GPU score, app cold start, guest `fio` 4K random, cold boot
   and Quick Boot) is measured on the plain stock emulator and recorded as
   MTPlayer's baseline, and MTPlayer-managed instances must stay within 10%
   of a plain stock-emulator launch on the same host. Phase 1 task 5 becomes
   the stock baseline benchmark (pending, no reboot needed). Task 6, the ABI
   check, stays pending on the user's work APKs. In Phase 7, MuMu stays
   installed until criterion 3 passes on both hosts and the user confirms
   the switch.

The review's doc fixes were applied to this plan, to phases 1, 2, 4 and 7,
and to the report: the full Android ID reset recipe, a configurable port
base of 5600 or higher, and the Phase 4 design notes from the spike.

### Session 6, 2026-11-02: Phase 2 acceptance and final review

All ten acceptance checks passed against the real endpoints, and the evidence is
in the git-ignored `.tmp/evidence/phase2/` folder. The full install made 3
downloads (1.31 GB) in 58 s, the root booted to `sys.boot_completed=1` in 64.7 s,
a second run downloaded nothing, and an independent compare found no difference
in 406 files. See the
[acceptance report](../reports/phase-02-261101-2313-google-package-provisioning.md).

1. **Acceptance by flags.** The user accepted both licenses with
   `--accept <id>=<sha1> --on-behalf-of personal` on 2026-11-02, for the scratch
   homes under `.tmp/` only. The interactive prompt was therefore not driven end
   to end, so Phase 2 stays in review until the user's one-minute terminal check.
2. **GPU driver caches.** The final review judged the NVIDIA cache updates seen
   during boot to be the driver's own, so criterion 8 now says they are outside
   uninstall. The link to the emulator rests on timing only. A future snapshot
   showing new per-application files there would reopen this, and they would then
   be listed in the README, still without deleting them.
3. **Final review fixes.** A kept zip that fails its check is now discarded with a
   warning, removing a directory without a record now gives its own error, and a
   loopback client ignores proxy variables so that `cargo test` works behind a
   proxy. Production traffic still honours the proxy variables. The 232 offline
   tests and 4 small live tests passed after these fixes, and the 1.3 GB acceptance
   was not repeated, because fixture tests cover those paths.
4. **Uninstall.** License records and the current Magisk binary stay after
   `mtp sdk remove`. This is by design, and Phase 7's uninstall owns deleting them.
