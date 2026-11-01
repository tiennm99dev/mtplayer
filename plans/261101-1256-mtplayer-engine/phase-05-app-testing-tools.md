---
phase: 5
title: "App testing tools"
status: pending
priority: P2
effort: "3-4 weeks"
dependencies: [4]
---

# Phase 5: App testing tools

## Goal

Expose the app-testing features AVD users expect as daemon API calls and CLI
commands, so that the GUI (Phase 6) and CI scripts share them.

## Requirements

- **Install:**
  - APK through adb;
  - split APKs with `adb install-multiple`;
  - AAB through bundletool (`build-apks --connected-device`, then
    `install-apks`);
  - XAPK by unpacking the split APKs and pushing the OBB to
    `/sdcard/Android/obb/<pkg>/`.

  Every install form can target a batch of instances.
- **ABI gate:** before installing, read the native libraries the package
  contains. Refuse a package with no x86_64 or x86 code (and no
  pure-Java/Kotlin fallback), naming the ABIs it does have.
- **Guest output is untrusted:**
  - check pulled file paths on the host before writing (reject `..`, drive
    letters and Windows reserved names);
  - strip terminal control sequences from logcat before printing it.
- **Logcat:** stream it with filters for package, tag and level, and save it
  to a file.
- **Capture:** screenshots and screen recordings through the gRPC control API,
  falling back to `screencap`/`screenrecord`.
- **Files:** push and pull, plus drag-and-drop in the GUI later.
- **Simulation** through the console or gRPC:
  - GPS fix and routes (a GPX file);
  - network speed and latency profiles;
  - battery level and charging state;
  - sensors (accelerometer, rotation);
  - incoming calls and SMS through the modem.
- **Automation:** document how UI Automator, Appium and scrcpy connect to
  instances by adb serial. Bundle nothing beyond the Apache-2.0 tools that are
  needed.

## Tasks & Steps

1. Add the tool services to the daemon API and add the CLI subcommands.
2. Build Windows `adb.exe` from the same AOSP tree (Apache-2.0, via the
   `win_sdk` host target), rather than redistribute platform-tools, which are
   under the SDK license. Bundle bundletool (Apache-2.0).
3. Write an integration test per tool against a running instance.

## Verification

- Each tool works on two instances at once from one CLI call.
- An AAB built by `bundletool` from a sample app installs and launches.
- An injected GPS fix shows up in `adb shell dumpsys location`.
- An ARM-only APK is refused with a message that names its ABIs.
