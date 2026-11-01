# Review: MTPlayer engine plan (`plans/261101-1256-mtplayer-engine/`)

Date: 2026-11-01 14:08 (Asia/Saigon). Reviewer: kongming (advisory). Scope: plan.md and phase-01..07, the three 2026-11-01 research reports, and the superseded 2026-08-27 plan plus its red-team report (`git show 6bda00d`). Fixed user decisions (AEMU fork, Android 16, x86_64-only v1, repo, no local AOSP build) are respected; where evidence argues against one, it is flagged, not overturned.

## TL;DR

The shape of the plan is right: feasibility spike first, prebuilt CI images plus host repack, Rust daemon owning the engine processes, Apache code separated from the GPLv2 engine by a process boundary. Three things must change before Phase 1 starts. (1) The plan forks the wrong branch: `emu-master-dev` has been frozen since 2025-03-27 (emulator 35.6.3 Canary); the live classic branch is `emu-main-dev` (HEAD 2026-10-27) and the next-generation QEMU 11 engine the research "could not find" is public at `emu-main-next-v11.0` (HEAD 2026-10-29) and lists Windows x64 as a supported host. (2) The multi-instance storage model is internally contradictory: `-read-only` instances discard disk writes on exit, and a shared Quick Boot RAM snapshot restores the golden instance's identity, so neither can carry "persistent per-instance data overlay plus per-instance identity". (3) The unrooted identity mechanism (vendor/odm `build.prop` overlay) cannot change `ro.product.model`, because init resolves `ro.product.*` from the `product` partition first. Everything else is Medium or Low and fixable inside the existing phases.

## What is sound

- Phase 1 as a kill-gate before the build-out, with a day-one image-availability check (`phase-01:44-47`, `73-74`).
- Prebuilt ci.android.com images plus host repack instead of a 250-400 GB AOSP tree (`plan.md:63-66`); the AOSP emulator images are ext4 system/vendor inside a `super` partition, signed with the public AOSP test key (`device/generic/goldfish/board/BoardConfigCommon.mk`: `EMULATOR_RO_PARTITION_FS ?= ext4`, `BOARD_AVB_SYSTEM_KEY_PATH := external/avb/test/data/testkey_rsa2048.pem`), so a host repack is realistic with `otatools`.
- Own ADB device-protocol client, loopback-only ports, guest treated as untrusted (`plan.md:70-74`, `phase-05:19-24`, `37-40`). These answer red-team D1, D6, D20, D21 from the superseded plan.
- `androidboot.serialno` -> `ro.serialno` is correct: init's `ExportKernelBootProps()` maps `ro.boot.serialno` to `ro.serialno` (`system/core/init/property_service.cpp`), and init imports `androidboot.*` from both kernel cmdline and bootconfig.
- Magisk supports Android 16 (v30.3 added the Android 16 QPR2 sepolicy format; v30.7, 2026-02-23, extends Zygisk to 16 QPR2+), so mechanism 2 in `phase-02:38-40` is viable. Magisk's `resetprop` runs before zygote in `post-fs-data`, so `Build.MODEL` picks it up.
- WHPX is Google's recommended Windows path (HAXM removed in 36.2.11, AEHD sunsets 2026-12-31), and Microsoft's 2018 WHPX announcement states Quick Boot works on it.

## Findings, ranked

### Critical

**C1. Wrong fork base; next-gen engine source is public.** `phase-01:36-37` says fork `emu-master-dev`, and `phase-01:22-24`, `59-62` and `plan.md:147-149` say the QEMU 11 engine's source "was not found". Verified on android.googlesource.com: `emu-master-dev` HEAD is "Merge 'Bump Emulator to 35.6.3 Canary'", 2025-03-27, i.e. 19 months stale; `emu-main-dev` HEAD is 2026-10-27 ("Snap ... to stage-aosp-emu-main-release"); `emu-main-next-v11.0` HEAD is 2026-10-29, its README says "modified version of QEMU, tailored for Android emulation", built with Bazel (`bazel build @goldfish//emulator:release` from an AOSP-style `.repo` workspace), Android devices live in `platform/hardware/generic/goldfish` (branch `emu-main-next` exists), and it lists "Linux x64, macOS, Windows x64" under platform-specific modifications. Google's release notes add that the next-gen engine is already the *default* for eligible (API 37+) AVDs in Studio Rabbit 3 Canary 1. A fork of `emu-master-dev` would miss every 36.x/37.x fix (Lavapipe default, new network stack, Intel black-screen fix, GLES host leak fix) and would not reflect the images Google tests against.
Fix: replace every `emu-master-dev` mention with `emu-main-dev` (classic) and make Phase 1 task 4 evaluate `emu-main-next-v11.0` + `hardware/generic/goldfish` first. Record the gate explicitly: "does the next-gen engine boot an API 36 image on WHPX on Windows?" If yes, fork it (future-proof, modular, Bazel). If no (Google documents it for API 37+ only), fork `emu-main-dev` and write the re-base trigger as "the day MTPlayer needs API 37", not "12-18 months".

