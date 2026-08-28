---
title: "Phase 4: mtp-scrcpy — Transport and Control"
status: todo
phase: 4
priority: P1
effort: "1-1.5 wk"
dependencies: [3]
---

# Phase 4: mtp-scrcpy — Transport and Control

## Overview

Push the pinned `scrcpy-server.jar` into the guest, launch it via `app_process`, and own both directions of its protocol: H.264 access units out, input events in. This crate is why MTPlayer needs no host-side GPU renderer for v1.

The protocol is **not a stable public API** — it changes between scrcpy releases. The jar is version-pinned and bundled, and the parser is written against that exact version. The original control-message table was written against an *older* scrcpy than the one being pinned, which is corrected below.

## Requirements

- [ ] Functional: push and launch `scrcpy-server.jar`, client and server versions matching exactly
- [ ] Functional: **forward tunnel** — MTPlayer connects *out* to the guest's `localabstract:` socket
- [ ] Functional: parse the video stream into H.264 access units with PTS and config/keyframe flags
- [ ] Functional: control subset — touch, key, scroll, text, display power, **and `RESET_VIDEO`**
- [ ] Functional: clean teardown — the server process dies when MTPlayer disconnects or crashes
- [ ] Non-functional: zero-copy where practical; no per-frame heap churn on the hot path
- [ ] Non-functional: **the access-unit parser is fuzzed** (S9)
- [ ] Non-functional: `assets/NOTICE` carries scrcpy's Apache-2.0 attribution and the pinned version

## Architecture

### Forward tunnel, not reverse — and this is a security decision

scrcpy's *default* is a **reverse** tunnel: the client listens on a host TCP port and the guest connects in. The red team found that this makes MTPlayer an unauthenticated local listener. Any process running as the user — or a guest app, since the guest runs arbitrary untrusted APKs by design (S1) — could race to connect first, then **receive everything MTPlayer writes to the control socket** (including inject-text: whatever the developer pastes into the device, tokens and passwords included) and **feed arbitrary H.264 into the decoder**, which is memory-unsafe C.

Phase 03's device-protocol client makes the fix structural rather than mitigating: with no ADB server there is no `adb reverse` anyway, so MTPlayer opens the guest's abstract socket itself.

```
mtp_adb: push scrcpy-server.jar -> /data/local/tmp/scrcpy-server.jar
mtp_adb: open("shell:CLASSPATH=/data/local/tmp/scrcpy-server.jar \
                app_process / com.genymobile.scrcpy.Server <ver> tunnel_forward=true ...")
                │  (keep this stream — it carries the server's stderr)
                │
mtp_adb: open("localabstract:scrcpy_<scid>")   x2, in the server's expected order
   ┌────────────┴───────────────────────────┐
   │ video    (guest -> client)  device meta, then frame headers + AUs
   │ control  (client -> guest)  input events, display power, reset video
   └────────────────────────────────────────┘
```

**There is no host listener.** MTPlayer is always the connecting side.

**Video framing:** a device-metadata header (name, initial width/height), then repeating frame headers carrying a PTS and a payload length. Config packets (SPS/PPS) are flagged distinctly from media packets and **must be surfaced, not swallowed** — phase 05 feeds them to the decoder before the first keyframe, and hiding them produces a green or black pane that looks like a decode bug.

### Control messages — verified against the pinned version

| Message | Used for |
|---|---|
| inject touch event | mouse -> single-touch (absolute guest coords + the screen size believed at send time) |
| inject key event | keyboard |
| inject scroll event | wheel |
| inject text | paste, fast text entry |
| **`SET_DISPLAY_POWER`** | keep the guest display on — *the original plan called this "set screen power mode", the name in an older scrcpy* |
| **`RESET_VIDEO`** | **new: resets the encoding session, re-emitting config + a keyframe** |

`RESET_VIDEO` was absent from the original plan and is what makes phase 12's pause-on-hide implementable at all — without it, resuming after dropping decoder state means waiting for the guest to spontaneously emit an IDR, which on a static home screen may be tens of seconds or never.

**What the protocol does *not* have:** pause, set-fps, set-bitrate. Encoder parameters (bit rate, max size, max fps, I-frame interval) are **launch-time server options only**. Phase 12's original "very low frame-rate cap for hidden panes" fallback was therefore unimplementable without a full session restart; it is corrected there.

Coordinates are absolute in **guest** space, and every touch carries the screen size the client believed at send time so the server can reject stale events after a rotation. Never send host-window coordinates.

## Related Code Files

- Create: `crates/mtp-scrcpy/src/lib.rs` — `ScrcpySession::start(device, opts)`, `pause()`/`resume()` semantics for phase 12
- Create: `crates/mtp-scrcpy/src/server.rs` — jar push, `app_process` launch, version handshake, teardown
- Create: `crates/mtp-scrcpy/src/video.rs` — meta + frame header parsing -> `AccessUnit { pts, is_config, is_key, bytes }`
- Create: `crates/mtp-scrcpy/src/control.rs` — control message encoding incl. `RESET_VIDEO`, `SET_DISPLAY_POWER`
- Create: `crates/mtp-scrcpy/src/keymap.rs` — winit/egui key -> Android keycode + metastate
- Create: `crates/mtp-scrcpy/tests/framing.rs` — parser tests over recorded fixtures
- Create: `crates/mtp-scrcpy/fuzz/fuzz_targets/parse_au.rs` — **fuzz the frame-header/AU parser (S9)**
- Create: `assets/scrcpy-server.jar` — pinned, version recorded in `assets/NOTICE`
- Modify: `assets/NOTICE` — scrcpy Apache-2.0 attribution + exact version

