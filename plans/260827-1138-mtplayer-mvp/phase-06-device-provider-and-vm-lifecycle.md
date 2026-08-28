---
title: "Phase 6: DeviceProvider Trait and VM Lifecycle"
status: todo
phase: 6
priority: P1
effort: "1-1.5 wk"
dependencies: [5]
---

# Phase 6: DeviceProvider Trait and VM Lifecycle

## Overview

Promote the phase-02 launch harness into a real provider behind the `DeviceProvider` trait, with correct lifecycle: start, boot detection, graceful stop, and orphan cleanup. This trait is the contract that makes phase 18 cheap.

**Four red-team findings land here, three of them Critical.** The original design had a supervision model that killed CLI-started instances, an orphan reaper that killed *live* instances, no cross-process enforcement for the `Stopped` precondition phase 11 depends on, and an ambiguous owner for a running instance's profile. All four are resolved below, before any provider code exists.

## Requirements

- [ ] Functional: `DeviceProvider` trait; `mtp-provider-vm` implements start/stop/status/adb_endpoint (snapshot/restore land in phase 11)
- [ ] Functional: **a supervision model that survives the launching process exiting** (CLI) while still cleaning up on crash (UI)
- [ ] Functional: **handle-scoped process termination** (S6) — never kill-by-pid
- [ ] Functional: **liveness leases** so a second MTPlayer process cannot reap live instances
- [ ] Functional: **atomic registry writes** and a per-instance lock usable cross-process
- [ ] Functional: boot-complete detection by polling `sys.boot_completed` — no fixed-duration substitute
- [ ] Functional: graceful shutdown (ACPI powerdown, then timeout, then handle-scoped kill)
- [ ] Functional: `DiskOps` abstraction — no unconditional `qemu-img`
- [ ] Functional: **`resolved_profile`** captured immutably at start
- [ ] Functional: guest clocksource verified; no A/V drift over 30 minutes
- [ ] Non-functional: no method blocks the UI thread; start reports progress states
- [ ] Non-functional: the trait mentions nothing QEMU-, crosvm-, or VM-specific

## Architecture

### The trait

```rust
trait DeviceProvider {
    fn start(&mut self, profile: &DeviceProfile) -> Result<InstanceHandle>;
    fn stop(&mut self, id: InstanceId) -> Result<()>;
    fn status(&self, id: InstanceId) -> Result<InstanceStatus>;   // Result, not bare
    fn adb_endpoint(&self, id: InstanceId) -> Option<SocketAddr>;
    fn snapshot(&mut self, id: InstanceId, name: &str) -> Result<()>;   // phase 11
    fn restore(&mut self, id: InstanceId, name: &str) -> Result<()>;    // phase 11
}
```

`status()` returns `Result`, not a bare `InstanceStatus`. The original signature had no representation for an unknown `InstanceId`, yet phase 13 specifies exit code 4 as "instance not found" — an unreachable state through the contract.

**This trait is expected to change at phase 18**, with a recorded diff. The original plan's success criterion "trait reviewed against Provider B needs" was unverifiable 12 phases before Provider B existed; it is replaced by the checkable property (no VM-specific vocabulary) plus an explicit expectation of change. Phases 19 and 21 add to `provider.rs` — those additions are named in their own files, not left as "additive only" across a three-month gap.

### `InstanceStatus` — a state machine, and only that

```
Stopped -> Starting -> Booting -> Ready -> Stopping -> Stopped
                  \                    \
                   -> Failed(reason)    -> Crashed(reason)
```

`Starting` = process spawned. `Booting` = ADB reachable, `sys.boot_completed` still 0. `Ready` = boot complete.

**`RenderPath` does not go here.** Phase 19 originally specified adding "a `RenderPath { Hardware, Software }` **field** on the instance status" — impossible, this is an enum. `RenderPath` belongs on `InstanceHandle`; phase 19 is corrected accordingly.