**C2. `-read-only` + shared Quick Boot snapshot cannot give persistent, individually-identified instances.** `phase-03:34-37` and `phase-04:19-23` want "`-read-only` instances sharing a copy-on-write Quick Boot snapshot", "per-instance qcow2 data overlay", and "shared Quick Boot snapshots per image version", while `plan.md:91`, `95-98` and `phase-04:26-29` want per-instance serial/IMEI/Android ID and persistent clones. Google's 28.0.16 notes: instances started with `-read-only` have "changes to the guest virtual disk discarded on exit" and auto-save disabled. And a Quick Boot snapshot is a RAM image: resuming two instances from one shared snapshot restores the same `ro.serialno`, `ro.product.*`, SSAID seed and guest MAC state regardless of what the engine passes on the new command line. Boot-time identity injection and shared-RAM-snapshot sharing are mutually exclusive.
Fix: choose two explicit modes. (a) *Persistent instance*: its own AVD directory, own `userdata` qcow2 overlay, own Quick Boot snapshot taken after its own first cold boot (the system image is already shared read-only across AVDs via the per-AVD `system.img.qcow2` overlay, so "one read-only system disk per image version" is still true). (b) *Ephemeral clone for CI*: `-read-only` from one golden snapshot, identical identity, data discarded. Rewrite `phase-03:34-37` and `phase-04:19-23` accordingly and drop "shared Quick Boot snapshots per image version" for mode (a).

**C3. Vendor/odm `build.prop` overlay cannot change model/brand/fingerprint.** `phase-02:41-44` (mechanism 3, unrooted instances) patches "a vendor or odm overlay whose `build.prop`". In `system/core/init/property_service.cpp`, `RO_PRODUCT_PROPS_DEFAULT_SOURCE_ORDER = "product,odm,vendor,system_ext,system"` and `property_initialize_ro_product_props()` takes the first partition in that order that defines `ro.product.<partition>.model`. Every AOSP partition's `build.prop` carries the full `ro.product.<partition>.*` set, so `product` wins and a vendor/odm patch is ignored. `ro.build.fingerprint` is set in system `build.prop`, not derived. Also, per-instance partition patches mean a per-instance system/product image, which breaks the "one shared system disk" store.
Fix: for the *base* image, patch `product`, `system` and `vendor` `build.prop` together (or set `ro.product.property_source_order` to put `odm` first and ship a tiny per-instance odm image). For *per-instance* unrooted identity, accept that only boot props (`androidboot.*`) are free and that model/brand/fingerprint spoofing requires either root (`resetprop`) or a per-instance partition. State that trade-off in Phase 2's requirements and acceptance criterion 4 (`plan.md:95-96`).

### High

**H1. ci.android.com target availability is unverified and likely harder than the plan assumes.** `phase-02:23-26` and `plan.md:157-158` assume an Android 16 `sdk_phone64_x86_64-userdebug` `emu_img_zip` exists on ci.android.com. The dashboard docs (source.android.com/docs/setup/contribute/dashboard, updated 2026-07-16) say CI "is no longer generating new builds for `aosp-main`" and only `aosp-android-latest-release` "is updated with new builds at every new Android release" — which has tracked Android 17 since 2026-06. The target list and retention of older build IDs could not be read (the grid is client-rendered). Day-one gate is correct; its outcome is uncertain, and the plan's only fallback is a rented cloud build (`phase-01:56-58`).
Fix: keep the gate, add two cheaper fallbacks before "cloud build": (1) an Android 16 QPR build ID on `aosp-android-latest-release` from before 2026-06, if retained; (2) a CI build on `aosp-main` from before it stopped (frozen but downloadable). Record the exact target name (likely `sdk_phone64_x86_64-trunk_staging-userdebug` or `-userdebug`) and mirror the artifact into a private store the same day, since retention is undocumented.

