---
title: "Phase 17: WSL2 Kernel Assessment and Consent Flow"
status: todo
phase: 17
priority: P3
effort: "3 d - 2 wk (depends entirely on the step-1 assessment)"
dependencies: [16]
---

# Phase 17: WSL2 Kernel Assessment and Consent Flow

## Overview

Provider B needs redroid to run inside WSL2. **The original version of this phase was built on a kernel config that no longer exists**, and the red team found it independently three times. This phase is restructured to **assess first, then build only what is actually required.**

The premise was: the stock WSL2 kernel lacks `CONFIG_ANDROID_BINDERFS`, `CONFIG_ASHMEM` (+`CONFIG_STAGING`), and `CONFIG_DMABUF_HEAPS`, so MTPlayer must ship a custom kernel — a **machine-global** change replacing the kernel for every WSL distro on the machine, justifying an elaborate consent/backup/revert apparatus and a per-WSL-version prebuilt-kernel matrix.

**What is actually true:**

- **`CONFIG_ASHMEM` was deleted from Linux in 5.18** (`torvalds/linux@721412e`, "staging: remove ashmem"). WSL2 ships 6.6 LTS / 6.18 rolling. **The config cannot be set at all** — the build would silently ignore the fragment or error on an unknown symbol. It was both a functional requirement and a success criterion.
- redroid marks ashmem **optional**, with `androidboot.use_memfd` as the runtime replacement.
- The plan **omitted** configs redroid does document as required: `CONFIG_ANDROID`, `CONFIG_ANDROID_BINDER_IPC`, `CONFIG_DMABUF_HEAPS_SYSTEM`, and several IPv6 options. Binderfs is not even selectable without binder IPC.

So the config table that justified the entire machine-global imposition was one-third impossible and otherwise incomplete. **It is entirely possible that a stock WSL2 6.6/6.18 kernel already runs redroid**, in which case most of this phase evaporates — including its defining risk.

## Requirements

- [ ] Functional: **step 1 — determine empirically whether a stock WSL2 kernel runs redroid at all**
- [ ] Functional: if a rebuild *is* needed, a kernel with the **verified** config set, matched to the user's WSL version
- [ ] Functional: if a rebuild is needed, consent UI explaining the machine-global effect **before** any change
- [ ] Functional: **key-level** `.wslconfig` modification and revert — never whole-file
- [ ] Functional: **a persisted mutation-intent journal** so an interrupted change is recoverable
- [ ] Functional: detect an existing third-party `kernel=` and stop rather than clobber
- [ ] Functional: **detect `networkingMode=mirrored`** and warn — it changes phase 18's exposure (S2)
- [ ] Non-functional: prebuilt `bzImage` shipped; users must not need a Linux toolchain
- [ ] Non-functional: no host change without an explicit, informed user action

## Architecture

### Step 1 is a gate, not a task

```
Does a stock WSL2 kernel (6.6 LTS / 6.18 rolling) run redroid?
  ├─ YES -> NO KERNEL REPLACEMENT. Skip the bzImage matrix, skip the
  │         version-match gate, skip most of the consent apparatus.
  │         Phase 17 shrinks to prerequisite detection + the
  │         networkingMode check. Effort: ~3 days.
  │         Provider B's defining risk disappears.
  └─ NO  -> Determine the ACTUAL minimal config delta (from redroid's
            current docs, not the stale list), then build the full
            consent/backup/revert/journal apparatus below.
            Effort: ~2 weeks.
```

Run step 1 before writing a single line of consent UI or a build script. The original plan had "confirm the config set empirically" as step 1 *after* freezing the list as a requirement and a success criterion.

### If a rebuild is needed — the config set

Source from **redroid's current documentation**, not the plan's original list. Known-required: `CONFIG_ANDROID`, `CONFIG_ANDROID_BINDER_IPC`, `CONFIG_ANDROID_BINDERFS`, `CONFIG_DMABUF_HEAPS`, `CONFIG_DMABUF_HEAPS_SYSTEM`, plus the documented IPv6 options. **Do not include `CONFIG_ASHMEM`** — it does not exist; use `androidboot.use_memfd` as a redroid *runtime* parameter instead. Verify the final set by actually booting redroid on the built kernel.

### The consent flow (only if a rebuild is needed)

`%USERPROFILE%\.wslconfig` `[wsl2]` settings apply to **every** WSL2 distribution — a developer's Ubuntu, their Docker Desktop backend, their work environment. That is a serious imposition and why Provider B is opt-in.