### Supervision — resolving the job-object contradiction

The original plan made a Windows job object with `KILL_ON_JOB_CLOSE` the *primary* mechanism, "far more reliable than any userspace reaping." But phase 13 requires `mtplayer start` to spawn an instance, print an id, and **exit** — closing the job handle and killing the VM. Every CI pipeline would have failed on its second command, and phase 06's own success criterion tested only the UI case, so it would have passed review.

**Resolution: the VMM child breaks away from the launching process's job.**

- Spawn with `CREATE_BREAKAWAY_FROM_JOB`; the VMM is *not* in the launcher's job.
- Cleanup therefore rests on **leases + handle-scoped reaping**, not on the OS killing the job.
- Trade-off accepted deliberately: a hard-killed UI leaves a VM running until the next launch reaps it. That is strictly better than a CLI that cannot start a durable instance.

A broker service that owns the job and serves both front-ends over a named pipe was considered and rejected for v1 — it is a whole new IPC surface and a service lifecycle to get wrong, for a benefit the lease model already provides.

### Reaping — liveness, not identity

The original rule ("match pid **and** executable path **and** start time before killing") proves *identity*, not *liveness* or *ownership*. Since phase 13 makes the CLI a second process over the same registry, every CLI invocation is a "startup" — and `mtplayer list` would match all three fields against genuinely running UI instances and kill them, destroying the user's guests.

**Resolution — a lease, held as an OS handle:**

```
%APPDATA%/MTPlayer/instances/<id>.toml     state (atomic write: temp + rename)
%APPDATA%/MTPlayer/instances/<id>.lease    exclusively opened by the OWNING process
```

- The owner opens `<id>.lease` exclusively and holds it for the instance's lifetime.
- An entry is **reapable only if the lease can be acquired** — meaning no live owner exists.
- The TOML additionally records `owner_pid`, `vmm_pid`, `vmm_image_path`, `vmm_start_time`.
- Reaping is then: acquire lease -> if acquired, the owner is gone -> **open the VMM process handle, verify image path and creation time *through the handle*, terminate *through the handle*** (S6). Never `TerminateProcess` by a pid resolved earlier — on Windows the pid binding is only stable while a handle is held, so check-then-kill-by-pid is a TOCTOU.
- **The registry is untrusted input.** It lives in a user-writable directory, so a planted pid+path+time triple could otherwise direct MTPlayer to kill a chosen process. The lease gate plus handle-scoped verification closes it; do not treat the TOML as authoritative for anything but hints.

### `Stopped` needs cross-process enforcement (for phase 11)

Phase 11 gates factory reset, snapshot, and restore on `Stopped` — but that is an in-process enum value. The persisted registry has no status field, and the original plan introduced locking only at phase 13, touching `registry.rs` and never `disk.rs`. A CLI `restore` against a UI-running instance would have renamed a qcow2 out from under a live VMM: the rename succeeds, the VMM keeps writing to the unlinked file, the guest looks fine for an hour, then the delayed corruption phase 11 warns about.

**Resolution, established here rather than at phase 13:**
- The per-instance lease **is** the cross-process status signal. If the lease cannot be acquired, an owner is live — refuse destructive operations regardless of what the TOML says.
- Destructive disk operations additionally **open the overlay file exclusively** before touching it. If exclusive open fails, refuse. Belt and braces, because this failure is unrecoverable and silent.
- Registry writes are atomic (temp + rename) from phase 06, not phase 13 — an interrupted write must not truncate the file. *(Moved here per red-team D15: if phase 13b is cut, this must not be lost.)*

### `resolved_profile` — one owner for a running instance's resources

The original plan had the registry record "the profile" while phase 10 made profiles independently-owned, live-mutable TOML files, and phase 12's RAM guard summed "running profiles' ram." Editing a template while an instance ran would have made the guard subtract a number the VMM never had — refusing legitimate instances while reporting false figures, or over-committing and swapping.

