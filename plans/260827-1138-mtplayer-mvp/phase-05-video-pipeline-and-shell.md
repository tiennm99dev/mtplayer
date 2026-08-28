---
title: "Phase 5: Video Pipeline and Minimal Shell (P0 EXIT)"
status: todo
phase: 5
priority: P1
effort: "1-1.5 wk"
dependencies: [4]
---

# Phase 5: Video Pipeline and Minimal Shell (P0 EXIT)

## Overview

Decode the H.264 access units from phase 04, get them onto the GPU as a texture, and draw them in an egui window that also forwards mouse and keyboard back through the control socket. **This is the P0 exit: a live Android window on Windows.** Ugly is fine — the deliverable is proof the architecture works end to end.

## Requirements

- [ ] Functional: H.264 access units -> NV12 frames -> wgpu texture -> egui pane
- [ ] Functional: NV12 -> RGB conversion in a shader, never on the CPU
- [ ] Functional: mouse and keyboard in the pane map to scrcpy control messages with correct guest coordinates
- [ ] Non-functional: sustained >= 25 fps at 1280x720
- [ ] Non-functional: no crash and no unbounded memory growth over a 10-minute session
- [ ] Non-functional: end-to-end input-to-pixel latency measured and recorded

## Architecture

```
mtp-scrcpy AccessUnit ──> mtp-video::Decoder ──> NV12 frame (Y plane + interleaved UV)
                                                        │
                                          upload as two wgpu textures
                                          (R8 for Y, RG8 for UV)
                                                        │
                                     WGSL shader: BT.601/709 limited-range -> RGB
                                                        │
                              egui::Image over a registered wgpu texture id
```

**Two textures, not one.** Uploading Y as `R8Unorm` and interleaved UV as `Rg8Unorm` at half resolution, then converting in the fragment shader, avoids a CPU colorspace pass entirely. A CPU NV12->RGBA conversion at 720p is ~1.3 MB per frame of pointless work and will not hold 25 fps in a debug profile.

**Decode threading.** Decoding runs off the UI thread. The UI holds the most recent decoded frame; if decode outruns display, drop older frames rather than queueing — for a dev tool, latency beats completeness. A single-slot mailbox (not a channel with a backlog) enforces this structurally.

**Decoder choice.** `ffmpeg-next` bound to a prebuilt shared ffmpeg via `vcpkg`, with `openh264` as the fallback if MSVC linking proves painful. Decide inside this phase and record it; do not carry both.

**Neither option is licence-free, and the difference is patents, not copyright.** The original plan treated `openh264` as the clean escape hatch. It is not: Cisco's AVC patent grant covers **only Cisco's own precompiled binary, and only when that binary is separately downloaded to the end user's device and not combined with third-party software beforehand** — which bundling in an installer forfeits. Self-built `openh264` was never covered, and an LGPL ffmpeg build carries the same AVC exposure. Whichever is chosen here, **record it as a patent question for phase 15 to resolve deliberately**, not as a solved licensing item.

**The decoder is the only C in the trust path (S9).** It consumes bytes influenced by whatever runs in the guest. Phase 04 fuzzes the access-unit parser; this phase must not assume the decoder itself is hardened. Prefer a decoder built with sanitizers available for a fuzz run, and never let a decode error escalate past the single frame that caused it.

**Color range is a real trap.** scrcpy's encoder output is limited-range; treating it as full-range yields washed-out grey blacks that look like a shader bug. Read the range and matrix from the stream where available rather than hardcoding.

## Related Code Files

- Create: `crates/mtp-video/src/lib.rs` — `Decoder::feed(AccessUnit) -> Option<Nv12Frame>`
- Create: `crates/mtp-video/src/decoder.rs` — ffmpeg (or openh264) wrapper, config-packet handling
- Create: `crates/mtp-video/src/upload.rs` — NV12 -> two wgpu textures, reused across frames
- Create: `crates/mtp-video/shaders/nv12_to_rgb.wgsl` — colorspace conversion
- Create: `crates/mtp-ui/src/main.rs` — eframe app entry
- Create: `crates/mtp-ui/src/video_pane.rs` — texture draw + input capture and mapping
- Create: `crates/mtp-ui/src/app.rs` — minimal state: one session, one pane
- Modify: `crates/mtp-scrcpy/src/lib.rs` — expose the access-unit stream as a consumable channel
- Modify: `assets/NOTICE` — ffmpeg/openh264 licenses (LGPL implications for ffmpeg: dynamic linking only) **plus a recorded note that AVC patent coverage is unresolved and owned by phase 15**

## Implementation Steps

