---
title: "Phase 1: Cuttlefish-on-Windows Feasibility Spike (GATED)"
status: todo
phase: 1
priority: P1
effort: "2-3 wk (hard stop at 3 wk)"
dependencies: []
---

# Phase 1: Cuttlefish-on-Windows Feasibility Spike

# ⚠️ THIS PHASE CAN KILL THE PROJECT'S IMAGE STRATEGY. IT IS NOT A BAKE-OFF.

## Overview

Answer one question: **can an AOSP x86_64 Android image reach `sys.boot_completed=1` on Windows under WHPX, and accept an authenticated ADB connection?**

The original version of this phase assumed the answer was yes and asked "QEMU or crosvm." The red team established that the premise was unfounded:

- **Cuttlefish is documented Linux + KVM only.** Windows is not a supported host.
- **Cuttlefish expects `vhost-vsock`, a file-backed `nvdimm` for ramoops/pstore, and TAP networking.** None exist on a Windows host — `memory-backend-file` is *excluded from Windows builds* (`if host_os != 'windows'` in `backends/meson.build`), `egl-headless` needs gbm, `-netdev tap` needs Linux TAP.
- The "Linaro documented direct-boot path" the plan cited is an `accel=kvm` Linux recipe that also invokes `assemble_cvd`/`run_cvd`, Linux ELF host binaries, against a runtime directory of unix sockets. **It cannot be reproduced on Windows.**

So the composite-disk assembly is not "follow a documented path." It is unknown-cost R&D, and it is the single thing v1 stands on. The VMM choice is a **sub-decision**, resolved only if something boots.

## Requirements

- [ ] Functional: an AOSP x86_64 API 33 image boots to `sys.boot_completed=1` on Windows under WHPX
- [ ] Functional: a host-side TCP connection to the guest's `adbd` succeeds and `getprop` returns
- [ ] Functional: **artifact availability confirmed first** — API 33 Cuttlefish x86_64 (or GSI) is still downloadable
- [ ] Functional: authenticated ADB (S3) attempted and the outcome recorded
- [ ] Functional: VMM sub-decision recorded, **with snapshot support weighted**
- [ ] Non-functional: hard stop at 3 weeks. Exceeding it triggers the fallback ladder, not an extension
- [ ] Non-functional: zero `crates/` code written

## Architecture

### Step 0 — availability gate (do this before anything else, it is cheap)

The plan previously instructed fetching from `aosp-android-latest-release`. **Never use a floating branch** — that branch always tracks the newest AOSP release, which has been Android 17 / API 37 since 2026-06-16, so it would deliver an image that fails the plan's own API check.

Confirm, for a **pinned** `android-13.0.0_rNN` build ID:
1. Does ci.android.com still publish `aosp_cf_x86_64_phone-img-<buildid>.zip` + `cvd-host_package.tar.gz` for it? CI retention on older branches is **unverified**.
2. If not, does AOSP **GSI x86_64** exist for API 33?
3. If neither, stop and escalate — the image strategy needs re-deciding before any engineering.

### The real question — device-set reduction

```
Cuttlefish's expected device set          Available on a Windows host?
────────────────────────────────          ────────────────────────────
vhost-vsock-pci (guest-cid)               NO  (/dev/vhost-vsock is Linux)
nvdimm + memory-backend-file (ramoops)    NO  (compiled out on Windows)
-netdev tap (cvd-mtap/etap)               NO  (Linux TAP)
egl-headless + virtio-gpu-gl-pci          NO  (needs gbm/Mesa)
unix-socket chardevs (qemu_monitor.sock)  NO  (named pipes instead)
assemble_cvd / run_cvd                    NO  (Linux ELF)
─────────────────────────────────────────────────────────────────────
Substitutes:  user-mode net + hostfwd · no vsock · no ramoops
              2D virtio-gpu · named-pipe monitor · own disk assembly
```

**The gate question:** does Cuttlefish's Android userspace reach `boot_completed` with vsock and ramoops **absent**? Cuttlefish's init and its `cuttlefish-common` services may hard-depend on vsock for logging, RIL, and its own control plane. Nobody has established this either way. Find out on day one, with the smallest possible harness.

### Fallback ladder (pre-committed — do not improvise under pressure)

1. **Cuttlefish x86_64** with a reduced device set.
2. **AOSP GSI x86_64** — *preferred fallback.* A GSI is a plain system image with no Cuttlefish host tooling, no vsock expectation, no `assemble_cvd`. If the goal is "an AOSP Android that boots under a stock VMM," GSI is the more natural fit and Cuttlefish may simply be the wrong artifact for this project.
3. **Own AOSP build** — expensive (~200 GB, hours per iteration), last resort.