**Resolution:** `start()` **copies** the effective profile into the registry entry as immutable `resolved_profile` for the instance's lifetime. `profiles/*.toml` are **templates only**. Phase 12's guard sums `resolved_profile.ram_mb` from the registry. Phase 10's live-apply writes to the guest and to the template, and the delta against `resolved_profile` is what drives the restart-required badge.

### `DiskOps`

Overlay creation must not hard-code `qemu-img` while the VMM is undecided (see `plan.md` Risks). Define here:

```rust
trait DiskOps {
    fn create_overlay(&self, base: &Path, overlay: &Path) -> Result<()>;
    fn copy_overlay(&self, from: &Path, to: &Path) -> Result<()>;
}
```

Implementations: `qemu-img` (if QEMU), `crosvm create_qcow2 --backing-file` (if crosvm), and preferably an in-process qcow2 writer so neither external binary is needed. Phases 07, 11, and 14 call only this.

## Related Code Files

- Create: `crates/mtp-core/src/provider.rs` — trait, `InstanceId`, `InstanceHandle` (incl. `RenderPath`), `InstanceStatus`
- Create: `crates/mtp-core/src/registry.rs` — atomic writes, lease acquire/hold/release, handle-scoped reaping
- Create: `crates/mtp-core/src/disk_ops.rs` — the `DiskOps` trait + implementations
- Create: `crates/mtp-core/src/profile.rs` — `DeviceProfile` (fields completed in phase 10), `resolved_profile` capture
- Create: `crates/mtp-provider-vm/src/lib.rs` — the provider impl
- Create: `crates/mtp-provider-vm/src/process.rs` — breakaway spawn, handle retention, graceful stop, handle-scoped kill
- Create: `crates/mtp-provider-vm/src/boot.rs` — boot-complete polling
- Create: `crates/mtp-provider-vm/tests/lifecycle.rs` — state machine + reaping with a fake VMM
- Create: `crates/mtp-core/tests/registry_concurrency.rs` — two-process lease and atomic-write tests
- Modify: `crates/mtp-core/src/launch.rs` — move VMM argv rendering into `mtp-provider-vm`, delete from core
- Modify: `crates/mtp-ui/src/app.rs` — drive the provider instead of the phase-02 harness

## Implementation Steps

1. **Define the trait**, with `status() -> Result<InstanceStatus>`. Review method *names* against Provider B for VM-specific vocabulary — a checkable property. Record that the trait is expected to change at phase 18.
2. **`InstanceStatus`** with explicit transitions and `Failed`/`Crashed` carrying a human-readable reason. Phase 13 maps these to distinct exit codes, so the variants must stay distinguishable.
3. **Registry with atomic writes and leases.** Write the TOML temp+rename. Open the lease exclusively *before* spawning, so a crash between spawn and write still leaves a reapable-but-locked entry rather than an untracked VM.
4. **Breakaway spawn.** `CREATE_BREAKAWAY_FROM_JOB`. **Retain the process handle** for the instance's lifetime — it is both the liveness signal and the safe kill target.
5. **Handle-scoped reaping (S6).** On startup: for each registry entry, try to acquire the lease. If it cannot be acquired, **skip — an owner is live.** If acquired, `OpenProcess` the recorded `vmm_pid`, verify image path and creation time *through the handle*, then `TerminateProcess(handle)`. Release and delete the entry.
6. **Boot-complete polling.** Once ADB answers, poll `getprop sys.boot_completed` every ~500 ms with an overall timeout (~180 s, profile-configurable). Report `Booting` throughout. On timeout, `Failed` with the last property values collected.
7. **Graceful stop.** ACPI powerdown via the VMM monitor (named pipe on Windows, not a unix socket), wait ~15 s, then handle-scoped kill. Remove the endpoint, release the lease, delete the entry — in that order, only after the process is confirmed gone.
8. **`resolved_profile` capture** at start. Nothing downstream reads the template for a running instance.
9. **`DiskOps`** with the implementation selected by the phase-01 VMM decision. Phases 07/11/14 call only this trait.
10. **Clocksource verification.** Check the guest's selected clocksource; compare guest vs host wall clock over 30 minutes and watch A/V sync. WHPX timer drift is a known class of problem and finding it here is far cheaper than at phase 19.
11. **Two-process tests.** A fake VMM executable plus a test that runs a second process against the same registry — the case that would have destroyed live instances.

