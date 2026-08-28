---
title: "Phase 18: mtp-provider-wsl — redroid Containers"
status: todo
phase: 18
priority: P3
effort: "1-1.5 wk"
dependencies: [17]
---

# Phase 18: mtp-provider-wsl — redroid Containers

## Overview

Implement the second `DeviceProvider` over redroid containers in WSL2. **This phase is the test of phase 06's abstraction**: if the trait was designed correctly, `mtp-ui` needs zero changes and the shell cannot tell which provider is running.

The payoff is boot in under 10 seconds and very cheap N-instance testing — five instances on 16 GB, versus three heavy VMs.

## Requirements

- [ ] Functional: `mtp-provider-wsl` implements all six `DeviceProvider` methods
- [ ] Functional: container lifecycle — create, start, stop, remove
- [ ] Functional: ADB port mapping from container to host
- [ ] Functional: snapshot/restore mapped onto container or volume semantics
- [ ] Functional: backend selectable in settings; the shell behaves identically
- [ ] Non-functional: **zero changes to `mtp-ui`** — any change needed is a phase-06 design defect to record
- [ ] Non-functional: boot to `Ready` in under 10 s; five concurrent instances on 16 GB

## Architecture

```
mtp-ui  ──>  DeviceProvider  ──┬──> mtp-provider-vm   (Provider A)
                               └──> mtp-provider-wsl  (Provider B)
                                        │
                              wsl.exe -d <distro> -- docker run redroid...
                                        │
                              redroid container, adbd on 5555
                                        │
                              port mapping -> 127.0.0.1:<host port>
```

**Everything above the trait is untouched.** scrcpy works identically inside a container — it only needs ADB and an Android >= 5 userspace — which is exactly why the phase-01 display decision made the backends interchangeable.

### Mapping the trait onto containers

| Trait method | Provider A (VM) | Provider B (container) |
|---|---|---|
| `start` | spawn VMM, poll boot | `docker run`, poll boot |
| `stop` | ACPI powerdown, then kill | `docker stop` (SIGTERM, then kill) |
| `status` | process + ADB state | container state + ADB state |
| `adb_endpoint` | host-forwarded port | published container port |
| `snapshot` | copy qcow2 overlay | `docker commit`, or copy the data volume |
| `restore` | copy overlay back | recreate from image/volume |

**Snapshot semantics differ and must be stated, not papered over.** A VM snapshot captures the full disk; `docker commit` captures the container filesystem layer. For a dev tool the practical effect is similar, but they are not identical, and the UI should not imply cross-provider snapshot portability. **Snapshots are provider-specific — refuse to restore a Provider A snapshot on Provider B.**

### Port mapping

**Bind loopback explicitly (S2).** Publish as **`-p 127.0.0.1:<host>:5555`**, never `-p <host>:5555`. The bare form binds `0.0.0.0` inside the WSL VM, and under `networkingMode=mirrored` the WSL instance takes a LAN IP — turning the guest's unauthenticated root ADB into something any machine on hotel or office Wi-Fi can reach, with `adb pull` access to the host folder phase 10 syncs. Phase 01 pins `hostfwd=tcp:127.0.0.1:` for Provider A, so omitting it here was an inconsistency, not a shared assumption.

The original plan asked only whether `127.0.0.1:<port>` *worked*, never what it bound to. Both questions matter: verify reachability from the host **and** verify non-reachability from a second machine. Phase 17 detects `networkingMode=mirrored` and warns; this phase must be safe even when it is set.

Allocate the host port via phase 02's **hold-until-bound** mechanism (not bind-then-release, which is a TOCTOU) so phase 12's instance manager needs no special case. WSL2 localhost forwarding normally presents the container on host loopback — **verify rather than assume**, since it has historically been a source of trouble.

### redroid image source

Pulled from a container registry, which is why GHCR was noted as phase 14's secondary hosting option — the infrastructure is shared. Pin the redroid image tag and API level; do not track `latest`.

## Related Code Files

- Create: `crates/mtp-provider-wsl/src/lib.rs` — the `DeviceProvider` impl
- Create: `crates/mtp-provider-wsl/src/container.rs` — run/stop/rm, inspect, port publishing
- Create: `crates/mtp-provider-wsl/src/snapshot.rs` — commit/volume-copy semantics
- Create: `crates/mtp-provider-wsl/src/wsl_exec.rs` — invoke commands inside the distro, capture output, handle absence
- Create: `crates/mtp-provider-wsl/tests/mapping.rs` — trait-mapping tests with a fake docker CLI
- Modify: `docs/provider-b.md` — extend phase 17's doc with capabilities and differences
- Modify: `docs/troubleshooting.md` — **required**: this phase adds three `ConditionKind` variants (no WSL, wrong kernel, no container runtime) and phase 16's test asserts every variant has a section. Omitting this breaks that test by construction
- Modify: `crates/mtp-core/src/snapshot.rs` — populate `SnapshotMeta.provider` (the field exists from phase 11); refuse cross-provider restore
- Modify: `crates/mtp-core/src/provider.rs` — **only if a defect is found**; record any change as a phase-06 design lesson
- Modify: `crates/mtp-ui/src/` — backend selector in settings **only**; no functional changes

