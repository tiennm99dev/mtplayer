---
title: "Phase 11: Snapshot, Restore, Factory Reset"
status: todo
phase: 11
priority: P1
effort: "5-8 d"
dependencies: [10]
---

# Phase 11: Snapshot, Restore, Factory Reset

## Overview

The single most valuable dev-tool feature: get a device into a state, save it, break it, and get back instantly. **If schedule pressure hits P2, cut phase 13b before this phase** (13a is not cuttable — phases 14/15/16 depend on its artifacts).

The disk model from phase 07 — immutable base plus per-instance overlay — makes the reliable version nearly free.

## Requirements

- [ ] Functional: factory reset — delete and recreate the overlay; near-instant
- [ ] Functional: named snapshot, restore, list, rename, delete with real sizes
- [ ] Functional: `snapshot`/`restore` implemented on the phase-06 trait
- [ ] Functional: **all disk operations go through `DiskOps`** — never a bare `qemu-img` call
- [ ] Functional: **cross-process safety** — lease held *and* exclusive overlay open before any destructive op
- [ ] Functional: **a non-destructive recovery path for a base-SHA mismatch**
- [ ] Non-functional: base image never mutated — verify by SHA after any operation
- [ ] Non-functional: RAM-state fast resume is a **stretch goal**, not a requirement

## Architecture

```
system.qcow2  (immutable base, shared by all instances, never written)
      │
      ├── instance-a.qcow2   (overlay, backing = base)
      │        └── snapshots/instance-a/<name>.qcow2   (copy of the overlay)
      └── instance-b.qcow2
```

- **Factory reset** = delete the overlay, create a fresh one via `DiskOps::create_overlay`. Instant and robust.
- **Named snapshot** = `DiskOps::copy_overlay` while the VM is stopped.
- **Restore** = copy the snapshot back over the overlay while stopped, atomically.

### `DiskOps`, not `qemu-img`

The original plan wrote this model entirely in `qemu-img` terms while `plan.md` declared the VMM **OPEN**. If crosvm won, that meant either shipping GPLv2 `qemu-img.exe` — reinstating the exact obligation crosvm was chosen to avoid, with no phase-15 coverage — or rewriting overlay/snapshot/reset mid-P2. Phase 06 defines `DiskOps`; this phase calls only that.

### Cross-process safety — the `Stopped` precondition needs teeth

"Require `Stopped`" is an in-process enum check. The persisted registry has no status field, and once phase 13 makes the CLI a second writer, a CLI `restore` against a UI-running instance would rename a qcow2 out from under a live VMM: the rename succeeds, the VMM keeps writing to the unlinked file, the guest looks normal for an hour, then the filesystem corrupts — the exact delayed failure this section warns about.

**Every destructive operation must, in order:**
1. Acquire the phase-06 **per-instance lease**. If it cannot be acquired, an owner is live — **refuse**, regardless of what the registry says.
2. Open the overlay file **exclusively**. If that fails, refuse. Belt and braces, because this failure is silent and unrecoverable.
3. Only then operate.

Copying a qcow2 under a running VMM is the worst failure mode available here, so there is deliberately **no "snapshot while running" convenience**.

### Base-SHA mismatch — recovery, not annihilation

Phase 11 records the base SHA per instance and per snapshot and refuses on mismatch. The original plan's only remedy was factory reset — which deletes all guest data *and*, because every `SnapshotMeta` carries the old `base_sha`, simultaneously bricks every snapshot for that instance. The feature designed to survive breakage failed permanently on the one event it existed for. An antivirus quarantine-and-restore altering one byte would trigger it.

**Remedy ladder, in this order:**

1. **Re-verify against the manifest.** The manifest carries `uncompressed_sha256`; recompute and compare. A transient read error is not a corrupt base.
2. **Repair in place.** Re-download the base and restore it to its **original id** when the incoming content matches the *recorded* SHA. This is non-destructive and restores every overlay and snapshot. Phase 14's no-overwrite rule must permit this specific case — repair to the recorded hash is not the same as replacing a base with different content.
3. **Rebase deliberately.** For an intentional image refresh, offer `DiskOps`-level rebase (repointing the backing file) with a clear warning.
4. **Factory reset** — offered **last**, never as the only option.

Distinguish "base changed" from "base damaged" in the message. The user cannot act sensibly on a conflation of the two.

### `SnapshotMeta` — record what validity depends on

```rust
struct SnapshotMeta {
    name: String,
    created: DateTime,
    size: u64,          // actual allocated, not virtual
    image_id: String,
    base_sha: String,
    provider: ProviderKind,   // phase 18: refuse cross-provider restore
    vmm_kind: String,         // required for RAM-state validity
    vmm_version: String,      // required for RAM-state validity
}
```

`provider`, `vmm_kind`, and `vmm_version` were all absent originally. Without the last two, the RAM-state decision rule below asks for a version-mismatch check that nothing could perform — and a phase-15 upgrade would silently invalidate every RAM-state snapshot.

### RAM-state fast resume — a stretch goal, explicitly

`savevm`/`-loadvm` gives sub-second resume instead of a 30-second boot. Genuinely valuable, but the state file is RAM-sized, tightly coupled to the exact VMM version and machine config, and silently invalid if either changes.

**It is not a requirement and not a success criterion.** The original plan bound it as both while its own prose called it "the stretch" — inside the phase that must not be cut, with a three-way validation protocol including a host reboot. The overlay model is the deliverable.

**Adopt only if** it survives (a) a VMM version bump, (b) a host reboot, and (c) a deliberately corrupted state file falling back to a cold boot rather than hanging. Otherwise record the rejection and move on.

