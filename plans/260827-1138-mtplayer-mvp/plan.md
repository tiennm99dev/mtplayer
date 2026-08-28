---
title: "MTPlayer — Android Dev Tool for Windows"
description: "AOSP x86_64 Android in a VM on Windows, displayed via the scrcpy protocol over a native ADB device-protocol client. APK-only, no Google services, no kernel driver. P0-P5, v1 = P0-P3."
status: pending
priority: P1
effort: "v1 (P0-P3): 5-8 months · full P0-P5: 7-12 months (derived from phase estimates + slack)"
tags: [rust, android, virtualization, whpx, scrcpy, adb, egui, dev-tool]
created: 2026-08-27
---

# MTPlayer — Android Dev Tool for Windows

**Inputs:** [`research-260827-1058-android-emulator-windows.md`](../reports/research-260827-1058-android-emulator-windows.md) · [`architecture-260827-1130-dev-tool-dual-backend.md`](../reports/architecture-260827-1130-dev-tool-dual-backend.md) · [`red-team-260827-1530-mtplayer-plan.md`](../reports/red-team-260827-1530-mtplayer-plan.md)

## Overview

An Android **dev tool** for Windows. Boots AOSP-based x86_64 Android in a VM, shows it in a window, installs APKs, exposes ADB. No Google services, no ads, no telemetry, no kernel driver.

**Not** a gaming emulator. No key mapping, no anti-cheat evasion, no FPS competition with BlueStacks/LDPlayer.

## Goals

| # | Goal | Priority |
|---|------|----------|
| R1 | Dev tool, not gaming — anti-cheat, key mapping, FPS competition all out of scope | P1 |
| R2 | APK-only install, zero Google services — eliminates GMS/Widevine licensing exposure | P1 |
| R3 | Runs on Windows with Hyper-V **or** WSL installed — dual backend behind one shell | P1 |
| G4 | v1 ships one image: **Android 13 / API 33** | P1 |
| G5 | ARM-only APKs fail loudly at install time, never as a mystery crash | P1 |
| G6 | Single installer; **no runtime dependency on the Android SDK or `adb.exe`** | P1 |
| G7 | Headless CLI so CI can drive instances without the UI | P2 |
| G8 | Provider B (WSL2 + redroid) opt-in, same `DeviceProvider` trait, zero shell changes | P3 |

## Architecture

```
                MTPlayer Shell  (Rust + egui/eframe + wgpu)
       instance list · logcat · APK drop · profile forms · video pane
                              |
        mtp-adb: native ADB DEVICE protocol (A_CNXN/A_OPEN/A_WRTE)
        direct TCP to the guest's adbd — no adb.exe, no :5037 server
                              |
   display: scrcpy H.264 (forward tunnel)   control: install/input/push/pull/logcat
                              |
                    +---------v----------+
                    |   DeviceProvider   |  start stop snapshot restore adb_endpoint status
                    +----+----------+----+
                         |          |
        Provider A: VM + WHPX      Provider B: WSL2 + redroid
        (phases 01-16, ships v1)   (phases 17-19, opt-in)
```

**Load-bearing decision:** the shell talks only to an **ADB endpoint**. Display and input ride the scrcpy protocol, which works on any Android >= 5 exposing ADB. Backends are therefore interchangeable, and the host-side GPU problem (gfxstream/virgl -> ANGLE/D3D12) stays out of v1 entirely.

**Second load-bearing decision (revised after red team):** MTPlayer speaks the ADB **device** protocol directly to the guest's `adbd` over TCP. It does **not** drive the ADB *server* on `127.0.0.1:5037`, because that server is `adb.exe` — an unbundled Android SDK artifact whose redistribution terms this project already rejects. Consequences:

- No runtime dependency on the Android SDK (G6). A clean Windows host works with nothing installed.
- MTPlayer never disturbs a developer's existing ADB server or their physical-device connections.
- **Security:** scrcpy uses a *forward* tunnel — MTPlayer connects **out** to the guest's abstract socket. There is no host-side listener, so no local process or guest app can race to accept MTPlayer's control socket.
- **Cost:** `mtp-adb` implements framing, `A_AUTH`, and the sync subprotocol itself (phase 03 grows accordingly).
- **Trade-off:** phase-12 auto-registration into `adb devices` requires the user's own `adb.exe` and is therefore a **conditional** feature — present when an ADB server is detected, absent otherwise.

