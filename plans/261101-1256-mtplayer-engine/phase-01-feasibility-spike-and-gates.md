---
phase: 1
title: "Feasibility spike and gates"
status: pending
priority: P1
effort: "3-4 weeks"
dependencies: []
---

# Phase 1: Feasibility spike and gates

## Goal

Prove that the stack boots and performs well enough on WHPX, and settle the
two decisions everything else depends on: the engine base and the build
route. Do this before any large investment. The ARM path is already decided:
v1 is x86_64 only.

## Context

- The research is linked from [plan.md](plan.md#research).
- Google is moving AEMU from its QEMU 2.12 fork to an upstream QEMU 11 engine.
  That engine is a preview for API 37+ only, and its source was not found
  during research.

## Tasks & Steps

1. **Baseline.**
   - Run a stock AVD (Google's binary, personal evaluation only) on miti-b550
     with Hyper-V, WSL2 and Docker running.
   - Record a benchmark set for both the AVD and MuMuPlayer: GPU score
     (GFXBench or 3DMark Wild Life), cold-start time for a reference app,
     guest `fio` 4K random read/write, cold boot, and Quick Boot.
   - Save the numbers to `D:/plans/reports/benchmark-<date>-emulator-baseline.md`.
2. **Build route.**
   - Build AEMU from `platform/external/qemu` (the emu-master-dev branch or
     the latest release tag) for Windows.
   - Try the documented Linux `--mingw` cross-build in WSL2 first, then the
     native `cmake.py` clang-cl build.
   - Record which one works and how long it takes.
3. **Image route.**
   - Build AOSP `sdk_phone_x86_64` userdebug from `android-latest-release`
     with `m emu_img_zip`.
   - Pick the build host: miti-x600, or WSL2 on miti-b550 C: (NVMe).
     Use a shallow sync (`repo sync -c --depth=1`), which needs about
     250 GB; a full sync needs about 400 GB. Base the choice on disk space
     and RAM.
   - Boot the image in the self-built engine on WHPX.
4. **Engine base.**
   - Look for the source of the QEMU 11 next-generation engine.
   - If it is public and builds for Windows with gfxstream, prefer it.
     Otherwise fork the classic engine and record a re-base trigger.
5. **ABI check.**
   - List the ABIs of the work apps the user tests (`aapt2 dump badging`, or
     the `lib/` folders inside each APK).
   - If any app is ARM-only, record it as a reason for the later translator
     plan.
   - Never copy libndk_translation or libhoudini out of Google, WSA or
     firmware images.
6. **Legal checklist.** Write the license inventory skeleton covering AEMU
   (GPLv2 and Apache), AOSP, gfxstream, Magisk (GPL-3), microG (Apache) and the
   modem simulator. Note the trademark rule: no "Android" in the product name.
7. **Go/no-go.** Compare the self-built engine against acceptance criterion 3.
   If it is below 80% of MuMu, stop and report before Phase 2.

## Files to create

- `D:/plans/reports/benchmark-<date>-emulator-baseline.md`
- `D:/plans/reports/spike-<date>-emulator-engine-gates.md`, recording the
  decisions and build logs
- In `tiennm99dev/mtplayer`: an `engine/` folder for the GPLv2 fork, with its
  own COPYING file, and a `spike/` folder for the build scripts.

## Verification

- The self-built engine and self-built image boot on WHPX with
  `Get-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform` enabled
  and `wsl -l -v` showing a running distro.
- `adb devices` lists the instance, and `adb root` succeeds.
- The benchmark report has numbers for AVD, MuMu and the self-built engine.

## Risks and rollback

- If the Windows build fails both ways, fall back to the stock binary for
  personal evaluation only and re-plan. The engine plan depends on a working
  build.
- Nothing in this phase changes the host beyond the WHPX feature and a WSL2
  distro.
