---
phase: 1
title: "Feasibility spike and gates"
status: in progress
priority: P1
effort: "2-3 weeks"
dependencies: []
---

# Phase 1: Feasibility spike and gates

## Status

As of 2026-11-01, see the
[spike report](../reports/spike-261101-1700-stock-emulator-gates.md).

| Task | Status | Note |
| --- | --- | --- |
| 1. Package pin | done | Pins are in `spike/packages.pin.json`. The license hash is a text-change detector, not proven to be Google's format. |
| 2. Boot and isolation | done | The console token is per Windows user and `adb.exe` writes to `%USERPROFILE%\.android`; both are stock limits. Not yet measured with WSL2 and Docker running. |
| 3. Identity proofs | done | `resetprop` reaches apps. The IMEI is fixed and the MAC address is partial. |
| 4. Multi-instance | done | Four instances ran at once on miti-b550. |
| 5. Baseline benchmarks | pending-user | The MuMu baseline needs the user's reboot with Hyper-V off. |
| 6. ABI check | pending-user | Needs the user's work APKs. |
| 7. License inventory | done | First version of [`LICENSES.md`](../../LICENSES.md). |
| 8. Go/no-go | partial | The review says GO for Phase 2. Six questions in the report wait for the user. |

## Goal

Before any large investment, prove that Google's stock emulator and the
unmodified Android 16 `default` image meet the plan's needs on WHPX:
performance, multi-instance, and per-instance identity set only at runtime.
Every proof runs on the stock packages, with no file of theirs changed.

## Context

- Done (session 4): the image gate. No ci.android.com goldfish image exists,
  so the image is `system-images;android-36;default;x86_64` from
  dl.google.com. The engine-recipe research showed that Android 16 runs on
  the classic engine in any case. See the
  [session 4 log](plan.md#session-4-2026-11-01-image-re-plan).
- Work files go in the repo's git-ignored `.tmp/` folder: packages in
  `.tmp/sdk`, AVDs in `.tmp/avd`, evidence in `.tmp/evidence`. Nothing goes
  in `C:\mtp` or in the user's default Android SDK home folder.
- The spike gets the packages by hand-written download plus hash check, in
  the layout Phase 2's downloader will produce. It doesn't use the existing
  Android Studio SDK, so the spike also proves that layout works on its own.

## Tasks & Steps

1. **Package pin.**
   - From Google's repository XML (`repository2-3.xml` for the emulator,
     `sys-img/android/sys-img2-3.xml` for images), record the pinned
     emulator version and the `android-36;default;x86_64` revision, with each
     zip's URL, size and SHA-1.
   - Download both into `.tmp/sdk` in the standard SDK layout. Extract each
     into its own empty directory.
   - Confirm the image is a userdebug build (`ro.build.type`), and list
     which files the emulator needs beside it (for example `package.xml`,
     `platform-tools`, or a `platforms` folder).
2. **Boot and isolation.**
   - Boot one AVD from `.tmp/avd`, with `ANDROID_AVD_HOME`,
     `ANDROID_EMULATOR_HOME` and `ANDROID_SDK_ROOT` all pointing into
     `.tmp`, while Hyper-V, WSL2 and Docker run.
   - Check `sys.boot_completed=1`, `adb root`, a screenshot, and that the
     console, adb and gRPC ports listen on `127.0.0.1` only.
   - Check that the console auth token is per instance (it lives in
     `ANDROID_EMULATOR_HOME`), and that gRPC token auth works.
   - Check that nothing is written to the user's default Android SDK home
     folder.
3. **Identity proofs**, each on one instance, with no Google file changed:
   - **Boot properties:** which of `ro.product.model`, `ro.product.brand`,
     `ro.product.device`, `ro.build.fingerprint` and `ro.serialno` the
     emulator's `-prop` flag or `-qemu -append androidboot.*` can set, and
     whether `Build.*` in a test app shows them;
   - **resetprop at runtime:** push the `magisk` binary from a pinned Magisk
     GitHub release to `/data/local/tmp`, run `magisk resetprop` as root,
     restart the framework (`stop; start`), and check `Build.*` in the test
     app. Record the time the restart adds to boot;
   - **Android ID:** a fresh AVD gets a new one; a cloned data overlay gets a
     new one after deleting `/data/system/users/0/settings_ssaid.xml` as
     root and rebooting headless with no network;
   - **Telephony and network:** whether `-phone-number`, the console, or any
     other flag changes the phone number, IMEI, IMSI and MAC address per
     instance.
4. **Multi-instance.** Run four instances on miti-b550, each with its own
   AVD directory, qcow2 overlays, ports and Quick Boot snapshot, all sharing
   one read-only system image. Record RAM, commit charge and boot times.
5. **Baseline benchmarks.**
   - Benchmark the stock emulator on miti-b550 with Hyper-V, WSL2 and Docker
     running.
   - Benchmark MuMuPlayer the same way. It needs Hyper-V off, so this needs
     a reboot that the user does; record the host state for every run.
   - Benchmark set: GPU score, app cold-start time, guest `fio` 4K random
     read/write, cold boot and Quick Boot.
   - Before picking each benchmark APK, check its `lib/` folder for x86_64
     code. Prefer GFXBench or a small self-built GLES/Vulkan test app.
6. **ABI check.** List the ABIs of the work apps the user tests (the `lib/`
   folders in each APK). Record any ARM-only app as a reason for the later
   translator plan. Never copy libndk_translation or libhoudini.
7. **License inventory, first version.** Cover the Android SDK License (the
   emulator and image, downloaded by the user), Magisk (GPL-3, downloaded by
   the user), bundletool (Apache-2.0) and OpenJDK (GPLv2 with Classpath
   Exception). Note the trademark rule: no "Android" in the product name.
8. **Go/no-go.**
   - Compare the results with acceptance criteria 1-4, metric by metric.
   - If a threshold is missed, or an identity field can't change, stop and
     report before Phase 2. The user decides whether to change the criterion
     or stop.

## Files to create

- `plans/reports/benchmark-<date>-emulator-baseline.md`
- `plans/reports/spike-261101-1700-stock-emulator-gates.md`: the pins, proofs
  and their evidence
- `LICENSES.md` (first version)
- `spike/` scripts that reproduce each proof from an empty `.tmp/`

## Verification

- An instance from `.tmp` boots on WHPX with `HypervisorPlatform` on and a
  WSL2 distro running, and `adb root` succeeds.
- Every identity proof in task 3 is recorded as passed or failed, with its
  evidence.
- The benchmark report has numbers for the stock emulator and MuMu, with the
  host state noted for each run.
- No emulator process is left running after the spike.

## Risks and rollback

- If runtime identity doesn't reach apps, criterion 4 has to shrink. The
  only way to keep it would be modifying the image, which the user ruled out,
  so this goes back to the user.
- Nothing in this phase changes the host beyond the WHPX feature, a WSL2
  distro, and the reboots for the MuMu baseline. Deleting `.tmp/` undoes the
  rest.
