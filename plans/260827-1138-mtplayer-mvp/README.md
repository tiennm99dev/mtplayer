# MTPlayer MVP — Plan Overview

**Created:** 2026-08-27 11:38 (Asia/Saigon) · **Branch:** main · **Status:** not started
**Inputs:** `plans/reports/research-260827-1058-android-emulator-windows.md`, `plans/reports/architecture-260827-1130-dev-tool-dual-backend.md`

## What MTPlayer Is

An Android **dev tool** for Windows. Boots AOSP-based x86_64 Android in a VM, shows it in a window, installs APKs, exposes ADB. No Google services, no ads, no telemetry, no kernel driver.

**Not** a gaming emulator. No key mapping, no anti-cheat evasion, no FPS competition with BlueStacks/LDPlayer.

### Locked requirements
- **R1** Dev tool, not gaming
- **R2** APK-only install, zero Google services
- **R3** Runs on Windows with Hyper-V **or** WSL installed

## Architecture

```
                MTPlayer Shell  (Rust + egui/eframe + wgpu)
       instance list · logcat · APK drop · profile forms · video pane
                              |
   display: scrcpy H.264 over ADB     control: ADB (input/install/push/pull)
                              |
                    +---------v----------+
                    |   DeviceProvider   |  start stop snapshot restore adb_endpoint status
                    +----+----------+----+
                         |          |
        Provider A: QEMU + WHPX   Provider B: WSL2 + redroid
        (P1, ships first)         (P4, opt-in)
```

**Load-bearing decision:** the shell talks only to an **ADB endpoint**. Display and input ride the scrcpy protocol, which works on any Android ≥5 exposing ADB. Backends are therefore interchangeable, and the host-side GPU problem (gfxstream/virgl → ANGLE/D3D12) is out of the MVP entirely.

## Stack

| Component | Choice | Rationale |
|---|---|---|
| Language | Rust (2021, stable) | Single static binary, no runtime deps |
| UI | `eframe`/`egui` + wgpu | Video frame as texture **and** tables/forms in one frame. Tauri's WebView is the wrong tool for a video surface |
| Video decode | ffmpeg via `ffmpeg-next` (or `openh264` fallback) | Same path scrcpy uses; proven |
| Guest display/input | `scrcpy-server` (Apache-2.0), version-pinned, bundled | Removes GPU-host work from MVP |
| ADB | Own client over TCP `127.0.0.1:5037` | Avoids shelling out; needed for logcat streaming anyway |
| VMM | **OPEN — QEMU vs crosvm. Decided by the P0.0 bake-off** | Both support WHPX. crosvm: Apache-2.0, Rust, native Cuttlefish host. QEMU: GPLv2, prebuilt Windows binaries. See Open Questions |
| Guest image | AOSP Cuttlefish x86_64 (v1) — Bliss OS 16.9 for the P0 spike only | Apache-2.0, current APIs. Bliss tops out at Android 13; see Image Strategy |
| Provider B | redroid + custom WSL2 kernel | Opt-in only |

### Licensing boundary — depends on the VMM choice
MTPlayer is **Apache-2.0**.

- **If QEMU:** GPLv2. Apache-2.0 cannot link GPLv2, so QEMU is invoked strictly as a **separate process** (`qemu-system-x86_64.exe`) — no linking, no static bundling.
- **If crosvm:** Apache-2.0. The boundary dissolves; crosvm can be linked as a Rust library or vendored as a crate. Simpler architecture, and same language as the rest of the workspace.

Either way `scrcpy-server` is Apache-2.0, so bundling the jar is clean — keep its `NOTICE` and pin the version.

**Also rejected: forking Google's Android Emulator** (`platform/external/qemu`, `emu-master-dev`). It is open source but **GPLv2**, forked from QEMU **2.7.0** (2016) and a decade diverged from upstream, with an idiosyncratic bundled-toolchain build. Forking it means adopting that divergence *and* relicensing MTPlayer to GPLv2, for no advantage over invoking a stock VMM.

