---
title: "Phase 20: GPU Profiling Gate (kill/proceed)"
status: todo
phase: 20
priority: P3
effort: "3-5 d"
dependencies: [16, 19]
---

# Phase 20: GPU Profiling Gate (kill/proceed)

## Overview

**This phase exists to decide whether phase 21 should happen at all.** Guest GPU acceleration for Provider A means building a host-side renderer and forking the display path away from scrcpy — a significant architectural commitment. It is justified only if measurement shows software GL is the actual bottleneck for real dev-tool use.

The deliverable is a measurement and a decision, not code. Under R1 (dev tool, not gaming), the honest answer may well be "not worth it, ever."

## Requirements

- [ ] Functional: a repeatable benchmark suite over representative dev-tool workloads
- [ ] Functional: measurements attributing time to guest render, encode, transport, decode, and display
- [ ] Functional: a written decision — proceed to phase 21, or close it — with the numbers behind it
- [ ] Non-functional: benchmarks are reproducible by someone else on different hardware
- [ ] Non-functional: cheaper alternatives evaluated **before** concluding a host renderer is needed

## Architecture

### What to measure

Only workloads a **dev tool** actually sees. Benchmarking a 3D game would justify phase 21 while telling us nothing about R1.

| Workload | Why |
|---|---|
| App cold start to first frame | The most common dev-loop action |
| Scrolling a `RecyclerView` list | The most common UI interaction |
| Compose/View animation smoothness | Typical modern-app rendering |
| Screen rotation | Full re-layout and re-render |
| Camera-preview or video-playback surface | Known-heavy surfaces, if a dev tool needs them |
| Idle desktop | Baseline cost of doing nothing |

### Where the time goes — attribution is the point

```
guest app render (software GL / SwiftShader)   <- phase 21 addresses ONLY this
        ↓
guest H.264 encode (scrcpy-server)
        ↓
ADB transport
        ↓
host decode (ffmpeg/openh264)
        ↓
host upload + shader + present (wgpu/egui)
```

**A total frame time of 60 ms means nothing on its own.** If 40 ms is encode and transport, phase 21 buys almost nothing. Attribution per stage is the whole value of this phase — measure the stages, not just the end-to-end number.

Guest-side render time is observable via `dumpsys gfxinfo` frame stats and `atrace`/Perfetto. Host-side stages are instrumented in MTPlayer's own pipeline.

### Cheaper alternatives to evaluate first

Before concluding that a host renderer is required, measure the effect of:

1. **scrcpy encoder tuning** — bit rate, max fps, I-frame interval, encoder selection.
2. **Lower resolution or DPI** — a dev tool at 1080p may not need 1440p.
3. **Decoder low-latency flags** — `AV_CODEC_FLAG_LOW_DELAY`, zero-latency tuning.
4. **Phase 12's pause-on-hide** already removing background cost.
5. **SwiftShader vs the guest's default software GL** — sometimes a large difference for no architectural change.

If any of these closes the gap, **phase 21 is closed** and that is a win, not a disappointment.

### Decision rule (pre-committed)

**Proceed to phase 21 only if all three hold:**

1. Guest-side render time is the dominant stage (> ~50% of frame time) on at least two representative workloads, **and**
2. The cheaper alternatives above do not bring interaction into a usable range, **and**
3. The affected workloads are ones a **dev tool** genuinely needs (R1) — not gaming performance.

Otherwise **close phase 21** and record the numbers.

## Related Code Files

- Create: `benches/gpu-profile/README.md` — methodology, reproducible by someone else
- Create: `benches/gpu-profile/workloads/` — the benchmark APKs or scripted interactions
- Create: `benches/gpu-profile/collect.ps1` — run workloads, gather `gfxinfo`/`atrace` and host-side traces
- Create: `benches/gpu-profile/RESULTS.md` — per-stage attribution, alternatives evaluated, the decision
- Modify: `crates/mtp-video/src/lib.rs` — per-stage timing instrumentation behind a feature flag
- Modify: `crates/mtp-scrcpy/src/lib.rs` — timestamp access units on arrival for transport attribution
- Modify: `plans/260827-1138-mtplayer-mvp/plan.md` — record the outcome; mark phase 21 proceeding or closed

