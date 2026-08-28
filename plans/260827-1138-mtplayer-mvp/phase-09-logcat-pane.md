---
title: "Phase 9: Logcat Pane"
status: todo
phase: 9
priority: P1
effort: "4-6 d"
dependencies: [6]
---

# Phase 9: Logcat Pane

## Overview

Stream `logcat` from the guest into a pane a developer can actually work in: filter by level and tag, search, pause, copy, clear. Together with phase 08 this is the core loop — drop an APK, watch it fail, read why — and it is the feature that makes MTPlayer a dev tool rather than a viewer.

Performance is the whole difficulty. A chatty Android boot emits thousands of lines per second, and a naive egui implementation that formats every line every frame will drop the video pane's frame rate along with it.

## Requirements

- [ ] Functional: stream `logcat -v threadtime` over a persistent ADB connection
- [ ] Functional: ring buffer capped at ~50k lines, oldest evicted
- [ ] Functional: level filter, tag filter, free-text search, pause/resume, copy selection, clear
- [ ] Functional: reconnect automatically when the guest reboots; mark the discontinuity in the log
- [ ] Non-functional: 5,000 lines/second sustained without dropping the video pane below 25 fps
- [ ] Non-functional: memory bounded — 50k lines must not grow unboundedly through filter churn

## Architecture

```
mtp_adb::shell_stream("logcat -v threadtime")
        │  (dedicated task, off the UI thread)
   line splitter -> parse threadtime -> LogLine { ts, pid, tid, level, tag, msg }
        │
   ring buffer (VecDeque, cap 50k)  ── owned by a mutex the UI locks briefly
        │
   filter pass -> Vec<index>  (recomputed only when a filter changes, not per frame)
        │
   egui ScrollArea::show_rows(visible_range)  -> renders ~50 rows, never 50k
```

**Three decisions carry the performance:**

1. **`show_rows`, not `show`.** egui's row-virtualized scroll area renders only visible rows. Rendering 50k rows per frame is the default failure mode here.
2. **Filters produce an index vector, cached.** Recompute on filter change only. Re-filtering 50k lines every frame at 60 fps is 3M string operations per second for no reason.
3. **Parse once at ingest.** `LogLine` holds parsed fields; the UI formats only visible rows. Never re-parse in the draw loop.

**`threadtime` format:** `MM-DD HH:MM:SS.mmm PID TID LEVEL TAG: message`. Parsing is positional and cheap, but not every line conforms — buffer-switch banners (`--------- beginning of main`) and multi-line stack traces both appear. Unparseable lines must be retained verbatim as a `Raw` variant, never dropped: a Java stack trace's continuation lines are exactly what the developer needs.

**Tag storage.** Tags repeat heavily; interning them into a small string cache keeps memory flat and makes the tag filter a cheap integer comparison.

## Related Code Files

- Create: `crates/mtp-core/src/logcat.rs` — `LogLine`, threadtime parser, `LogBuffer` ring + filter index
- Create: `crates/mtp-core/tests/logcat_parse.rs` — parser tests including malformed and continuation lines
- Create: `crates/mtp-ui/src/logcat_pane.rs` — virtualized table, filter controls, search, copy
- Modify: `crates/mtp-adb/src/device.rs` — ensure `shell_stream` reconnect semantics are usable here
- Modify: `crates/mtp-ui/src/app.rs` — pane layout alongside the video pane

## Implementation Steps

