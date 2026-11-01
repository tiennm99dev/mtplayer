---
title: "MTPlayer: own Android emulator engine for Windows"
description: "Build a free, commercially licensable Android emulator for Windows from the open-source Android Emulator (AEMU) and AOSP sources to replace MuMuPlayer, LDPlayer and BlueStacks for app testing."
status: pending
priority: P2
effort: "26-39 full-time weeks plus 25% slack (33-49 weeks); about 15-23 months at half time"
branch: main
tags: [android, emulator, windows, whpx, qemu, aosp, rust, tauri, research]
blockedBy: []
blocks: []
created: 2026-11-01
---

# MTPlayer: own Android emulator engine for Windows

## Outcome

MTPlayer is a Windows Android emulator the user builds and owns, in
[tiennm99dev/mtplayer](https://github.com/tiennm99dev/mtplayer). Work use stays within the
licenses because every part is built from open-source code (GPLv2 and
Apache-2.0) instead of a vendor's consumer EULA. It replaces MuMuPlayer,
LDPlayer and BlueStacks for testing the user's apps on both miti-b550 and the
work PC (`tiennm5-pc`). It also runs with Hyper-V, WSL2 and Docker turned on,
which MuMu 12 cannot do.

## Decisions taken

- **2026-11-01, user: build an emulator engine,** not just a launcher over
  Google's stock emulator binary.
- **2026-11-01, user: required features** are a multi-instance manager, app
  testing tools, and root plus device spoofing. Spoofing includes ARM
  translation for ARM-only apps. Keymapping and macros are not required.
- **Engine base (from research): fork AEMU from source rather than write a
  VMM.** AEMU means Google's Android Emulator, `platform/external/qemu`. It is
  the only stack with gfxstream GPU acceleration, the goldfish Android
  devices, snapshots and multi-instance support working on WHPX today. Writing
  a VMM from scratch on crosvm or QEMU would take a team of 8-15 people 2-3
  years. Building AEMU from source also avoids the Android SDK License. That
  license forbids modifying or redistributing Google's binaries but doesn't
  cover the open-source code.
- **2026-11-01, user: this plan replaces the MTPlayer MVP plan of
  2026-08-27.** That plan rejected an AEMU fork in favour of stock QEMU or
  crosvm with scrcpy display, and deferred GPU acceleration to its last
  phase. The repo name stays `mtplayer`.
- **2026-11-01, user: v1 runs x86_64 apps only.** Installing an ARM-only APK
  is refused with a clear message. A separate plan, started after v1, will
  write MTPlayer's own ARM64-to-x86_64 translator. One candidate is an ARM64
  front end for AOSP's Apache-2.0 Berberis, which today only translates
  riscv64 apps. Its feasibility is unverified, and it stays off v1's critical
  path.
- **2026-11-01, user: Android 16** (API 36) is the default image. The review
  noted that Android 17 matches the engine Google is investing in; the user
  kept 16. Re-basing to API 37 is the first item after v1.
- **Engine base (after review):** Phase 1 tries the next-generation engine
  (`emu-main-next-v11.0`) first, and falls back to the classic `emu-main-dev`.
  `emu-master-dev` has been frozen since 2025-03.
- **Licenses:**
  - The engine fork is GPLv2 and lives in its own repos (`mtplayer-engine`
    and a manifest).
  - MTPlayer's own code stays Apache-2.0, because the engine runs as a
    separate program.
  - The user agreed to relicense if that is ever needed.
  - Public releases are GPL distribution, so each one ships its complete
    corresponding source: engine, kernel, and Magisk for the magisk variant.
    This is settled at the Phase 2 and 3 exit gates, before the first
    public release.

## Constraints

- **Hypervisor:** WHPX only. No kernel driver (no HAXM, VirtualBox or AEHD), so
  it coexists with Hyper-V, WSL2 and Docker.
- **No bundled Google software:** no Google Play, no GMS, and no proprietary
  ARM translator copied from SDK images, WSA or firmware.
- **Hardware targets:**
  - miti-b550: Ryzen 7 5800X, 32 GB RAM, RTX 3060.
  - tiennm5-pc: i5-9600K, 16 GB RAM, GTX 1660.
- **Disk:** no AOSP build. Guest images are pinned, prebuilt AOSP emulator
  images from ci.android.com, patched on the host for root, microG and
  identity. The image pipeline should need under 20 GB and runs in WSL2.
  Phase 1 measures the disk the engine build needs. It builds on miti-b550 or
  a self-hosted runner, because GitHub-hosted runners are too small.
- **Code location:** public repo `tiennm99dev/mtplayer`. Company code never
  goes there.
- **Security:**
  - every console, adb and gRPC port binds to `127.0.0.1` only, because
    instances run with root adb;
  - console and gRPC require per-instance tokens that only the daemon holds;
  - the guest is treated as untrusted input, including file paths and
    logcat text;
  - the image-manifest signing key stays outside the repo;
  - the README states the residual threat: any local process on the host has
    root on every instance.

## Non-goals

- Keymapping, gamepad, macros, or a gaming-oriented UI.
- Google Play Store, Play Billing or Play Integrity testing. Those stay on real
  devices.
- macOS or Linux hosts.
- Writing a new VMM or porting goldfish devices to upstream QEMU.
- ARM translation in v1. It is a separate later plan.

## Acceptance criteria

1. With Hyper-V, WSL2 and Docker running, the engine boots on both target
   hosts:
   - cold boot ≤ 60 s;
   - Quick Boot ≤ 10 s.
2. **Instance counts:**
   - four persistent instances run at once on miti-b550;
   - two run on tiennm5-pc, with 2 GB guest RAM each and WSL2 capped at 4 GB
     (`.wslconfig`), and the commit charge stays below physical RAM;
   - each instance has its own adb serial.
3. Performance against MuMuPlayer on miti-b550, using the Phase 1 benchmark
   set. The same x86_64 APKs run on both, and MuMu is measured with Hyper-V
   off. The starting thresholds are below; Phase 1 may propose changes, and
   the user signs off on them:
   - GPU score ≥ 80% of MuMu;
   - app cold start ≤ 125% of MuMu's time;
   - guest `fio` 4K random ≥ 50% of MuMu.
4. **Cloning and identity:**
   - a cloned instance gets a new Android ID, serial, MAC address, IMEI, IMSI
     and phone number;
   - on rooted instances, model, brand and fingerprint can be set per
     instance;
   - unrooted instances use their image profile's values.
5. Root can be turned on or off per instance: `adb root` on every instance, and
   Magisk as an option.
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
   user's default Android SDK home folder.
9. **Licenses:**
   - `LICENSES.md` lists every component's license;
   - every public release, from the first one onward, carries its complete
     corresponding GPL source: engine, kernel and Magisk.

## Phases

| # | Phase | Status | Effort | Depends on |
| --- | --- | --- | --- | --- |
| 1 | [Feasibility spike and gates](phase-01-feasibility-spike-and-gates.md) | pending | 3-4 weeks | none |
| 2 | [Guest image pipeline](phase-02-guest-image-pipeline.md) | pending | 3-5 weeks | 1 |
| 3 | [Engine build (3a) and patch series (3b)](phase-03-engine-fork.md) | pending | 3a: 2 weeks, 3b: 4-8 weeks | 1 |
| 4 | [Instance daemon and CLI](phase-04-instance-daemon-and-cli.md) | pending | 4-6 weeks | 2, 3a |
| 5 | [App testing tools](phase-05-app-testing-tools.md) | pending | 3-4 weeks | 4 |
| 6 | [Manager GUI](phase-06-manager-gui.md) | pending | 4-6 weeks | 4, 5 |
| 7 | [Packaging, performance and rollout](phase-07-packaging-performance-and-rollout.md) | pending | 3-4 weeks | 5, 6 |

Order for one developer:

1. Phase 1.
2. Phase 2.
3. Phase 3a.
4. Phases 4 and 5, with 3b interleaved. 3b only adds engine patches behind
   flags that Phase 4 already passes.
5. Phase 6.
6. Phase 7.

The phases sum to 26-39 full-time weeks. With 25% slack for integration and
rework, that is 33-49 weeks, or about 15-23 months at half time.

## Architecture

```text
 Tauri 2 manager GUI ──┐
                       ├──> instance daemon (Rust, gRPC + CLI)
 CLI / CI scripts ─────┘         │  store: per-instance AVD dir + qcow2 overlays + own snapshot
                                 │  (shared read-only system image; ephemeral CI mode: -read-only)
                                 ▼
                    engine.exe per instance (mtplayer-engine fork on WHPX, token auth)
                       │ gfxstream ──> host GPU driver (ANGLE fallback)
                       │ modem simulator (per-instance IMEI/IMSI/number)
                       │ console + gRPC control + adb port (mtp-adb client, no adb.exe)
                       ▼
                    guest: pinned prebuilt AOSP sdk_phone64_x86_64 userdebug image
                       repacked and re-signed on the host (test key, verity on):
                       androidboot.serialno, image profile, Magisk resetprop on rooted
                       instances, optional microG (x86_64 apps only in v1)
```

## Key risks

- **Google is moving the emulator from its QEMU 2.12 fork to an upstream
  QEMU 11 engine** (`emu-main-next-v11.0`, documented for API 37+). Phase 1
  tries that engine with Android 16 first. If it falls back to the classic
  engine, the re-base trigger is "MTPlayer needs API 37".
- **There is no free ARM translator you can legally redistribute.** v1 is
  x86_64 only. Own translator work comes in a separate plan.
- **WHPX VM exits cost more than the kernel drivers MuMu and LDPlayer use.**
  The Phase 1 benchmark checks acceptance criterion 3 before the big build-out
  starts.
- **GPU drivers break in vendor-specific ways.** Phase 7 keeps a list of
  known-good drivers.
- **No Android 16 emulator image may be published on ci.android.com.** Phase
  1 checks this first. The fallbacks are older `aosp-android-latest-release`
  builds, then frozen `aosp-main` builds, then one build on a rented cloud
  VM. Any image found is mirrored the same day.

## Research

- [Earlier ChatGPT findings on licensing and Hyper-V](../reports/research-261101-1241-android-emulator-prior-chatgpt-findings.md)
- [Engine stack: VMM, images, graphics](../reports/researcher-261101-1241-android-emulator-engine-stack.md)
- [Feature layer: ARM, root, spoofing, tools, GMS, multi-instance, GUI](../reports/researcher-261101-1241-android-emulator-feature-layer.md)
- [Fable review of this plan, 2026-11-01](../reports/review-261101-1408-mtplayer-engine-plan.md)
- The superseded MTPlayer MVP plan and its research stay in git history
  (commit `6bda00d`).

## Validation Log

### Session 1, 2026-11-01

- ARM path: the user asked whether we can write our own translator. Yes, as
  an ARM64 front end for Berberis, but it is a large effort. The user chose
  a separate later track; v1 is x86_64 only.
- Repo: the user chose `tiennm99dev/mtplayer`, which already existed. This
  plan replaces its 2026-08-27 plan, and the repo name stays.
- Build host: the user asked why AOSP needs about 400 GB. Source is
  100-150 GB and build output 150-250 GB; a shallow sync cuts the total to
  about 250 GB. Phase 1 still picks the host.
- Android version: Android 16 confirmed.
- Not run: the agent red-team pass. Its evidence filter needs `file:line`
  citations from code, and the repo has no code yet.

### Session 2, 2026-11-01

- Disk: the user doesn't have the free disk an AOSP build needs, and asked
  which parts of the superseded MVP plan fit a low-disk plan. Decision: no
  AOSP build. The plan now keeps these parts of the MVP plan:
  - pinned prebuilt CI images;
  - a host-side repack instead of a rebuild;
  - split system and data images, distributed through signed GitHub
    Releases;
  - its own ADB device-protocol client instead of `adb.exe`.

  It still drops the MVP plan's Cuttlefish images (their host tools run on
  Linux only), its stock QEMU or crosvm with scrcpy display (no GPU speed),
  and Android 13.