## Security Model (invariants — violating any of these is a defect)

The red team found no threat model in the original plan. These are now stated once and enforced per phase.

| # | Invariant | Enforced in |
|---|---|---|
| S1 | **The guest is untrusted.** It runs arbitrary third-party APKs by design. Everything crossing guest -> host is hostile input. | 08, 09, 10, 16 |
| S2 | **Every ADB endpoint binds `127.0.0.1` only** — Provider A hostfwd and Provider B container publish alike. Verified by a negative test from a second machine, not by reasoning. | 02, 12, 18 |
| S3 | **The guest accepts authenticated ADB only.** Ship a per-install MTPlayer keypair and implement `A_AUTH`; do not rely on `ro.adb.secure=0`. See Known Limitations if this proves impossible. | 03, 07 |
| S4 | **The image manifest is signed** and its signature verified before parsing. `id` is validated against `^[a-z0-9_.-]+$`; asset URLs against an allowlisted scheme+host. | 14 |
| S5 | **Guest-supplied paths are canonicalized and validated host-side** before any filesystem use — reject `..`, separators, `:`, Windows reserved names, over-long paths. Never preserve guest file modes on the host. | 10 |
| S6 | **Process termination is handle-scoped**, never kill-by-pid. Open the handle, verify image path + creation time through it, terminate through it. | 06 |
| S7 | **Guest-controlled text is escaped** before reaching a terminal, the clipboard, or JSON — strip C0/C1 and OSC sequences (OSC 52 is a clipboard write). | 09, 13 |
| S8 | **Diagnostics redact at the value level**, not the field level. Every string is scrubbed for user paths and identifiers before entering a report. | 16 |
| S9 | **The H.264 decoder is the only C in the trust path.** It consumes attacker-influenced bytes and must be fuzzed. | 04, 05 |

## Stack

| Component | Choice | Rationale |
|---|---|---|
| Language | Rust (2021, stable) | Single static binary, no runtime deps |
| UI | `eframe`/`egui` + wgpu | Video frame as texture **and** tables/forms in one frame. Tauri's WebView is the wrong tool for a video surface |
| Video decode | ffmpeg via `ffmpeg-next` (`openh264` fallback) | Same path scrcpy uses; proven. **Both carry AVC patent exposure — see Licensing** |
| Guest display/input | `scrcpy-server` (Apache-2.0), version-pinned, bundled, **forward tunnel** | Removes host GPU work from v1; forward tunnel removes the host listener |
| ADB | **Own device-protocol client** (`A_CNXN`/`A_OPEN`/`A_WRTE`) direct to guest adbd | No `adb.exe`, no SDK dependency, no :5037 server. See Architecture |
| VMM | **OPEN — QEMU vs crosvm. Sub-decision of phase 01** | Both claim WHPX. See Open Questions |
| Guest image | AOSP **API 33 (Android 13)** x86_64, from a **pinned build ID** | Mature, past bug shakeout, widest cumulative reach. See Image Strategy |
| Disk overlays | **`DiskOps` abstraction** — never a bare `qemu-img` call | The VMM is undecided; see Risks |
| Provider B | redroid + WSL2 (kernel rebuild **may be unnecessary** — see phase 17) | Opt-in only |

### Licensing boundary

MTPlayer is **Apache-2.0**.

- **If QEMU:** GPLv2. Apache-2.0 cannot link GPLv2, so `qemu-system-x86_64.exe` ships as a **standalone child process**. **GPLv2 §3 offers exactly three compliance routes** — ship complete corresponding source, provide *your own* >=3-year written offer, or pass along an upstream offer (noncommercial only). **A URL to qemu.org is none of them.** Therefore: build QEMU from a pinned source tarball that MTPlayer also publishes at the same release, and ship that tarball. A third-party prebuilt binary cannot be made compliant, because its corresponding source is not in our possession.
- **If crosvm:** **BSD-3-Clause** (not Apache-2.0 — the original plan had this wrong in five places). Permissive and linkable, so no process boundary is forced; BSD-3's notice-reproduction and no-endorsement clauses still apply.
- **`qemu-img` is not free of this.** If crosvm wins, shipping GPLv2 `qemu-img.exe` merely to create an overlay reinstates the whole obligation. Hence the `DiskOps` abstraction, and a preference for creating qcow2 overlays in-process.
- **AVC/H.264 patents are a separate axis from copyright.** Cisco's openh264 grant covers **only Cisco's own binary, and only when separately downloaded and not combined with third-party software beforehand** — bundling it in an installer forfeits it, and self-built openh264 was never covered. An LGPL ffmpeg build has the same exposure. Phase 15 must record a deliberate position, not assume one.
- `scrcpy-server` is Apache-2.0 — bundling the jar is clean. Keep its `NOTICE`, pin the version.

