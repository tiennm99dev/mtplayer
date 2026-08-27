# MTPlayer MVP — Phase Detail

Companion to [`README.md`](README.md). Each phase ends with a demoable exit criterion.

---

## P0 — Prove the Pipe + VMM Bake-Off (2–3 wk)

**Goal:** a window on Windows showing live Android, responding to mouse and keyboard — **and a decided VMM.** Ugly is fine. This phase exists to de-risk WHPX, the scrcpy path, and the QEMU-vs-crosvm choice before any product code is written.

### Tasks

**P0.0 — VMM bake-off: QEMU vs crosvm (do this first)**

The one architectural decision still open. Cheap now, expensive once `mtp-provider-vm` exists.

Boot the **same AOSP Cuttlefish x86_64 API 36 image** on Windows under each, and record:

| Criterion | Why it matters |
|---|---|
| Does it boot at all under WHPX? | Gate. Everything else is moot |
| Effort to obtain a working Windows binary | crosvm ships none; dev flow assumes a Linux container cross-compiling. This is the main risk against it |
| Boot time, idle CPU, RAM overhead | Directly user-visible |
| Cuttlefish image compatibility | crosvm is Cuttlefish's native host; QEMU needs Linaro's direct-boot workaround |
| Snapshot/restore support | P2.1 depends on it |
| Disk/net/input device config effort | Determines provider complexity |
| Reproducible build in CI | Determines release engineering cost |

**Decision rule:** crosvm wins unless getting a working Windows build costs more than ~1 week, or it fails to boot the Cuttlefish image under WHPX. Then QEMU, and the GPLv2 process boundary stands.

**Why crosvm is the lean:** Apache-2.0 (linkable, matches MTPlayer — no process boundary), Rust (same as the workspace), native Cuttlefish host (de-risks P1.7, the biggest v1 unknown), first-class gfxstream for P5. Supports Linux/Windows/Android hosts and KVM/WHPX/HAXM/Gunyah.

**Explicitly rejected — do not spend time here:** forking Google's Android Emulator (`platform/external/qemu`, `emu-master-dev`). Open source but GPLv2, forked from QEMU 2.7.0 (2016), a decade diverged, idiosyncratic bundled-toolchain build. Costs a relicense of MTPlayer and buys nothing over a stock VMM.

Record the result in this file before starting P0.4.

**P0.1 — Host preflight**
- Detect VT-x/AMD-V enabled and the `HypervisorPlatform` optional feature.
  - `Get-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform`
  - Enable path: `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All` (admin + reboot)
- Throwaway script is fine here; productionized in P3.

**P0.2 — Guest image (spike only — Bliss OS 16.9 / Android 13)**
- Obtain Bliss OS 16.9 x86_64. Run its installer once into a qcow2 to produce a bootable disk (`base.qcow2`).
- **This is a throwaway target.** Bliss tops out at Android 13 and cannot deliver API 36; v1 images come from AOSP Cuttlefish (see P1.7). Bliss is used here purely because it is the fastest way to get *something* booting and de-risk WHPX.
- Inject `service.adb.tcp.port=5555` so `adbd` listens on TCP — **without this the whole architecture doesn't start**.
- Verify boot to launcher headless.

**P0.3 — VM launch (QEMU form shown; crosvm equivalent if P0.0 picks it)**
```
qemu-system-x86_64.exe \
  -accel whpx,kernel-irqchip=off \
  -m 4096 -smp 4 -cpu max \
  -drive file=base.qcow2,format=qcow2,if=virtio \
  -device virtio-net-pci,netdev=n0 \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:<port>-:5555 \
  -device virtio-tablet-pci -device virtio-keyboard-pci \
  -device intel-hda -device hda-duplex \
  -display none
```
- `kernel-irqchip=off` is a known WHPX requirement — expect to tune this.
- Confirm `adb connect 127.0.0.1:<port>` succeeds from the host.

**P0.4 — `mtp-adb` skeleton**
- Client speaking the ADB server protocol over `127.0.0.1:5037` (length-prefixed `host:` commands).
- Minimum: `connect`, `shell`, `push`, `forward`.

**P0.5 — `mtp-scrcpy` skeleton**
- Push pinned `scrcpy-server.jar` to `/data/local/tmp/`, launch via `app_process` with `CLASSPATH`.
- Open the video socket, parse the frame header stream, emit H.264 access units.
- Implement the control message subset: touch, key, scroll.

**P0.6 — `mtp-video` + minimal `mtp-ui`**
- Decode H.264 → NV12; upload to a wgpu texture; draw in an egui window via `ui.image()`.
- Convert NV12→RGB in a shader, not on the CPU.
- Map mouse/keyboard events to scrcpy control messages.

### Exit criteria
1. **VMM decided and recorded** (P0.0).
2. Live Android window, mouse and keyboard working, sustained ≥25 fps at 1280×720, no crash over a 10-minute session.

### Kill/pivot signals
- WHPX cannot boot the guest stably after a week of tuning → re-evaluate Provider B as primary.
- scrcpy end-to-end latency >150 ms → investigate encoder settings before changing architecture.

