---
title: "Phase 19: WSL Guest GPU via /dev/dxg"
status: todo
phase: 19
priority: P3
effort: "1-2 wk"
dependencies: [18]
---

# Phase 19: WSL Guest GPU via /dev/dxg

## Overview

Give Provider B's containers hardware-accelerated rendering through WSL2's GPU paravirtualization (`/dev/dxg`) with Mesa's D3D12 driver, falling back to software when unavailable. This is the cheapest hardware acceleration available anywhere in the plan — WSL already exposes the GPU, so no host-side renderer is needed.

**Note the contrast with phase 21:** this phase reuses infrastructure Microsoft already ships. Phase 21 builds a host-side renderer from scratch and is gated behind a profiling check for that reason.

## Requirements

- [ ] Functional: redroid containers get `/dev/dxg` access
- [ ] Functional: Mesa D3D12 available in the guest; Android's GL/Vulkan uses it
- [ ] Functional: automatic fallback to software rendering when GPU-PV is unavailable
- [ ] Functional: current rendering path (hardware/software) visible in the UI
- [ ] Non-functional: measured improvement over software rendering, or the phase is honestly reported as no-benefit
- [ ] Non-functional: a GPU failure never prevents an instance from starting

## Architecture

```
Windows GPU + WDDM driver
        │  GPU-PV
   /dev/dxg  (exposed by WSL2)
        │
   container: --device /dev/dxg + /usr/lib/wsl/lib mounted
        │
   Mesa D3D12 gallium driver (d3d12)
        │
   Android EGL/GLES in redroid
        │
   frames still leave via scrcpy H.264 over ADB  (display path UNCHANGED)
```

**The display path does not change.** Acceleration affects how the guest *renders*; frames still reach MTPlayer through scrcpy. That is what keeps this phase contained — and it is precisely the property phase 21 gives up.

**Requirements on the host side:** a WDDM 2.9+ driver with GPU-PV support, and WSL's user-mode libraries (`/usr/lib/wsl/lib`, containing `libd3d12.so` and the vendor `libdxcore`) visible inside the container. Both are outside MTPlayer's control, so detection and fallback matter more than the happy path.

**Encoding is a second, separate question.** If the GPU exposes a hardware H.264 encoder reachable from the guest, scrcpy's encode could be accelerated too — but that depends on redroid's media stack and the driver, and it is not the primary goal here. Note it, measure it if cheap, do not let it expand the phase.

## Related Code Files

- Create: `crates/mtp-provider-wsl/src/gpu.rs` — GPU-PV detection, device/mount wiring, capability report
- Create: `crates/mtp-provider-wsl/tests/gpu_detect.rs` — detection over fixtures
- Modify: `crates/mtp-provider-wsl/src/container.rs` — pass the device and mounts when available
- Modify: `crates/mtp-core/src/provider.rs` — a `RenderPath { Hardware, Software }` field on **`InstanceHandle`**, not on `InstanceStatus`. Phase 06 defines `InstanceStatus` as an enum, so a "field on the instance status" is not expressible; phase 21 must reuse or supersede this `RenderPath` rather than introducing a parallel display-path concept
- Modify: `crates/mtp-ui/src/sidebar.rs` — render-path badge per instance
- Modify: `docs/provider-b.md` — GPU requirements, how to verify, known-bad configurations

## Implementation Steps