- Session 1's build-host answer is superseded: no machine needs to hold an
  AOSP tree.
- Phases changed: 1 (image gate and identity proofs), 2 (rewritten as a
  repack pipeline), 5 (own ADB client) and 7 (installer contents).

### Session 3, 2026-11-01: Fable review

The user asked for a review on the Fable model, then chose to apply every
Critical, High and Medium finding. The Low findings stay in the report as
notes. The fork-base finding was checked against android.googlesource.com
before applying it.

| Finding | Change |
| --- | --- |
| C1: the plan forked a branch frozen since 2025-03 | Fork `emu-main-next-v11.0` if it boots Android 16 on WHPX, otherwise `emu-main-dev` (Phase 1 task 3, Phase 3) |
| C2: `-read-only` plus a shared RAM snapshot contradicts per-instance identity | Two instance modes, persistent and ephemeral, each persistent instance with its own snapshot (Phase 4) |
| C3: a vendor or odm `build.prop` patch can't change `ro.product.*` | Per-instance model and brand on rooted instances only; per-image profiles patch product, system and vendor; criterion 4 updated (Phase 2) |
| H1: ci.android.com availability is uncertain | More fallbacks and a same-day mirror (Phase 1 task 1) |
| H2: "AVB off" was the wrong mechanism | Re-sign with the AOSP test key and regenerate `VerifiedBootParams.textproto`, keeping verity on (Phases 1 and 2) |
| H3: the Android ID can't be reset before first boot | Fresh clone by default; full clone through a provisioning boot (Phases 2 and 4) |
| H4: public releases count as GPL distribution | The engine fork goes in its own repos; source is shipped with every release from the first one; the inventory moves to Phase 1 and the Phase 2/3 exit gates. The user allows a relicense if needed, but none is needed: the engine is a separate program. |
| H5: work was serialized for no reason | Phase 3 split into 3a and 3b; Phase 4 now depends on 3a; a single-developer order is given |
| M1-M9 | Per-instance AVD directories and environment variables; no Job Object memory cap; benchmark APK ABI checks and the MuMu host state; a bundled minimal JRE; no GitHub-hosted runner builds; WSL2-only otatools; token auth and the residual-threat note; WSL2 memory cap and per-metric thresholds; effort restated with slack |

The Android version stays 16, as the user decided. The review's note about
Android 17 is recorded under Decisions taken.