**H2. "AVB turned off" is the wrong mechanism; the emulator reads dm-verity params from `VerifiedBootParams.textproto`.** `phase-01:55` and `phase-02:43-44`. The emulator has no bootloader; the kernel cmdline dm-verity table comes from `VerifiedBootParams.textproto`, generated by `device/generic/goldfish/tools/mk_verified_boot_params.sh` from `vbmeta.img` and `system-qemu.img` (it writes `dm_param` entries plus `androidboot.verifiedbootstate=orange`, `root=/dev/dm-0`). A patched `system` whose hash tree no longer matches will fail to mount unless the textproto is regenerated or verity is removed from it.
Fix: because the image is signed with the public AOSP test key (`testkey_rsa2048.pem`), the repack should re-run `avbtool add_hashtree_footer` and `make_vbmeta_image` with that key and regenerate the textproto — keeping verity on, no "AVB off" decision to record. Add `mk_verified_boot_params.sh` (needs `sgdisk`, `avbtool`) to the Phase 2 tool list and Phase 1 proof list.

**H3. Android ID reset "over root adb before the first boot" is impossible as written.** `phase-02:45-47`, `phase-04:28-29`. adb needs a booted guest, and `userdata` is file-based-encrypted (the product copies `encryptionkey.img`; `/data/system/users/0/settings_ssaid.xml` is inside the encrypted tree), so host-side editing of the qcow2 overlay is impractical.
Fix: clone = copy overlay, boot once headless in a quarantined "provisioning" step (no network), `rm /data/system/users/0/settings_ssaid.xml`, reboot, then take the instance's own Quick Boot snapshot. Or offer "fresh clone" (new overlay from the base `userdata`, no app data copied) as the cheap default.

**H4. Public GitHub Releases is distribution; the plan treats the project as internal-only.** `phase-02:55-58`, `phase-03:45-46`, `63-66`, `phase-07:31-32`, `plan.md:109-110`. Publishing engine zips (GPLv2) and images (GPLv2 kernel, GPL-3 Magisk in the magisk variant, Apache AOSP) on a public release page triggers GPLv2 §3 and GPL-3 §6 for everyone who downloads, and the superseded red-team's F11 still applies: a URL is not a source offer. The kernel is a prebuilt GKI (`kernel-ranchu`) whose exact source commit must be recorded for a valid corresponding-source offer.
Fix: either (a) host releases privately (private repo releases, or an internal share) until a legal pass, or (b) move the license inventory, the engine source tag and the kernel/Magisk source pointers from Phase 7 into the exit criteria of Phases 2 and 3, before the first public release. Also keep the GPLv2 fork in its own repo (`mtplayer-engine`) rather than `engine/` inside the Apache repo (`plan.md:50-52`, `phase-01:81-82`): a QEMU fork is tens of thousands of files, and a separate repo makes the license boundary and the upstream remote trivial.

**H5. Phases 4-6 are serialized behind the identity patches without need.** `plan.md:116-122` makes Phase 4 depend on Phase 3 (6-10 weeks). The daemon, CLI, store, `mtp-adb` and GUI only consume the engine's command line, console, gRPC and adb, which the fork keeps unchanged (`phase-03:42-43`). A single part-time developer also cannot run Phases 2 and 3 "in parallel" (`plan.md:124-125`).
Fix: split Phase 3 into 3a "unmodified build of the chosen branch + versioned zip + CI" (gate for Phase 4, ~2 weeks after Phase 1 proves the build) and 3b "branding and identity patch series" (runs alongside Phases 4-5). Phase 4 then starts against a legally clean, unmodified self-built engine.

### Medium

**M1. Per-instance identity requires generating an AVD directory, not just flags.** `phase-04:45-46`. AEMU resolves images through `config.ini`/`hardware-qemu.ini` in an AVD dir and writes state under `%USERPROFILE%\.android` unless `ANDROID_AVD_HOME`/`ANDROID_EMULATOR_HOME` are set. Add "generate an AVD dir per instance under `%LOCALAPPDATA%\MTPlayer` and set those env vars" to Phase 4; it also makes acceptance criterion 8 ("uninstall leaves nothing behind", `plan.md:107-108`) achievable.

