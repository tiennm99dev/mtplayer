---
title: "Phase 12: Multi-Instance Manager and ADB Endpoint"
status: todo
phase: 12
priority: P1
effort: "1-1.5 wk"
dependencies: [11]
---

# Phase 12: Multi-Instance Manager and ADB Endpoint

## Overview

Run several instances at once, manage them from a sidebar, and surface each one's ADB endpoint so Android Studio, Gradle, and `adb` can attach. The endpoint work is what makes MTPlayer fit an existing toolchain instead of being a closed box.

At v1 "multi-instance" means different profiles, resolutions, and states over the **one** API 33 image. **Build for N images regardless** — retrofitting a single-image assumption is expensive.

## Requirements

- [ ] Functional: N concurrent instances, each with its own overlay, loopback port, and `resolved_profile`
- [ ] Functional: sidebar list with per-instance status, RAM, port, image
- [ ] Functional: video panes as tabs; **only the visible pane decodes**
- [ ] Functional: host-RAM guard rail computed from **`resolved_profile`**, not templates
- [ ] Functional: ADB endpoint shown prominently with one-click copy
- [ ] Functional: auto-registration into `adb devices` — **conditional**, only when the user has an ADB server
- [ ] Functional: identity confirmed via the **per-instance** identifier, not `ro.serialno`
- [ ] Non-functional: three instances at 1080p on a 16 GB host without an unresponsive UI
- [ ] Non-functional: **every endpoint binds `127.0.0.1`** (S2), verified negatively

## Architecture

```
InstanceManager  (over the phase-06 registry)
   ├─ Instance A  overlay-a.qcow2  127.0.0.1:5561  Ready    resolved_profile: phone
   ├─ Instance B  overlay-b.qcow2  127.0.0.1:5563  Booting  resolved_profile: tablet
   └─ Instance C  overlay-c.qcow2  127.0.0.1:5565  Stopped  resolved_profile: low-ram
        │
   sidebar list  ──  active pane (tab)  ──  per-instance logcat / snapshots
```

### Pause on hide — the mechanism, corrected

Three 1080p H.264 decoders running for panes nobody is looking at will saturate a laptop. But **scrcpy has no pause, set-fps, or set-bitrate control message** — encoder parameters are launch-time server options only. The original plan scheduled `pause()`/`resume()` as if the protocol offered them, and its fallback ("a very low frame-rate cap for hidden panes") would have required a full server relaunch on every tab switch.

**Actual mechanism, defined in phase 04 and used here:**

- **Hide:** keep draining the video socket, **drop decode and upload**. Free the GPU textures; retain cached SPS/PPS (phase 05).
- **Show:** re-init the decoder from cached config and send **`RESET_VIDEO`** to get config + a keyframe immediately, rather than waiting for a spontaneous IDR that may never come on a static screen.

Decode, colorspace conversion, and texture upload dominate the cost, so dropping them captures most of the saving. **Never stop reading the socket** — that fills the buffer, blocks the guest-side encoder, stalls `MediaCodec`, and contradicts "the guest keeps running." Cap per-instance `max-fps`/`max-size` at launch from the profile instead of trying to change them at runtime.

### Port assignment and identity

Allocate via phase 02's **hold-until-bound** mechanism — bind `127.0.0.1:0`, keep the socket until the VMM binds, then close. The original bind-then-release pattern was a TOCTOU.

**Identity must use the per-instance identifier from phase 07** (`androidboot.serialno` or the overlay marker), **not `ro.serialno`**. Every instance boots the same immutable base and phase 07 injects one shared prop, so `ro.serialno` is identical across instances — it can distinguish "an MTPlayer guest" from "an unrelated service" but not A from B, which is the case that loses data. After connecting, and after every reconnect, confirm the identifier matches the instance record before treating the connection as that instance's.

### Auto-registration — conditional

Registering an instance into `adb devices` means calling `host:connect` against **the user's own ADB server**. Since phase 03 removed MTPlayer's dependency on `adb.exe` entirely (G6), that server may not exist.

So: **detect an ADB server; if present, offer auto-registration; if absent, the feature is simply unavailable** and the endpoint is still shown and copyable. Do not install ADB, do not start a server the user does not have, and never present the feature as guaranteed.

When it is available: opt-in per instance or globally; `host:disconnect` on stop for ports MTPlayer registered; disconnect all on exit; **never** disconnect a device MTPlayer did not register. Reconcile at startup to clear stale MTPlayer-range entries from a previous crash.

### Host-RAM guard rail

`available_physical - sum(running resolved_profile.ram_mb) - reserve >= requested`

**From `resolved_profile`, never the template.** Phase 10 makes templates live-mutable; summing them would subtract numbers the VMMs never had, refusing legitimate instances while reporting false figures. On refusal, state requested / available / reserve as actual numbers.

## Related Code Files

