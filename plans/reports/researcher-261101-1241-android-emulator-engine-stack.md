# Research: tech stack for building a WHPX-based Android emulator engine on Windows

Date: 2026-11-01 · Host target: Windows 11 Pro x86_64 (miti-b550) · Scope: the VMM, guest image, graphics, I/O and effort. Root, spoofing, ARM translation and GMS are covered by the other researcher.

## 1. Outcome (read this first)

**Recommendation: do not write a VMM. Build the engine as the Google Android Emulator (AEMU) running on WHPX, with gfxstream graphics, your own AOSP `sdk_phone_x86_64` userdebug images, and your own launcher and instance manager.** Roll it out in two steps.

1. **Step 0 (weeks).** Run the unmodified Google emulator binary with WHPX, your own AOSP images, and your own launcher, which drives the CLI, gRPC and adb. The binary is free for commercial app development under the Android SDK License. It already coexists with Hyper-V, WSL2 and Docker, and it already supports multiple instances, Quick Boot snapshots and qcow2 overlays. This gives you a working MuMu replacement before you change a line of engine code.
2. **Step 1 (months, only if Step 0 is missing something you need).** Fork the open-source emulator source (`platform/external/qemu`, GPLv2 for the QEMU parts plus Apache-2.0 components) when you need engine changes, for example deeper device spoofing or custom virtual sensors. Rebrand it, and avoid the Android trademark in the product name.

**Main caveat.** Google is moving the emulator from its QEMU 2.12 fork to upstream QEMU 11+, the "next-generation engine" (preview, API 37+ only). A fork of the classic engine made today would be a fork of a codebase Google is retiring. Fork late and stay as close to upstream as you can.

Ranking of VMM choices:
1. **AEMU (Google emulator).** It is the only option with production gfxstream, goldfish devices, snapshots and multi-instance support on WHPX today.
2. **Upstream QEMU 11 + WHPX + rutabaga/gfxstream.** This is the future direction, but no documented Windows-host gfxstream path exists yet.
3. **crosvm on Windows.** Google ships it for Play Games on PC, but the open-source Windows/WHPX path is officially "downstream, may not be functional".
4. **Cloud Hypervisor / Firecracker.** Not viable here, because they have no WHPX backend and no Windows host support.

## 2. VMM options on Windows with WHPX

### 2.1 Trade-off matrix

| Criterion | AEMU (Google emulator) | Upstream QEMU 11 + WHPX | crosvm (Windows) | Cloud Hypervisor |
|---|---|---|---|---|
| WHPX backend | Yes, and Google's recommended path; AEHD sunsets 2026-12-31 | Yes, maintained, documented | Yes (`whpx` feature) | No (KVM/MSHV only) |
| Coexists with Hyper-V/WSL2/Docker | Yes | Yes | Yes | n/a |
| Android 3D (GLES/Vulkan) on Windows host | gfxstream, production | rutabaga/gfxstream upstream, but build guides are Linux-only | gfxstream feature exists; Windows runtime unverified | n/a |
| Android-specific devices (goldfish pipe, sensors, battery, modem, GNSS) | Yes, full | No (goldfish devices are not upstream) | Partial (virtio-centric) | n/a |
| Snapshots / Quick Boot | Yes, mature (RAM file-backed, CoW shared) | Generic savevm, no Android integration | Limited | n/a |
| Multi-instance tooling | Yes (`-read-only` instances, gRPC, adb ports) | Build yourself | Build yourself | n/a |
| Builds on Windows | Officially Linux/Darwin plus `--mingw` cross-build (see 2.2) | MSYS2/mingw, documented | Native MSVC via `cargo build --features all-msvc64,whpx` | n/a |
| License | GPLv2 (QEMU) + Apache-2.0 (android/, gfxstream) | GPLv2 | BSD-3 | Apache/BSD |
| Abandonment risk | Classic engine is being replaced by the QEMU 11 engine | Low | Medium: the Windows path is driven by Google's internal product | n/a |

### 2.2 Evidence per option

**QEMU WHPX.** The current docs (QEMU 11.1.50) say WHPX is tested on Windows 10 2004+ and also covers arm64 hosts. The documented gaps are:
- slow legacy VGA modes;
- MMIO through MMX/SSE/AVX instructions fails to decode;
- on Windows 10, the PIC/HLT wake bug forces the Hyper-V interrupt controller off;
- `-accel whpx,ssd=off` gives a "significant MMIO performance increase" at a security cost.