## Implementation Steps

1. **Pin the jar and read its source.** Choose a release, record the exact version string as a Rust constant, vendor the jar. **The pinned release's server source is the only authoritative protocol spec** — do not work from blog posts describing a different version. Enumerate its control-message types and confirm the table above against it.
2. **Push + launch with `tunnel_forward=true`.** Keep the shell stream — the server's stderr is the only diagnostic when launch fails.
3. **Connect the two `localabstract:` streams** in the order the server expects. On timeout, **dump the server's stderr** rather than reporting a bare connection failure. Budget a day here: socket-order mistakes hang both sides silently and this is the most common scrcpy-integration failure.
4. **Read device metadata**, then loop on frame headers. Emit `AccessUnit` with PTS and flags. Do not concatenate or re-time — that is phase 05's business.
5. **Surface config packets explicitly**, flagged, before the first media packet.
6. **Control encoding** with exact field widths and big-endian ordering. **Unit-test each message against a byte fixture** — one wrong offset yields silently ignored input, the hardest bug class in this crate.
7. **Key mapping.** winit key codes -> Android keycodes + metastate: letters, digits, arrows, Enter, Backspace, Tab, Esc, modifiers. Non-ASCII goes through inject-text.
8. **Rotation handling.** On a server-reported resolution change, update the cached screen size used by touch messages. Stale sizes make touches land wrong.
9. **Pause/resume semantics for phase 12.** Define them here so phase 12 is not retrofitting: `pause()` **keeps draining the video socket** and stops decode/upload; `resume()` sends `RESET_VIDEO` to get config + a keyframe immediately. Never stop reading the socket — that fills the buffer and blocks the guest-side encoder, which contradicts "the guest keeps running."
10. **Teardown.** Dropping the session closes both streams and the shell stream, killing the guest process. Verify with `ps` in the guest that no orphaned `app_process` survives across ten connect/disconnect cycles.
11. **Fuzz the parser (S9).** The video stream is attacker-influenced and feeds C decode downstream. Fuzz `AccessUnit` parsing over mutated fixtures; it must never panic or produce an out-of-bounds slice.
12. **Fixture tests:** truncated frame header, zero-length payload, mid-AU disconnect, absurd declared length, and a config packet arriving mid-stream.

## Success Criteria

- [ ] Session starts against the phase-01 guest; device metadata parsed and logged
- [ ] **MTPlayer opens no listening socket** during a session (verify with `netstat` — no LISTEN owned by the process)
- [ ] Config (SPS/PPS) access units emitted before the first media unit, flagged distinctly
- [ ] 60 s stream with no parse error and monotonically increasing PTS
- [ ] Touch, key, scroll, and text verified as *arriving* — via guest-side `getevent` or observable UI response, never by visual impression alone
- [ ] `SET_DISPLAY_POWER` keeps the guest display on; `RESET_VIDEO` produces config + a keyframe on demand
- [ ] Rotating the guest updates the cached screen size; touches still land correctly
- [ ] `pause()` keeps the socket draining — the guest-side encoder does not stall (verify guest CPU and `MediaCodec` health)
- [ ] `resume()` yields a decodable picture within ~1 s via `RESET_VIDEO`
- [ ] Ten connect/disconnect cycles leave zero orphaned `app_process` in the guest
- [ ] **Fuzzer runs clean** over the AU parser — no panic, no OOB
- [ ] Framing tests pass in CI over fixtures, no device needed
- [ ] `assets/NOTICE` records scrcpy's license and pinned version

## Risk Assessment

**Risk: the protocol differs from the table because it is version-specific and undocumented.**
Signal: version handshake rejected, or metadata does not parse.
Response: read the pinned release's server source (step 1). The original plan's table already drifted — it named a control message by its older identifier — which is direct evidence that working from memory fails here.

**Risk: socket establishment order deadlocks.**
Signal: both sides hang with no error, reproducibly.
Response: explicit per-connect timeouts plus server-stderr dumping. Forward tunneling changes the order semantics from the reverse-tunnel documentation, so do not assume the ordering described for reverse tunnels applies.

**Risk: control messages encode wrongly and input is silently ignored.**
Signal: video works, taps do nothing, no error anywhere.
Response: byte-fixture tests per message type plus guest-side `getevent` verification, both success criteria.

**Risk: the AU parser is treated as trusted-input code.**
Signal: no fuzz target; parse errors handled as "correctness" cases only.
Response: S9 makes this explicit. The stream is influenced by whatever runs in the guest, and it feeds ffmpeg/openh264 — the only C in the trust path.

**Risk: orphaned `app_process` accumulating.**
Signal: guest CPU rising; multiple server processes in `ps`.
Response: the ten-cycle check. **Do not use a broad `pkill` to clean strays** — the original plan suggested `pkill` on `app_process`, which also matches zygote and `system_server` and runs as root. Kill only the PID MTPlayer launched, captured from the shell stream.

**Risk: pinning the jar creates an upgrade cliff.**
Signal: a needed scrcpy fix requires reworking the parser.
Response: accepted, and why parsing lives in one small module. Record the pinned version prominently so an upgrade is a scoped task.
