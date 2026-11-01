---
phase: 4
title: "Instance daemon and CLI"
status: pending
priority: P1
effort: "4-6 weeks"
dependencies: [2]
---

# Phase 4: Instance daemon and CLI

## Goal

One Rust daemon owns every instance, with a gRPC API and a CLI on top. The
GUI and scripts never start emulator processes themselves.

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
    `hardware-qemu.ini`), with `ANDROID_AVD_HOME`, `ANDROID_EMULATOR_HOME`
    and `ANDROID_SDK_ROOT` set for every emulator process;
  - the SDK root is Phase 2's private one;
  - nothing is written to the user's default Android SDK home folder.
- **Lifecycle:** create, clone (fresh or full), start, stop, restart, delete,
  and snapshot save/load, with batch forms (`--all`, `--group`). The daemon
  never uses the emulator console. It stops an instance with
  `EmulatorController` `setVmState` set to `SHUTDOWN`, and saves and loads
  snapshots with `SnapshotService` `SaveSnapshot` and `LoadSnapshot`, all
  over gRPC.
- **Identity at runtime,** using only the mechanisms Phase 1 proved:
  - emulator flags and boot properties at start: `-phone-number`, a
    per-instance copy of the ICC profile through `-icc-profile` for IMSI and
    ICCID, and `-qemu -append androidboot.serialno=…` for the serial;
  - after boot, `magisk resetprop` as root from `/data/local/tmp` for the
    rest, then a framework restart before the instance is reported ready;
  - each instance's values (phone number, IMSI and ICCID, model, brand,
    device, fingerprint, serial and Android ID) live in its config. No
    Google file is changed;
  - the IMEI and the eth0 MAC address stay fixed. An optional setting
    changes the MAC address in RAM only, with `ip link set` as root, until
    the next cold boot.
- **Clone** creates new identity values, copied or overridden.
  - A fresh clone starts from the base `userdata` and gets a new Android ID.
  - A full clone copies the overlay, then resets the Android ID, the per-app
    SSAIDs and the SSAID user key. As root, it runs
    `settings delete secure android_id`, stops the framework (`stop`) and
    deletes `/data/system/users/0/settings_ssaid.xml`. Then it stops the
    emulator and cold starts it headless without network
    (`-feature -VirtioWifi` plus `restrict=on`). It never uses
    `adb reboot`, which froze emulator 37.2.13.
- **Resources:**
  - RAM and core count are set with the emulator's `-memory` and `-cores`.
  - A Job Object per emulator process limits only the CPU rate and handles
    orphan cleanup. It never caps memory, because a limit below `-memory`
    kills the emulator.
- **Ports:**
  - fixed per instance index, counted from a configurable base port of 5600
    or higher (console and adb as an even-odd pair, gRPC from its own base).
    Never 5554 or 5555, which stock AVDs and MuMu use;
  - bound to `127.0.0.1` only, and checked before start. The console port
    also binds to `127.0.0.1`, although the daemon never uses it;
  - the gRPC token is per instance (`-grpc-use-token`), and only the daemon
    holds it. The stock console token is one file per Windows user,
    `%USERPROFILE%\.emulator_console_auth_token`. The daemon warns if that
    file is empty, because an empty file turns console auth off.
- **Process hygiene:**
  - `KILL_ON_JOB_CLOSE` is off, so instances survive a daemon crash;
  - on start, the daemon reattaches to the instances it recorded (PID,
    creation time, ports), and stops any it can't verify through the process
    handle;
  - it never leaves orphans after `stop --all`.

## Design notes

These come from the Phase 1 spike and its
[report](../reports/spike-261101-1700-stock-emulator-gates.md).

- Spawn the emulator with explicit stdio handles. The emulator and the adb
  server inherit the caller's handles, which keeps a caller's pipe open until
  the instance stops.
- Redirect `TMP`, `TEMP` and `LOCALAPPDATA` for the emulator child process
  into the instance store, as `spike/start-instance.ps1` does.
- Wait at least 90 seconds after a boot or a framework restart before saving
  a Quick Boot snapshot. A snapshot saved earlier restores a guest that hangs
  at "Phone is starting".
- Check the host's commit headroom before launching an instance, and refuse
  with a clear message when it is too low.
- `netsimd` is shared by all instances and listens on `127.0.0.1:6402`. It is
  not one instance's child, so stopping one instance never kills it.
- `crashpad_handler.exe` runs even with `-no-metrics`. Confirm that no crash
  report is uploaded without the user's consent.
- Apply the identity once, let the guest settle, run `sync`, then save the
  instance's Quick Boot snapshot. Later starts from that snapshot keep the
  identity and skip the framework restart.
- Do not use `-android-serialno`, which crashed or hung the emulator, or
  `-prop` for `ro.*` properties, which the host rejects.

## Tasks & Steps

1. Define the gRPC API (`proto/instances.proto`) and the CLI command tree,
   then implement them.
2. Implement the store, the AVD-directory generator, and a package registry
   that reads the Phase 2 store.
3. Implement the launcher. It turns an instance config into emulator flags,
   boot properties, tokens and environment variables, and applies the
   runtime identity after boot.
4. Write the core of the `mtp-adb` crate, which speaks the ADB device
   protocol to the guest: connection, auth and shell. The boot-ready check,
   the runtime identity and the provisioning boot use it; Phase 5 extends it.
5. Write integration tests that boot real instances:
   - create, then a fresh clone and a full clone;
   - start four;
   - stop all;
   - confirm no processes, ports or files in the default Android SDK home
     folder are left.

## Verification

- `mtp create a; mtp clone a b; mtp start a b` gives two adb ports, two
  Android IDs, and two different `Build.MODEL` values in a test app.
- After a restart, a persistent instance keeps its app data and identity. An
  ephemeral instance keeps nothing.
- After `mtp stop --all`, `netstat -ano` shows no emulator ports and no
  emulator processes are left.
- If the daemon is killed while instances run, it reattaches to them when it
  restarts.
- An adb connection from a second machine on the LAN fails.
- Stop, snapshot save and snapshot load work without the daemon ever opening
  the console port.
- `cargo test` and the integration suite pass.

## Risks

- **Port collisions** with a stock AVD or MuMu on the same host. Detect them
  and report the owner; never pick a random port.
- **An app that starts before the identity is applied** sees the stock
  values. The daemon reports an instance ready only after the framework
  restart, and Phase 1 measures what that costs in boot time.
