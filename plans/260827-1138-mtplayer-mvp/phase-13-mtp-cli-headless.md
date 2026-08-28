---
title: "Phase 13: mtp-cli — Headless and CI (P2 EXIT)"
status: todo
phase: 13
priority: P2
effort: "4-6 d (13a: 2-3 d · 13b: 2-3 d)"
dependencies: [12]
---

# Phase 13: mtp-cli — Headless and CI (P2 EXIT)

## Overview

A headless binary that drives instances without the UI, so CI can start a device, install an APK, read logs, and tear down.

**This phase is now split, because the original "cut it under pressure" guidance was unsafe.** Phase 14 declared `dependencies: [13]`, phase 15 had to bundle and prove `mtplayer-cli.exe`, and phase 16 required `mtplayer doctor` as a **v1** success criterion — so cutting phase 13 wholesale would have broken three downstream phases and lost the registry's atomic-write and locking guarantees.

| Split | Contents | Cuttable? |
|---|---|---|
| **13a** | `mtp-core` leak audit · exit-code enum · `doctor`, `list`, `stop --all` | **No** — phases 14/15/16 depend on these |
| **13b** | `install`, `logcat --follow`, `--json`, `--wait-boot`, `snapshot`/`restore`/`reset`, `docs/ci-usage.md` | Yes |

Registry atomic writes and per-instance leases have **moved to phase 06**, where the registry is created, so they survive a 13b cut.

Beyond CI, the value is architectural: a CLI that can do what the UI can proves `mtp-core` is UI-independent. Anything the CLI cannot reach is logic that leaked into `mtp-ui`.

## Requirements

- [ ] Functional (13a): `doctor`, `list`, `stop --all`; the exit-code contract; the `mtp-core` leak audit
- [ ] Functional (13b): `start`, `install`, `logcat`, `snapshot`, `restore`, `reset`, `adb-endpoint`, `--json`, `--wait-boot`
- [ ] Functional: `--headless` constructs **no** video pipeline
- [ ] Functional: UI and CLI share one registry, coordinated by the phase-06 lease
- [ ] Non-functional: guest-controlled output escaped before reaching a terminal or JSON (S7)
- [ ] Non-functional: exit codes are a public contract from v1

## Architecture

```
mtp-cli  ──┐
           ├──> mtp-core (InstanceManager, registry+leases, profiles, DiskOps) ──> mtp-provider-vm
mtp-ui   ──┘
```

**Headless means the video pipeline is never constructed**, not constructed and hidden.

**A correction to the original plan:** it asserted that `mtp-scrcpy` and `mtp-video` must be made *optional dependencies* of the run path, and made "`mtp-video` absent from the CLI's dependency graph" a success criterion. But under the declared architecture the video pipeline lives in `mtp-ui` and `mtp-core` never depends on it — so there was no edge to feature-gate. Either the work was unnecessary, or `mtp-core` had silently acquired a video dependency that no phase declared.

**Resolved as an invariant instead of a feature flag:** `mtp-core` must not depend on `mtp-scrcpy` or `mtp-video`. Assert it in CI with a dependency-graph check. That is cheaper than a feature matrix and catches the real defect — a future phase adding the edge.

**Cross-process coordination** uses the phase-06 lease, not a new lock. The CLI acquires the lease for destructive operations and refuses when a UI owns it. A short acquisition timeout plus a clear "another MTPlayer process is operating on this instance" message.

### Exit codes — the contract CI depends on

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | generic failure |
| 2 | invalid arguments |
| 3 | host preflight failed (no virtualization / feature off) |
| 4 | instance not found |
| 5 | boot timeout |
| 6 | APK rejected — incompatible ABI |
| 7 | install failed (guest-side) |
| 8 | resource guard — insufficient RAM or disk |
| **9** | **image verification failed** (SHA mismatch, missing part, bad signature) |
| **10** | **network / download failure** |
| **11** | **instance busy** — another process holds the lease |

Codes 9-11 are new. The original nine could not express phase 14's failure modes: SHA mismatch, missing part, and a ranges-unsupported server all collapsed into generic 1, defeating the exact distinction the plan insisted on for 6 vs 7. Code 11 is needed now that the lease can refuse an operation.

Codes 6 and 7 stay distinct: CI must tell "this APK can never run here" from "the install went wrong". `Failed` and `Crashed` map to distinct codes rather than both to 1.

**Codes may be added, never renumbered.** Phase 14's `image` subcommands and phase 16's `doctor` use this table; no later phase may introduce a command without mapping its failures onto it.

## Related Code Files

- Create: `crates/mtp-cli/src/main.rs` — `clap` derive, subcommand dispatch
- Create: `crates/mtp-cli/src/commands/` — one module per subcommand
- Create: `crates/mtp-cli/src/exit.rs` — the exit-code enum, single source of truth (13a)
- Create: `crates/mtp-cli/src/output.rs` — human vs `--json`, **with S7 escaping**
- Create: `crates/mtp-cli/tests/cli.rs` — argument parsing, exit codes, JSON shape
- Create: `.github/workflows/dep-graph.yml` — assert `mtp-core` does not depend on `mtp-scrcpy`/`mtp-video`
- Create: `docs/ci-usage.md` — worked example (13b)
- Modify: `crates/mtp-core/src/instance_manager.rs` — a start path taking no display pipeline

