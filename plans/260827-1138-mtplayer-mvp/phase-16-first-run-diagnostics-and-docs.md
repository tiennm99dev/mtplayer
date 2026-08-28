---
title: "Phase 16: First-Run Diagnostics and Docs (v1 EXIT)"
status: todo
phase: 16
priority: P1
effort: "5-8 d"
dependencies: [15]
---

# Phase 16: First-Run Diagnostics and Docs (v1 EXIT)

## Overview

Turn every possible first-run failure into an actionable message, and write the docs. **This phase is the v1 exit.**

MTPlayer's hard prerequisite — the Windows Hypervisor Platform feature, needing admin and a reboot — is the biggest first-run drop-off risk. Diagnostics exist so the user is guided through it rather than hitting a bare "failed to start".

**Note what is no longer a condition:** `adb.exe`. Phase 03's device-protocol client removed the dependency, so a clean host with no Android SDK is a supported configuration rather than a failure to diagnose.

## Requirements

- [ ] Functional: first-run wizard — preflight, guided WHPX enable, image download, first boot
- [ ] Functional: every preflight failure yields a specific, actionable message
- [ ] Functional: elevated WHPX enable with a reboot prompt and correct resume after reboot
- [ ] Functional: a diagnostics report the user can paste into a bug report
- [ ] Functional: **redaction at the value level (S8)**, not the field level
- [ ] Functional: README with limitations up front, quick start, CI usage, troubleshooting
- [ ] Non-functional: no failure path ends in a generic error or a silent no-op

## Architecture

```
first run
   ├─ preflight (phase 02's HostCapabilities, productionized)
   │    ├─ VT-x/AMD-V disabled in firmware   -> BIOS/UEFI guidance
   │    ├─ HypervisorPlatform off            -> offer elevated enable + reboot
   │    ├─ RAM below requirement             -> required vs available
   │    ├─ Disk below requirement            -> required vs available, per volume
   │    └─ conflicting hypervisor            -> explain the conflict
   ├─ image step  (phase 14: verify signature, download API 33, show size first)
   └─ first boot  (phase 06: Starting -> Booting -> Ready)
```

### `Remedy` — one shape, decided here

The original plan called it an "enum" on one line and described a three-field struct fourteen lines later. Two incompatible shapes, and phase 18 was then told to add conditions "consistent with phase 16's `Remedy` structure" — with nothing precise to be consistent with. Settled:

```rust
struct Remedy {
    condition: ConditionKind,      // the enum — one variant per detectable condition
    message: String,               // what to tell the user, with real numbers substituted
    action: Option<RemedyAction>,  // None | EnableFeature | OpenDocs | Download | Retry
    needs_elevation: bool,
    needs_reboot: bool,
    troubleshooting_anchor: &'static str,  // links message <-> docs section
}
```

`ConditionKind` is the enum; `Remedy` is the struct carrying it. The UI renders it, the CLI's `doctor` prints it, and `docs/troubleshooting.md` is cross-checked against it via `troubleshooting_anchor` — which is what makes "docs match the in-app text" mechanically testable rather than aspirational.

**Phase 18 adds three `ConditionKind` variants** (no WSL, wrong kernel, no container runtime) **and must also add their `docs/troubleshooting.md` sections** — the original plan's completeness test would otherwise have broken at phase 18 by construction.

### The diagnostic messages are the deliverable

| Condition | Message shape |
|---|---|
| VT-x/AMD-V off in firmware | "Hardware virtualization is disabled in your BIOS/UEFI. Restart, enter setup (commonly F2/F10/Del), and enable Intel VT-x or AMD-V / SVM Mode." **Cannot be fixed from Windows — say so plainly.** |
| `HypervisorPlatform` off | "MTPlayer needs the Windows Hypervisor Platform feature. Enable it now? Requires administrator rights and a restart." |
| Insufficient RAM | "This profile requests 4096 MB; 2100 MB is available. Close applications or choose the low-RAM profile." |
| Insufficient disk | "The API 33 image needs 6.8 GB plus 1.7 GB to download; drive C: has 4.1 GB free." |
| Image signature invalid | "This image manifest could not be verified. Do not install it. This may indicate a network problem or a tampered download." |
| Anti-virus blocking the VMM | "The device failed to start and the VMM process was terminated externally. Some anti-virus products block it — see troubleshooting." |
| Boot timeout | "The device did not finish booting within 180 s. Last state: `<property values>`." |
| Instance busy | "Another MTPlayer process is operating on this instance." |

