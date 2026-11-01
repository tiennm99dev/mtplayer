---
phase: 4
title: "Instance daemon and CLI"
status: pending
priority: P1
effort: "4-6 weeks"
dependencies: [2, 3]
---

# Phase 4: Instance daemon and CLI

## Goal

A single Rust daemon owns every instance, with a gRPC API and a CLI on top.
The GUI and scripts never start engine processes themselves.

## Requirements

- **Instance store** (under `%LOCALAPPDATA%\MTPlayer\`):
  - one read-only system disk shared per image version;
  - a qcow2 copy-on-write data overlay and an identity/config file per
    instance;
  - shared Quick Boot snapshots per image version.
- **Lifecycle:** create, clone, start, stop, restart, delete, and snapshot
  save/load, with batch forms (`--all`, `--group`).
- **Clone** creates a new Android ID seed, serial, MAC address, IMEI, IMSI and
  phone number. Model and brand are copied unless they are overridden.
- **Resources:** a CPU, RAM and disk cap per instance, enforced through a
  Windows Job Object per engine process. Ports are fixed per instance index
  (console 5554+2n, adb 5555+2n, gRPC base+n), bind to `127.0.0.1` only, and
  are checked before start.
- **Process hygiene:** the daemon tracks the PID and ports of every instance.
  A crashed daemon reattaches to running instances on restart, or stops them
  cleanly. It never leaves orphans.
- **Root toggle:** choose the base or Magisk ramdisk per instance.

## Tasks & Steps

1. Define the gRPC API (`proto/instances.proto`) and the CLI command tree,
   then implement them.
2. Implement the store with an image-version registry that reads the Phase 2
   artifacts.
3. Implement the engine launcher, which turns an instance config into engine
   flags and identity inputs.
4. Write integration tests that boot real instances: create, clone, start
   four, stop all, and confirm there are no leftover processes or ports.

## Verification

- `<cli> create a; <cli> clone a b; <cli> start a b` gives two adb serials
  with different identities.
- After `<cli> stop --all`, `netstat -ano` shows no engine ports and no engine
  processes are left.
- Killing the daemon while instances run, then restarting it, reattaches to
  them.
- `cargo test` and the integration suite pass.
- From a second machine on the LAN, connecting to an instance's adb port
  fails.

## Risks

- Port collisions with a stock AVD or MuMu on the same host. Detect them and
  report the owner; never pick a random port.