## Implementation Steps

**13a — not cuttable:**

1. **Audit `mtp-ui` for leaked logic first.** Anything the CLI needs that lives in `mtp-ui` moves to `mtp-core` now. This is the phase's real architectural payoff; do it before writing commands. **Perform this even if 13b is cut.**
2. **Exit codes as an enum** in one file, mapped from `mtp-core` error types. Never a bare `exit(1)` scattered around.
3. **Dependency-graph CI check** replacing the phantom feature-gating.
4. **`doctor`** running phase 16's checks headlessly. **`list`** and **`stop --all`** — the latter idempotent, exit 0 with nothing running, because phase 15's installer calls it.

**13b — cuttable:**

5. **Headless start path.** `InstanceManager::start_headless()` constructing no scrcpy session and no decoder.
6. **`--wait-boot`.** Block until `Ready` or timeout, exit 0 or 5. Without it every CI script reinvents a polling loop.
7. **`install`** reuses phase 08's ABI gate — one implementation, two front ends. Exit 6 on an incompatible verdict, printing the same message the UI shows.
8. **`logcat`.** Without `--follow`, dump and exit; with it, stream until interrupted. **Escape guest-controlled bytes (S7)** before writing to a terminal or JSON. Handle Ctrl-C cleanly so no guest-side process is left running.
9. **`--json`** on read commands, one stable envelope shape, documented.
10. **`snapshot`/`restore`/`reset`** acquiring the lease; exit 11 when busy.
11. **Worked CI example** on a self-hosted Windows runner. State plainly that hosted runners lack nested virtualization — *and confirm that claim once, since phase 02 flags it as asserted-but-unverified.*

## Success Criteria

- [ ] **13a:** `doctor`, `list`, `stop --all` work; exit-code enum is the single source; the `mtp-ui` leak audit is done and recorded
- [ ] **13a:** CI asserts `mtp-core` does not depend on `mtp-scrcpy` or `mtp-video`
- [ ] **13a:** `stop --all` is idempotent and exits 0 with nothing running
- [ ] `--headless` start creates no decoder and no wgpu context
- [ ] Every exit code in the table is reachable and correct — test each deliberately
- [ ] Exit 6 distinct from 7; 9 distinct from 10; 11 returned when the lease is held
- [ ] `Failed` and `Crashed` map to distinct codes, not both to 1
- [ ] A CLI-started instance appears in the UI's sidebar, and vice versa
- [ ] **A CLI-started instance survives the CLI exiting** (phase 06 breakaway)
- [ ] **`mtplayer list` while the UI has instances running kills nothing** (phase 06 lease)
- [ ] CLI destructive op while the UI owns the instance -> exit 11 with a clear message, no corruption
- [ ] `--json` output parses; shape documented
- [ ] A logcat line containing OSC 52 and raw control bytes is inert in terminal and JSON output (S7)
- [ ] Ctrl-C during `logcat --follow` leaves no guest-side process running
- [ ] **P2 EXIT:** three instances on different profiles; snapshot, break, restore; drive the whole thing from the CLI without opening the UI

## Risk Assessment

**Risk: business logic already leaked into `mtp-ui`.**
Signal: a command needs to duplicate ABI checking, profile validation, or boot polling.
Response: the step-1 audit before any command. Duplicated logic means the ABI gate can drift between UI and CLI — a CI job accepting an APK the UI rejects is a real defect.

**Risk: 13b is cut and downstream phases break anyway.**
Signal: phase 14/15/16 criteria referencing missing commands.
Response: the split exists for this. `doctor`, `list`, and `stop --all` are in 13a precisely because phases 14, 15, and 16 name them. If 13b is cut, phase 14's `image` subcommands move behind the UI and phase 14's headless criterion is struck — **record that in `plan.md`, do not leave it dangling.**

**Risk: exit codes become unstable and break CI scripts.**
Signal: a pipeline breaking after an update.
Response: the table is a public contract from v1. Add, never renumber. Say so in `docs/ci-usage.md`. Codes 9-11 were added here rather than discovered later precisely to reduce future pressure.

**Risk: CI usefulness limited because hosted runners lack nested virtualization.**
Signal: users trying the example on `windows-latest`.
Response: state the self-hosted requirement prominently. Also verify the claim — it is asserted in three phases and never checked, and platform limits change.

**Risk: the lease makes routine CLI reads fail spuriously.**
Signal: `mtplayer list` returning exit 11 while the UI is merely open.
Response: **read-only commands must not acquire the lease at all** — only destructive ones. Getting this wrong reintroduces the original defect from the other direction, making the CLI useless whenever the UI is running.
