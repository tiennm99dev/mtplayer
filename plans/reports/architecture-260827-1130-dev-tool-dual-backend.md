# Architecture Decision: MTPlayer as an Android Dev Tool (dual-backend, APK-only, no GMS)

**Date:** 2026-08-27 11:30 (Asia/Saigon) · **Supersedes scope in:** `research-260827-1058-android-emulator-windows.md`

> **Decisions 1-3 still hold. Decision 4 (guest image) is superseded.** Bliss OS is now a spike-only target and ships nothing; v1 ships exactly one AOSP image (Android 13 / API 33), and AOSP image work moved from post-v1 into v1. The phase numbering here (P0-P5) has been expanded into 21 phases. Canonical plan: [`../260827-1138-mtplayer-mvp/plan.md`](../260827-1138-mtplayer-mvp/plan.md).

## Locked Requirements

| # | Requirement | Consequence |
|---|---|---|
| R1 | **Dev tool**, not a gaming emulator | Anti-cheat, key mapping, publisher partnerships, FPS competition — all **out of scope** |
| R2 | **APK-only install**, no Google services | GMS/Play/Widevine blockers **eliminated**; AOSP images become cleanly redistributable |
| R3 | Runs on Windows with **Hyper-V *or* WSL** installed | Dual backend behind one shell |

**Net effect: both of the two blocking risks from the prior report are gone.** No ARM-bridge licensing exposure (see §Known Limitation), no GMS licensing exposure. What remains is ordinary engineering.

---

## Decision 1 — Display and control layer: ADB + scrcpy protocol

**The single highest-leverage decision.** Do not build a custom display pipeline.

scrcpy (Apache-2.0) captures the guest screen inside Android, encodes H.264/H.265, and streams it over ADB; input is injected back the same way. It works on **any** Android ≥5 exposing ADB — QEMU guest, redroid container, physical device, no difference.

Consequences:
- **Both backends become interchangeable.** The shell's only dependency is "an ADB endpoint."
- **The hard GPU work (gfxstream/virgl → ANGLE/D3D12) drops out of the MVP entirely.** Guest can start on SwiftShader/software GL; upgrade guest-side rendering later without touching the shell.
- Latency ~30–70 ms — irrelevant for a dev tool, fatal for a gaming one. R1 makes this free.
- Screen capture, recording, and resolution control come for free.

Rejected: custom virtio-gpu host renderer in the MVP (months of work, zero MVP value under R1).

---

## Decision 2 — Backend abstraction: `DeviceProvider`

```
              MTPlayer Shell (window, APK drop, logcat, instance list)
                              |
        display: scrcpy H.264 over ADB   control: ADB (input/install/push/pull)
                              |
                    +---------v----------+
                    |  DeviceProvider    |   start() stop() snapshot() adbEndpoint()
                    +----+----------+----+
                         |          |
        Provider A: QEMU+WHPX    Provider B: WSL2 + redroid
        (Hyper-V path)           (WSL path, opt-in)
        AOSP/Bliss x86_64 img    redroid container, custom WSL kernel
```

One interface, ~6 methods. Everything above it is backend-agnostic.

---

## Decision 3 — Provider A (QEMU + WHPX) ships first

| | Provider A: QEMU+WHPX | Provider B: WSL2+redroid |
|---|---|---|
| Prereq | "Windows Hypervisor Platform" feature (admin + reboot, once) | WSL2 + Docker + **custom WSL2 kernel** |
| Invasiveness | Self-contained; touches nothing | `.wslconfig kernel=` is **machine-global** — replaces the kernel for *every* distro |
| Boot time | ~20–40 s | ~5 s |
| Multi-instance | N QEMU processes, heavier RAM | `docker run` ×N, very cheap |
| Guest control | Full (own image, own kernel) | Constrained to redroid images |
| GPU path later | gfxstream/virgl → ANGLE/D3D12 | `/dev/dxg` GPU-PV + Mesa D3D12 |

**Provider A first** — it is the one that can be shipped as a single installer that doesn't mutate a developer's existing WSL setup. Rewriting a dev's global WSL kernel by default is not acceptable; Provider B must be **opt-in**, for users who want instant boot and cheap N-instance matrix testing.

Note the prerequisites overlap more than they appear: WSL2 requires Virtual Machine Platform, and WHPX requires the Windows Hypervisor Platform feature — different toggles, same hardware and same underlying hypervisor. A machine that runs WSL2 can run Provider A after one feature-enable + reboot.

**Rejected: Cuttlefish inside WSL2.** Nested virtualization does work (`nestedVirtualization=true`), but GPU passthrough to nested VMs under WSL2 is impossible (VFIO/IOMMU not exposed) and AMD SVM/NPT isn't exposed to WSL2 at all. Dead end for a graphical device.

---

## Decision 4 — Guest image: build from AOSP, seed with Bliss OS

R2 removes the GMS constraint, so an AOSP x86_64 build is fully Apache-2.0 and freely redistributable — no grey area, unlike shipping Play Services.

