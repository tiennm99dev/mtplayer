---
phase: 6
title: "Manager GUI"
status: pending
priority: P2
effort: "4-6 weeks"
dependencies: [4, 5]
---

# Phase 6: Manager GUI

## Goal

A Tauri 2 desktop manager that covers what the MuMu and LDPlayer
multi-instance managers do for app testing. It talks only to the daemon.

## Requirements

- **Instance list:** status, image version, identity summary, and CPU/RAM
  usage per instance.
- **Actions:** create, clone, edit identity, start, stop, delete and snapshot,
  each for one instance or for a selected group.
- **Display:** each instance's screen stays in the emulator's own native
  window. The manager does not embed or stream the screen. It offers window
  arrangement: tile and cascade.
- **Sync input:** one leader instance's touch and keys are copied to selected
  followers as normalised coordinates sent through the virtual input device
  (gRPC input injection), not `adb shell input`. This is best-effort: it
  stays correct only while the followers show the same UI as the leader.
- **Tool panels:**
  - drag-and-drop install for APK, AAB and XAPK;
  - a logcat viewer;
  - screenshot and recording;
  - a file browser;
  - GPS, network, battery and sensor controls.
- **Identity fields** in the instance settings.

## Tasks & Steps

1. Scaffold Tauri 2 with a gRPC client to the daemon.
2. Build the instance list, actions and settings, then the tool panels, then
   sync input.
3. Run an end-to-end UI test with the Tauri WebDriver or Playwright against a
   real daemon.

## Verification

- Every acceptance criterion (6) action works from the GUI.
- With sync input on, a tap on the leader appears in the same spot on two
  followers that have different screen sizes.
- Closing the GUI leaves the instances running under the daemon.
