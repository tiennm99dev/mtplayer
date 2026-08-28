---
title: "Phase 2: Workspace, Host Preflight, VM Launch Harness"
status: todo
phase: 2
priority: P1
effort: "3-5 d"
dependencies: [1]
---

# Phase 2: Workspace, Host Preflight, VM Launch Harness

## Overview

Stand up the Cargo workspace and the two things every later phase depends on: a **host capability check** and a **repeatable way to launch a guest** with the VMM chosen in phase 01.

**Bliss OS is gone from this phase.** It capped at Android 13 with no path to any current API level, and phase 01 now spikes on the actual v1 image (API 33). One image, sourced once, used from phase 01 through v1 — so a success here proves something about what ships.

## Requirements

- [ ] Functional: `cargo check --workspace` passes with the **seven** crates this phase populates
- [ ] Functional: preflight reports VT-x/AMD-V state, `HypervisorPlatform` state, free RAM, free disk
- [ ] Functional: one command launches the phase-01 image and yields a reachable ADB endpoint
- [ ] Functional: **the endpoint is `127.0.0.1` only** (S2), verified negatively from a second machine
- [ ] Non-functional: preflight is pure detection — never mutates host state without an explicit call
- [ ] Non-functional: **no dependency on `adb.exe`** anywhere in this phase or any later one

## Architecture

`mtp-core` owns host detection and the launch abstraction from the start, so phase 06 promotes rather than rewrites.

```
mtp-core::preflight  ->  HostCapabilities { virt_enabled, whpx_feature, ram_mb, disk_mb }
mtp-core::launch     ->  VmmArgs -> Child + adb_endpoint (loopback)
```

**Seven crates, not eight.** The original plan required "all eight crates present as stubs" and enumerated seven — `mtp-provider-wsl` does not exist until phase 17, so a workspace declaring eight members with seven directories cannot `cargo check`. Create exactly what this phase populates:

`mtp-core`, `mtp-adb`, `mtp-scrcpy`, `mtp-video`, `mtp-provider-vm`, `mtp-cli`, `mtp-ui`.

`mtp-provider-wsl` is added at phase 17; `mtp-render` at phase 21 and only if phase 20 says proceed.

**Detection uses documented Win32 APIs**, not parsed PowerShell text output. `IsProcessorFeaturePresent` and the hypervisor-present CPUID bit cover the CPU side; the optional-feature state needs an API or registry read.

**QEMU baseline — start from upstream's documented form.** The original plan asserted `kernel-irqchip=off` as "a known WHPX requirement." QEMU documents it as a debugging option not expected to be set in normal operation, it is a `-M` property rather than an `-accel` sub-option, and there is an open QEMU bug hanging WHPX SMP boot with that flag on Windows 11 build 26200.x. Phase 01 pins a known-good QEMU version and records the working argv; this phase renders exactly that, with `-smp` as a profile value rather than a hardcoded 4.

## Related Code Files

- Create: `Cargo.toml` — workspace root, resolver 2, shared `[workspace.lints]`, `rust-toolchain.toml`
- Create: `crates/mtp-core/src/lib.rs`, `src/preflight.rs`, `src/launch.rs`
- Create: `crates/mtp-adb/`, `crates/mtp-scrcpy/`, `crates/mtp-video/`, `crates/mtp-provider-vm/`, `crates/mtp-cli/`, `crates/mtp-ui/` — stubs with `lib.rs`/`main.rs` only
- Create: `assets/NOTICE` — start third-party attribution now, not at phase 15
- Create: `.github/workflows/ci.yml` — `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` on `windows-latest`
- Modify: `README.md` — replace the placeholder with the Known Limitations from `plan.md`, stated up front

## Implementation Steps

