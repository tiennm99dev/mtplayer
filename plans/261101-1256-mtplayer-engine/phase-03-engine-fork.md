---
phase: 3
title: "Own engine fork"
status: deferred
priority: P3
effort: "later plan"
dependencies: ["user-funded cloud build"]
---

# Phase 3: Own engine fork (deferred)

## Why it is deferred

On 2026-11-01 the user dropped all source builds. v1 uses Google's stock
emulator and image instead ([session 4](plan.md#session-4-2026-11-01-image-re-plan)).

An own engine fork would let MTPlayer patch hardware identifiers (IMEI,
IMSI, MAC), brand the engine, and ship it. A license-clean own image would
let it ship root and identity built into the image. Both need source builds
of about 150-250 GB each, which only fit on a rented cloud VM.

## When to start it

Start a separate plan when one of these is true, and the user approves the
cloud cost:

- a needed identity field can't be changed on the stock emulator (Phase 1,
  task 3);
- MTPlayer has to ship its engine or image to people who won't accept the
  Android SDK License themselves;
- Google withdraws the pinned packages with no usable replacement.

## Known facts for that plan

- The classic engine branch is `emu-main-dev`. `emu-master-dev` has been
  frozen since 2025-03-27.
- The next-generation engine (`emu-main-next-v11.0`) hands AVDs below API 37
  to the classic engine and gives them no host GPU. For Android 16, the fork
  base is the classic engine.
- The Windows build route found by the cook workflow, including a stand-in
  for the Windows ADK DISM API, is in the
  [gate report](../reports/spike-261101-1629-image-and-engine-gates.md).