A 2025 issue reports that x86-64-v3 CPU features were not exposed under WHPX in QEMU 9.2. That matters for Android 16+/17 images built for newer ISA levels (unverified for current QEMU).
Sources: https://www.qemu.org/docs/master/system/whpx.html, https://gitlab.com/qemu-project/qemu/-/issues/2782

**Google Android Emulator (AEMU).**
- Google recommends WHPX on Windows and states it "works alongside Hyper-V features". AEHD sunsets on 2026-12-31, and HAXM was removed in 36.2.11 (Oct 2025).
- Recent releases: 36.4.9 (Feb 2026) made Lavapipe the default software renderer; 36.5.10 (Apr 2026) added a new multi-device network stack; 37.2.13 (Oct 28, 2026) is the latest patch.
- The next-generation engine runs on upstream QEMU 11+ with the engine and UI in separate processes. It is in preview for API 37+ AVDs only.

Sources: https://developer.android.com/studio/run/emulator-acceleration, https://developer.android.com/studio/releases/emulator

**AEMU source and build.**
- Source: `repo init -u https://android.googlesource.com/platform/manifest -b emu-master-dev`, then `external/qemu/android/rebuild.sh`.
- DEVELOPMENT.md says the scripts "work on Linux and Darwin systems only". Windows binaries are cross-built from Linux with `--mingw`.
- A newer CMake entry point, `android/build/python/cmake.py`, appears in community notes and may support native Windows clang-cl builds. **I could not verify this.** Plan for a Linux build machine that cross-compiles Windows binaries.
- The repo's LICENSE is GPLv2 for QEMU, with compatible per-file licenses.
- **I did not find a public source location for the next-generation (QEMU 11) engine.**

Sources: https://android.googlesource.com/platform/external/qemu/+/emu-master-dev/android/docs/DEVELOPMENT.md, https://android.googlesource.com/platform/external/qemu/+/emu-master-dev/LICENSE

**Forking and rebranding AEMU.**
- The source licenses (GPLv2 and Apache-2.0) allow modification and redistribution. GPL obligations apply only when you distribute binaries. Internal use within one legal entity is generally not distribution, but **confirm with counsel**.
- The *prebuilt* emulator from the SDK falls under the Android SDK License. Section 3.1 grants use "solely to develop applications for compatible implementations of Android", which covers commercial app testing. Section 3.4 forbids modifying or redistributing the SDK, except where open-source component licenses say otherwise. So Step 0 is legal for internal testing, and modifications must come from your own source build.
- Avoid "Android" in the product name (Google brand guidelines; not fetched in this session).

Source: https://developer.android.com/studio/terms

**crosvm on Windows.**
- Builds natively with MSVC (`cargo build --features all-msvc64,whpx`).
- crosvm's own docs say the Windows-specific flags "are specific to downstream projects and may not be functional", and are enabled upstream only for CI compile coverage.
- An upstream commit says the WHPX backend had **bitrotted** and added a clippy-only presubmit. A Cargo `gfxstream_stub` feature "will not be functional at runtime".
- Play Games on PC's main executable is `crosvm.exe` according to SamMobile (2022, single secondary source). Google's own docs only confirm it requires WHPX.
- Conclusion: the open-source tree is not the product Google ships. Expect to rebuild the Windows display, input and GPU glue yourself.

Sources: https://crosvm.dev/book/building_crosvm/windows.html, https://crosvm.dev/doc/crosvm/, https://chromium.googlesource.com/chromiumos/platform/crosvm/+/c59ce5a137126ee434dc0a298ed3b431ff7a93d0%5E%21/, https://www.sammobile.com/news/google-play-games-for-samsung-windows-pcs-now-available-in-select-regions/, https://support.google.com/googleplay/answer/11358888

**Cloud Hypervisor, Firecracker, VirtualBox, HAXM.** Cloud Hypervisor and Firecracker have no WHPX backend. VirtualBox's own driver and HAXM are excluded by your constraint, and HAXM is discontinued. I did not research these further.

## 3. Guest Android images