**"Anti-virus blocking" cannot be detected reliably** — it presents as a child process dying immediately with no error. Detect the *symptom* (VMM exited abnormally within a few seconds of spawn) and name the likely cause **without asserting it**. Do not claim certainty the code does not have.

**Nested-virtualization conflicts:** WHPX coexists with Hyper-V and WSL2 (shared hypervisor), but a third-party hypervisor holding exclusive VT-x can block it. Detect what is detectable; where detection is unreliable, describe the symptom rather than guessing.

### Redaction — at the value level (S8)

The original guarantee was "no file paths containing the username and no environment dump," implemented as an allowlist of field *names* — while including "the last N errors", and while phases 03, 06, and 08 all **mandate that error strings be preserved verbatim** ("ADB's own error strings are better than anything invented"; "paraphrasing them destroys information"). An allowlist of field names sanitizes nothing about field values.

So a report pasted into a public issue could carry `C:\Users\<name>\...`, a private project path from an `INSTALL_FAILED_*` line, `ro.serialno`, and GPU driver versions.

**Every string entering the report passes through a scrubber**: rewrite `C:\Users\<name>` -> `%USERPROFILE%`, any path outside the install/data roots -> `<redacted>`, and drop identifiers. Verbatim-for-the-user and scrubbed-for-the-report are different code paths — the UI still shows the real string.

## Related Code Files

- Create: `crates/mtp-core/src/diagnostics.rs` — `ConditionKind`, `Remedy`, `RemedyAction`, detection
- Create: `crates/mtp-core/src/diagnostics/scrub.rs` — **value-level redaction (S8)**
- Create: `crates/mtp-core/src/diagnostics/report.rs` — copyable report over scrubbed values
- Create: `crates/mtp-core/tests/diagnostics.rs` — message generation per condition, snapshot-tested
- Create: `crates/mtp-core/tests/scrub.rs` — **redaction over synthetic errors containing a username, a private path, and a serial**
- Create: `crates/mtp-ui/src/first_run.rs` — wizard with resume-after-reboot
- Create: `crates/mtp-ui/src/diagnostics_panel.rs` — re-runnable check plus report copy
- Create: `docs/troubleshooting.md` — one section per `ConditionKind`, anchored
- Create: `docs/quick-start.md`
- Modify: `README.md` — limitations up front, quick start, CI pointer, troubleshooting link
- Modify: `crates/mtp-core/src/preflight.rs` — productionize phase 02's detection
- Modify: `crates/mtp-cli/src/commands/doctor.rs` — phase 13a; same checks headlessly

## Implementation Steps

