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

Before any large investment, prove that a self-built engine and a repacked
prebuilt image boot and perform well enough on WHPX. Settle the two decisions
everything else depends on:

- **the engine base:** the next-generation engine or the classic one;
- **the image source:** which pinned Android 16 build.

The ARM path is already decided: v1 runs x86_64 apps only.

## Context

The research is linked from [plan.md](plan.md#research). Google maintains
two emulator engines; both branches were active in October 2026:

| Engine | Branches | Notes |
| --- | --- | --- |
| Classic (the QEMU 2.12 fork) | `emu-main-dev` in `platform/external/qemu` | `emu-master-dev` has been frozen since 2025-03-27, so never use it. |
| Next-generation (upstream QEMU 11) | `emu-main-next-v11.0` in `platform/external/qemu`, plus `emu-main-next` in `platform/hardware/generic/goldfish` | Built with Bazel. Its README lists Windows x64 as supported. Google documents it for API 37+ AVDs only. |

## Tasks & Steps

1. **Image gate (day one).**
   - Find a ci.android.com build that publishes an Android 16
     `sdk_phone64_x86_64` userdebug `emu_img_zip`. Try the sources in this
     order:
     1. an Android 16 or 16 QPR build ID on `aosp-android-latest-release`
        from before 2026-06, if Google still keeps it;
     2. a frozen `aosp-main` build from before CI stopped;
     3. one build on a rented cloud VM, with about 250 GB of disk for a few
        hours. Record the cost.
   - Record the exact target name, for example
     `sdk_phone64_x86_64-trunk_staging-userdebug`.
   - Mirror the artifact to a private store the same day, because Google
     doesn't document how long it keeps builds.
   - Check the artifact for any terms beyond the AOSP component licenses.
2. **Baseline.**
   - Benchmark a stock AVD on miti-b550 with Hyper-V, WSL2 and Docker
     running. The Android SDK License allows commercial app development, so
     the stock emulator is fine as a baseline.
   - Benchmark MuMuPlayer the same way. It needs Hyper-V off, so reboot and
     record the host state for every run.
   - Benchmark set: GPU score, app cold-start time, guest `fio` 4K random
     read/write, cold boot and Quick Boot.
   - Before picking each benchmark APK, check its `lib/` folder for x86_64
     code. 3DMark reportedly rejects x86_64 emulators, so prefer GFXBench or
     a small self-built GLES/Vulkan test app.
   - Save the results to
     `plans/reports/benchmark-<date>-emulator-baseline.md`.
3. **Engine base gate.**
   - Build `emu-main-next-v11.0` with its goldfish companion repo for
     Windows. Give it one or two days to boot the Android 16 image on WHPX.
   - If it boots, fork the next-generation engine.
   - If it doesn't, fork `emu-main-dev`. Record the re-base trigger as "the
     day MTPlayer needs API 37".
   - For the chosen base, record the build route, the build time, and the
     disk used by the checkout and build output. Build on miti-b550 or a
     self-hosted runner: GitHub-hosted runners have 14 GB of disk and a
     6-hour limit.
4. **Identity proofs**, each on one instance:
   - `androidboot.serialno` on the kernel command line or in bootconfig
     shows up in `ro.serialno`;
   - Magisk `resetprop` in `post-fs-data` changes `ro.product.model`, and
     `Build.MODEL` reflects it;
   - a host repack of `product`, `system` and `vendor` `build.prop` boots
     after re-signing with the AOSP test key and regenerating
     `VerifiedBootParams.textproto` with `mk_verified_boot_params.sh`;
   - a provisioning boot (headless, no network) resets the Android ID on a
     cloned data overlay;
   - check where the chosen engine serves the IMEI. The classic goldfish
     modem returns a fixed `+CGSN`, while the newer modem simulator reads
     per-instance NVRAM.
5. **ABI check.**
   - List the ABIs of the work apps the user tests (the `lib/` folders in
     each APK).
   - Record any ARM-only app as a reason for the later translator plan.
   - Never copy libndk_translation or libhoudini out of Google, WSA or
     firmware images.
6. **License inventory, first version.**
   - Cover the engine (GPLv2 plus Apache), the AOSP image including the GKI
     kernel (GPLv2, with the exact kernel source commit), gfxstream, Magisk
     (GPL-3), microG (Apache) and OpenJDK (GPLv2 with Classpath Exception).
   - Define how each public release ships its complete corresponding source.
     A link to upstream is not enough.
   - Note the trademark rule: no "Android" in the product name.
7. **Go/no-go.**
   - Compare the self-built engine with acceptance criterion 3, metric by
     metric.
   - If it misses a threshold, stop and report before Phase 2. The user
     decides whether to change the threshold or stop.

## Files to create

- `plans/reports/benchmark-<date>-emulator-baseline.md`
- `plans/reports/spike-<date>-emulator-engine-gates.md`: the decisions,
  sizes, build logs and proofs
- `LICENSES.md` (first version)

## Verification

- The self-built engine boots the pinned image on WHPX with
  `HypervisorPlatform` on and a WSL2 distro running.
- The guest's adbd answers on `127.0.0.1` and `adb root` succeeds.
- The benchmark report has numbers for the stock AVD, MuMu and the
  self-built engine, with the host state noted for each run.
- Every identity proof in task 4 is recorded as passed or failed, with its
  evidence.

## Risks and rollback

- If neither engine builds for Windows, stop and re-plan; the plan depends on
  a working build.
- Nothing in this phase changes the host beyond the WHPX feature, a WSL2
  distro, and the reboots for the MuMu baseline.