**Rejected: forking Google's Android Emulator** (`platform/external/qemu`, `emu-master-dev`). GPLv2, forked from QEMU **2.7.0** (2016), a decade diverged, idiosyncratic bundled toolchain. Costs a relicense of MTPlayer and buys nothing over a stock VMM.

## Repo Layout

```
MTPlayer/
├── Cargo.toml                  # workspace — 7 members at phase 02
├── crates/
│   ├── mtp-core/               # DeviceProvider, registry, config, profiles, DiskOps
│   ├── mtp-adb/                # ADB device protocol: A_CNXN/A_AUTH/A_OPEN, sync, logcat
│   ├── mtp-scrcpy/             # server push/launch, control protocol, H.264 demux
│   ├── mtp-video/              # decode -> wgpu texture
│   ├── mtp-provider-vm/        # Provider A: QEMU-or-crosvm on WHPX, overlays, snapshots
│   ├── mtp-cli/                # headless binary for CI
│   └── mtp-ui/                 # egui app -> mtplayer.exe
│   # added later, NOT at phase 02:
│   ├── mtp-provider-wsl/       # phase 17 — Provider B
│   └── mtp-render/             # phase 21 — ONLY if phase 20 says proceed
├── assets/                     # scrcpy-server.jar, default profiles, NOTICE files
├── tools/                      # image pipeline, WSL kernel (maintainer-side)
└── images/                     # downloaded at runtime, NOT vendored
```

Crate names kebab-case; Rust source files snake_case per language convention. **Phase 02 creates exactly the seven crates it populates** — the original plan asserted eight and listed seven, which cannot `cargo check`.

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | [Phase 1: Cuttlefish-on-Windows Feasibility Spike (GATED)](./phase-01-image-feasibility-spike.md) | Pending |
| 2 | [Phase 2: Workspace, Host Preflight, VM Launch Harness](./phase-02-workspace-and-host-preflight.md) | Pending |
| 3 | [Phase 3: mtp-adb — Native ADB Device Protocol](./phase-03-mtp-adb-client.md) | Pending |
| 4 | [Phase 4: mtp-scrcpy — Transport and Control](./phase-04-mtp-scrcpy-transport.md) | Pending |
| 5 | [Phase 5: Video Pipeline and Minimal Shell (P0 EXIT)](./phase-05-video-pipeline-and-shell.md) | Pending |
| 6 | [Phase 6: DeviceProvider Trait and VM Lifecycle](./phase-06-device-provider-and-vm-lifecycle.md) | Pending |
| 7 | [Phase 7: AOSP API 33 Image Pipeline](./phase-07-aosp-api33-image-pipeline.md) | Pending |
| 8 | [Phase 8: APK Install and ABI Gate](./phase-08-apk-install-and-abi-gate.md) | Pending |
| 9 | [Phase 9: Logcat Pane](./phase-09-logcat-pane.md) | Pending |
| 10 | [Phase 10: File Transfer and Device Profiles (P1 EXIT)](./phase-10-file-transfer-and-profiles.md) | Pending |
| 11 | [Phase 11: Snapshot, Restore, Factory Reset](./phase-11-snapshot-restore-factory-reset.md) | Pending |
| 12 | [Phase 12: Multi-Instance Manager and ADB Endpoint](./phase-12-multi-instance-and-adb-endpoint.md) | Pending |
| 13 | [Phase 13: mtp-cli — Headless and CI (P2 EXIT)](./phase-13-mtp-cli-headless.md) | Pending |
| 14 | [Phase 14: Image Manager and Distribution](./phase-14-image-manager-and-distribution.md) | Pending |
| 15 | [Phase 15: Installer and License Bundling](./phase-15-installer-and-license-bundling.md) | Pending |
| 16 | [Phase 16: First-Run Diagnostics and Docs (v1 EXIT)](./phase-16-first-run-diagnostics-and-docs.md) | Pending |
| 17 | [Phase 17: WSL2 Kernel Assessment and Consent Flow](./phase-17-wsl2-kernel-and-consent.md) | Pending |
| 18 | [Phase 18: mtp-provider-wsl — redroid Containers](./phase-18-mtp-provider-wsl-redroid.md) | Pending |
| 19 | [Phase 19: WSL Guest GPU via /dev/dxg](./phase-19-wsl-guest-gpu-dxg.md) | Pending |
| 20 | [Phase 20: GPU Profiling Gate (kill/proceed)](./phase-20-gpu-profiling-gate.md) | Pending |
| 21 | [Phase 21: gfxstream Host Renderer on ANGLE/D3D12](./phase-21-gfxstream-host-renderer.md) | Pending |