**If none boots under WHPX within the box:** the v1 image strategy is invalid. Phases 07, 14, and 16 all change. **Escalate — do not absorb it.**

### VMM sub-decision (only if something boots)

| Criterion | Weight | Why |
|---|---|---|
| Boots the image under WHPX | **Gate** | Everything else is moot |
| **Snapshot / overlay support** | **Heavy** | Phase 11 is "the single most valuable dev-tool feature." crosvm's snapshotting is upstream-declared "**100% not supported**", format unstable. **This is not a tiebreaker row, it is a primary criterion** |
| qcow2 backing-file overlay creation **without shipping GPLv2 code** | **Heavy** | If crosvm wins but overlays need `qemu-img.exe`, the BSD-3 licensing advantage evaporates. crosvm has `crosvm create_qcow2 --backing-file` |
| Effort to obtain a working Windows binary | Medium | crosvm ships none, but its documented flow is a **native MSVC build** (`./tools/install-deps.ps1`; `cargo build --features all-msvc64,whpx`) — *not* a container cross-compile as the original plan claimed |
| WHPX maturity | Medium | crosvm's WHPX tier is "**tested upstream: no**". QEMU's is exercised but has an open SMP hang bug |
| Boot time, idle CPU, RAM overhead | Medium | User-visible |
| Device config effort | Low | Determines provider complexity |
| gfxstream availability | Low | Phase 21, may never happen |

**Corrected facts** (the original plan had these wrong): crosvm is **BSD-3-Clause**, not Apache-2.0. Its Windows build is native MSVC, not a cross-compile. Both were load-bearing arguments for the old "crosvm leans" conclusion — **there is no lean now.**

### QEMU command line — start from upstream's baseline

The original plan's pre-baked line walked into an open bug. **`kernel-irqchip=off` is not a WHPX requirement** — QEMU documents it as "not expected to be manually set during normal operation. It remains as a debugging option," it is a `-M` property (not an `-accel` sub-option), and QEMU's own WHPX example omits it.

Worse: **QEMU issue 3178** reports a WHPX SMP boot hang with `-smp cores=2` + `kernel-irqchip=off` on Windows 11 build **26200.6901** — the dev host here is 10.0.26200 — affecting QEMU 10.0.6, 10.1.2, and master, bisected and unresolved.

Start here instead, single-vCPU:

```
qemu-system-x86_64.exe \
  -accel whpx -M pc \
  -m 4096 -smp 1 -cpu max \
  -drive file=composite.qcow2,format=qcow2,if=virtio \
  -device virtio-net-pci,netdev=n0 \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:<port>-:5555 \
  -device virtio-tablet-pci -device virtio-keyboard-pci \
  -display none
```

Then scale `-smp` and record where it breaks. `kernel-irqchip=off` and `-M q35,pic=off` are **diagnostic knobs to try when stuck**, not defaults. Pin the QEMU version and record it in `RESULTS.md`.

Note `hostfwd=tcp:127.0.0.1:` — **invariant S2**, loopback only, from the very first boot.

## Related Code Files

- Create: `spikes/image-feasibility/00-availability.md` — step-0 gate result, pinned build ID, download URLs
- Create: `spikes/image-feasibility/assemble.ps1` — composite-disk attempt (throwaway)
- Create: `spikes/image-feasibility/qemu-launch.ps1`, `crosvm-launch.ps1` — throwaway
- Create: `spikes/image-feasibility/RESULTS.md` — device-set findings, boot outcome, VMM matrix, decision, date
- Modify: `plans/260827-1138-mtplayer-mvp/plan.md` — close Open Questions 1-3, update Risks

No `crates/` work in this phase.

## Implementation Steps