## Repo Layout

```
MTPlayer/
├── Cargo.toml                  # workspace
├── crates/
│   ├── mtp-core/               # DeviceProvider trait, instance registry, config, profiles
│   ├── mtp-adb/                # ADB client: connect, install, push/pull, logcat, forward
│   ├── mtp-scrcpy/             # server push/launch, control protocol, H.264 demux
│   ├── mtp-video/              # decode -> wgpu texture
│   ├── mtp-provider-vm/        # Provider A: QEMU-or-crosvm on WHPX, disk overlays, snapshots
│   ├── mtp-provider-wsl/       # P4: redroid on WSL2
│   ├── mtp-cli/                # headless binary for CI
│   └── mtp-ui/                 # egui app -> mtplayer.exe
├── assets/                     # scrcpy-server.jar, default device profiles, NOTICE files
└── images/                     # image manifest (downloaded at runtime, NOT vendored)
```

Crate names kebab-case; Rust source files snake_case per language convention.

## Phases

Detail in [`phases.md`](phases.md).

| Phase | Goal | Est. |
|---|---|---|
| **P0** | Prove the pipe: QEMU+WHPX boots, ADB connects, scrcpy frames render, input works | 1–2 wk |
| **P1** | Usable device: APK drop + ABI check, logcat, push/pull, profiles, clean shutdown | 2–4 wk |
| **P2** | Dev-tool value: snapshot/restore, multi-instance, ADB endpoint surfaced, CLI | 2–3 wk |
| **P3** | Shippable: installer, WHPX preflight diagnostics, 2–3 API-level images | 2–3 wk |
| **P4** | Provider B: opt-in WSL2 + redroid behind the same trait | 2–4 wk |
| **P5** | Guest GPU (virgl/gfxstream) — only if profiling proves software GL is the bottleneck | TBD |

**~2–4 months to P3** for one focused engineer. P4 and P5 are post-v1.

## Image Strategy (decided 2026-08-27)

### Which API levels

**v1 ships exactly one image: Android 16 (API 36).**

Most-deployed version (24.4%, Aug 2026) **and** the current Play `targetSdk` bar — it is simultaneously the compliance target and the largest installed base. One image keeps the image manager, the installer, and the test matrix trivial.

Deferred to v1.1, in this order: **API 34** (Android 14, 17.2%), **API 33** (Android 13, ~68.9% cumulative reach — the minSdk floor). Both are additive; nothing in the v1 design blocks them.

**Consequence:** nothing Bliss-derived ships. Bliss OS is now purely a P0 spike target and drops out of the product entirely.

### Where images come from — this changed the plan

Bliss OS 16.9 (the only publicly downloadable build) is **Android 13**. Bliss OS 18 (Android 15) exists in source but **public PC images are paused**, and no Android 16 build exists as of Aug 2026. Bliss therefore cannot deliver API 36.

**Consequence: AOSP image work moves from post-v1 into v1.**

- **P0 spike:** Bliss OS 16.9 — cheapest path to a booting guest, throwaway.
- **v1:** AOSP **Cuttlefish x86_64** images from ci.android.com — official, Apache-2.0, current API levels. Linaro has documented booting Cuttlefish guests directly under QEMU; that work lands in P1/P3 rather than being deferred.
- **Fallback:** AOSP GSI x86_64, or own AOSP builds if Cuttlefish-under-QEMU proves too awkward on Windows.

### Hosting: GitHub Releases

Confirmed viable — **2 GiB per file, up to 1000 assets per release, no total-size cap and no bandwidth limit**. GitHub's own recommended path for binary distribution.

**GitHub Pages is disqualified:** 1 GB published-site size limit; not a binary CDN.

To stay under 2 GiB per file: **ship only the system image; create the empty data overlay client-side** with `qemu-img create`. zstd-compressed system partitions should fit in a single asset. If one doesn't, split to `.partNN` and reassemble after download. SHA-256 per asset, resumable.