### Release boundaries

- **P0 = phases 1-5** — **phase 1 is a hard feasibility gate.** If Android will not boot on Windows/WHPX, nothing downstream matters.
- **P1 = phases 6-10** — usable device.
- **P2 = phases 11-13** — dev-tool value. **Phase 13 is split**: 13a is not cuttable (phases 14/15/16 depend on its artifacts), 13b is.
- **P3 = phases 14-16** — **v1 ships here.**
- **P4 = phases 17-19** — Provider B, post-v1, opt-in.
- **P5 = phases 20-21** — guest GPU. **Phase 20 is a gate that may kill phase 21.**

### Effort — derived, not inherited

The original "2-4 months to v1" was carried over from a six-phase roadmap that predated the AOSP image pipeline entering v1 scope. Summing this plan's own per-phase estimates:

| Group | Working days | Calendar (1 engineer) |
|---|---|---|
| P0-P3 (v1), phase execution only | 85-132 | 4.0-6.2 months |
| P0-P3 **with 25% slack** for integration and rework | ~106-165 | **5-8 months** |
| Full P0-P5 with slack | ~151-256 | **7-12 months** |

**Phase 01's box and phase 21's spike are serial prefixes**, not overlappable. Treat the v1 figure as provisional until phase 01 closes — it is the phase with unbounded downside.

## Image Strategy

### Which API level

**v1 ships exactly one image: Android 13 / API 33.**

Chosen for maturity over recency. Released 2022, ~3.5 years of bug shakeout by v1, the minSdk floor most apps still support, and the widest cumulative device reach. For a dev tool the guest is a *test target*, and a test target that behaves predictably is worth more than one that is new.

**Deferred to v1.1, in this order: API 37 (Android 17)** — the newest release and the Aug-2027 Play `targetSdk` bar, so it is the compliance-testing target — **then API 34 (Android 14)**. Both additive; nothing in the v1 design blocks them, because phase 14's manifest is written for N images from the start.

### Where images come from — **never a floating branch**

The original plan instructed fetching from `aosp-android-latest-release`. That branch always tracks the most recent AOSP release, which has been **Android 17 / API 37 since 2026-06-16** — so it would have delivered an image that failed the plan's own API-level check.

- **Pin an explicit build ID** from an Android-13 branch (`android-13.0.0_rNN`). A moving target makes every later bug unreproducible.
- **Verify artifact availability first.** Phase 07 must confirm that Cuttlefish x86_64 (or GSI x86_64) artifacts for API 33 are still published and downloadable **before** committing to the pipeline. CI artifact retention on older branches is unverified and is a phase-07 gate, not an assumption.
- **Fallback ladder:** AOSP Cuttlefish x86_64 -> **AOSP GSI x86_64** -> own AOSP build. GSI is the preferred fallback because it carries none of Cuttlefish's host-tooling expectations (see Risks).
- **Bliss OS is gone from the plan entirely.** It capped at Android 13 and cannot serve as anything but a spike, and phase 01 now spikes on the v1 image itself.

### Hosting: GitHub Releases

**2 GiB per file, up to 1000 assets per release, no total-size cap, no bandwidth limit** — verified. **GitHub Pages is disqualified** (1 GB published-site limit; not a binary CDN).

Ship only the system image; create the data overlay client-side. SHA-256 per asset, resumable, **manifest signed (S4)**. If an asset exceeds 2 GiB, split to `.partNN`. GHCR is the secondary option — shared with Provider B, which pulls redroid from a registry anyway.

## Known Limitations (state these publicly)