## Implementation Steps

1. **Instrument the host pipeline** behind a feature flag: access-unit arrival, decode start/end, upload, present. Emit percentiles, not averages — a p99 stall is what users feel, and averages hide it.
2. **Guest-side capture.** `dumpsys gfxinfo <pkg> framestats` for per-frame render timing; Perfetto/`atrace` when finer detail is needed.
3. **Pick or write workloads.** Prefer small purpose-built APKs over third-party apps so results stay reproducible. Scripted interaction via `input swipe` keeps runs comparable.
4. **Baseline first.** Current v1 configuration, unmodified. Every later number is relative to this.
5. **Attribute per stage** for each workload. Present as a stacked breakdown so the dominant stage is unarguable.
6. **Evaluate the cheaper alternatives** in the listed order, measuring each. Encoder tuning in particular is often worth more than any renderer work.
7. **Apply the decision rule mechanically.** Do not reinterpret the criteria to reach a preferred answer — the rule is pre-committed above precisely because guest GPU work is technically appealing regardless of value.
8. **Write `RESULTS.md`** with methodology, per-stage numbers, alternatives evaluated, and the decision with its rationale.
9. **Update `plan.md`.** If closed: mark phase 21 closed with the reason, so it is not silently revisited later. If proceeding: record which workloads and numbers justify it, so phase 21 has a concrete target rather than "make it faster".

## Success Criteria

- [ ] Per-stage frame-time attribution measured for at least four representative workloads
- [ ] Percentiles (p50/p95/p99) reported, not averages alone
- [ ] Guest render time measured independently via `gfxinfo`, cross-checked against host-side timings
- [ ] All five cheaper alternatives evaluated with recorded before/after numbers
- [ ] `benches/gpu-profile/README.md` lets someone else reproduce the results on different hardware
- [ ] Decision rule applied and the outcome written in `RESULTS.md`
- [ ] `plan.md` updated: phase 21 explicitly **proceeding with a target** or **closed with a reason**
- [ ] If proceeding: the specific workloads and stage numbers that justify it are named
- [ ] Zero renderer code written in this phase

## Risk Assessment

**Risk: the decision rule gets reinterpreted to justify the more interesting work.**
Signal: arguing about criteria rather than applying them; redefining "dev-tool workload" to include a game.
Response: the rule is pre-committed and R1 is explicit. Phase 21 is weeks of work that forks the display path permanently — the bar is deliberately high. If the numbers do not clear it, closing the phase is the correct outcome and should be recorded as a decision, not a failure.

**Risk: benchmarks are unrepresentative and drive the wrong conclusion.**
Signal: results dominated by one synthetic workload, or by a workload no dev-tool user runs.
Response: four-plus workloads from the R1 table, reported separately rather than averaged into one score. Publish the methodology so the choice of workloads is auditable.

**Risk: measurement overhead distorts the result.**
Signal: instrumented runs materially slower than uninstrumented.
Response: feature-flagged instrumentation; compare instrumented vs clean end-to-end fps and report the overhead. Prefer timestamp collection over per-frame logging.

**Risk: encoder tuning fixes it, but only after phase 21 has already started.**
Signal: alternatives evaluated late or skipped for time.
Response: step 6 comes **before** the decision, by construction. Cheap wins first is the entire point of gating.

**Risk: the phase concludes "inconclusive" and phase 21 starts by default.**
Signal: `RESULTS.md` without a clear decision.
Response: inconclusive means **closed** — the burden of proof is on proceeding. Record it as such rather than leaving phase 21 ambiently open.