Secondary option: **GHCR** (free for public repos, no 2 GiB layer awkwardness) — shared infrastructure with Provider B, which pulls redroid from a registry anyway.

## Known Limitations (state these publicly)

1. **ARM-only APKs will not run.** No native bridge — `libhoudini` is discontinued, `libndk_translation` has no redistribution license. Same limitation as the official Android Studio x86_64 emulator. MTPlayer must detect this at install time and fail with a clear message, never a mystery crash.
2. **No Google Play / GMS / Widevine L1.** By design (R2).
3. **Anti-cheat-protected apps will detect the VM.** Out of scope (R1).
4. **Requires VT-x/AMD-V + the Windows Hypervisor Platform feature** (admin + one reboot).

## Risks

| Risk | Impact | Mitigation |
|---|---|---|
| WHPX quirks (`kernel-irqchip`, timer drift, virtio stalls) | P0 slips | Spike it first — P0 exists precisely to de-risk this |
| **Cuttlefish images won't boot on Windows under the chosen VMM** | v1 has no current-API image — the biggest v1 risk | Now tested in **P0.0**, not P1.7. crosvm is Cuttlefish's native host, so this risk is much lower there than under QEMU. Fallbacks: AOSP GSI x86_64, then own AOSP builds |
| **crosvm won't build/run cleanly for a Windows host** | Forces QEMU + the GPLv2 process boundary | P0.0 bake-off resolves it before any provider code exists. QEMU is the known-good fallback, so this is a cost risk, not a project risk |
| Bliss OS image not cleanly convertible to a headless qcow2 | P0 slips | Spike-only dependency; fall back to Android-x86 generic |
| `adbd` not listening on TCP in guest by default | P0 blocker | `service.adb.tcp.port=5555` via init prop injection into the image |
| ffmpeg build/link pain on Windows/MSVC | P0 friction | Prebuilt shared ffmpeg + `vcpkg`; `openh264` as fallback |
| Guest audio/video clock drift under WHPX | Late bug | Pin a known-good guest kernel clocksource; test early in P1 |
| egui video pane perf (texture upload per frame) | P2 polish | Upload as NV12 → wgpu shader convert; avoid CPU RGBA conversion |

## Resolved

- **API levels** → **Android 16 (API 36) only** at v1. API 34 then 33 at v1.1. See Image Strategy.
- **Image hosting** → GitHub Releases. Pages rejected.
- **Shell stack** → Rust + egui/eframe (wgpu).
- **License** → Apache-2.0; QEMU stays a child process.

## Open Questions

1. **VMM: QEMU or crosvm?** — **the one architectural decision still open.** Resolved by the P0.0 bake-off; blocks nothing before then.

   | | QEMU | crosvm |
   |---|---|---|
   | License | GPLv2 → forced process boundary | **Apache-2.0 → linkable, matches MTPlayer** |
   | Language | C | **Rust — same as the workspace** |
   | Windows + WHPX | Yes, prebuilt binaries | Yes (Linux, Windows, Android hosts; KVM/WHPX/HAXM/Gunyah) |
   | Build story on Windows | **Download an exe** | **Rough** — dev flow assumes a Linux container cross-compiling; no prebuilt Windows binaries |
   | Cuttlefish images | Awkward — Linaro's direct-boot workaround | **Native. Cuttlefish is built on crosvm** |
   | gfxstream (P5) | Via `rutabaga_gfx` | First-class |

   Current lean: **crosvm**, on license + language + Cuttlefish alignment — which also de-risks the v1 image path. Held open because the Windows build friction is exactly the kind of thing that reads fine in docs and costs two weeks in practice.

2. **CLI/headless split** — is CI mode a v1 requirement? Planned in P2; if it slips to v1.x the `mtp-cli` crate can be deferred and `mtp-core` stays simpler.
3. **ARM64 Windows hosts** (Snapdragon X) — in scope later? Inverts the ABI problem: ARM64 guests run natively, x86_64 APKs become the failing case.
