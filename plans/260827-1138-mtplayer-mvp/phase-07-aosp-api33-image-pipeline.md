---
title: "Phase 7: AOSP API 33 Image Pipeline"
status: todo
phase: 7
priority: P1
effort: "1.5-3 wk (highest variance in v1; inherits phase 01's measured cost)"
dependencies: [1, 6]
---

# Phase 7: AOSP API 33 Image Pipeline

## Overview

Turn phase 01's manual result into **the one image v1 ships**: AOSP x86_64, Android 13 / API 33, bootable under the phase-01 VMM on Windows, with `adbd` reachable and — if achievable — authenticated (S3).

**Phase 01 already answered feasibility.** This phase's job is *repeatability*: a scripted, reproducible pipeline producing a versioned artifact. Its effort estimate is therefore not independent — it inherits the hours phase 01 recorded for disk assembly.

Whether the artifact is Cuttlefish or GSI is **decided by phase 01, not here.** The steps below cover both; delete the branch that does not apply.

## Requirements

- [ ] Functional: a scripted pipeline from pinned AOSP artifacts to a bootable disk image
- [ ] Functional: the produced image boots to `sys.boot_completed=1` under the chosen VMM on Windows
- [ ] Functional: `adbd` reachable on TCP, **loopback only (S2)**
- [ ] Functional: `ro.build.version.sdk` == `33`, read from the running guest
- [ ] Functional: **a per-instance unique identifier** injected at start — not just the shared image prop (D19)
- [ ] Functional: S3 attempted — `ro.adb.secure=1` + pre-installed MTPlayer public key; outcome recorded
- [ ] Functional: system image and data overlay separate; only the system image is distributed
- [ ] Non-functional: pipeline reproducible; **pinned build ID**, never a floating branch
- [ ] Non-functional: compressed system image under 2 GiB per GitHub Release asset

## Architecture

```
ci.android.com — PINNED build id from an android-13.0.0_rNN branch
  (NEVER `aosp-android-latest-release` — that branch tracks the newest
   AOSP release, API 37 since 2026-06-16, and would fail the API check)
                    │
          ┌─────────┴──────────┐
          │  Cuttlefish path   │  or  │  GSI path (phase-01 fallback)  │
          │  reduced device set│      │  plain system image + kernel   │
          │  no vsock/ramoops  │      │  no host tooling at all        │
          └─────────┬──────────┘      └────────────┬──────────────────┘
                    └──────────┬───────────────────┘
                               │
              inject: adbd TCP · ro.adb.secure (S3) · per-instance id hook
                               │
                    convert -> qcow2 (or raw, per VMM)
                               │
      ┌────────────────────────┴────────────────────────┐
      │ system.qcow2   distributed, immutable, signed   │
      │ data overlay   created client-side via DiskOps  │
      └─────────────────────────────────────────────────┘
```

**`DiskOps`, not `qemu-img`.** The original plan hard-coded `qemu-img create` here and in phases 11 and 14 while the VMM was officially undecided. If crosvm won, that meant either shipping GPLv2 `qemu-img.exe` — reinstating the exact obligation crosvm was chosen to avoid — or rewriting the disk model mid-P2. All overlay creation goes through `mtp-core`'s `DiskOps` abstraction (phase 06), which has a `qemu-img` implementation, a `crosvm create_qcow2 --backing-file` implementation, and ideally an in-process qcow2 writer that needs no external binary.

**Why split system and data:** keeps each distributed asset under 2 GiB, makes factory reset instant (phase 11 deletes the overlay), and lets one base back many instances (phase 12).

**Prop injection under AVB.** `service.adb.tcp.port=5555` must survive a factory reset, so it belongs in the system partition — but modifying an AVB-signed image invalidates verification. Three-option ladder, in order: (a) vendor prop overlay, (b) kernel cmdline `androidboot.` prop, (c) disable AVB and edit the prop file. **Record which was used** — S3 and any future API 37/34 image inherit it.

**Per-instance identity (D19).** The red team found that phase 12's "confirm the device's identity rather than trusting the port" check was unimplementable: every instance boots the same immutable image, phase 07 injected exactly one prop, and MTPlayer bypasses `launch_cvd`, which normally assigns Cuttlefish instance identity. So `ro.serialno` is *identical* across instances — the check could distinguish "an MTPlayer guest" from "an unrelated service" but not instance A from B, which is the case that loses data. This phase must provide a **boot-time** identity hook: a kernel-cmdline `androidboot.serialno=<uuid>` per instance, or a marker file written into the overlay before first boot. Phase 12 compares *that*.

## Related Code Files

- Create: `tools/image-pipeline/00-verify-availability.ps1` — confirm the pinned artifacts are still published (gate)
- Create: `tools/image-pipeline/fetch.ps1` — download by pinned build ID, verify published checksums
- Create: `tools/image-pipeline/assemble.ps1` — compose the bootable disk (Cuttlefish or GSI branch)
- Create: `tools/image-pipeline/inject-props.ps1` — adbd TCP, `ro.adb.secure`, identity hook; record the mechanism
- Create: `tools/image-pipeline/package.ps1` — zstd, SHA-256, **sign the manifest (S4)**, emit manifest
- Create: `tools/image-pipeline/README.md` — full procedure, reproducible by a second person
- Create: `images/manifest.json` — **schema owned by phase 14**; this phase produces a conforming file
- Modify: `crates/mtp-provider-vm/src/lib.rs` — resolve an image via the local manifest; create the overlay via `DiskOps`
- Delete: `spikes/image-feasibility/` — phase 01's throwaway harness, once the pipeline reproduces it