1. **Workspace skeleton.** Root `Cargo.toml` with the seven members. Pin the toolchain in `rust-toolchain.toml`. Set `[workspace.lints]` once rather than per crate.
2. **`mtp-core::preflight`.** Return a `HostCapabilities` struct, never a bool: firmware virtualization enabled, `HypervisorPlatform` present/enabled, physical RAM total and available, free disk on the image volume. Each field carries enough detail for phase 16 to write an actionable message.
3. **Separate enable from detect.** A distinct `enable_whpx()` running `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All` elevated, reporting "reboot required". Never called implicitly by detection.
4. **`mtp-core::launch`.** A `VmmArgs` struct (memory, cores, disk path, forwarded ADB port, display off) rendering to the phase-01 VMM's argv. Keep VMM-specific rendering behind one function so phase 06 can move it into `mtp-provider-vm` without touching callers. Use the argv phase 01 recorded as working — do not reintroduce flags it found unnecessary or harmful.
5. **Port allocation — hold, do not release.** Bind `127.0.0.1:0`, read the assigned port, and **keep the socket open until the VMM has bound it**, then close. The original bind-then-release-then-hand-over pattern is a TOCTOU: another process can take the port in between, and phase 12 showed port reuse across instances causes a logcat pane to reattach to the wrong guest. Always bind the forward to `127.0.0.1` (S2).
6. **Verify the endpoint.** Connect with `mtp-adb` once it exists (phase 03); until then, a raw TCP connect plus phase 01's harness is enough to confirm reachability. **Also verify the port is not reachable from a second machine** — S2 is a negative test, not an assumption.
7. **CI.** Format, clippy with warnings denied, tests on `windows-latest`. Guest-booting tests are explicitly out of CI scope: hosted runners do not offer nested virtualization. *(This is stated as fact in three phases and is worth confirming once, since platform limits change.)*

## Success Criteria

- [ ] `cargo check --workspace` and `cargo clippy --workspace -- -D warnings` both clean with seven members
- [ ] No stub crate for `mtp-provider-wsl` or `mtp-render` exists yet
- [ ] `preflight` correctly reports all four capability fields on the dev machine
- [ ] `preflight` reports honestly with `HypervisorPlatform` disabled (test by disabling it once)
- [ ] `preflight` never changes host state; `enable_whpx()` is the only mutating path
- [ ] One call to the launch harness boots the phase-01 API 33 image and yields a reachable endpoint
- [ ] The endpoint is bound to `127.0.0.1` and is **not reachable from a second machine on the LAN**
- [ ] Two consecutive launches get different ports and do not collide; the port is held until the VMM binds it
- [ ] Nothing in this phase requires `adb.exe` to be installed
- [ ] CI green on `windows-latest`

## Risk Assessment

**Risk: preflight detection differs across Windows builds or between Intel and AMD.**
Signal: false negatives on a second test machine.
Response: prefer documented Win32 APIs over text parsing; record which machines it was verified on. Phase 16 hardens this with real diagnostics and a documented override — a false negative that hard-blocks the app is worse than a failed start attempt.

**Risk: the phase-01 argv does not generalize** (worked once, by hand, on one machine).
Signal: launches succeed for the developer and fail elsewhere.
Response: phase 01 pins the VMM version; pin it here too and record it in the launch harness. If `-smp` above 1 is unstable (see the open QEMU WHPX bug), cap it and surface the cap in phase 10's default profiles rather than letting users discover it.

**Risk: workspace stubs invite premature structure.**
Signal: stub crates accumulating speculative modules and traits.
Response: stubs stay empty except `lib.rs`. The `DeviceProvider` trait is deliberately phase 06 work, after phases 03-05 have shown what a provider needs. The same reasoning is why the crate list is now minimal rather than aspirational.

**Risk: the "hosted runners lack nested virtualization" assumption is stale.**
Signal: it is asserted in phases 02, 13, and 16 and was never verified.
Response: confirm it once here. If hosted runners *can* do it, phase 13's self-hosted-only CI story becomes unnecessarily restrictive and should be relaxed.