```
user enables Provider B
   ├─ detect WSL version, current kernel, existing kernel= and networkingMode
   ├─ EXPLAIN, before any change:
   │    "This replaces the Linux kernel for ALL WSL distributions on this
   │     computer, including Docker Desktop and any other distros. Your
   │     existing setting will be backed up and can be restored with one
   │     click. WSL will need to shut down, ending anything running in it."
   ├─ require explicit confirmation (not a pre-checked box)
   ├─ WRITE THE INTENT JOURNAL  <- before touching .wslconfig
   ├─ back up .wslconfig -> .wslconfig.mtplayer-backup-<timestamp>
   ├─ write ONLY the kernel= key, preserving all other keys and comments
   ├─ wsl --shutdown  (naming the distros currently running)
   ├─ verify: uname -r reports the expected kernel
   └─ on failure: restore + wsl --shutdown + report; clear the journal
```

**Key-level, not whole-file.** The original plan wrote carefully ("preserve unrelated keys and comments") and then reverted by restoring the whole backup — silently discarding anything the user changed afterward. A user who later adds `memory=8GB` for Docker would lose it on revert, precisely because the write path led them to expect such keys were safe. **Revert removes or restores only the `kernel=` key**; the backup is for diff and audit.

**The intent journal is what makes an interrupted mutation survivable.** The original auto-restore existed only inside the running process, so a crash between the write and the verification left machine-global broken WSL with nothing to recover it. Write `wslconfig-mutation-in-progress.json` (backup path, target key, manual revert command) *before* touching `.wslconfig`; check for it on every launch and offer recovery. Also drop a plain-text `HOW-TO-REVERT.txt` beside the backup, so recovery is possible with MTPlayer uninstalled and the docs unreachable.

**`networkingMode=mirrored` matters for security, not just networking.** Under mirrored mode the WSL instance takes a LAN IP, which turns phase 18's published container port into LAN-reachable unauthenticated root ADB. This phase already parses `.wslconfig`, so detect it here and warn — and note that the key-preservation rule means MTPlayer faithfully *keeps* mirrored mode while opening the port.

### Kernel sourcing (if needed)

Build from `microsoft/WSL2-Linux-Kernel` at the tag matching the user's WSL kernel version. WSL updates independently, so a shipped `bzImage` can fall behind. Ship prebuilts for tested versions and **refuse, with an explanation, on an untested version** rather than installing a mismatched kernel.

## Related Code Files

- Create: `spikes/redroid-stock-kernel/RESULTS.md` — **step 1's answer and the evidence**
- Create: `tools/wsl-kernel/config-fragment` — *only if needed*; the verified delta, with sources cited
- Create: `tools/wsl-kernel/build-kernel.sh` — *only if needed*; reproducible, in a Linux container
- Create: `tools/wsl-kernel/README.md` — maintainer procedure per WSL version
- Create: `crates/mtp-provider-wsl/` — **the crate is created here, not at phase 02**
- Create: `crates/mtp-provider-wsl/src/host.rs` — WSL detection, version check, `.wslconfig` key-level read/write/backup/restore, `networkingMode` detection
- Create: `crates/mtp-provider-wsl/src/journal.rs` — mutation-intent journal
- Create: `crates/mtp-provider-wsl/tests/wslconfig.rs` — parse/modify/revert fixtures incl. existing `kernel=`, user-added keys, `networkingMode`
- Create: `crates/mtp-ui/src/provider_b_consent.rs` — consent dialog and revert control
- Create: `docs/provider-b.md` — what changes, why, how to revert manually
- Create: `assets/kernels/` — *only if needed*; prebuilt `bzImage` per supported WSL version, with SHA-256
- Modify: `docs/troubleshooting.md` — **phase 16 requires every new `ConditionKind` to have a section**
- Modify: `Cargo.toml` — add `mtp-provider-wsl` to the workspace

## Implementation Steps