---

## P1 — Usable Device (2–4 wk)

**Goal:** something a developer would actually choose over a physical device for a quick check.

### Tasks

**P1.1 — `DeviceProvider` trait + `mtp-provider-vm`** (QEMU or crosvm per P0.0)
```rust
trait DeviceProvider {
    fn start(&mut self, profile: &DeviceProfile) -> Result<InstanceHandle>;
    fn stop(&mut self, id: InstanceId) -> Result<()>;
    fn status(&self, id: InstanceId) -> InstanceStatus;
    fn adb_endpoint(&self, id: InstanceId) -> Option<SocketAddr>;
    fn snapshot(&mut self, id: InstanceId, name: &str) -> Result<()>;   // P2
    fn restore(&mut self, id: InstanceId, name: &str) -> Result<()>;    // P2
}
```
- QEMU child-process supervision: spawn, health check, graceful shutdown, orphan reaping on MTPlayer crash.
- Free-port allocation per instance.
- **Boot-complete detection:** poll `getprop sys.boot_completed` — do not guess with a sleep.

**P1.2 — APK install + ABI pre-check (the loud-failure requirement)**
- Read the APK as a zip; enumerate `lib/<abi>/` entries.
- No `lib/` → pure-Java, proceed.
- Contains `x86_64` (or `x86`) → proceed.
- **Only** `arm64-v8a` / `armeabi-v7a` → refuse with an explicit message:
  > `foo.apk` ships native libraries for arm64-v8a only. MTPlayer runs x86_64 Android and has no ARM translation layer. Rebuild with an x86_64 ABI split, or test this APK on a physical device.
- Drag-and-drop onto the window; also accept multiple APKs (split-APK install via `install-multiple`).

**P1.3 — Logcat pane**
- Stream `shell logcat -v threadtime` over a persistent ADB connection.
- Ring buffer (cap ~50k lines), level filter, tag filter, text search, pause/resume, copy, clear.

**P1.4 — File transfer**
- Push/pull with drag-drop; a designated shared host folder mapped to `/sdcard/Shared`.

**P1.5 — Device profiles**
- Resolution, DPI, RAM, cores, locale, timezone. Persist as TOML in `%APPDATA%/MTPlayer/profiles/`.
- Apply resolution/DPI via `wm size` / `wm density`; RAM and cores via QEMU args at boot.

**P1.6 — Lifecycle correctness**
- Clean shutdown (ACPI powerdown, then timeout → kill).
- Kill orphaned `qemu-system-x86_64.exe` from a previous crashed session on startup.
- Verify guest clocksource stability — check for audio/video drift **now**, not at P4.

**P1.7 — Real guest images: AOSP Cuttlefish x86_64 (promoted from post-v1)**
- Bliss OS cannot deliver API 36, so v1 images must come from AOSP.
- **Availability confirmed:** ci.android.com publishes `aosp_cf_x86_64_phone-img-<buildid>.zip` on the `aosp-android-latest-release` branch, plus a matching `cvd-host_package.tar.gz`. Free, Apache-2.0, current API levels.
- **The work is assembly, not acquisition.** The zip holds partition images (`super.img`, `boot.img`, `vbmeta.img`, …), not a bootable disk — `launch_cvd` normally composes them. Booting directly under QEMU/crosvm means building that composite disk yourself. Linaro documented the path on Linux; **confirm under WHPX on Windows** (started in P0.0).
- crosvm is Cuttlefish's native host, so if P0.0 picks crosvm this step is substantially cheaper.
- Produce **one image: API 36 (Android 16)**. That is all v1 ships.
- Ensure `adbd` on TCP, same as P0.2.
- **Fallback if Cuttlefish-under-QEMU proves impractical on Windows:** AOSP GSI x86_64, then own AOSP builds. Decide within this phase — do not let it drift into P3.

### Exit criteria
Install an APK by dragging it in, see it launch, read its logcat, pull a file it wrote, restart the instance, and get a clear error when dropping an ARM-only APK — on an **API 36** image.

---

## P2 — Dev-Tool Value (2–3 wk)

**Goal:** the features that make this better than "a VM with Android in it."

**P2.1 — Snapshot / restore / factory reset**
- Disk model: immutable `base.qcow2` + per-instance overlay.
  - `qemu-img create -f qcow2 -b base.qcow2 -F qcow2 instance.qcow2`
  - Factory reset = delete and recreate the overlay. Instant, robust.
  - Named snapshot = copy the overlay (VM stopped).
- **Fast-boot enhancement:** QEMU `savevm`/`-loadvm` captures RAM state → sub-second resume. Evaluate here; the overlay model is the reliable fallback.

**P2.2 — Multi-instance manager**
- Run N instances concurrently — at v1 that means different profiles/resolutions/states on the one API 36 image; the API-level matrix arrives with v1.1 images. Build for N images regardless.
- Sidebar list with per-instance status, RAM, ADB port.
- Tabbed or windowed video panes.
- Enforce a host-RAM guard rail before starting instance N+1.

