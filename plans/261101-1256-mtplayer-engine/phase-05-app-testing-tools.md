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

- **ADB:** the daemon talks to each guest's `adbd` directly, using its own
  Rust client for the ADB device protocol: connection, auth, open/write, and
  the sync sub-protocol. This is reused from the superseded MVP plan, so
  MTPlayer needs no `adb.exe` and never disturbs the user's own adb server.
  Tools that need an adb server, such as Appium, connect with the user's own
  `adb connect 127.0.0.1:<port>`.
- **Install:**
  - APK through the client's `pm install` streaming;
  - split APKs as one install session;
  - AAB through bundletool (`build-apks` for the instance's device spec), then
    a split install of the result;
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
2. Extend the Phase 4 `mtp-adb` crate with the sync sub-protocol, install
   sessions and logcat streaming, and test each against a real guest.
3. Bundle bundletool (Apache-2.0) with a minimal OpenJDK runtime made with
   `jlink` (GPLv2 with Classpath Exception; add it to the license
   inventory). The JRE is used only for AAB installs, and the device spec
   comes from `getprop` over `mtp-adb`.
4. Write an integration test per tool against a running instance.

## Verification

- Each tool works on two instances at once from one CLI call.
- An AAB built by `bundletool` from a sample app installs and launches.
- An injected GPS fix shows up in `dumpsys location`.
- Everything above works on a host with no Android SDK, `adb.exe` or system
  Java installed.
- An ARM-only APK is refused with a message that names its ABIs.