**M2. Job Object RAM cap will kill the engine.** `phase-04:30-31`. Guest RAM is committed inside the engine process; a job memory limit below `-memory` terminates it. Use `-memory`/`-cores` flags for RAM and CPU count, Job Object only for CPU rate and for orphan control. Decide explicitly whether `KILL_ON_JOB_CLOSE` is set: it conflicts with "a crashed daemon reattaches" (`phase-04:34-36`).

**M3. Benchmark set may not run in MTPlayer at all.** `phase-01:31-33` names 3DMark Wild Life; a user report says 3DMark's compatibility check rejects x86_64 emulators, and x86_64-only guests can only run APKs with x86_64 libs. Verify each benchmark APK's `lib/` ABIs before Phase 1 and prefer GFXBench or a self-built GLES/Vulkan test app. Also note MuMu 12 needs Hyper-V off, so the MuMu baseline requires a reboot cycle; record host state per run.

**M4. "No Android SDK or adb.exe" conflicts with bundletool.** `phase-05:27-29`, `69-70`, `phase-07:19-21`. bundletool needs a JRE; nothing bundles one. Either bundle a JRE (OpenJDK, GPLv2+CPE, add to inventory) or implement AAB-to-split extraction in Rust (bundletool's `build-apks` is zip plus protobuf manipulation; `get-device-spec` can be replaced by `getprop` over `mtp-adb`).

**M5. GitHub Actions free runners cannot build the engine.** `phase-03:45-46`. Hosted Windows/Linux runners have 14 GB SSD and a 6 h job limit; the emulator workspace (prebuilt toolchains, Qt, ANGLE, SwiftShader) alone will exceed that. Plan on the local build host or a self-hosted runner, and record the measured checkout/build size in Phase 1 as the plan already asks (`phase-01:40-43`).

**M6. otatools are Linux ELF.** `phase-02:30-32` says "WSL2 or Git Bash". `lpunpack`, `lpmake`, `simg2img`, `mkbootfs`, `e2fsdroid`, `sgdisk` from `otatools.zip` are Linux x86_64 binaries; only `avbtool` (Python) runs under Git Bash. Say "WSL2 only".

**M7. Guest is a root shell for every local process; gRPC and console auth not mentioned.** `plan.md:70-74`. Loopback binding blocks the LAN but not other users or malware on the host. AEMU supports a console auth token (`~/.emulator_console_auth_token`) and `-grpc-use-token`/TLS on the gRPC endpoint; enable both from the daemon, and write the residual threat ("any local process has root on all instances") into the README. Also name the manifest signing scheme and where the private key lives (not in the public repo).

**M8. 16 GB host criterion needs numbers.** `plan.md:91`, `plan.md:62`. Two Android 16 instances at 2 GB guest RAM plus about 1 GB host-side gfxstream each, Docker Desktop's WSL2 VM (defaults to up to 50% of RAM) and the GUI will not fit in 16 GB without a `.wslconfig` `memory=` cap. Make the criterion "two instances at 2 GB each with WSL2 capped at 4 GB" and measure commit charge, not "runs".

**M9. Effort is full-time-equivalent but labelled part-time; zero slack.** `plan.md:6`, phase frontmatter. The phases sum to 26-39 weeks; the plan says "6-9 months (one developer, part-time)". If part-time means half time, the calendar is 12-18 months before slack. State the assumption and add the 25% integration slack the superseded plan already learned to add (`6bda00d` plan.md, "Effort — derived, not inherited").

### Low

**L1.** `phase-01:28-29` labels running the stock AVD "personal evaluation only". The Android SDK License §3.1 permits commercial app development; the only open question is §3.2 ("develop another SDK") if the stock binary were part of the product. Using it as a benchmark baseline is fine; say so.
**L2.** `phase-02:81` "repacking twice gives the same checksums" requires fixed timestamps in `build_image.py`/`mke2fs` (`-T`) and deterministic `avbtool` salts; note it or weaken to "same content hash per partition".
**L3.** `phase-03:52-53` verifies IMEI/IMSI via `service call iphonesubinfo`; first confirm where the chosen engine serves IMEI (classic goldfish modem returns a fixed `+CGSN`; the newer modem simulator reads per-instance NVRAM). The fix location differs by base.
**L4.** `plan.md:45-48` describes the later ARM plan as "an ARM64 front end for Berberis". Berberis today is riscv64-only; keep that plan's feasibility unverified and out of v1's critical path.
**L5.** Phase 6's sync input with "normalised coordinates" (`phase-06:26-28`) only works while follower UIs match; the feature-layer research already says "best-effort". Write that into acceptance so it is not a Phase 7 failure.

## Missing phases and ordering

- No legal checkpoint before the first public artifact (see H4). Add it as an exit criterion of Phases 2 and 3, not a Phase 7 task.
- No "image re-pin" cadence: Google publishes AOSP source twice a year and CI only on new releases (H1). Add an operational note: re-pin at each Android release, re-run Phase 2 verification, keep the previous image installable.
- Phase 1 should include a 1-day check that `emu-main-next-v11.0` boots the chosen API 36 image (C1); it is the single input that decides the fork base and the re-base cost.
- Phase 3a/3b split (H5) and the storage model rewrite (C2) change Phase 4's requirements; update them together.

## Acceptance criteria

- Criterion 3 "within 80% of MuMu" (`plan.md:93-94`) is measurable only if the same APKs run on both (M3) and the MuMu run is documented with Hyper-V off. Define the metric as the geometric mean of three ratios and set the threshold per metric, because WHPX will lose on `fio` 4K and win or tie on GPU.
- Criterion 2 "four instances" is fine on 32 GB; "two on 16 GB" needs M8's constraints.
- Criterion 4 (new model/brand/fingerprint per clone) must say "rooted or per-instance partition" (C3).
- Criterion 9 should move earlier (H4).

## Simpler alternatives that keep the fixed decisions

1. **Fork the next-gen engine if it boots API 36** (C1). It is upstream QEMU with Android as runtime modules, Bazel-built, Windows-listed. Patches become modules in `hardware/generic/goldfish`, not a diff against a 2.12-era tree. Cost: Bazel plus a repo workspace; unverified disk size; Google documents it for API 37+ only. Evidence that flips the recommendation: it fails to boot API 36 on WHPX in Phase 1.
2. **Unmodified self-built engine first** (H5). Zero engine patches for v0; identity via `androidboot.*` and Magisk only. The fork earns its keep only when a feature needs engine code (modem identity, MAC, branding).
3. **Root-only identity spoofing in v1** (C3). Drop the unrooted per-instance model/brand mechanism; `userdebug` plus Magisk covers the testing use case, and it removes the per-instance partition problem entirely.
4. **Noted disagreement, not a blocker:** Android 17 instead of 16 would align v1 with the engine Google is now investing in and remove the re-port risk, at the cost of a 4 GB guest-RAM floor (two instances on 16 GB become hard). The user fixed Android 16; keep it, but write the Android 17 re-base as the first post-v1 item.

## Assumptions

- `emu-main-dev` is the public mirror of the classic engine Google ships as 36.x/37.x (high: branch name, "stage-aosp-emu-main-release" snap subjects, 2026-10-27 date).
- `emu-main-next-v11.0` is the "next-generation engine" from the release notes (high: QEMU 11 branch name, modular README, 2026-10-29 date, `hardware/generic/goldfish` companion repo).
- Android 16 `sdk_phone64_x86_64` CI artifacts exist somewhere downloadable (medium-low: dashboard only lists `aosp-android-latest-release` as active; target list not readable without a browser).
- ci.android.com imposes no terms beyond AOSP component licenses (medium: none found; the dashboard page carries only the site content license).
- Emulator `userdata` is FBE-encrypted (high: `encryptionkey.img` is in the product's copy files).
- "Part-time" means roughly half time (low; stated only to show the calendar effect).

Status: DONE_WITH_CONCERNS
Summary: The plan's structure is sound, but it forks a branch frozen since 2025-03 while the live classic branch and the public next-gen QEMU 11 engine are ignored, its multi-instance storage model (`-read-only` plus shared RAM snapshot) contradicts per-instance identity and persistence, and the unrooted vendor/odm `build.prop` spoofing cannot work given init's product-first property order.
Concerns/Blockers: ci.android.com's current target list and retention of Android 16 build IDs could not be read (client-rendered grid; shell probe blocked by a hook); Phase 1's day-one gate must settle it. The AEMU workspace size and the next-gen engine's API 36 compatibility are unverified and decide the fork base.