| Image | Android versions | Expected devices | Build effort | License | Fit |
|---|---|---|---|---|---|
| **AOSP `sdk_phone_x86_64` (goldfish/ranchu)** | Every AOSP release; Android 16 (API 36) and 16 QPR2 are public | goldfish pipe, goldfish address space, goldfish events/battery/RTC, virtio-blk/net/console, virtio-gpu with gfxstream capsets. **Requires AEMU** | Low: standard AOSP lunch target, `m emu_img_zip` (A13+) | Apache-2.0 + GPLv2 kernel; GMS not included | **Best** for AEMU |
| **Cuttlefish `aosp_cf_x86_64_phone`** | API 28+ | All-virtio (blk, net, console, vsock, gpu, input), launched by `cvd`/`launch_cvd` | Medium, and the host tooling is **Linux-only** | Apache-2.0 | Poor on Windows: launcher, crosvm/QEMU config and vsock services assume Linux |
| **Android-x86 / BlissOS** | Android-x86 discontinued; BlissOS 16.9.x = Android 13 (kernel 6.1) | Generic PC hardware, virtio-gpu with **virgl** | Low to use, high to maintain | Apache/GPL; some builds bundle proprietary bits | Weak: old Android, virgl only, no Windows-host 3D path |

Notes:
- **Lunch target name.** The AOSP product file is `sdk_phone_x86_64.mk` (CalyxOS uses `breakfast sdk_phone_x86_64` on Android 16 QPR2). `sdk_phone64_x86_64` exists under `device/generic/goldfish` in recent trees as the 64-bit-only variant. **Check your tree's lunch menu.**
  Sources: https://source.android.com/docs/setup/create/avd, https://calyxos.org/docs/development/build/emulator/, https://android.googlesource.com/platform/build/+/796d90c14183218bbf200da9195ac9c9d7d85623/target/product/sdk_phone_x86_64.mk
- **AOSP source drops.** Since 2026, Google publishes AOSP source only twice a year (Q2 and Q4). `aosp-main` stopped receiving regular updates in March 2025, and you should build from `android-latest-release`. Your images will trail Pixel releases by up to about six months, which is acceptable for app testing.
  Sources: https://www.androidauthority.com/aosp-source-code-schedule-3630018/, https://fosdem.org/2026/events/attachments/SW83YJ-state_of_foss_on_mobile/slides/262166/fosdem26-_6jjfukg.pdf
- **Cuttlefish hosts.** Cuttlefish documents only Linux x86/ARM64 hosts. Source: https://source.android.com/docs/devices/cuttlefish
- **Google's prebuilt images.** The "Google APIs" and "Google Play" images, and GMS GSIs, are licensed for testing only, with no redistribution. Build your own AOSP images for the commercial engine. GMS is the other researcher's area. Source: https://developer.android.com/topic/generic-system-image/releases
- **Build host.** An AOSP build needs a Linux machine (about 400 GB disk and 64 GB RAM recommended). WSL2 on miti-b550 works for this but is slow on NTFS-backed paths. Keep the tree on ext4 inside the WSL2 VHD.

## 4. Graphics

| Option | What it is | Windows-host support | Near-native? | VMM support |
|---|---|---|---|---|
| **gfxstream** | Serializes guest GLES and Vulkan calls to the host. Vulkan 1.3 from API 34 | **Yes.** Builds with CMake + VS2019 clang; needs host `libEGL`/`libGLESv2` (vendor driver or ANGLE) and `vulkan-1.dll` | **Yes.** It is what the Android Emulator and Play Games use | AEMU (production), crosvm (feature), QEMU via rutabaga (Linux-documented) |
| **virgl (virtio-gpu GL)** | Gallium/GL forwarding | Historically Linux; a QEMU issue asks for Windows virgl and remains unresolved | No: GL-only, high overhead | QEMU (Linux), Cuttlefish `drm_virgl` |
| **Venus (virtio-gpu Vulkan)** | Vulkan forwarding via virglrenderer ≥1.0 | Documented setups are Linux hosts (kernel 6.13+, QEMU 9.2+) | Good on Linux; **not proven on Windows** | QEMU, crosvm (Linux) |
| **ANGLE** | GLES on top of D3D11 or Vulkan | Yes, the standard Windows GLES layer | Host-side layer under gfxstream (`-gpu angle_indirect`/`swangle`) | AEMU |
| **SwiftShader / Lavapipe** | CPU software rendering | Yes | **No** (CPU-bound). Lavapipe has been AEMU's default software renderer since 36.4.9 | AEMU, Cuttlefish |

**Verdict.** Only gfxstream on the host GPU driver gives near-native GLES and Vulkan on Windows hosts today. In AEMU that is `-gpu host`, with ANGLE as a fallback for driver bugs. For an NVIDIA/AMD/Intel mix, expect per-vendor quirks. For example, 36.5.11 fixed a black screen on Intel GPUs, and 37.2.13 fixed a GLES host memory leak. Keep a known-good driver matrix.