## Success Criteria

- [ ] `start()` -> `Ready` against the phase-01 guest; status progresses Starting -> Booting -> Ready
- [ ] Boot detection polls `sys.boot_completed`; **no fixed-duration wait substitutes for the check** (the poll interval and the overall timeout are the only time-based waits — the original "grep confirms no sleep anywhere" criterion could not pass on correct code)
- [ ] **A CLI-style launcher that starts an instance and then exits leaves the instance running** (breakaway verified)
- [ ] **Starting a second MTPlayer process while instances are running kills nothing** (lease gate verified)
- [ ] A simulated leaked VM (owner killed, lease free) is reaped; a same-pid unrelated process is **not** killed
- [ ] Reaping opens a handle and verifies through it — no `TerminateProcess` on a pid resolved earlier
- [ ] A registry entry with a hand-planted pid/path/time triple **does not** cause an unrelated process to be killed
- [ ] Interrupting a registry write leaves the old file intact, never a truncated one
- [ ] Destructive disk ops refuse when the lease is held **and** when the overlay cannot be opened exclusively
- [ ] `stop()` exits gracefully within 15 s; endpoint removed; lease released; entry cleared
- [ ] Boot timeout produces `Failed` with a diagnosable reason, not a hang
- [ ] `status()` on an unknown id returns an error (phase 13 exit code 4 is reachable)
- [ ] `resolved_profile` immutable: editing the template mid-run does not change the instance's recorded RAM
- [ ] Overlay creation goes through `DiskOps`; swapping the implementation needs no caller changes
- [ ] 30-minute run: guest clock within a few seconds of host; no observable A/V drift
- [ ] Trait contains no VM-specific vocabulary
- [ ] Lifecycle and two-process concurrency tests pass in CI with the fake VMM

## Risk Assessment

**Risk: breakaway spawn means a hard-killed UI leaves a VM running.**
Signal: Task-Manager-killed MTPlayer, VM still holding 4 GB until the next launch.
Response: accepted and deliberate — the alternative broke the CLI entirely. The lease makes the leak reliably reclaimable on next launch. If this proves unacceptable in practice, the broker-service design is the escalation, not a return to the launcher's job.

**Risk: the lease does not behave as expected across processes on Windows.**
Signal: two processes both acquiring, or a lease surviving process death.
Response: use an exclusively-opened file handle (the OS releases it on process death, including a hard kill) rather than an advisory lock or a lockfile-with-pid. Verify the hard-kill case explicitly — a lease that outlives its owner would make every instance permanently unreapable.

**Risk: the trait leaks VM assumptions and phase 18 needs more than an additive change.**
Signal: `mtp-provider-wsl` needs a method removed or its semantics changed.
Response: expected and now stated. Phase 18 records the diff as a design lesson. The pre-decided response is to change the trait and note it — not to add provider-specific branches in `mtp-ui`, which is what would actually damage the architecture.

**Risk: WHPX timer drift is unfixable.**
Signal: guest clock diverging; A/V desync worsening over time.
Response: pin a known-good clocksource via kernel cmdline. If drift persists across clocksources, record it as a documented limitation now rather than discovering it at phase 19.

**Risk: boot timeouts too aggressive on slow hardware.**
Signal: `Failed` on guests that would have booted.
Response: 180 s is a starting value, configurable via `boot_timeout_secs` in the profile (phase 10). The timeout message must state what it was waiting for and how to raise it.
