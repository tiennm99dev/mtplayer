---
phase: 4
title: "Instance daemon and CLI"
status: pending
priority: P1
effort: "4-6 weeks"
dependencies: [2, "3a"]
---

# Phase 4: Instance daemon and CLI

## Goal

One Rust daemon owns every instance, with a gRPC API and a CLI on top. The
GUI and scripts never start engine processes themselves.

## Requirements

- **Two instance modes:**
  - **Persistent:**
    - its own AVD directory, its own `userdata` qcow2 overlay, and its own
      Quick Boot snapshot, taken after its own first cold boot;
    - the system image is shared read-only across instances, through each
      AVD's system overlay;
    - its own identity.
  - **Ephemeral (for CI):**
    - started `-read-only` from one golden snapshot;
    - the identity is the same as the golden instance's;
    - disk writes are discarded on exit.

  A persistent instance never resumes from another instance's RAM snapshot,
  because that would restore the other instance's identity.
- **Instance store** under `%LOCALAPPDATA%\MTPlayer\`:
  - one generated AVD directory per instance (`config.ini`,
    `hardware-qemu.ini`), with `ANDROID_AVD_HOME` and `ANDROID_EMULATOR_HOME`
    set for every engine process;
  - nothing is written to the user's default Android SDK home folder.
- **Lifecycle:** create, clone (fresh or full), start, stop, restart, delete,
  and snapshot save/load, with batch forms (`--all`, `--group`).
- **Clone** creates a new serial, MAC address, IMEI, IMSI and phone number.
  - A full clone runs the Phase 2 provisioning boot to reset the Android ID.
  - On rooted instances, model and brand come from the Magisk identity
    module, with values copied or overridden.
  - Unrooted instances use their image profile.
- **Resources:**
  - RAM and core count are set with the engine's `-memory` and `-cores`.
  - A Job Object per engine process limits only the CPU rate and handles
    orphan cleanup. It never caps memory, because a limit below `-memory`
    kills the engine.
- **Ports:**
  - fixed per instance index: console 5554+2n, adb 5555+2n, gRPC base+n;
  - bound to `127.0.0.1` only, and checked before start;
  - console and gRPC tokens are generated per instance, and only the daemon
    holds them.
- **Process hygiene:**
  - `KILL_ON_JOB_CLOSE` is off, so instances survive a daemon crash;
  - on start, the daemon reattaches to the instances it recorded (PID,
    creation time, ports), and stops any it can't verify through the process
    handle;
  - it never leaves orphans after `stop --all`.
- **Root toggle:** choose the base or Magisk ramdisk per instance.

## Tasks & Steps

1. Define the gRPC API (`proto/instances.proto`) and the CLI command tree,
   then implement them.
2. Implement the store, the AVD-directory generator, and an image-version
   registry that reads the Phase 2 manifest.
3. Implement the engine launcher, which turns an instance config into engine
   flags, `androidboot.*` values, tokens and environment variables.
4. Write the core of the `mtp-adb` crate, which speaks the ADB device
   protocol to the guest: connection, auth and shell. The boot-ready check
   and the provisioning boot use it; Phase 5 extends it.
5. Write integration tests that boot real instances:
   - create, then a fresh clone and a full clone;
   - start four;
   - stop all;
   - confirm no processes, ports or files in the default Android SDK home
     folder are left.

## Verification

- `mtp create a; mtp clone a b; mtp start a b` gives two adb serials, two
  Android IDs and two IMEIs.
- After a restart, a persistent instance keeps its app data. An ephemeral
  instance keeps nothing.
- After `mtp stop --all`, `netstat -ano` shows no engine ports and no engine
  processes are left.
- If the daemon is killed while instances run, it reattaches to them when it
  restarts.
- An adb connection from a second machine on the LAN fails.
- `cargo test` and the integration suite pass.

## Risks

- **Port collisions** with a stock AVD or MuMu on the same host. Detect them
  and report the owner; never pick a random port.