- **MVP:** prebuilt Bliss OS x86_64 image (fast start, don't burn weeks on AOSP builds).
- **v1:** own AOSP + x86 patch builds — reproducible, minimal, no third-party bundle, and it lets you strip everything a dev tool doesn't need.
- Ship an image **per Android API level** (e.g. 11 / 13 / 14) — that is the core value proposition of a dev tool.

Avoid Android Studio SDK system images as a shipping dependency: their license restricts redistribution.

---

## Known Limitation: ARM-only APKs will not run

No native bridge (`libhoudini` discontinued, `libndk_translation` has no redistribution license). APKs whose native libs are `arm64-v8a`-only will fail to install or crash on `System.loadLibrary`.

This is the **same limitation the official Android Studio x86_64 emulator has** — an accepted norm in the dev-tool space, which R1 makes survivable. Requirement: detect it and **fail loudly with a clear message** ("APK ships arm64-v8a only; no x86_64 ABI") rather than crashing mysteriously. Parse ABIs from the APK at install time.

---

## Feature Set Implied by "Dev Tool"

Different product from a gaming emulator. What actually matters:

- Drag-drop APK install; ABI pre-check with a clear error
- Always-on ADB, with the endpoint shown and copyable for Android Studio / Gradle
- Live logcat pane with filtering
- File push/pull, shared folder
- **Snapshot / restore / factory-reset in one click** — the killer dev-tool feature
- Multiple concurrent instances at different API levels (test matrix)
- Headless + CLI mode for CI
- Configurable device profile: resolution, DPI, RAM, cores, locale, network throttling
- Sensor/location injection (later)

Not needed: key mapping, macro recorder, gamepad, ad/offer-wall SDK, telemetry.

---

## Revised Roadmap

| Phase | Goal | Exit criteria |
|---|---|---|
| **P0 (1–2 wk)** | Prove the pipe | QEMU+WHPX boots Bliss OS x86_64; ADB connects; scrcpy stream renders in a window; mouse/keyboard injected |
| **P1 (2–4 wk)** | Usable device | APK drag-drop + ABI check; logcat pane; push/pull; device profile config; clean shutdown |
| **P2 (2–3 wk)** | Dev-tool value | snapshot/restore; multi-instance manager; ADB endpoint surfaced; CLI for CI |
| **P3 (2–3 wk)** | Shippable | signed installer; WHPX/VT-x preflight diagnostics with actionable fixes; 2–3 API-level images |
| **P4 (2–4 wk)** | Provider B | opt-in WSL2+redroid backend behind the same `DeviceProvider`; ship prebuilt WSL kernel + explicit warning about the global `.wslconfig` change |
| **P5** | Guest GPU | virgl or gfxstream in-guest; only if profiling shows software GL is the bottleneck |

Roughly **2–4 months to P3** for one focused engineer — versus 4–7 months under the previous gaming scope. R1+R2 cut the project roughly in half.

---

## Stack Recommendation

| Component | Choice | Why |
|---|---|---|
| VMM | QEMU (Windows build) + `-accel whpx` | Only sane hypervisor path under VBS/HVCI |
| Guest | Bliss OS x86_64 → own AOSP builds | Redistributable under R2 |
| Display/input | scrcpy protocol (embed `scrcpy-server`) | Apache-2.0; removes the entire GPU-host problem from MVP |
| Shell | **Rust + egui/eframe (wgpu)** — decided 2026-08-27 | Video frame as a texture *and* tables/forms in one immediate-mode frame; Tauri's WebView is wrong for a video surface, scrcpy's SDL2 client is wrong for a tooling UI |
| Video decode | FFmpeg (as scrcpy does) | Proven path |
| Provider B | redroid + custom WSL2 kernel | Opt-in only |

Licensing caution: QEMU is GPLv2. Invoking `qemu-system-x86_64.exe` as a separate process keeps MTPlayer's own license unconstrained; linking QEMU into your binary does not. Keep the boundary at process spawn.

---

## Resolved

1. **Shell stack** — **Rust + egui/eframe (wgpu)**. Decided 2026-08-27.
2. **MTPlayer license** — **Apache-2.0** (repo LICENSE). Compatible with bundling `scrcpy-server` (Apache-2.0); **incompatible with linking GPLv2**, which locks in the QEMU-as-child-process boundary above.
3. **Provider B at v1?** — No. v1.x add-on (plan phase P4). Provider A satisfies R3 on any machine that can run WSL.

## Open Questions

1. **Which API levels** to ship images for at v1 — 13 + 14, or wider?
2. **CI/headless mode** a v1 requirement or later? Planned as P2.4; deferring it keeps `mtp-core` simpler.
3. **Image hosting** — guest images are ~2-4 GB each and are not vendored in the installer. GitHub Releases by default?
4. **ARM64 Windows hosts** (Snapdragon X) in scope later? There the situation inverts — ARM64 Android guests run natively and x86_64 APKs become the failing case.

---

**Plan:** `plans/260827-1138-mtplayer-mvp/`