1. **ARM-only APKs will not run.** No native bridge — `libhoudini` is discontinued, `libndk_translation` has no redistribution license. Same limitation as the official Android Studio x86_64 emulator. Detected at install time (phase 08), never a mystery crash.
2. **No Google Play / GMS / Widevine L1.** By design (R2).
3. **Anti-cheat-protected apps will detect the VM.** Out of scope (R1).
4. **Requires VT-x/AMD-V + the Windows Hypervisor Platform feature** (admin + one reboot).
5. **If S3 cannot be met, the guest accepts unauthenticated root ADB on loopback.** AOSP CI images are `userdebug`, where `ro.adb.secure` defaults to 0 and `ALLOW_ADBD_NO_AUTH` is compiled in. Phase 07 attempts authenticated ADB; if it fails, this becomes a stated limitation and S2 (loopback-only) becomes the sole protection.

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| **Cuttlefish is documented Linux+KVM only; Windows is not a supported host** | **v1 has no image — the plan's single largest risk** | Phase 01 is now an explicit feasibility spike with a kill criterion. GSI x86_64 is the pre-identified fallback precisely because it lacks Cuttlefish's host-tooling dependencies |
| **Cuttlefish expects vsock, ramoops/pstore nvdimm, and TAP networking — none available on a Windows host** | Phase 01 fails for reasons unrelated to the VMM choice | The real phase-01 question: does the Android userspace reach `boot_completed` with all three absent? Answer it before anything else |
| **Open QEMU bug: WHPX SMP boot hang** with `-smp cores=2` + `kernel-irqchip=off` on Windows 11 build 26200.x (QEMU 10.0.6 / 10.1.2 / master, unresolved) | Phase 01's QEMU arm hangs with no POST output, misread as image failure | Start from QEMU's documented baseline (`-accel whpx -M pc`), boot single-vCPU first, then scale `-smp`. `kernel-irqchip` and `pic=off` are **diagnostic knobs, not requirements**. Pin a known-good QEMU version and record it |
| **crosvm snapshotting is upstream-declared "100% not supported" with an unstable format** | Phase 11 — the most valuable dev-tool feature — has no VMM support | Phase 01 must weight snapshot support, not merely note it. If crosvm wins, phase 11 is overlay-copy-only and RAM-state resume is rejected outright |
| **crosvm's WHPX backend is "tested upstream: no"** | Instability with no upstream safety net | Standing risk. QEMU is the known-exercised path |
| `adbd` not authenticated / not on TCP in the guest | S3 unmet, or P0 blocker | Phase 07 owns both. Prop injection under AVB has a three-option ladder |
| ffmpeg link pain on Windows/MSVC | Phase 05 friction | `openh264` fallback — but see AVC patents, it is not license-free |
| Guest audio/video clock drift under WHPX | Late bug | Pin a known-good guest clocksource; test in phase 06 |
| egui video pane perf (texture upload per frame) | P2 polish | Upload NV12 -> wgpu shader convert; never CPU RGBA |
| **scrcpy has no pause / set-fps / set-bitrate control message** | Phase 12's pause-on-hide has no mechanism | Redefined as *keep draining, drop decode*; `RESET_VIDEO` on resume. Encoder params are launch-time only |
| **`.wslconfig kernel=` is machine-global** | Breaks a developer's existing WSL distros | Phase 17: assess whether a rebuild is needed **at all** first; if it is, key-level revert + a persisted intent journal |
| Maintaining prebuilt WSL kernels per WSL version | Ongoing burden | May evaporate — see phase 17. Version-match refusal fails safe |
| P5 forks the display path away from scrcpy | Two rendering paths forever | Phase 20 is a hard gate. Also: stock Windows QEMU almost certainly lacks `rutabaga_gfx_ffi`, so the QEMU branch needs a self-built QEMU — changing the GPLv2 posture again |

## Resolved Decisions

