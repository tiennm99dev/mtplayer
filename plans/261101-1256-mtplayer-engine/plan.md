---
title: "MTPlayer: own Android emulator engine for Windows"
description: "Build a free, commercially licensable Android emulator for Windows from the open-source Android Emulator (AEMU) and AOSP sources to replace MuMuPlayer, LDPlayer and BlueStacks for app testing."
status: pending
priority: P2
effort: 6-9 months (one developer, part-time estimates in phases)
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
  write MTPlayer's own ARM64-to-x86_64 translator: an ARM64 front end for
  AOSP's Apache-2.0 Berberis, which today only translates riscv64 apps.
- **2026-11-01, user: Android 16** (API 36) is the default image.
- **Licenses:** MTPlayer's own code (the daemon, CLI and GUI) stays
  Apache-2.0, like the repo. The engine fork stays GPLv2 under `engine/`, and
  runs as a separate process.

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
  identity. The image pipeline should need under 20 GB. Windows engine
  binaries are cross-compiled with mingw or built natively; Phase 1 decides
  which, and measures the disk the engine build needs.
- **Code location:** public repo `tiennm99dev/mtplayer`. Company code never
  goes there.
- **Security:**
  - every console, adb and gRPC port binds to `127.0.0.1` only, because
    instances run with root adb;
  - the guest is treated as untrusted input, including file paths and
    logcat text.

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
2. Four instances run at once on miti-b550, and two on tiennm5-pc, each with
   its own adb serial.
3. Performance is within 80% of MuMuPlayer on miti-b550 on the benchmark set
   from Phase 1 (GPU benchmark score, app cold-start time, guest `fio`).
4. A cloned instance gets a new Android ID, serial, MAC address, IMEI, IMSI
   and phone number. Model, brand and fingerprint can be set per instance.
5. Root can be turned on or off per instance: `adb root` on every instance, and
   Magisk as an option.
6. From the CLI and the GUI, you can:
   - install APKs, split APKs, AABs and XAPKs (APKs plus OBB);
   - read logcat;
   - take screenshots and screen recordings;
   - push and pull files;
   - set GPS location, network speed and latency, battery and sensors.
7. Installing an ARM-only APK fails at once with a message naming the missing
   ABI. The limitation is in the README.
8. Install and uninstall on tiennm5-pc with no admin steps beyond turning on
   the WHPX Windows feature. Uninstall leaves nothing behind.
9. A license inventory lists every component's license. The GPL source offer is
   ready in case the tool is ever handed to anyone else.

## Phases

| # | Phase | Status | Effort | Depends on |
| --- | --- | --- | --- | --- |
| 1 | [Feasibility spike and gates](phase-01-feasibility-spike-and-gates.md) | pending | 3-4 weeks | none |
| 2 | [Guest image pipeline](phase-02-guest-image-pipeline.md) | pending | 3-5 weeks | 1 |
| 3 | [Engine fork](phase-03-engine-fork.md) | pending | 6-10 weeks | 1 |
| 4 | [Instance daemon and CLI](phase-04-instance-daemon-and-cli.md) | pending | 4-6 weeks | 2, 3 |
| 5 | [App testing tools](phase-05-app-testing-tools.md) | pending | 3-4 weeks | 4 |
| 6 | [Manager GUI](phase-06-manager-gui.md) | pending | 4-6 weeks | 4, 5 |
| 7 | [Packaging, performance and rollout](phase-07-packaging-performance-and-rollout.md) | pending | 3-4 weeks | 5, 6 |

Phases 2 and 3 can run in parallel after Phase 1, because they touch separate
trees (the image pipeline versus the engine).

## Architecture

```text
 Tauri 2 manager GUI ──┐
                       ├──> instance daemon (Rust, gRPC + CLI)
 CLI / CI scripts ─────┘         │  instance store: shared read-only system disk
                                 │  + per-instance qcow2 data overlay + identity config
                                 ▼
                    engine.exe per instance (AEMU fork on WHPX)
                       │ gfxstream ──> host GPU driver (ANGLE fallback)
                       │ modem simulator (per-instance IMEI/IMSI/number)
                       │ console + gRPC control + adb port (mtp-adb client, no adb.exe)
                       ▼
                    guest: pinned prebuilt AOSP sdk_phone64_x86_64 userdebug image
                       patched on the host: identity overlay or Magisk resetprop,
                       optional Magisk ramdisk, optional microG (x86_64 apps only in v1)
```

## Key risks

- **Google is moving AEMU from its QEMU 2.12 fork to an upstream QEMU 11
  engine.** A fork made now may need re-porting within 12-18 months. Phase 1
  picks the base, and Phase 3 keeps the patches small and grouped.
- **There is no free ARM translator you can legally redistribute.** v1 is
  x86_64 only. Own translator work comes in a separate plan.
- **WHPX VM exits cost more than the kernel drivers MuMu and LDPlayer use.**
  The Phase 1 benchmark checks acceptance criterion 3 before the big build-out
  starts.
- **GPU drivers break in vendor-specific ways.** Phase 7 keeps a list of
  known-good drivers.
- **No Android 16 emulator image may be published on ci.android.com.** Phase
  1 checks this first. The fallback is one build on a rented cloud VM.

## Research

- [Earlier ChatGPT findings on licensing and Hyper-V](../reports/research-261101-1241-android-emulator-prior-chatgpt-findings.md)
- [Engine stack: VMM, images, graphics](../reports/researcher-261101-1241-android-emulator-engine-stack.md)
- [Feature layer: ARM, root, spoofing, tools, GMS, multi-instance, GUI](../reports/researcher-261101-1241-android-emulator-feature-layer.md)
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