**If phase 01 chose crosvm, skip the evaluation entirely.** crosvm's snapshotting is upstream-declared "100% not supported" with an unstable format — phase 01 was required to record that consequence, so this arrives as a decision, not a discovery.

## Related Code Files

- Create: `crates/mtp-provider-vm/src/disk.rs` — overlay lifecycle via `DiskOps`; base SHA verification
- Create: `crates/mtp-provider-vm/src/snapshot.rs` — snapshot store, `SnapshotMeta`, RAM-state probe (feature-gated)
- Create: `crates/mtp-core/src/snapshot.rs` — `SnapshotId`, `SnapshotMeta`, the remedy ladder as typed states
- Create: `crates/mtp-provider-vm/tests/disk_ops.rs` — overlay/snapshot ops over throwaway files
- Create: `crates/mtp-provider-vm/tests/destructive_guards.rs` — **lease-held and exclusive-open refusals**
- Create: `crates/mtp-ui/src/snapshot_panel.rs` — list, create, restore, delete, factory reset, stop-required prompt, remedy ladder UI
- Modify: `crates/mtp-provider-vm/src/lib.rs` — implement `snapshot()`/`restore()`
- Modify: `crates/mtp-core/src/registry.rs` — record overlay and snapshot paths per instance

## Implementation Steps

1. **All disk file access behind `disk.rs`**, which calls `DiskOps`. Scattered external-binary calls become impossible to reason about once snapshots exist.
2. **Base immutability.** Record the base SHA-256 in the registry at first use; re-verify on start. On mismatch, enter the remedy ladder — do not boot a suspect chain.
3. **Factory reset.** Guards (lease + exclusive open), then delete and recreate the overlay, keeping `resolved_profile`. Destructively-worded confirmation — this deletes guest data.
4. **Named snapshot.** Guards, then `copy_overlay` into `snapshots/<instance>/<name>.qcow2` with full `SnapshotMeta` including provider, vmm kind, and version.
5. **Restore.** Guards, validate `image_id` + `base_sha` + `provider` (+ `vmm_*` for RAM-state), refuse with a clear reason on mismatch. **Write to a temp name and rename**, so an interrupted restore leaves either the old overlay or the new one — never a corrupt file.
6. **Remedy ladder** implemented as the four steps above, surfaced in the UI in that order, with "base damaged" and "base changed" worded differently.
7. **Snapshot listing** with **actual allocated** sizes — qcow2 overlays are sparse, and reporting virtual size makes a 200 MB snapshot look like 8 GB.
8. **UI honesty about stopping.** Any operation needing the lease prompts, and offers stop -> operate -> restart as one action.
9. **RAM-state probe (optional, feature-gated).** Only if phase 01 chose a VMM that supports it. Test against the three-part rule; record the outcome either way.
10. **Cleanup.** Deleting an instance deletes its overlay and snapshots; deleting the last instance using a base does **not** delete the base.
11. **Tests** over throwaway qcow2 files, plus a second-process test proving the guards refuse.

## Success Criteria

- [ ] Factory reset completes in ~2 s and yields a clean-boot device
- [ ] Snapshot -> install an app and change settings -> restore -> app and changes gone
- [ ] Base image SHA-256 unchanged after a full session of snapshot/restore/reset cycles
- [ ] **All disk operations route through `DiskOps`** — no direct external-binary invocation in `disk.rs` callers
- [ ] **A restore attempted from a second process while the instance runs is refused** (lease guard), and refused again if the lease is somehow free but the overlay is open (exclusive-open guard)
- [ ] Restore against a mismatched base, image id, or provider is refused with a specific reason
- [ ] Interrupting a restore (kill mid-copy) leaves the old overlay or the new one intact — never corrupt
- [ ] **A corrupted base offers re-verify and repair-in-place before factory reset**, and repair restores existing snapshots to working order
- [ ] "Base damaged" and "base changed" produce different messages
- [ ] `SnapshotMeta` records provider, vmm kind, and vmm version
- [ ] Snapshot list shows actual allocated sizes
- [ ] Deleting an instance removes its overlay and snapshots but never the shared base
- [ ] RAM-state resume either adopted after passing all three tests, or **explicitly rejected with the reason recorded** (or skipped outright if phase 01 chose crosvm)
- [ ] Disk-op and destructive-guard tests pass in CI

## Risk Assessment

**Risk: a snapshot taken against a running VM corrupts later.**
Signal: a restored snapshot boots, then the guest filesystem fails hours later.
Response: the lease + exclusive-open guards, enforced inside `disk.rs` rather than as a caller convention. This failure is delayed and looks unrelated to snapshotting, which makes it exceptionally expensive to diagnose.

**Risk: an interrupted restore destroys the working overlay.**
Signal: crash mid-restore leaves an unbootable instance.
Response: temp-write-then-rename, verified by the kill-mid-copy criterion. Cheap to get right, unrecoverable if wrong.

**Risk: RAM-state resume looks fine in development and breaks on user machines.**
Signal: works locally; users report hangs after an app update.
Response: the three-part rule, and it is no longer a requirement. Version-coupled state that hangs on mismatch is worse than a 30-second cold boot. Reject rather than ship half-verified — and note that `vmm_version` in `SnapshotMeta` is what makes the mismatch detectable at all.

**Risk: crosvm was chosen and snapshotting is unusable.**
Signal: phase 01's `RESULTS.md` records it.
Response: overlay-copy-only, RAM-state skipped. This is why phase 01 now weights snapshot support as a primary criterion instead of merely noting it.

**Risk: snapshot storage fills the disk unnoticed.**
Signal: users out of space with no visible cause.
Response: show total snapshot storage in the panel; warn below a free-space threshold. Never auto-delete. Note phase 14's disk precheck must account for snapshot growth between its check and its decompress step.
