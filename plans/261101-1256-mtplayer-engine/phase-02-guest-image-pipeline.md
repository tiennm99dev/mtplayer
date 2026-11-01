---
phase: 2
title: "Guest image pipeline"
status: pending
priority: P1
effort: "4-6 weeks"
dependencies: [1]
---

# Phase 2: Guest image pipeline

## Goal

Build reproducible AOSP x86_64 userdebug images that read their identity from
the engine at boot. Root and microG are build variants.

## Requirements

- Base: AOSP `sdk_phone_x86_64` on Android 16 (API 36). Pin an
  `android-16.0.0_rNN` tag. Never use a moving branch: `android-latest-release`
  already tracks Android 17.
- An identity init service, run as an early `init.rc` service, copies
  boot-time properties into `ro.product.*`, `ro.build.fingerprint`,
  `ro.serialno` and the Android ID seed:
  - **Inputs:** the engine passes boot-time properties (`qemu.*` / `-prop`
    values).
  - **Unverified:** the order in which property overrides apply, and where
    the Android ID is stored. Confirm both in AOSP source before writing the
    service.
- Root: userdebug gives `adb root` everywhere. A second boot ramdisk patched
  with Magisk (GPL-3) is chosen per instance at boot.
- microG (Apache-2.0) is an opt-in build variant. The default image has no
  Google services.
- No native bridge in v1. Leave `ro.dalvik.vm.native.bridge` set to `0`, so
  the later Berberis-based translator can be added as a new image variant.

## Tasks & Steps

1. Script the build on the Phase 1 build host: repo sync, local manifest,
   lunch target, `m emu_img_zip`, and an artifact name that includes the
   AOSP tag.
2. Add a `device/mtplayer/` overlay with the identity service, its SELinux
   policy, and product property defaults.
3. Add the variants: `base`, `magisk` (ramdisk only) and `microg`.
4. Publish the artifacts (system, vendor, ramdisk variants and kernel) with
   checksums to a local artifact folder that Phase 4's instance store reads
   from.
5. Document the rebuild routine for each AOSP drop, which comes twice a year.

## Verification

- `adb shell getprop ro.product.model` returns the value passed by the engine.
- Two boots with different identity inputs show different Android IDs and
  serials.
- The magisk variant passes `adb shell su -c id`. The base variant has no
  `su`.
- The microg variant passes microG's self-check.
- The build is reproducible: two builds of the same tag give the same
  artifact list.

## Risks

- AOSP builds need a lot of disk and RAM. If WSL2 runs out, move to
  miti-x600.
- SELinux denials in the identity service. Fix them with policy, never by
  setting SELinux to permissive.