1. **Step-0 availability gate.** Pin an `android-13.0.0_rNN` build ID; confirm the artifacts download. If Cuttlefish artifacts are gone, switch to GSI x86_64 **now** and record it. Record exact URLs and SHA-256s.
2. **Host prep.** VT-x/AMD-V in firmware; `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All` (admin + reboot).
3. **Sanity-check WHPX itself first.** Boot a plain Linux live ISO under `-accel whpx -M pc -smp 1`. If *that* misbehaves, every later failure is ambiguous. This isolates hypervisor bugs from image problems — the red team specifically warned these get conflated.
4. **Attempt the smallest possible Android boot.** If GSI: a system image plus a generic kernel is far less assembly than Cuttlefish. Try the cheapest artifact first — the goal is a yes/no on "Android boots on WHPX," not fidelity to Cuttlefish's layout.
5. **If Cuttlefish: assemble the composite disk with the reduced device set.** Replicate what `assemble_cvd` produces, minus vsock/ramoops/TAP. **Log every hour spent** — this measurement is the phase's main output, because phase 07 inherits the work.
6. **Watch for vsock/ramoops hard dependencies.** Capture the guest's early boot via serial console. If `cuttlefish-common` services block on vsock, that is the answer: Cuttlefish is the wrong artifact, go to GSI.
7. **Inject `service.adb.tcp.port=5555`** and attempt **authenticated** ADB (S3). AOSP CI images are `userdebug`, so `ro.adb.secure` defaults to 0 — try setting it to 1 with an MTPlayer public key in the guest. Record whether S3 is achievable without a custom build; the answer decides Known Limitation 5.
8. **Confirm `boot_completed` and `getprop ro.build.version.sdk` == 33.** Do not trust the artifact filename.
9. **Only now, the VMM sub-decision.** Run the surviving path under the other VMM. Fill the weighted matrix. **Probe snapshot and qcow2-overlay support explicitly** — phase 11 and the `DiskOps` abstraction both depend on the answer.
10. **Write `RESULTS.md`**: device-set reductions applied, what broke, hours spent, boot outcome, S3 outcome, VMM matrix, decision, QEMU/crosvm versions pinned. Then update `plan.md`.

## Success Criteria

- [ ] Step-0 gate passed: a pinned API 33 artifact set is confirmed downloadable, with URLs and hashes recorded
- [ ] A plain Linux guest boots under WHPX (hypervisor sanity established before Android is blamed)
- [ ] An AOSP x86_64 API 33 image reaches `sys.boot_completed=1` on Windows under WHPX — **or** the fallback ladder is exhausted and escalated
- [ ] `getprop ro.build.version.sdk` returns `33`, read from the running guest
- [ ] Host connects to guest `adbd` on **`127.0.0.1` only** (S2 verified from the first boot)
- [ ] S3 outcome recorded: authenticated ADB working, or documented as infeasible with the reason
- [ ] Whether vsock and ramoops are hard boot dependencies is answered explicitly
- [ ] Hours spent on disk assembly recorded (phase 07 inherits this estimate)
- [ ] VMM matrix filled with snapshot and overlay-creation support **weighted, not merely noted**
- [ ] QEMU and/or crosvm versions pinned and recorded
- [ ] Decision written in `RESULTS.md` **and** `plan.md` Open Questions 1-3 closed
- [ ] Zero lines of `crates/` code

## Risk Assessment

**Risk: Cuttlefish's userspace hard-depends on vsock and will never boot without it.**
Signal: init or `cuttlefish-common` services blocking on vsock in the serial log; boot stalls before `boot_completed`.
Response (pre-decided): **go to GSI x86_64 immediately.** Do not try to emulate vsock. This is the most likely single outcome of this phase, and it is survivable — GSI was always the better fit for "AOSP under a stock VMM."

**Risk: API 33 artifacts have aged out of CI.**
Signal: step-0 gate fails.
Response: GSI for API 33; if that is also unavailable, escalate to the plan owner for an API-level re-decision. **Do not silently substitute a different API level** — that choice was made deliberately.

**Risk: QEMU issue 3178 hangs the boot with no output, and it is misread as image failure.**
Signal: no SeaBIOS POST output, process must be force-killed, symptom appears/disappears with `-smp`.
Response: single-vCPU baseline first (step 3-4), pinned known-good QEMU version, and step 3's Linux-guest sanity check. If SMP is unusable, record it as a constraint and cap `-smp` in phase 10's default profiles.

**Risk: the 3-week box expires with no boot.**
Signal: elapsed time.
Response: **stop.** Escalate with the device-set findings. An extension here is how a project spends three months proving something is impossible. The findings themselves have value — they tell the plan owner whether to re-scope, change artifacts, or abandon the VM backend for Provider B as primary.

**Risk: crosvm wins the sub-decision, then phase 11 discovers snapshotting is unusable.**
Signal: already known — upstream says "100% not supported", format unstable.
Response: the matrix now weights it heavily. If crosvm is chosen anyway, **phase 11 is overlay-copy-only and RAM-state resume is rejected at that moment**, not evaluated. Record that consequence in `RESULTS.md` so phase 11 inherits a decision, not a surprise.

**Risk: sunk-cost pull toward whichever VMM was touched first.**
Signal: arguing about criteria rather than filling the matrix.
Response: the weighted matrix and the gate ordering exist so this phase cannot rationalize. There is no lean to defend — the original one rested on two facts that were wrong.