**P2.3 — ADB endpoint surfaced**
- Show `127.0.0.1:<port>` prominently, one-click copy, so Android Studio / Gradle / `adb connect` can attach.
- Optional: auto-register with the host's ADB server so instances appear in `adb devices` without manual connect.

**P2.4 — `mtp-cli` (headless / CI)**
- `mtplayer start --profile pixel-api34 --headless`
- `mtplayer install <apk> --instance <id>`
- `mtplayer logcat --instance <id> --filter <tag>`
- `mtplayer snapshot|restore|stop`
- Exit codes meaningful for CI.

### Exit criteria
Three instances running at once on different profiles; snapshot, break, restore; drive the whole thing from the CLI without opening the UI.

---

## P3 — Shippable (2–3 wk)

**P3.1 — Installer**
- Signed MSI or NSIS. Bundles: `mtplayer.exe`, QEMU binaries (as separate executables — GPLv2 boundary), `scrcpy-server.jar`, ffmpeg DLLs, NOTICE/license files for every bundled component.
- **Do not vendor guest images** in the installer (~2–4 GB each) — download on first run.

**P3.2 — First-run diagnostics**
Actionable, not just a failure:
- VT-x/AMD-V disabled → "Enable virtualization in BIOS/UEFI" with vendor-specific hints.
- `HypervisorPlatform` off → offer the elevated enable command + reboot prompt.
- Insufficient RAM/disk → state the requirement and what's available.
- Anti-virus blocking QEMU → known-issue link.

**P3.3 — Image manager**
- Ship **API 36 (Android 16)** only. API 34, then 33, at v1.1 — the manager must still be written to handle N images, just seeded with one.
- Hosted on **GitHub Releases** (2 GiB/file, 1000 assets/release, no total-size or bandwidth cap). GitHub Pages is not usable — 1 GB site limit.
- **Ship the system image only; create the empty data overlay client-side** via `qemu-img create`. Keeps each asset under 2 GiB with zstd and avoids multi-part downloads.
- If an asset ever exceeds 2 GiB: split to `.partNN`, reassemble after download.
- Download with progress, SHA-256 verification per asset, resumable.
- GHCR is the secondary option if Releases becomes awkward — shared with Provider B, which pulls redroid from a registry anyway.

**P3.4 — Docs**
- README with the limitations from the overview stated up front, quick start, CI usage, troubleshooting.

### Exit criteria
Clean Windows 11 VM → install → first run → guided through enabling WHPX → image downloads → device boots → APK installs. No manual steps outside the app.

---

## P4 — Provider B: WSL2 + redroid (2–4 wk, opt-in)

**Goal:** satisfy R3's WSL path, and give near-instant boot plus very cheap N-instance testing.

**Blocker to handle honestly:** redroid needs `CONFIG_ANDROID_BINDERFS`, `CONFIG_ASHMEM` (+`CONFIG_STAGING`), `CONFIG_DMABUF_HEAPS`. The stock Microsoft WSL2 kernel has none of them. Setting `kernel=` in `.wslconfig` is **machine-global** — it replaces the kernel for every WSL distro on the machine.

**P4.1** — Build a WSL2 kernel from `microsoft/WSL2-Linux-Kernel` (matching the user's WSL version) with the required configs; ship the prebuilt `bzImage`.
**P4.2** — Consent flow: explain the global `.wslconfig` change in plain language, back up the existing config, offer one-click revert. **Never do this silently.**
**P4.3** — `mtp-provider-wsl`: manage redroid containers, map ADB ports, implement the same `DeviceProvider` trait. The shell should require zero changes — that's the test of the P1 abstraction.
**P4.4** — Guest GPU via `/dev/dxg` GPU-PV + Mesa D3D12; fall back to software.

### Exit criteria
Toggle backend in settings; identical shell behavior; boot <10 s; five instances on 16 GB RAM.

### Explicitly rejected
**Cuttlefish inside WSL2.** Nested virt works (`nestedVirtualization=true`), but GPU passthrough to nested VMs is impossible (VFIO/IOMMU unexposed) and AMD SVM/NPT isn't exposed to WSL2 at all.

---

## P5 — Guest GPU (post-v1, conditional)

**Only if profiling shows software GL is the actual bottleneck.** Under R1 it may never be worth it.

- Guest-side virgl or gfxstream driver; host-side renderer on ANGLE/D3D12.
- gfxstream is preferred over virgl: it's what Google's own emulator uses and has the documented Windows/ANGLE layering story, plus QEMU integration via `rutabaga_gfx`.
- Note this changes the display path away from scrcpy for accelerated instances — a significant architectural fork. Do not start it without a measured reason.

---

## Sequencing Notes

- **P0 is a spike.** Expect to throw most of it away. Its output is knowledge (does WHPX behave?), not code.
- **P1.1's trait is the contract that makes P4 cheap.** Get it right before writing provider-specific code elsewhere.
- **P2.1 (snapshot) is the single most valuable dev-tool feature.** If schedule pressure hits, cut P2.4 (CLI) before P2.1.
- **P4 is not required for v1.** Provider A satisfies R3 on any machine that can run WSL, since both need the same underlying hypervisor.
