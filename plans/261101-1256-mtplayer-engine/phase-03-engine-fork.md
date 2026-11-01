---
phase: 3
title: "Engine fork"
status: pending
priority: P1
effort: "6-10 weeks"
dependencies: [1]
---

# Phase 3: Engine fork

## Goal

Ship our own engine binary for Windows, built from AEMU source with the base
chosen in Phase 1. It should run on WHPX only, with gfxstream graphics and a
per-instance identity, and carry as few patches as possible.

## Requirements

- Rebrand the binaries, window titles and paths. There is no "Android" in the
  product name.
- WHPX is the default and only accelerator. The engine refuses to start on
  HAXM or AEHD.
- gfxstream with `-gpu host` by default, and ANGLE (`angle_indirect`) as the
  fallback for driver bugs. The engine picks the mode from a per-instance
  config value.
- Per-instance identity, passed through from the instance config to the
  guest:
  - boot properties for model, brand, fingerprint and serial;
  - MAC address;
  - modem identity (IMEI, IMSI, ICCID and phone number) in AEMU's telephony
    modem;
  - the GPS start position.
- Keep the multi-instance features AEMU already has: `-read-only` instances
  sharing a copy-on-write Quick Boot snapshot, qcow2 overlays, the console and
  adb port pair per instance, the gRPC control endpoint, and the inter-AVD
  network stack.

## Tasks & Steps

1. Set up the fork in the project repo with upstream as a remote. Keep every
   change as a named patch series: branding, identity, defaults.
2. Add the identity plumbing above. Prefer existing AEMU flags and console
   commands over new code.
3. Set up CI: build the Windows binaries with the Phase 1 build route, on
   GitHub Actions or the build host, and produce a versioned zip.
4. Write a re-base runbook covering upstream releases and the move to the
   QEMU 11 engine.

## Verification

- Two instances started with different identity configs show different
  IMEI/IMSI (`adb shell service call iphonesubinfo` or the dialer `*#06#`),
  MAC address and model.
- `-gpu host` renders on the RTX 3060 and the GTX 1660. A forced ANGLE
  fallback also renders.
- The engine starts while Docker Desktop and a WSL2 distro are running.
- The patch series applies cleanly to the next upstream tag. Check this once
  before the phase closes.

## Risks

- Google may retire the classic engine. Keep the patch series small and
  track upstream's next-generation engine.
- GPLv2: internal use does not trigger distribution duties, but keep the
  source and the build scripts in the repo so the tool can be handed on.
