---
phase: 7
title: "Packaging, performance and rollout"
status: pending
priority: P2
effort: "3-4 weeks"
dependencies: [5, 6]
---

# Phase 7: Packaging, performance and rollout

## Goal

A per-user installer that meets the performance targets on both hosts, plus
the license paperwork. Then replace MuMuPlayer on the work PC.

## Tasks & Steps

1. **Installer:**
   - a per-user MSI or MSIX bundling the engine, daemon, GUI, CLI and
     bundletool;
   - images are downloaded or copied on first run;
   - it checks the `HypervisorPlatform` feature and prints the one command
     to enable it;
   - uninstall removes the instance store only when the user confirms.
2. **Performance tuning.** Re-run the Phase 1 benchmark set on miti-b550 and
   tiennm5-pc. Tune the defaults (cores, RAM, ANGLE versus host, Quick Boot)
   until acceptance criteria 1-3 pass.
3. **GPU driver list.** Record the known-good NVIDIA drivers for the RTX 3060
   and GTX 1660, and note the ANGLE fallback.
4. **License inventory.** Finish the license inventory and the GPL source
   offer, and add a NOTICE file to the installer.
5. **Rollout:**
   - install on tiennm5-pc;
   - test a real work app;
   - uninstall MuMuPlayer from both hosts once the user confirms;
   - update `D:/docs/about-me/work.md` and the machine files in
     `.claude/rules/machines/`.
6. **Docs:** a README in the project repo covering install, the CLI, image
   rebuilds and engine re-bases.

## Verification

- Acceptance criteria 1-9 in [plan.md](plan.md#acceptance-criteria) all pass.
  Record the results in `plans/reports/benchmark-<date>-emulator-final.md`.
- Install, uninstall and reinstall on tiennm5-pc leaves no orphan processes,
  ports or services.

## Rollback

- MuMuPlayer stays installed until criterion 3 passes on both hosts and the
  user confirms the switch.