## Implementation Steps

1. **Prerequisite detection.** WSL present, the custom kernel active (phase 17), a container runtime available inside the distro. Each missing piece gets a specific message, consistent with phase 16's `Remedy` structure.
2. **`wsl_exec` helper.** Run a command in a named distro, capture stdout/stderr/exit code, with a timeout. Every container operation goes through it. Handle a distro that is stopped (WSL starts it on demand) and one that does not exist.
3. **Container run.** Pinned redroid image and tag, published ADB port, memory and CPU limits from the `DeviceProfile`, a named data volume per instance. Map profile resolution/DPI to redroid's own parameters where it accepts them, and fall back to phase 10's `wm size`/`wm density` path otherwise — reuse, do not reimplement.
4. **Boot detection — reuse phase 06's `sys.boot_completed` polling.** Do not write a container-specific heuristic; the polling code belongs in `mtp-core` if it is not there already.
5. **Status mapping.** Container state plus ADB reachability onto the phase-06 state machine, including `Failed`/`Crashed` with a reason taken from container logs.
6. **Port verification.** Confirm `127.0.0.1:<port>` on the *host* reaches the container's adbd. If WSL localhost forwarding is unreliable, add an explicit forward. Verify empirically.
7. **Snapshot mapping.** Implement via `docker commit` or a volume copy — choose based on which restores more reliably, and record the choice. Tag snapshots with their provider and refuse cross-provider restore.
8. **Backend selector** in settings. Switching affects newly created instances; existing instances stay with their provider. Persist the provider per instance in the registry.
9. **The zero-UI-change audit.** Diff `mtp-ui`: anything beyond the settings selector is a phase-06 trait defect. **Record each one in this file** — that is the abstraction lesson worth keeping, and papering over it silently loses the finding.
10. **Scale test.** Five concurrent instances on 16 GB: measure boot time, RSS, and whether all five ADB endpoints work simultaneously.
11. **Fake-docker tests.** A stub CLI returning canned JSON lets trait mapping and status parsing be tested in CI without WSL.

## Success Criteria

- [ ] All six trait methods implemented and working against real redroid containers
- [ ] Boot to `Ready` in under 10 s
- [ ] Five concurrent instances on a 16 GB host, all five ADB endpoints reachable
- [ ] scrcpy video and input work identically to Provider A, with no `mtp-scrcpy` changes
- [ ] APK install, ABI gate, logcat, file transfer, and profiles all work unchanged
- [ ] Snapshot and restore work within Provider B; a cross-provider restore is refused with a clear reason
- [ ] Backend switch in settings works; existing instances keep their original provider
- [ ] **`mtp-ui` diff contains only the settings selector** — any other change recorded here as a phase-06 defect
- [ ] Missing prerequisites (no WSL, wrong kernel, no container runtime) each give a specific message
- [ ] `127.0.0.1:<port>` verified reachable from the host, not assumed
- [ ] **The published port is NOT reachable from a second machine on the LAN** — tested, including with `networkingMode=mirrored` set (S2)
- [ ] `docs/troubleshooting.md` has a section for each new `ConditionKind` (phase 16's test passes)
- [ ] Boot detection reuses phase 06's `sys.boot_completed` polling — no container-specific heuristic
- [ ] Instance identity uses the per-instance identifier, not `ro.serialno` (same rule as phase 12)
- [ ] Stopping MTPlayer removes or stops all containers it started
- [ ] Trait-mapping tests pass in CI with the fake docker CLI

## Risk Assessment

**Risk: the phase-06 trait leaks VM assumptions and `mtp-ui` needs changes.**
Signal: needing to add a method, or return "unsupported".
Response: this phase is the test, and the step-9 audit is how the result gets recorded. A small, well-understood trait change is acceptable; **discovering that snapshots or lifecycle cannot be expressed at all would mean the abstraction failed** — record it plainly rather than hiding it behind provider-specific UI branches.

**Risk: WSL2 localhost forwarding does not reliably expose the container port.**
Signal: `adb connect 127.0.0.1:<port>` failing from the host while working inside the distro.
Response: verify explicitly (step 6). Fall back to an explicit port forward or a proxy inside the distro. This has historically been flaky, so do not assume it works.

**Risk: redroid image updates break the integration.**
Signal: Provider B failing after a registry pull.
Response: pin the image tag; never track `latest`. Treat an image bump as a deliberate, tested change.

**Risk: container snapshot semantics quietly differ from VM snapshots and users lose data.**
Signal: a restore producing an unexpected state, or a cross-provider restore attempt.
Response: provider-tagged snapshots and refused cross-provider restore (step 7). Document the difference in `docs/provider-b.md` rather than implying equivalence.

**Risk: this phase silently becomes the primary backend because it is faster.**
Signal: development effort shifting to Provider B while Provider A stagnates.
Response: Provider A ships v1 and satisfies R3 on any machine that can run WSL, because both need the same underlying hypervisor. Provider B stays opt-in — it mutates the user's machine globally (phase 17), and that is disqualifying as a default regardless of speed.