Sources:
- https://android.googlesource.com/platform/hardware/google/gfxstream/+/refs/heads/main/README.md (gfxstream build and host)
- https://android.googlesource.com/platform/hardware/google/gfxstream/+/refs/heads/main/LICENSE (gfxstream license: Apache-2.0 primary, plus MIT/CC-BY)
- https://github.com/qemu/qemu/blob/master/docs/system/devices/virtio-gpu.rst (QEMU virtio-gpu, rutabaga, experimental `x-gfxstream-gles` capsets)
- https://linaro.atlassian.net/wiki/spaces/ORKO/pages/28985622530/Building+QEMU+with+virtio-gpu+and+rutabaga_gfx (rutabaga build guide, Linux only)
- https://gitlab.com/qemu-project/qemu/-/issues/1461 (Windows virgl request)
- https://github.com/tibixdev/helios (Venus on Linux hosts)
- https://developer.android.com/studio/releases/emulator (emulator release notes)

## 5. Display, input, audio, network, storage, snapshots

**AEMU provides all of these, so Step 0 needs no engine code here.**

- **Display and UI.** The classic engine has a Qt UI with an embedded gfxstream renderer. The next-generation engine splits the UI and engine into separate processes. Headless mode is `-no-window`. Automation uses the gRPC endpoint (screenshots, input, sensors) plus the console port and adb. A custom launcher should drive gRPC and adb rather than embed the window. If you fork, multi-window "instance wall" views are the main UI work.
- **Input.** virtio-input / goldfish events, multi-touch, keyboard, and the gRPC input injection API. A keymapping overlay (MuMu-style game keys) is not in AEMU, so you would build it in the launcher. That is out of scope for app testing unless you need it.
- **Audio.** Goldfish audio with a Windows host backend (WinMM/WASAPI in the QEMU audio layer). Low value for app testing. I did not research it further.
- **Networking.**
  - Default is user-mode NAT (slirp-style) with port forwarding on the console/adb ports (5554/5555, +2 per instance).
  - The new stack in 36.5.10 lets AVDs discover each other on a shared virtual network (Wi-Fi Direct, NSD) and fixes legacy TCP data loss.
  - TAP is possible in QEMU but adds a driver install (OpenVPN TAP-Windows). For app testing behind Docker/WSL2, user-mode is the safer default because it adds no host driver and no conflict with Hyper-V vSwitch.

  Source: https://developer.android.com/studio/run/emulator-networking
- **Storage.** System images stay read-only. Per-AVD userdata and cache are qcow2 overlays over the base images. Cloning an instance means creating a new overlay with `qemu-img create -b`, which takes seconds and needs no full copy.
- **Snapshots and Quick Boot.** Quick Boot RAM snapshots are file-backed. Since emulator 28, `-read-only` lets several instances of one AVD run at once and share the Quick Boot snapshot as a copy-on-write RAM source. This is the key to dense multi-instance (100+ app-test runs from one golden snapshot). That source is 2018-era. The feature still exists, but **re-verify the current flags**.
  Sources: https://androidstudio.googleblog.com/2018/11/emulator-28016-stable.html, https://androidstudio.googleblog.com/2018/07/emulator-2800-canary.html
- **Memory floor.** API 37 AVDs need at least 4 GB guest RAM (36.6.11). That caps instance density, so size host RAM accordingly.

## 6. Performance versus MuMu/LDPlayer (inferred, not benchmarked)

- MuMu/LDPlayer/BlueStacks use their own kernel-mode VMM paths (VirtualBox-derived or proprietary). Under WHPX every VM exit goes through the Windows hypervisor and a user-mode exit handler. MMIO- and PIO-heavy paths are therefore slower than with an in-kernel VMM. QEMU's own docs point to `ssd=off` for this reason.
- Mitigations: virtio devices, a paravirtual clock, the Hyper-V irqchip, and gfxstream so that rendering does not exit per draw call.
- Expect CPU-bound app code to run near-native, and graphics close to host-GPU speed via gfxstream. Boot, I/O and interrupt-heavy workloads will lag MuMu somewhat.
- **I found no 2026 head-to-head benchmark of AEMU+WHPX against MuMu or LDPlayer.** Benchmark it yourself on miti-b550 (boot time, 3DMark Wild Life / GFXBench, an app cold start, `fio` in the guest).
- ARM-only apps need ARM translation (other researcher). Without it, x86_64 images run only x86 or universal APKs, which is often the real performance and compatibility gap with commercial emulators.