## Implementation Steps

1. **Availability gate.** Re-confirm phase 01's pinned build ID is still downloadable. CI retention on older branches is a live risk; if the artifact has aged out since phase 01, escalate — do not silently repin to a different API level.
2. **Lift phase 01's manual procedure into scripts, one step at a time.** Do not redesign. The value here is repeatability.
3. **`fetch.ps1`** with the exact pinned build ID recorded in a constant, plus published-checksum verification.
4. **`assemble.ps1`** for whichever path phase 01 chose. If GSI, this is substantially simpler — a system image plus a generic kernel, no composite-disk work. If Cuttlefish, replicate the reduced device set phase 01 proved.
5. **Resolve the AVB/prop question** via the (a)/(b)/(c) ladder. Record the choice and why.
6. **Attempt S3.** Set `ro.adb.secure=1` and pre-install MTPlayer's public key at `/adb_keys` or `/data/misc/adb/adb_keys`. If the image cannot be made to demand authentication without a custom AOSP build, record it: **Known Limitation 5 ships, and S2 loopback-only becomes the sole protection.**
7. **Wire the per-instance identity hook** (D19) and confirm two concurrently running instances report *different* identifiers.
8. **Verify `ro.build.version.sdk` == 33** from the running guest. Never trust the filename.
9. **Split system and data via `DiskOps`.** Immutable `system.qcow2`; overlay created client-side at first start. This is the model phase 11 builds snapshots on.
10. **Compress, hash, sign.** zstd; confirm under 2 GiB; SHA-256; **sign the manifest per S4**, since phase 14 verifies the signature before parsing. If an asset exceeds 2 GiB, implement `.partNN` split now.
11. **Emit a manifest conforming to phase 14's schema.** The original plan ordered the schema "defined here" but only specified it in phase 14 — with divergent fields and three candidate file locations. Phase 14 owns the schema; this phase produces a valid instance of it.
12. **Boot the packaged artifact on a machine that never had the spike image**, to prove the pipeline's output rather than the developer's working directory.

## Success Criteria

- [ ] Availability gate passed; pinned build ID recorded in the pipeline and the manifest
- [ ] `00-verify` + `fetch` + `assemble` + `inject-props` + `package` run end to end unattended
- [ ] Packaged image boots to `sys.boot_completed=1` under the chosen VMM on Windows
- [ ] `adb shell getprop ro.build.version.sdk` returns `33`
- [ ] `adbd` reachable on **`127.0.0.1` only**; verified *not* reachable from a second machine (S2)
- [ ] adbd reachability **survives a factory reset** (delete + recreate the overlay)
- [ ] S3 outcome recorded — authenticated ADB working, or Known Limitation 5 confirmed with the reason
- [ ] Two concurrent instances report **different** per-instance identifiers (D19)
- [ ] Compressed system image < 2 GiB; SHA-256 recorded; **manifest signed and signature verifiable**
- [ ] Overlay created via `DiskOps`, not a direct `qemu-img` call; base SHA unchanged after a session
- [ ] Manifest validates against phase 14's schema
- [ ] Full phase-05 pipeline works against this image: video renders, input works
- [ ] `tools/image-pipeline/README.md` lets a second person reproduce the artifact
- [ ] Phase 01's spike harness removed

## Risk Assessment

**Risk: assembly is impractical to script on Windows** (needs `simg2img`, loop mounts, sparse handling).
Signal: the procedure needs Linux-only tools with no Windows equivalent.
Response: run the pipeline **in WSL or a Linux container as a maintainer-side build tool.** Image production is a maintainer task; only the *output* must be Windows-consumable. Do not force a native Windows pipeline for its own sake.

**Risk: AVB blocks prop injection and disabling it breaks boot.**
Signal: verification failure, or adbd still not on TCP.
Response: the three-option ladder. If all fail, fall back to the phase-01 ladder (GSI, then own build). **Decide within this phase** — do not let it drift into 14 or 15.

**Risk: the pinned artifact ages out of CI between phase 01 and phase 07.**
Signal: step-1 gate fails.
Response: this is why the gate is step 1 and not an assumption. Escalate for an API-level or artifact re-decision; mirroring the artifact locally after phase 01 is cheap insurance and should be done at phase 01's close.

**Risk: S3 turns out to be infeasible** without a custom AOSP build.
Signal: `ro.adb.secure=1` either does not stick or the guest ignores it on a `userdebug` build.
Response: pre-decided — ship Known Limitation 5, and treat S2 (loopback-only, verified negatively) as load-bearing rather than defence-in-depth. Do **not** quietly drop the limitation from the docs; it is the difference between a documented trade-off and a surprise.

**Risk: per-instance identity injection conflicts with the immutable-base model.**
Signal: injecting a serial requires writing to the system partition per instance.
Response: prefer the kernel-cmdline route (per-boot, no image mutation) over anything that touches `system.qcow2`. If only an overlay marker file works, write it before first boot and have phase 12 read it over ADB after connect.

**Risk: pinning a build means shipping a stale image.**
Signal: the pinned Android 13 build has a known defect.
Response: accepted — maturity is why API 33 was chosen. The manifest records the build ID so refreshing it is a scoped, deliberate task. Note that phase 14 forbids overwriting a base in place, so a refresh installs alongside.
