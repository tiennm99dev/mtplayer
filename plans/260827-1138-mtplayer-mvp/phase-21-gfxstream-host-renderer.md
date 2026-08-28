---
title: "Phase 21: gfxstream Host Renderer on ANGLE/D3D12"
status: todo
phase: 21
priority: P3
effort: "4-8 wk (highest-variance phase in the plan)"
dependencies: [20]
---

# Phase 21: gfxstream Host Renderer on ANGLE/D3D12

# ⚠️ GATED PHASE — DO NOT START WITHOUT A PHASE-20 PROCEED DECISION

## Overview

Hardware-accelerated guest rendering for Provider A: a gfxstream guest driver talking to a host-side renderer on ANGLE/D3D12. This is the largest and riskiest phase in the plan, and **it permanently forks the display path away from scrcpy** for accelerated instances.

**Phase 20 must have recorded an explicit proceed decision with target numbers.** An inconclusive phase 20 means this phase is closed. If you are reading this without that decision in hand, stop.

## Requirements

- [ ] **Precondition: phase 20 recorded a proceed decision naming specific workloads and stage numbers**
- [ ] Functional: gfxstream guest driver in the image; host renderer on ANGLE/D3D12
- [ ] Functional: accelerated instances hit the phase-20 target on the phase-20 workloads
- [ ] Functional: per-instance opt-in; the scrcpy path remains the default and stays fully working
- [ ] Functional: fallback to the scrcpy path on any renderer failure, without losing the instance
- [ ] Non-functional: no regression to phase 05's display path on any measured workload
- [ ] Non-functional: the two display paths are separated well enough to be maintainable

## Architecture

**gfxstream over virgl** — it is what Google's own emulator uses, it has a documented Windows/ANGLE layering story, and QEMU integration exists via `rutabaga_gfx`. If phase 01 chose crosvm, gfxstream is first-class there.

**Caveat the original plan missed:** QEMU's rutabaga device requires an external `rutabaga_gfx_ffi` pkg-config dependency, so **stock prebuilt Windows QEMU almost certainly lacks it.** On the QEMU branch this phase therefore requires a self-built QEMU on Windows — which phase 15 may already require for GPLv2 source compliance, but it compounds that cost and re-opens the distribution posture. Confirm before estimating.

```
guest Android app
      │  GLES / Vulkan
gfxstream guest driver  (in the guest image — phase 07's pipeline must add it)
      │  command stream over virtio-gpu
host: rutabaga_gfx / gfxstream backend
      │
   ANGLE  ->  D3D12  ->  GPU
      │
   rendered texture ── presented DIRECTLY in the MTPlayer window
                       (NOT encoded, NOT via scrcpy)
```

### The architectural fork — the reason for the gate

| | scrcpy path (default) | gfxstream path (this phase) |
|---|---|---|
| Guest render | software GL | hardware via gfxstream |
| Transport | H.264 over ADB | virtio-gpu command stream |
| Host display | decode -> texture | rendered texture directly |
| Works with Provider B | yes | no |
| Input | scrcpy control protocol | still scrcpy control, or virtio-input |
| Screen recording | free (H.264 already) | must be rebuilt |

**Two display paths must coexist.** The scrcpy path stays default and must not regress: it is the only one that works with Provider B, with physical devices, and on machines where the renderer fails. Features built on it — recording, resolution control, and phase 12's pause-on-hide — either need reimplementing for the new path or must be documented as unavailable there.

**This is a genuine maintenance burden, not a temporary state.** It is exactly why phase 20 gates it.

### Guest-side implications