- Create: `crates/mtp-core/src/instance_manager.rs` — collection ops, RAM accounting from `resolved_profile`, port bookkeeping
- Create: `crates/mtp-core/tests/ram_guard.rs` — guard arithmetic, including the edit-template-while-running case
- Create: `crates/mtp-ui/src/sidebar.rs` — instance list, status badges, start/stop/delete
- Create: `crates/mtp-ui/src/tabs.rs` — pane switching, activate/deactivate hooks
- Create: `crates/mtp-ui/src/endpoint_badge.rs` — endpoint display, copy, conditional auto-register toggle
- Modify: `crates/mtp-scrcpy/src/lib.rs` — use phase 04's `pause()`/`resume()` (drain-and-drop + `RESET_VIDEO`)
- Modify: `crates/mtp-video/src/lib.rs` — release GPU resources on pause, retain cached config
- Modify: `crates/mtp-ui/src/logcat_pane.rs` — per-instance buffers, not one global buffer
- Modify: `crates/mtp-core/src/registry.rs` — multi-instance aware; persist assigned ports

## Implementation Steps

1. **`InstanceManager`** over the phase-06 registry. It owns port allocation and RAM accounting so nothing else has to.
2. **Per-instance state isolation audit.** Walk phases 04, 05, 09, 10, 11 for everything implicitly single-instance — logcat buffer, current profile, decoder, snapshot selection. Key each by `InstanceId`. **Do this before adding UI**, or the bugs get found through the UI instead of by reading. A snapshot applied to the wrong overlay destroys user data.
3. **Sidebar.** Status badge from the phase-06 state machine, plus RAM, port, image, profile name. `Failed` shows its reason on hover.
4. **Tabs with activate/deactivate** wired to phase 04's drain-and-drop pause. Verify with a profiler that a hidden pane costs near-zero CPU.
5. **RAM guard** from `resolved_profile`, with specific numbers on refusal.
6. **Endpoint badge.** `127.0.0.1:<port>`, monospace, one-click copy. Also expose the full `adb connect 127.0.0.1:<port>` string, since that is what people paste.
7. **Conditional auto-registration** per the design above. Probe for a server; disable the toggle with an explanatory tooltip when absent.
8. **Identity verification** against the per-instance identifier on connect and on every reconnect.
9. **Concurrency load test.** Three instances at 1080p: UI responsiveness, total RSS, host CPU with two panes hidden, and all three endpoints usable from an external `adb` (where one exists).
10. **Cleanup on exit.** Stop all instances gracefully, remove forwards, disconnect registrations.

## Success Criteria

- [ ] Three instances run concurrently on different profiles, each with its own state
- [ ] **Hidden panes consume < 2% host CPU each** (profiler-measured — the original "near-zero" had no threshold); switching tabs shows a decoded picture within ~1 s via `RESET_VIDEO`
- [ ] Pausing a hidden pane does **not** stall the guest-side encoder (guest CPU and `MediaCodec` healthy)
- [ ] Per-instance logcat buffers independent — no cross-contamination
- [ ] Per-instance snapshots operate on the correct overlay (verify by snapshotting B while A runs)
- [ ] RAM guard refuses instance N+1 with specific numbers, and does **not** refuse when memory is genuinely available
- [ ] **Editing a profile template while an instance runs does not change the guard arithmetic for that instance**
- [ ] Endpoint copy yields a string that works pasted into an external terminal
- [ ] All endpoints bound to `127.0.0.1`; **not reachable from a second machine on the LAN** (S2)
- [ ] With no ADB server present, auto-registration is cleanly unavailable and everything else still works
- [ ] With a server present and auto-register on, all three appear in `adb devices`; stopping one removes exactly that one; the user's pre-existing physical-device connections are never disrupted
- [ ] Instance identity verified via the per-instance identifier — confirm two instances report different values
- [ ] MTPlayer exit leaves zero VMM processes, zero forwards, zero registered devices
- [ ] Guard-arithmetic tests pass in CI

## Risk Assessment

**Risk: single-instance assumptions surface as cross-instance data corruption.**
Signal: logcat from A under B; a snapshot applied to the wrong overlay.
Response: the step-2 audit before UI work, plus the two explicit success criteria. This is not cosmetic — a misapplied snapshot destroys user data.

**Risk: drain-and-drop pause does not save enough CPU.**
Signal: three instances still stutter with two panes hidden.
Response: the < 2% threshold makes this measurable rather than arguable. If draining alone is too expensive, the next lever is a lower launch-time `max-fps` per instance — not a runtime change, which the protocol does not support.

**Risk: leaving dead entries in the user's `adb devices`.**
Signal: offline MTPlayer entries after the app closes.
Response: disconnect on stop and exit, tracking only MTPlayer-registered ports, plus startup reconciliation. Made less likely by the feature now being conditional and off by default.

**Risk: port collision or reuse attaching to the wrong guest.**
Signal: an instance's pane showing another instance's content.
Response: hold-until-bound allocation plus per-instance identity verification on connect *and* reconnect. Phase 09's reconnect loop has the matching check.

**Risk: v1's single image hides multi-image bugs until v1.1.**
Signal: works with one image; adding API 37 breaks instance management.
Response: key instances by image id throughout even though only one exists. Mark shortcuts `// TODO(v1.1 multi-image)` so the retrofit is a search, not archaeology.