1. **Enumerate every failure condition** reachable across phases 02, 03, 06, 07, 08, 11, 12, 14, 15 — including boot timeout, ABI rejection, RAM/disk guards, image signature failure, SHA mismatch, base-SHA mismatch, and instance-busy. The list is the phase's real scope; write it before any UI. **`adb.exe` missing is not on the list** (G6).
2. **`Remedy` as the struct above.** One source; the UI, `doctor`, and the docs all consume it.
3. **Productionize preflight** using documented Win32 APIs over parsed PowerShell text. Verify on Intel and AMD if possible.
4. **Elevated feature enable.** An elevated helper for `Enable-WindowsOptionalFeature -Online -FeatureName HypervisorPlatform -All`; handle UAC decline gracefully (not as a crash), then prompt for restart.
5. **Reboot resume.** Persist the wizard step; on next launch continue where it stopped **and re-verify the feature actually took effect** — the enable can fail silently and the user should not discover that three steps later.
6. **Symptom-based VMM-death detection.** VMM exits within ~5 s with no output -> report the symptom, name anti-virus as *likely*, include the exit code.
7. **Value-level scrubbing (S8)** applied to every string entering the report, including preserved-verbatim guest and installer errors.
8. **Diagnostics report** over scrubbed values: OS build, CPU model, virtualization flags, feature state, RAM/disk, MTPlayer/VMM/scrcpy/image versions, last N errors. **No environment dump. No raw user paths.**
9. **`mtplayer doctor`** (phase 13a) running identical checks with the phase-13 exit codes.
10. **Docs.** README leads with the five Known Limitations from `plan.md` — including the unauthenticated-ADB one if phase 07 could not achieve S3. Stating them up front prevents the most common misdirected bug reports.
11. **Snapshot-test the messages.** Message text is the deliverable; a test locks it against degrading into "an error occurred". Also assert **every `ConditionKind` has a matching troubleshooting section** via its anchor.
12. **v1 exit walkthrough on a clean VM.**

## Success Criteria

- [ ] Clean Windows 11 VM: install -> first run -> guided through enabling WHPX -> reboot -> wizard resumes -> image downloads -> device boots -> APK installs. **No manual steps outside the app**
- [ ] The walkthrough succeeds with **no Android SDK and no `adb.exe`** present (G6)
- [ ] Every condition from step 1 produces a specific message naming the fix; none falls through to a generic error
- [ ] Messages containing numbers show real values, not placeholders
- [ ] VT-x-disabled case tested by actually disabling it in the test VM's firmware
- [ ] UAC decline handled gracefully and re-offerable
- [ ] A silently failed feature enable is detected after reboot, not later
- [ ] **A report generated from synthetic errors containing a username, a private project path, and a serial contains none of the three** (S8, snapshot-tested)
- [ ] No environment dump in the report
- [ ] `mtplayer doctor` reports the same conditions with the phase-13 exit codes
- [ ] Every `ConditionKind` has a matching `docs/troubleshooting.md` section (test-asserted via anchor)
- [ ] README states the Known Limitations before any feature description
- [ ] Message snapshot tests pass in CI
- [ ] **v1 EXIT:** the full walkthrough completed on a clean VM without consulting the source or the plan. *(Ideally observe someone unfamiliar with the app attempt it — valuable, but not a release gate, since the original criterion made shipping contingent on recruiting a test subject.)*

## Risk Assessment

**Risk: an undetectable failure mode leaves the user with a generic error.**
Signal: a first-run failure producing no actionable message.
Response: symptom-based reporting plus the diagnostics report as universal fallback. Every dead end must at minimum offer "copy diagnostics" — that is the floor.

**Risk: preflight detection is wrong on hardware the developer does not own.**
Signal: false negatives blocking users whose machines would work.
Response: prefer documented APIs; record verified hardware; make the panel re-runnable. Provide a documented override for a user certain detection is wrong — a false negative that hard-blocks the app is worse than a failed start attempt.

**Risk: the WHPX enable succeeds but virtualization still does not work.**
Signal: feature reports enabled, VMM still fails.
Response: usually firmware VT-x still off, or a third-party hypervisor holding exclusive access. Re-check firmware-level virtualization *after* the enable and report both facts together.

**Risk: redaction misses a path shape.**
Signal: a user path in a pasted report.
Response: the scrub tests cover username paths, private project paths, and serials. Prefer an allowlist of report fields **plus** value scrubbing — the original plan had only the former, which was the defect.

**Risk: first-run drop-off from the reboot requirement regardless of messaging.**
Signal: users abandoning at the WHPX step.
Response: cannot be engineered away. Set the expectation *before* the installer runs — on the download page and in the README — so it is a known cost rather than a mid-setup surprise.

**Risk: docs and in-app messages drift.**
Signal: troubleshooting sections describing text the app no longer shows.
Response: the anchor-based cross-check is test-asserted, and phase 18 is explicitly required to add its own sections.