The gfxstream guest driver must be in the image, which means **phase 07's pipeline changes**: either build AOSP with gfxstream guest support or add it to an existing image. Verify what the phase-07 API 33 image already ships before assuming work is needed. **This depends on phase 01's artifact choice:** a Cuttlefish image plausibly includes it (Cuttlefish is gfxstream's primary consumer), but a **GSI fallback almost certainly does not** — which would push this into custom-AOSP territory and materially change the estimate.

## Related Code Files

- Create: `crates/mtp-render/src/lib.rs` — host renderer: virtio-gpu/rutabaga bridge, ANGLE/D3D12 context
- Create: `crates/mtp-render/src/present.rs` — rendered texture -> wgpu/egui presentation
- Create: `crates/mtp-render/src/fallback.rs` — failure detection and switch back to the scrcpy path
- Create: `crates/mtp-ui/src/display_path_badge.rs` — which path an instance is using, and why
- Create: `docs/display-paths.md` — the two paths, their feature differences, and what is unavailable where
- Modify: `crates/mtp-provider-vm/src/lib.rs` — virtio-gpu/gfxstream device configuration when enabled
- Modify: `tools/image-pipeline/` — ensure the gfxstream guest driver is present (verify first)
- Modify: `crates/mtp-core/src/provider.rs` — **reuse or supersede phase 19's `RenderPath` on `InstanceHandle`; do not introduce a parallel display-path concept.** Two overlapping hardware-vs-software indicators would surface as two badges whose semantics differ ("Software" in phase 19 means llvmpipe in a container; "scrcpy path" here means software guest render *plus* H.264 encode)
- Modify: `crates/mtp-core/src/profile.rs` — add the display-path selection field to `DeviceProfile` **with `#[serde(default)]`**, per phase 10's rule that every field carries a default; add it to phase 10's documented schema
- Modify: `plans/260827-1138-mtplayer-mvp/plan.md` — `mtp-render` is listed in the repo layout as conditional-on-phase-20; confirm it is added to the workspace only now
- Modify: `crates/mtp-ui/src/tabs.rs` — route panes to either display path

## Implementation Steps

1. **Re-read phase 20's `RESULTS.md` and confirm the proceed decision and its target.** If absent or inconclusive, stop here and close this phase.
2. **Check what the guest image already provides.** Inspect the phase-07 API 33 image for a gfxstream guest driver. If phase 01 chose Cuttlefish it is plausibly already there; **if phase 01 fell back to GSI it almost certainly is not.** Confirm before estimating — this is the difference between a large slice of the phase evaporating and a custom AOSP build entering scope.
3. **Spike the host side minimally.** Get one triangle from the guest onto the host through gfxstream on ANGLE/D3D12 before writing any integration. **Time-box this spike to 2 weeks** — if a triangle does not render in that time, the phase is not viable at the estimated cost and should be re-gated.
4. **Renderer crate.** Own the virtio-gpu/rutabaga bridge and the ANGLE/D3D12 context. Keep it entirely separate from `mtp-video` — mixing the two paths in one crate is how both become unmaintainable.
5. **Presentation.** Deliver the rendered texture into the existing egui pane. Where possible share the target with wgpu to avoid a copy; measure whether a copy actually matters before optimizing it away.
6. **Per-instance opt-in.** Default off. A profile flag selects it. The scrcpy path remains untouched for every other instance.
7. **Fallback.** Any renderer initialization or runtime failure switches that instance to the scrcpy path, keeps it running, and shows why in the badge. **A renderer failure must never lose an instance.**
8. **Input.** Reuse the scrcpy control channel if it can stay connected alongside; otherwise implement virtio-input. Prefer reuse — reimplementing input is a whole extra source of bugs.
9. **Feature parity audit.** Recording, resolution change, and pause-on-hide against the new path. Reimplement or document as unavailable — do not let them silently break.
10. **Measure against phase 20's targets** on phase 20's workloads. Also re-measure the scrcpy path to prove no regression.
11. **Document both paths** in `docs/display-paths.md`, including which features work where.

## Success Criteria

- [ ] Phase 20's proceed decision confirmed present with target numbers before any code
- [ ] Guest-image gfxstream driver situation established (already present, or added with the work recorded)
- [ ] Triangle spike renders guest -> host through ANGLE/D3D12 within the 2-week box
- [ ] Accelerated instances meet phase 20's targets on phase 20's workloads
- [ ] **Zero regression** to the scrcpy path — re-measured, not assumed
- [ ] Per-instance opt-in; default remains the scrcpy path
- [ ] Renderer failure falls back cleanly; the instance keeps running; the badge explains why
- [ ] Input works on the accelerated path
- [ ] Recording, resolution change, and pause-on-hide either work or are documented as unavailable
- [ ] Provider B continues to work unchanged on the scrcpy path
- [ ] `docs/display-paths.md` states the feature differences plainly
- [ ] The two paths are separable — deleting `mtp-render` leaves a working application

## Risk Assessment

**Risk: the phase costs far more than estimated.**
Signal: the 2-week triangle spike does not produce a triangle.
Response (pre-decided): **stop and re-gate.** 4-8 weeks is the estimate; gfxstream host integration on Windows is genuinely hard and this estimate has the widest error bars in the plan. Hitting the spike box without a triangle means the estimate was wrong by enough to invalidate phase 20's cost/benefit — go back to that decision rather than continuing on momentum.

**Risk: two display paths become an unmaintainable burden.**
Signal: bugs needing fixing twice; features shipping for one path only.
Response: the separability criterion — deleting `mtp-render` must leave a working app — keeps the fork contained and the retreat available. Documented feature differences beat silently divergent behavior. If the burden proves excessive, **removing this path later is a legitimate outcome**; build it so that stays possible.

**Risk: the scrcpy path regresses while attention is on the new one.**
Signal: fps or latency degradation on the default path.
Response: re-measure the scrcpy path against phase 05's and phase 20's baselines as a success criterion. The default path serves every Provider B user and every fallback case — it cannot be allowed to rot.

**Risk: ANGLE/D3D12 behaves inconsistently across GPU vendors.**
Signal: works on one vendor, fails or corrupts on another.
Response: the fallback path is the mitigation, and it must be robust rather than best-effort. Record which vendors were verified; treat unverified vendors as fallback-by-default rather than shipping speculative acceleration.

**Risk: guest driver work reopens the phase-07 image pipeline.**
Signal: needing a custom AOSP build to get the gfxstream guest driver.
Response: step 2 checks first — Cuttlefish likely already ships it. If a custom build is genuinely required, that is a significant addition to the estimate and should return to phase 20's cost/benefit rather than being absorbed.

**Risk: this phase is started because it is interesting rather than because it is needed.**
Signal: work beginning without phase 20's recorded decision.
Response: the gate banner at the top of this file, and step 1. Under R1 this phase may correctly never happen — MTPlayer is a dev tool, and a dev tool that renders a UI at 30 fps is fine.
