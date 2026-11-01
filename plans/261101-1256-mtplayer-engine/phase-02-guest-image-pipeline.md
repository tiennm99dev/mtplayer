---
phase: 2
title: "Guest image pipeline"
status: pending
priority: P1
effort: "3-5 weeks"
dependencies: [1]
---

# Phase 2: Guest image pipeline

<!-- Updated: Validation Session 2 - prebuilt CI images plus host-side repack replace the own AOSP build -->

## Goal

Turn a pinned, prebuilt AOSP emulator image into MTPlayer's guest images
without building AOSP. Root, microG and per-instance identity are added by
patching on the host, so the pipeline needs only a few GB of disk.

## Requirements

- **Base:**
  - the AOSP `sdk_phone64_x86_64` userdebug emulator image (`emu_img_zip`)
    from ci.android.com;
  - Android 16 (API 36), pinned by build ID; Phase 1 confirms the image is
    published;
  - never use a floating branch;
  - record the build ID, the SHA-256 and the artifact URL in
    `images/manifest.json`.
- **Tools:** only AOSP's `otatools` package (Apache-2.0): `lpunpack`,
  `lpmake`, `avbtool`, `simg2img` and `mkbootfs`. They run on Windows through
  WSL2 or Git Bash, so no AOSP tree is needed.
- **Per-instance identity:** apply the cheapest mechanism that works, in this
  order. Each one is proven in Phase 1.
  1. **Serial and boot properties:** `androidboot.*` kernel command line
     entries from the engine, for example `androidboot.serialno`, which AOSP
     copies to `ro.serialno`.
  2. **Model, brand and fingerprint on rooted instances:** a Magisk module
     whose `post-fs-data` script calls `resetprop` with values read from the
     instance config.
  3. **Model, brand and fingerprint on unrooted instances:** a small
     per-instance vendor or odm overlay whose `build.prop` the host patches
     before first boot. AVB is turned off for MTPlayer images, and that
     choice is recorded.
  4. **Android ID:** a fresh data overlay creates a new one. On clone, the
     daemon clears `settings_ssaid.xml` over root adb before the first boot
     of the clone.
- **Root:** userdebug already gives `adb root`. The magisk variant patches
  the ramdisk on the host with Magisk's boot-patch script, and each instance
  picks its ramdisk.
- **microG:** an opt-in variant that installs the microG APKs as privileged
  system apps through the repack, using signature spoofing support only if
  the image supports it. The default image has no Google services.
- **No native bridge in v1.** Leave `ro.dalvik.vm.native.bridge` set to `0`.
- **Distribution:**
  - the system image stays separate from the data image;
  - assets are compressed and kept under 2 GiB each;
  - published on GitHub Releases with a signed manifest.

## Tasks & Steps

1. `tools/image-pipeline/fetch`: download by pinned build ID, then check the
   checksum against the manifest.
2. `tools/image-pipeline/repack`: unpack, apply the variant patch (base,
   magisk or microg), repack, then write the checksums.
3. `tools/image-pipeline/identity`: build the per-instance overlay and the
   Magisk module that Phase 4 calls when it creates or clones an instance.
4. `tools/image-pipeline/package`: compress, sign the manifest, and upload to
   a GitHub Release.
5. Document how to re-pin to a newer build ID, and which checks to re-run.

## Verification

- The pipeline uses less than 20 GB of disk from download to packaged output.
- `adb shell getprop ro.serialno` and `ro.product.model` return the values
  set for the instance, on both rooted and unrooted instances.
- Two clones show different serials and Android IDs.
- The magisk variant passes `adb shell su -c id`. The base variant has no
  `su`.
- The microg variant passes microG's self-check.
- Repacking the same build ID twice gives the same checksums.

## Risks

- **Google may stop publishing AOSP emulator images on ci.android.com, or
  remove old build IDs.** Mirror every pinned artifact into the project's
  GitHub Release. If no Android 16 image is published at all, fall back to
  one build on a rented cloud VM (about 250 GB of disk for a few hours),
  then continue with this repack pipeline.
- **The license terms of ci.android.com artifacts are unverified.** The
  artifacts are built from AOSP source (Apache-2.0 and GPLv2 kernel), but
  check for any extra terms in Phase 1 before redistributing them.
- **Patching ro properties changes how the image boots.** Keep the base image
  unchanged, and put every change in a variant or overlay that can be turned
  off.
