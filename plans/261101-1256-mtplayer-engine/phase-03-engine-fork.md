---
phase: 3
title: "Engine build (3a) and patch series (3b)"
status: pending
priority: P1
effort: "3a: 2 weeks, 3b: 4-8 weeks"
dependencies: [1]
---

# Phase 3: Engine build (3a) and patch series (3b)

## Goal

Ship MTPlayer's own Windows engine, built from the base Phase 1 chose. 3a is
an unmodified self-built engine, and it unblocks Phase 4. 3b adds a small,
named patch series. It runs alongside Phases 4 and 5.

## Where the fork lives

- The emulator source is a repo-manifest workspace with many repositories.
  The fork therefore lives in its own repos under `tiennm99dev`:
  - `mtplayer-engine`, the fork of `platform/external/qemu`;
  - a goldfish fork, if the next-generation engine is the base;
  - a manifest repo.
- Each fork keeps upstream as a remote.
- `mtplayer` keeps the daemon, CLI, GUI and image pipeline. The two talk only
  through the engine's command line, console, gRPC and adb.
- Licenses: the engine repos are GPLv2. MTPlayer's own code can stay
  Apache-2.0, because the engine is a separate program. The user agreed to
  relicense if that ever becomes necessary.

## 3a: unmodified build

1. Script the Phase 1 build route.
2. Produce a versioned engine zip, and a source tarball of exactly what was
   built.
3. Build on miti-b550 or a self-hosted runner, not on GitHub-hosted runners.
4. **Verification:**
   - the zip boots the Phase 2 image on both target hosts;
   - the source tarball rebuilds the same binaries.

## 3b: patch series

Keep each item as a named patch so it can be re-based:

- **Branding:** binaries, window titles and paths. There is no "Android" in
  the product name.
- **WHPX only:** it is the default, and the engine refuses HAXM and AEHD.
- **Graphics:** gfxstream with `-gpu host` by default, and ANGLE
  (`angle_indirect`) as a per-instance fallback.
- **Per-instance identity:**
  - MAC address;
  - IMEI, IMSI, ICCID and phone number, where Phase 1 found the engine
    serves them;
  - GPS start position.

  Serial and boot properties need no patch, because they use `androidboot.*`.
- **Security defaults:** the console auth token and gRPC token
  authentication are always on, and every port binds to `127.0.0.1`.

Then:

- Write a re-base runbook. For the classic base, the trigger is "MTPlayer
  needs API 37".
- Check once before the phase closes that the series applies cleanly to the
  next upstream snapshot.

## Verification

- Two instances with different identity configs show different MAC
  addresses and IMEI/IMSI.
- `-gpu host` renders on the RTX 3060 and the GTX 1660, and a forced ANGLE
  fallback renders too.
- The engine starts while Docker Desktop and a WSL2 distro are running.
- Console and gRPC calls without the token are refused.
- **Exit gate:** every public engine release carries its complete
  corresponding source, and the license inventory is updated.

## Risks

- **Upstream churn.** Keep the series small and track whichever engine
  Google is investing in.
- **IMEI handling depends on the engine base.** The fix lives in the classic
  goldfish modem or in the new modem simulator, depending on the base.