- **Display/control** -> ADB + scrcpy protocol, **forward tunnel**. Custom virtio-gpu host renderer rejected for v1.
- **ADB** -> **native device-protocol client**, no `adb.exe`, no `:5037`. Auto-registration is conditional.
- **Backend abstraction** -> `DeviceProvider`, six methods, everything above it backend-agnostic.
- **Provider A ships first** — self-contained, mutates nothing on the host. Provider B opt-in.
- **API level** -> **API 33 only** at v1; API 37, then 34, at v1.1.
- **Image sourcing** -> pinned build ID, never a floating branch. Availability verified before pipeline work.
- **Image hosting** -> GitHub Releases, signed manifest. Pages rejected.
- **Shell stack** -> Rust + egui/eframe (wgpu).
- **License** -> Apache-2.0. crosvm is **BSD-3-Clause**. QEMU stays a child process **and its source ships**.
- **Disk overlays** -> `DiskOps` abstraction; no unconditional `qemu-img`.
- **Rejected: Cuttlefish inside WSL2.** GPU passthrough to nested VMs is impossible (VFIO/IOMMU unexposed). *(The original rationale also claimed AMD SVM/NPT is not exposed to WSL2 at all — that is stale; AMD nested virtualization is supported on Windows 11 with VM config 9.3+. The GPU-passthrough reason stands on its own.)*
- **Rejected: Android Studio SDK system images** as a shipping dependency — license restricts redistribution. **This is the same reasoning that rules out bundling `adb.exe`.**

## Open Questions

1. **VMM: QEMU or crosvm?** — now a *sub-decision* of phase 01, subordinate to "does anything boot at all."

   | | QEMU | crosvm |
   |---|---|---|
   | License | GPLv2 -> process boundary **+ ship source** | **BSD-3-Clause -> linkable** |
   | Language | C | **Rust — same as the workspace** |
   | Windows + WHPX | Yes, prebuilt binaries; **open SMP hang bug** | Yes, **"tested upstream: no"** |
   | Windows toolchain | Download an exe | **Native MSVC build** (`install-deps.ps1`, `cargo build --features all-msvc64,whpx`) — *not* a container cross-compile |
   | Snapshots (phase 11) | `savevm`/`-loadvm`, mature | **"100% not supported", format unstable** |
   | qcow2 overlay creation | `qemu-img` (GPLv2) | `crosvm create_qcow2 --backing-file` |
   | gfxstream (phase 21) | `rutabaga_gfx` — **absent from stock Windows builds** | First-class |

   **No current lean.** The original lean toward crosvm rested on it being Apache-2.0 (it is BSD-3) and on a container-cross-compile cost that is not the real flow. Snapshot support now cuts hard the other way. Decide on evidence in phase 01.

2. **Can S3 (authenticated ADB) be met** on an AOSP `userdebug` image without a custom build? Determines whether Known Limitation 5 ships.
3. **Are API 33 Cuttlefish x86_64 artifacts still published?** Gate at the top of phase 07; GSI is the fallback.
4. **Who owns the manifest signing key (S4), and is it the same key as the installer code-signing certificate?** Both unowned. Unsigned MSI triggers SmartScreen on every download.
5. **ARM64 Windows hosts** (Snapdragon X) — post-v1? Inverts the ABI problem. Not planned in any phase.

## Red Team Review

### Session — 2026-08-27
**Findings:** 34 after dedup (31 accepted, 3 rejected) · **Severity:** 9 Critical, 14 High, 11 Medium
**Reviewers:** Security Adversary, Failure Mode Analyst, Assumption Destroyer, Scope & Complexity Critic
**Full report:** [`red-team-260827-1530-mtplayer-plan.md`](../reports/red-team-260827-1530-mtplayer-plan.md)

13 of the 31 accepted findings were **verified-false external facts** — including the floating-branch/API-level contradiction, crosvm's license and Windows build flow, crosvm snapshot support, `kernel-irqchip=off`, `CONFIG_ASHMEM`'s removal from Linux 5.18, scrcpy's absent pause message, and the openh264 AVC patent condition.