1. **Decoder skeleton.** Open an H.264 decoder. Feed config units first, then media units. Verify the first keyframe decodes before wiring any UI — a headless test that decodes 100 frames and prints dimensions is the cheapest possible checkpoint.
2. **Handle the config/keyframe ordering.** If the stream is joined mid-GOP, discard until the first keyframe rather than feeding the decoder garbage and logging errors per frame.
3. **Texture upload.** Allocate the Y and UV textures once at the stream's resolution and reuse them; reallocate only on a resolution change. Per-frame texture creation is the classic cause of a stuttering egui video pane.
4. **WGSL conversion.** Sample Y and UV, apply the limited-range matrix, output RGB. Get the range right before blaming the decoder for dull colors.
5. **Register with egui.** Use the eframe wgpu render state's texture registry to obtain a `TextureId`, then draw with `ui.image()`. Keep the pane aspect-correct and letterboxed — do not stretch.
6. **Single-slot frame mailbox.** Decode thread writes the latest frame, UI thread takes it. Overwrite on write. This is the mechanism that keeps latency bounded under load.

6a. **Retain SPS/PPS across a pause.** Phase 12 pauses hidden panes by dropping decode while phase 04 keeps draining the socket. Cache the config access units so `resume()` can re-init the decoder immediately, and pair it with phase 04's `RESET_VIDEO` to get a keyframe on demand. Discarding config on pause is what would otherwise make resume wait indefinitely for a spontaneous IDR.
7. **Input mapping.** Convert pane-relative cursor position to guest coordinates using the *stream's* dimensions, accounting for letterboxing. Send touch down on press, move while held, up on release. Wheel -> scroll. Keyboard -> keycodes, with non-ASCII routed to inject-text.
8. **Focus discipline.** Only forward keyboard events when the video pane has focus, or typing in future UI fields (phase 09's filters, phase 10's profile forms) will leak into the guest.
9. **Measure.** Log decoded fps and dropped-frame count. Measure input-to-pixel latency by tapping a guest-side stopwatch app or by timing a screen-color change on click, and record the number in the phase notes.
10. **Ten-minute soak.** Run 10 minutes at 720p watching RSS and fps. Growing RSS means frames or ffmpeg buffers are leaking.

## Success Criteria

- [ ] Live Android window renders on Windows from the phase-01 API 33 guest
- [ ] Sustained >= 25 fps at 1280x720, measured and logged, not eyeballed
- [ ] Mouse click/drag, wheel, and keyboard all produce the expected guest response
- [ ] Coordinates land correctly with the window resized and letterboxed, and after a guest rotation
- [ ] Blacks look black — colorspace range verified against a known test image, not judged casually
- [ ] 10-minute session: no crash, RSS flat within ~10%, no unbounded frame queue
- [ ] Input-to-pixel latency measured and recorded (expect ~30-70 ms; > 150 ms triggers the kill/pivot check)
- [ ] Zero per-frame texture allocations (verify by instrumenting or by a profiler pass)
- [ ] **P0 exit sign-off:** VMM decision from phase 01 is recorded, and this window works

## Risk Assessment

**Risk: ffmpeg does not link cleanly under MSVC.**
Signal: more than a day lost to `vcpkg`/linker errors.
Response (pre-decided): switch to `openh264`, which is a much smaller dependency and sufficient for H.264-only decode. Do not spend a week on ffmpeg for a P0 spike. Record which one was chosen — **and record that the switch does not resolve the AVC patent question**, which the original plan wrongly assumed it did.

**Risk: the video pane stutters despite adequate decode fps.**
Signal: decoder reports 30 fps, the pane looks like 10.
Response: almost always per-frame texture allocation or a CPU colorspace pass. Both are structurally excluded above; if it still stutters, profile before redesigning.

**Risk: end-to-end latency exceeds 150 ms.**
Signal: measurement in step 9.
Response: **kill/pivot signal from the plan.** First investigate scrcpy encoder options (bit rate, max fps, I-frame interval) and decoder buffering (`AV_CODEC_FLAG_LOW_DELAY`, zero-latency tuning). Do not change architecture on the first measurement — latency this high is usually a buffering setting, not the design.

**Risk: WHPX cannot boot the guest stably enough to soak for 10 minutes.**
Signal: guest crashes or hangs repeatedly during the soak.
Response: **kill/pivot signal.** After a week of WHPX tuning without stability, re-evaluate Provider B (phases 17-19) as the primary backend and reorder the plan. That is a replan, not a workaround — escalate.

**Risk: P0 code is treated as production code.**
Signal: effort spent on error handling, abstraction, or polish in `mtp-ui` during this phase.
Response: P0 is a spike. Its output is knowledge. `mtp-adb`, `mtp-scrcpy`, and `mtp-video` largely survive; `mtp-ui`'s phase-05 form does not. Expect to rewrite the app shell in phases 09-12 and do not defend it.