1. **STEP 1 — THE GATE.** Install redroid on a stock WSL2 distro (6.6 LTS and 6.18 rolling if both are reachable) with `androidboot.use_memfd=1`. Does it start? Record the answer, the kernel version, and the failure mode if any, in `spikes/redroid-stock-kernel/RESULTS.md`. **Everything below branches on this.**
2. **If stock works:** implement prerequisite detection (WSL present, version, container runtime) and the `networkingMode` check. Add the corresponding `ConditionKind` variants and their troubleshooting sections. **Skip steps 3-9 entirely** and record in `plan.md` that Provider B needs no kernel replacement — a material reduction in the plan's risk profile.
3. **If stock fails:** derive the actual minimal config delta from redroid's current docs plus the observed failure. Cite sources. **Never include `CONFIG_ASHMEM`.**
4. **Reproducible build** in a Linux container: clone at the matching tag, apply the fragment, build, emit `bzImage` + SHA-256 + the source tag. Fail the build if any requested symbol is unknown to the tree — silent ignoring is how the original defect would have shipped.
5. **Confirm redroid runs on the built kernel** before shipping it.
6. **WSL detection and version-match gate.** Map WSL versions to shipped kernels; refuse with an explanation on an unsupported version and point advanced users at the build script. **Never install a mismatched kernel** — a broken WSL affects the user's unrelated work.
7. **`.wslconfig` handling, key-level.** Parse the INI-ish format preserving unrelated keys and comments. Existing `kernel=`: absent -> proceed after consent; ours -> update; a third party's -> **stop and ask, showing the value.**
8. **Journal, then backup, then write.** In that order. Check for a stale journal on every launch and offer recovery.
9. **Consent, `wsl --shutdown`, verify, auto-restore on failure** — including the second `wsl --shutdown` the original plan omitted from the auto-restore path.
10. **Revert control** in settings: remove/restore only `kernel=`, `wsl --shutdown`, verify. Document the manual procedure too.
11. **Docs.** `docs/provider-b.md`: what changes, that it is machine-global (if applicable), how to revert, what breaks if WSL updates underneath it.

## Success Criteria

- [ ] **Step 1 answered and recorded with evidence** — does redroid run on a stock WSL2 kernel?
- [ ] If stock works: no kernel replacement ships; `plan.md` updated to reflect the reduced risk; consent apparatus not built
- [ ] If a rebuild is needed: the config set is **sourced from current redroid docs with citations**, contains no `CONFIG_ASHMEM`, and the build **fails loudly on an unknown symbol**
- [ ] If a rebuild is needed: redroid verified to start on the built kernel; prebuilt `bzImage` shipped with SHA-256 and source tag
- [ ] Consent dialog appears before **any** host change; declining changes nothing at all
- [ ] **The intent journal is written before `.wslconfig` is touched**; a process killed between write and verify is offered recovery on next launch
- [ ] `HOW-TO-REVERT.txt` beside the backup works with MTPlayer uninstalled
- [ ] Unrelated `.wslconfig` keys and comments survive the modification
- [ ] **User-added keys survive a revert** (revert is key-level, not whole-file)
- [ ] An existing third-party `kernel=` stops the flow and shows the value
- [ ] After install, `uname -r` reports the expected kernel; a failed verification auto-restores **and shuts WSL down again**
- [ ] An unsupported WSL version is refused with an explanation, not force-installed
- [ ] A machine without WSL reports Provider B unavailable, cleanly
- [ ] **`networkingMode=mirrored` is detected and warned about**, with the phase-18 exposure explained
- [ ] New `ConditionKind` variants each have a `docs/troubleshooting.md` section (phase 16's test)
- [ ] `.wslconfig` fixture tests pass in CI

## Risk Assessment

**Risk: the whole phase was scoped around an impossible config.**
Signal: already confirmed — `CONFIG_ASHMEM` does not exist in any kernel WSL2 ships.
Response: step 1 now gates everything. The pre-decided response to "stock works" is to **delete most of this phase**, not to build the apparatus anyway. This is a scope correction, not a cut of requested functionality.

**Risk: breaking a developer's existing WSL setup.**
Signal: Docker Desktop or a work distro failing after enabling Provider B.
Response: consent, journal, key-level write, backup, verification, auto-restore. **If any part cannot be made reliable, Provider B should not ship** — the feature is optional, the user's environment is not.

**Risk: a WSL update replaces the kernel or invalidates the custom one.**
Signal: Provider B breaking after a Windows/WSL update.
Response: detect a version mismatch at startup and report it with the revert option. This assumption *will* break eventually; the pre-decided response is to disable Provider B and tell the user, never to auto-reinstall.

**Risk: maintaining prebuilt kernels per WSL version becomes an ongoing burden.**
Signal: the supported-versions list going stale, refusing most users.
Response: may evaporate entirely at step 1. If not, it is a real argument for keeping Provider B opt-in and clearly labelled — and deprecating it later is legitimate, since Provider A satisfies R3 on any machine that can run WSL.

**Risk: `wsl --shutdown` destroys unsaved work.**
Signal: complaints about lost work in a running distro.
Response: warn explicitly, naming the distros currently running, and let the user choose the moment.