| # | Finding | Severity | Disposition | Applied To |
|---|---------|----------|-------------|------------|
| F1 | Floating branch yields API 37, not 36 | Critical | Accept | plan.md, 01, 07 |
| F2 | Linaro direct-boot path unreproducible on Windows | Critical | Accept | plan.md, 01, 07 |
| F3 | crosvm is BSD-3-Clause, not Apache-2.0 | High | Accept | plan.md, 01, 15 |
| F4 | crosvm Windows flow is native MSVC, not cross-compile | High | Accept | plan.md, 01 |
| F5 | crosvm snapshotting unsupported, format unstable | High | Accept | plan.md, 01, 11 |
| F6 | `kernel-irqchip=off` is debug-only; open WHPX SMP hang bug | High | Accept | plan.md, 01, 02 |
| F7 | `CONFIG_ASHMEM` removed in 5.18; config list wrong and incomplete | Critical | Accept | plan.md, 17 |
| F8 | scrcpy has no pause/set-fps/set-bitrate message | High | Accept | plan.md, 04, 12 |
| F9 | Control table written against an older scrcpy than pinned | Medium | Accept | 04 |
| F10 | openh264 AVC grant forfeited by bundling | High | Accept | plan.md, 05, 15 |
| F11 | GPLv2 §3 does not accept a source "link" | High | Accept | plan.md, 15 |
| F12 | AMD SVM/NPT claim stale | Medium | Accept | plan.md |
| F13 | `rutabaga_gfx` absent from stock Windows QEMU | Medium | Accept | plan.md, 21 |
| D1 | `adb.exe` unbundled hard dependency | Critical | Accept | plan.md, 03, 12, 15, 16 |
| D2 | Job object vs CLI lifecycle contradiction | Critical | Accept | 06, 13 |
| D3 | Reaping kills live instances; TOCTOU; writable pid file | Critical | Accept | 06, 13 |
| D4 | `Stopped` has no cross-process enforcement | Critical | Accept | 06, 11 |
| D5 | Unauthenticated root ADB; no threat model | Critical | Accept | plan.md, 03, 07 |
| D6 | Phase 18 publishes ADB port with no bind address | Critical | Accept | plan.md, 17, 18 |
| D7 | Unsigned manifest; `id` path traversal; no URL allowlist | Critical | Accept | plan.md, 14 |
| D8 | scrcpy reverse tunnel = unauthenticated host listener | High | Accept | 04 |
| D9 | Profile ownership ambiguous; RAM guard arithmetic wrong | High | Accept | 06, 10, 12 |
| D10 | `qemu-img` hard-coded while VMM undecided | High | Accept | plan.md, 01, 07, 11 |
| D11 | Base-SHA mismatch remedy destroys all data and snapshots | High | Accept | 11, 14 |
| D12 | Image install non-atomic; orphaned directories | High | Accept | 14 |
| D13 | Installer vs running instances; `SnapshotMeta` lacks vmm_version | High | Accept | 11, 15 |
| D14 | `.wslconfig` whole-file revert; no intent journal | High | Accept | 17 |
| D15 | Cutting phase 13 breaks phases 14/15/16 | High | Accept | 11, 13, 14, 15, 16 |
| D16 | v1 estimate arithmetically unreachable | High | Accept | plan.md |
| D17 | Crate count wrong; phantom feature-gate target | High | Accept | plan.md, 02, 13, 21 |
| D18 | 5 of 6 cross-phase contracts inconsistent | High | Accept | 06, 10, 13, 14, 16, 19, 21 |
| D19 | `ro.serialno` identical across instances | Medium | Accept | 07, 09, 12 |
| D20 | Guest -> host path traversal in shared-folder sync | Medium | Accept | plan.md, 03, 10 |
| D21 | Diagnostics allowlist is field-level, values verbatim; logcat OSC | Medium | Accept | plan.md, 09, 13, 16 |
| D22a | RAM-state resume bound as requirement | Medium | Accept (narrow) | 11 |
| D22b | Two-way diff sync unrequested | Medium | Accept | 10 |
| D22c | `.apk` file association unrequested | Medium | Accept | 15 |
| D22d | CI license gate "unrequested" | Medium | **Reject** | — |
| D22e | ADB auto-registration "unrequested" | Medium | **Reject** | — |
| D23 | Five unverifiable or self-contradicting success criteria | Medium | Accept | 06, 08, 12, 16, 19 |
| — | Phase 12 a "phantom DeviceProvider consumer" | Medium | **Reject** | — |

**Rejection rationale:**
- **D22d (CI license gate):** F3, F10, and F11 each show a license or patent error that a human review missed. Automated dependency-license checking is *more* justified after this review, not less.
- **D22e (ADB auto-registration):** "Always-on ADB, endpoint shown and copyable" reasonably includes appearing in `adb devices`; phase 12 already specifies disconnect-on-stop and tracks only its own registrations. It is now *conditional* on the user having `adb.exe` (D1), which further reduces its cost.
- **Phantom consumer:** phase 12 consumes `InstanceStatus` and `InstanceManager`, not the trait. Naming it as a state-machine consumer is correct.

### Whole-Plan Consistency Sweep

Run 2026-08-27 after applying all 31 accepted findings. **Result: zero unresolved contradictions.**

**Decision delta applied across all 22 files:**