1. **Detect GPU-PV.** Inside the distro, check `/dev/dxg` exists and `/usr/lib/wsl/lib` is populated. Record the host GPU vendor and driver version for the phase-16 diagnostics report.
2. **Wire the container.** Pass `--device /dev/dxg` and mount the WSL library directory. Set the loader path so Mesa finds the D3D12 driver.
3. **Verify Mesa picks D3D12.** Inside the container, confirm the renderer string reports the D3D12 gallium driver and the actual GPU, not `llvmpipe`. **This check is the whole phase's ground truth** — everything downstream is meaningless if Mesa silently fell back to software.
4. **Confirm Android uses it.** Check the guest's EGL/GLES vendor and renderer strings via `dumpsys SurfaceFlinger` or a GL info app. redroid's own GL configuration may need a property set — verify rather than assuming the container wiring is sufficient.
5. **Fallback path.** No `/dev/dxg`, missing libraries, or `llvmpipe` detected -> software rendering, instance still starts, badge shows Software. **A GPU problem must never block a boot.**
6. **Measure.** Compare hardware vs software on the same instance and profile: a GPU-bound test app's frame rate, UI smoothness scrolling a list, and boot time. Record real numbers. If there is no measurable improvement, **say so** — that is a legitimate finding and it informs phase 20.
7. **Render-path badge.** Per instance in the sidebar, with a tooltip explaining why software was selected when it was.
8. **Diagnostics integration.** GPU vendor, driver version, `/dev/dxg` presence, and the Mesa renderer string into the phase-16 report — these are the first things anyone debugging a graphics issue needs.
9. **Document known-bad configurations** as they are found, in `docs/provider-b.md`.

## Success Criteria

- [ ] `/dev/dxg` detection correct on a GPU-PV-capable machine and on one without it
- [ ] Container gets the device and mounts; Mesa reports the **D3D12 driver and the real GPU**, not `llvmpipe`
- [ ] Android's EGL/GLES strings confirm hardware rendering inside the guest
- [ ] Software fallback works: instance starts, badge shows Software, tooltip explains why
- [ ] Removing GPU access mid-configuration does not prevent an instance from starting
- [ ] Hardware vs software comparison measured and recorded with real numbers — including a negative result if that is the outcome
- [ ] Render-path badge accurate per instance
- [ ] GPU details present in the phase-16 diagnostics report
- [ ] Detection tests pass in CI over fixtures
- [ ] **Record explicitly which GPU vendors and driver versions were tested**, and mark every untested vendor as fallback-by-default rather than shipping speculative acceleration. *(The original criterion — "two vendors if hardware is available; if not, state which was tested" — passed unconditionally and asserted nothing.)*

## Risk Assessment

**Risk: Mesa silently falls back to `llvmpipe` and the phase appears to work.**
Signal: no measurable improvement, renderer string shows `llvmpipe`.
Response: step 3 checks the renderer string explicitly, and it is a success criterion. Never infer acceleration from a subjective impression of smoothness.

**Risk: redroid's GL stack does not use Mesa D3D12 even when it is present.**
Signal: Mesa reports D3D12 in the container, but Android still renders in software.
Response: step 4's guest-side check catches the gap. redroid may need specific properties set; if it cannot be made to use the driver, report the finding rather than claiming acceleration.

**Risk: GPU-PV varies by vendor and driver, so it works on the dev machine and nowhere else.**
Signal: users on other GPUs reporting software fallback.
Response: robust detection plus a graceful, *visible* fallback. Record which vendors and drivers were verified. Partial support with an accurate badge is an acceptable outcome; silent inconsistency is not.

**Risk: a GPU issue prevents instances from starting.**
Signal: instances failing to boot on machines without GPU-PV.
Response: hard requirement in step 5 — GPU is an enhancement and never a prerequisite. Explicitly tested above.

**Risk: scope creep into guest media/encoder acceleration.**
Signal: effort going into hardware H.264 encode inside redroid.
Response: out of scope. Note the possibility, measure only if it is nearly free, and leave it for a future phase.

**Risk: this phase's success is read as validating phase 21.**
Signal: "GPU acceleration worked for Provider B, so let us do it for Provider A."
Response: they are not comparable. This phase reuses Microsoft's existing GPU-PV path and keeps the scrcpy display path intact; phase 21 builds a host renderer and forks the display path. **Phase 20's measurement still gates phase 21** regardless of how well this phase goes.