1. **Parser first, with tests.** Handle a well-formed threadtime line, the buffer-switch banner, a stack-trace continuation line, and a truncated final line from a dropped stream. Retain anything unparseable as `Raw`.
2. **Ring buffer.** `VecDeque<LogLine>` with a hard cap; evict from the front. Keep it behind a mutex held only for push and for the brief UI snapshot — never across a render.
3. **Ingest task.** One task owns the ADB stream, splits on newlines across chunk boundaries (a line will split across reads — handle the partial-line carry), parses, and pushes. Never touch egui from this task.
4. **Tag interning.** A `HashMap<String, TagId>` plus a `Vec<String>`. The filter compares `TagId`.
5. **Filter index.** On any filter/search change, walk the buffer once and produce `Vec<usize>` of matching indices. Store the filter's generation number so the UI knows when the index is stale. Case-insensitive substring search is enough; do not add regex in v1 unless asked.
6. **Virtualized rendering.** `ScrollArea::show_rows` with a uniform row height. Color by level (V/D/I/W/E/F). Monospace font. Wrap long messages only if it does not break the uniform row height assumption — otherwise truncate with the full text available on hover or copy.
7. **Auto-scroll with a stick-to-bottom rule.** Follow the tail while the user is at the bottom; stop following the moment they scroll up. Yanking the view away from something a developer is reading is the most irritating possible bug here.
8. **Pause/resume.** Pause stops *display*, not ingest — the buffer keeps filling so nothing is lost. Show a "paused, N new lines" indicator.
9. **Clear** empties the buffer and optionally issues `logcat -c` in the guest. Make which one it does unambiguous in the UI.
10. **Copy.** Selected rows, or all filtered rows, as plain text to the clipboard.
11. **Reconnect — bounded, and re-verified.** On stream end, retry with backoff and insert a visible marker line so the discontinuity is obvious rather than looking like missing logs. Two constraints the original plan omitted:
   - **Terminate the loop on an explicit stop.** A user- or CLI-initiated stop is not a reboot; without a stop condition the loop reconnects forever to a port that may since have been reassigned.
   - **Re-verify instance identity after every reconnect, before appending a single line.** Compare the per-instance identifier phase 07 injects (`androidboot.serialno` or the overlay marker) — *not* `ro.serialno`, which is identical across every instance of the shared base image. Otherwise a stopped instance's pane can reattach to a different guest that inherited its port and silently interleave another instance's logs under the wrong tab.

12. **Escape guest-controlled text before it leaves the pane (S7).** Logcat content is fully controlled by any app in the guest. Strip C0/C1 control characters and OSC sequences before text reaches a terminal, the clipboard, or JSON output — unescaped OSC 52 is a clipboard write, and OSC title sequences rewrite the terminal title. This applies to the copy action here and to phase 13's `logcat --follow` and `--json`.
13. **Load test.** Generate 5,000 lines/second in the guest (`while true; do log -t LOAD ...; done` or a small APK) and confirm the video pane holds >= 25 fps.

## Success Criteria

- [ ] Logcat streams live from the guest, correctly parsed, colored by level
- [ ] Level filter, tag filter, and text search each work and compose together
- [ ] A Java stack trace is readable — continuation lines retained in order, not dropped
- [ ] 5,000 lines/s sustained for 60 s with the video pane >= 25 fps and no UI stall
- [ ] 50k-line cap enforced; RSS flat during a 10-minute high-volume run
- [ ] Filter changes do not re-filter per frame (verify by instrumenting the filter-pass counter)
- [ ] Auto-scroll follows the tail, and stops immediately when the user scrolls up
- [ ] Pause keeps ingesting; resume shows the lines that arrived while paused
- [ ] Guest reboot -> stream reconnects automatically with a visible discontinuity marker
- [ ] An explicit stop terminates the reconnect loop; it does not retry indefinitely
- [ ] Stop instance A, start instance B so B inherits A's port -> A's pane **refuses** to reattach (identity re-check works against the per-instance id, not `ro.serialno`)
- [ ] A log line containing OSC 52, an OSC title sequence, and raw C0 control bytes is rendered inert — and stays inert through copy-to-clipboard and `--json` (S7)
- [ ] Typing in the filter box does not leak keystrokes into the guest (phase-05 focus rule holds)
- [ ] Parser tests pass in CI, including malformed input

## Risk Assessment

**Risk: the pane tanks the video frame rate under log volume.**
Signal: fps drops when a chatty app runs.
Response: the three architectural decisions above are the mitigation, and the step-12 load test is the proof. If it still stalls, the cause is almost certainly a lock held across a render or a non-virtualized scroll area — check those before redesigning.

**Risk: partial lines across read boundaries corrupt entries.**
Signal: occasional truncated or merged log lines under high volume.
Response: explicit partial-line carry in step 3, exercised by the load test. This bug is invisible at low volume and obvious at high volume, which is why the load test is a success criterion rather than optional.

**Risk: memory grows despite the line cap.**
Signal: RSS climbing during the 10-minute run.
Response: usual causes are un-interned tags, the filter index not being reused, or retained `String`s for evicted lines. The flat-RSS criterion catches it; tag interning prevents the most likely one.

**Risk: keystrokes leak into the guest while typing a filter.**
Signal: typing a tag name causes guest UI input.
Response: phase 05 established the focus rule; this phase is its first real test. Verify explicitly — it is listed in Success Criteria for that reason.

**Risk: scope creep into a full log analysis tool.**
Signal: requests for regex, saved filter sets, log export, crash-signature detection.
Response: v1 ships the listed feature set. These are reasonable v1.1 items; note them, do not build them here.