## 7. Effort, team size, risks

### 7.1 Effort by approach

| Approach | Team | Time to usable | Ongoing |
|---|---|---|---|
| **Step 0:** stock emulator + own AOSP images + launcher | 1–2 engineers (C#/Python launcher, AOSP build) | 4–8 weeks | Rebuild images per AOSP drop (2 per year); track emulator releases |
| **Step 1:** fork AEMU (custom devices, spoofing hooks, branding) | 3–5 (QEMU/C++, AOSP platform, graphics, launcher) | 4–9 months | High: rebase onto Google's changes, especially the QEMU 11 engine transition |
| Upstream QEMU 11 + port goldfish devices + gfxstream on Windows | 5–8, including graphics/virtualization specialists | 12–24 months | Medium–high |
| Own VMM on crosvm or from scratch | 8–15+ | 2–3+ years | Very high. Google needed a dedicated team for Play Games on PC |

These are estimates by analogy and are not sourced.

### 7.2 Biggest risks (ranked)

1. **Engine transition.** The classic QEMU 2.12 fork is being superseded by the QEMU 11 engine (API 37+ preview). A Step-1 fork made now may need a re-port within about 12–18 months. Mitigation: stay on Step 0 as long as possible, and keep your changes in separable devices and patches.
2. **GPU driver variance on Windows.** gfxstream depends on host GLES/Vulkan drivers or ANGLE, and vendor-specific bugs appear in almost every emulator release. Mitigation: a driver allow-list and ANGLE fallback.
3. **WHPX overhead and gaps.** Exit cost, the MMIO decode limitations, and possibly missing CPU features (x86-64-v3) that newer images may assume. Mitigation: benchmark early; prefer Windows 11 (the irqchip issues are on Windows 10).
4. **Windows build toolchain.** The official AEMU build path is Linux plus mingw cross-compilation, and native Windows builds are unverified. The CI must reproduce Google's prebuilts (`prebuilts/android-emulator-build`).
5. **AOSP cadence and visibility.** Twice-yearly source drops and private development mean images lag, and goldfish changes land in bulk.
6. **Licensing hygiene.** Keep the SDK-licensed prebuilt binary separate from the GPL fork. Do not redistribute Google APIs/Play images. Comply with GPL source obligations if you ever distribute outside the company.
7. **Feature parity with commercial emulators.** ARM translation, GMS, keymapping and anti-detection are not in AEMU. These are the other researcher's scope, but they decide whether developers and testers actually accept the tool.

### 7.3 Is "fork AEMU + AOSP images + own launcher" the practical way?

**Yes.** It is the only path where the hard parts already exist and are maintained by Google on exactly your constraint set: WHPX, a Windows host, gfxstream, goldfish devices, snapshots and multi-instance. Build the launcher first and fork only when a requirement forces it. Writing a VMM or adopting crosvm on Windows means rebuilding what Google keeps in private downstream code.

## 8. Limitations of this research

- No hands-on benchmarks or builds were run on miti-b550, so all performance claims are inferred.
- The next-generation engine's source location and its Windows build path were not found.
- The native Windows build of AEMU (`cmake.py`, clang-cl) is unverified.
- crosvm's use in Play Games on PC rests on one 2022 secondary source.
- Venus/virgl on Windows hosts: no confirming source, so treat them as unsupported.
- Android brand guidelines and the GPL "internal distribution" question need a legal review, which this research did not do.
- Audio and TAP networking on Windows were not researched in depth.

## Unresolved questions

1. Has Google published the next-generation (QEMU 11+) emulator source, and does it build natively on Windows?
2. Does Step 0 (stock emulator + own images) already meet the spoofing and root requirements from the other researcher's findings? If it does, Step 1 may never be needed.
3. What instance density is the target (instances per host, RAM per instance)? This decides whether Quick Boot CoW sharing is essential.
4. Is GPL distribution ever in play (contractors or subsidiaries using the tool), or is it strictly internal?

Status: DONE_WITH_CONCERNS
Summary: Recommend AEMU (Google Android Emulator) on WHPX with gfxstream and self-built AOSP sdk_phone_x86_64 images, starting with the unmodified emulator plus your own launcher and forking the open-source emulator only when required. crosvm-on-Windows, upstream QEMU and Cloud Hypervisor are ranked below it.
Concerns/Blockers: Google is migrating the emulator to an upstream QEMU 11 engine whose source and Windows build path I could not locate; native Windows AEMU builds and WHPX performance versus MuMu are unverified; licensing points need legal review.