| Old | New | Files touched |
|---|---|---|
| API 36 / Android 16 | **API 33 / Android 13** | plan.md, 01, 02, 05, 07, 08, 10, 12, 14, 16, 21 |
| Floating branch `aosp-android-latest-release` | **Pinned `android-13.0.0_rNN` build id** | plan.md, 01, 07 |
| ADB *server* on `:5037` via `adb.exe` | **Native ADB device protocol** | plan.md, 02, 03, 12, 15, 16 |
| scrcpy *reverse* tunnel (host listens) | **Forward tunnel (host connects out)** | plan.md, 03, 04 |
| Phase 01 = VMM bake-off | **Phase 01 = feasibility spike with a kill criterion** | plan.md, 01, 02, 07, 11 |
| crosvm "Apache-2.0" | **BSD-3-Clause** | plan.md, 01, 15 |
| Unconditional `qemu-img` | **`DiskOps` abstraction** | plan.md, 06, 07, 11, 14 |
| Job object as primary supervision | **Breakaway spawn + liveness leases** | 06, 11, 13 |
| Kill-by-pid reaping | **Handle-scoped termination (S6)** | 06 |
| Registry holds "the profile" | **Immutable `resolved_profile`** | 06, 10, 12 |
| `CONFIG_ASHMEM` required | **Removed; assess-stock-kernel gate first** | plan.md, 17 |
| Phase 13 wholly cuttable | **13a not cuttable / 13b cuttable** | plan.md, 11, 13, 14, 15, 16 |
| 8 workspace crates | **7 at phase 02; +1 at 17, +1 at 21** | plan.md, 02, 13, 17, 21 |
| "2-4 months to v1" | **5-8 months, derived from phase sums** | plan.md |

**Renamed files** (links updated in `plan.md`, no dangling references remain):
- `phase-01-vmm-bakeoff.md` -> `phase-01-image-feasibility-spike.md`
- `phase-07-cuttlefish-api36-image-pipeline.md` -> `phase-07-aosp-api33-image-pipeline.md`

**Contract reconciliation** — all six previously-failing cross-phase contracts now consistent:
- `DeviceProvider`: `status()` returns `Result`; change at phase 18 is stated as expected, not "additive only".
- `InstanceStatus`: stays an enum; `RenderPath` moved to `InstanceHandle` (19), and 21 reuses it rather than adding a parallel concept.
- `DeviceProfile`: `resolved_profile` owns running-instance values; templates are templates; phase 21's new field must carry `#[serde(default)]` and appear in phase 10's schema.
- Image manifest: schema owned solely by phase 14; phase 07 produces a conforming instance; one authoritative location each for remote and local.
- Exit codes: 9 (image verification), 10 (network), 11 (instance busy) added for phase 14's and phase 11's failure modes; read-only CLI commands must not take the lease.
- `Remedy`: settled as a struct carrying a `ConditionKind` enum, with a `troubleshooting_anchor`; phase 18 required to add its own docs sections.

**Dependency graph relaxed** where the original chain forced needless serialization: phase 09 `[8]`->`[6]`, phase 10 `[9]`->`[8, 9]`, phase 14 `[13]`->`[7, 12]`, phase 20 `[16]`->`[16, 19]`.

**Security invariants S1-S9 added to `plan.md` and threaded** into 02, 03, 04, 05, 06, 07, 09, 10, 12, 13, 14, 16, 17, 18.

**Known contradictions deliberately retained** (documented trade-offs, not defects):
1. **Breakaway spawn leaks a VM if the UI is hard-killed** until the next launch reaps it. Accepted in phase 06 — the alternative broke the CLI entirely.
2. **S3 (authenticated ADB) may prove infeasible** on a `userdebug` image. Phase 07 attempts it; Known Limitation 5 documents the fallback, with S2 as sole protection.
3. **Provider B's scope is unresolved until phase 17 step 1.** Effort is stated as a range (3 d - 2 wk) rather than a false single figure.

**Verification note:** `ak plan validate` passed before these edits. It could not be re-run afterward — a session-wide safety classifier began blocking write-capable and compound Bash invocations partway through this session (it also blocked `WebSearch`, which is why three of the red team's external claims remain marked UNVERIFIED). Structure was instead verified by reading: all 21 phase links resolve to existing files, all `dependencies:` refs resolve, and frontmatter is intact. **Re-run `ak plan validate ./plans/260827-1138-mtplayer-mvp` in a fresh session to confirm mechanically.**

<!-- slug: mtplayer-mvp -->
