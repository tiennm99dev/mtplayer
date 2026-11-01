---
phase: 2
title: "Guest image pipeline"
status: pending
priority: P1
effort: "3-5 weeks"
dependencies: [1]
---

# Phase 2: Guest image pipeline

## Goal

Turn the pinned prebuilt AOSP emulator image into MTPlayer's guest images
without building AOSP. Root, microG and identity profiles are added by
patching on the host. The pipeline needs only a few GB of disk.

## Requirements

- **Base:**
  - the Android 16 `sdk_phone64_x86_64` userdebug `emu_img_zip` pinned in
    Phase 1;
  - its build ID, SHA-256, target name and mirror URL are recorded in
    `images/manifest.json`.
- **Tools:**
  - AOSP `otatools` (Apache-2.0): `lpunpack`, `lpmake`, `simg2img`,
    `e2fsdroid` and `avbtool`;
  - `mk_verified_boot_params.sh` from `device/generic/goldfish/tools`, plus
    `sgdisk`.

  These are Linux binaries, so the pipeline runs in WSL2 only.
- **Verified boot stays on.**
  - After any patch, re-sign the partitions with the public AOSP test key
    (`testkey_rsa2048.pem`): `avbtool add_hashtree_footer` and
    `make_vbmeta_image`.
  - Then regenerate `VerifiedBootParams.textproto`.
- **Identity:** the cheapest mechanism that works.
  1. **Per-instance serial** for every instance: `androidboot.serialno` from
     the engine.
  2. **Per-instance model, brand and fingerprint** on rooted instances only:
     a Magisk module whose `post-fs-data` script calls `resetprop` with
     values from the instance config.
  3. **Unrooted instances:** they use the model, brand and fingerprint of
     their image's profile. A profile is a repack of the `product`,
     `system` and `vendor` `build.prop` files together. It has to be all
     three, because init reads `ro.product.*` from `product` first.

     Profiles are per image, not per instance, so many instances still share
     one read-only system disk.
- **Android ID on clone:**
  - **Fresh clone** (the default): a new data overlay from the base
    `userdata`, with no app data copied.
  - **Full clone:** copies the overlay, then runs a provisioning boot that
    deletes `/data/system/users/0/settings_ssaid.xml` over root adb and
    reboots. The boot is headless and has no network.

  Host-side editing isn't possible, because userdata uses file-based
  encryption.
- **Root:** userdebug already gives `adb root`. The magisk variant patches
  the ramdisk on the host with Magisk's boot-patch script, and each instance
  picks its ramdisk.
- **microG:** an opt-in variant that installs the microG APKs as privileged
  system apps. The default image has no Google services.
- **No native bridge in v1.** Leave `ro.dalvik.vm.native.bridge` set to `0`.
- **Distribution:**
  - the system image stays separate from the data image;
  - assets are compressed and kept under 2 GiB each;
  - published on GitHub Releases with a signed manifest;
  - the signing key is kept outside the repo, in the user's secret store;
  - every release carries the matching kernel source pointer and source
    archive, plus Magisk's source for the magisk variant.

## Tasks & Steps

1. `tools/image-pipeline/fetch`: download from the private mirror by build
   ID and check the checksum.
2. `tools/image-pipeline/repack`: unpack, apply the variant and profile,
   re-sign, regenerate the textproto, repack, and write the checksums.
3. `tools/image-pipeline/identity`: the Magisk identity module, and the
   provisioning-boot script that Phase 4 runs for full clones.
4. `tools/image-pipeline/package`: compress, sign the manifest, attach the
   GPL source archives, and upload.
5. **Re-pin routine:**
   - at each Android release, pin a new build ID;
   - re-run this phase's verification;
   - keep the previous image installable.

## Verification

- From download to packaged output, the pipeline uses less than 20 GB.
- A repacked image boots with verity on (`androidboot.verifiedbootstate`
  orange, `/system` mounted through dm-verity).
- `ro.serialno` differs per instance. `ro.product.model` follows the instance
  config on rooted instances and the image profile on unrooted ones.
- A fresh clone and a full clone both get Android IDs that differ from their
  source.
- The magisk variant passes `su -c id`. The base variant has no `su`.
- The microg variant passes microG's self-check.
- Repacking the same build ID twice gives the same per-partition content
  hash. Byte-identical images would need fixed timestamps and avbtool salts.
- **Exit gate:** the license inventory covers every artifact this phase
  publishes, and the corresponding source is attached before the first
  public image release.

## Risks

- Google may remove old build IDs. Phase 1 mirrors them on day one.
- A patch can break boot. Keep the base image unpatched, and keep every
  change in a variant or profile that can be turned off.
