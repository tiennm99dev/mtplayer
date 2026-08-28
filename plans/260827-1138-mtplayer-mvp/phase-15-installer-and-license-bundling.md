---
title: "Phase 15: Installer and License Bundling"
status: todo
phase: 15
priority: P1
effort: "5-8 d"
dependencies: [14]
---

# Phase 15: Installer and License Bundling

## Overview

Package everything into one Windows installer, with correct third-party attribution and a licensing position that actually holds. **The GPLv2 and AVC-patent questions are legal constraints, not preferences**, and the red team found the original plan wrong on both.

Images are not bundled (phase 14 downloads them), so the installer stays in the tens of megabytes. **`adb.exe` is not bundled either** — phase 03 removed the dependency, which is what makes G6 achievable.

## Requirements

- [ ] Functional: one MSI producing a working install with no manual steps
- [ ] Functional: bundles `mtplayer.exe`, `mtplayer-cli.exe`, VMM binaries, `scrcpy-server.jar`, decoder DLLs
- [ ] Functional: complete NOTICE/license files for every bundled component
- [ ] Functional: **running instances stopped before binaries are replaced or removed**
- [ ] Functional: clean uninstall — binaries removed, user data retained by default with the size stated
- [ ] Functional: per-user install by default (admin needed only for the WHPX feature)
- [ ] Non-functional: **if QEMU — its corresponding source is published and shipped**, not linked
- [ ] Non-functional: **the AVC/H.264 patent position is recorded**, for whichever decoder is chosen
- [ ] Non-functional: installer signed, or the SmartScreen consequence documented

## Architecture

### Correcting the licensing analysis

**crosvm is BSD-3-Clause, not Apache-2.0.** The original plan asserted Apache-2.0 in five places, including as the lead argument in phase 01's decision rule. The "permissive and linkable" conclusion survives; the attribution text does not — BSD-3 requires notice reproduction and carries a no-endorsement clause. And the CI license gate below is seeded from this table, so a wrong entry would either fail on the crate that is supposed to be vendored, or force a silently widened allowlist.

**GPLv2 §3 does not accept a link.** The original plan permitted "a source link or offer in `docs/third-party-licenses.md`". GPLv2 offers exactly three routes: (a) accompany the binary with complete corresponding source, (b) provide **your own** written offer valid at least three years, (c) pass along an upstream offer — noncommercial distributors only. A URL to qemu.org is none of them.

Worse, the original plan bundled **third-party prebuilt** QEMU Windows binaries, whose "complete corresponding source" MTPlayer does not possess and cannot supply.

**Therefore, if phase 01 chose QEMU:** build it from a **pinned source tarball that MTPlayer publishes at the same release**, and ship that tarball (route (a)). A prebuilt third-party binary cannot be made compliant. This is a real cost — a QEMU Windows build is not trivial — and it belongs in this phase's estimate.

**`qemu-img` is not exempt.** If crosvm won but overlays need `qemu-img.exe`, the whole obligation returns for the sake of creating a file. Phase 06's `DiskOps` exists to avoid this; prefer the in-process qcow2 writer or `crosvm create_qcow2`.

### AVC/H.264 patents — a separate axis, entirely missing before

The original plan claimed `openh264` "sidesteps the question" and reduced its obligation to "attribution; note the binary-distribution condition". That is wrong in a way that matters:

Cisco's AVC patent grant covers **only Cisco's own precompiled binary**, and **only when that binary is separately downloaded to the end user's device and not integrated into or combined with third-party software prior to download**. Bundling it in an MSI is exactly that combination, so the grant is forfeited. Self-built `openh264` was never covered. An LGPL ffmpeg build carries the same AVC exposure.

The original plan named "shipping a GPL-enabled ffmpeg" as "the most likely licensing mistake in the project" — a real risk, but it singled out the copyright axis while missing patents entirely.

**Options, to be chosen deliberately:**
1. **Download Cisco's binary at first run** (what Firefox does) — preserves the grant, but adds a network dependency and a second supply-chain surface needing phase 14's signature work.
2. **Obtain an AVC patent position** deliberately (MPEG-LA/Via LA licensing).
3. **Accept the exposure knowingly** and record that decision, with whatever risk assessment the project owner is comfortable with.

Doing nothing and calling it clean is the one unacceptable outcome.

### Component licenses to satisfy

| Component | License | Obligation |
|---|---|---|
| MTPlayer | Apache-2.0 | LICENSE + NOTICE |
| QEMU (if chosen) | GPLv2 | COPYING + **corresponding source published and shipped** + process-only boundary |
| crosvm (if chosen) | **BSD-3-Clause** | Notice reproduction + no-endorsement clause |
| `scrcpy-server.jar` | Apache-2.0 | NOTICE + pinned version |
| ffmpeg (if used) | LGPL-2.1+ (**verify no GPL components**) | Dynamic linking only; LGPL text; **+ AVC patents** |
| openh264 (if used) | BSD-2-Clause + a Cisco-binary-conditioned patent grant | Attribution; **+ AVC patents, grant forfeited by bundling** |
| Rust crates | mixed | Generated attribution file |
| **`adb.exe`** | — | **Not bundled.** Phase 03 removed the dependency; this is why G6 holds |

### Installer

MSI (WiX) over NSIS for upgrade semantics. Per-user install to `%LOCALAPPDATA%\Programs\MTPlayer` by default, so no admin is needed.

**`.apk` file association is dropped.** It was unrequested scope, and hijacking `.apk` — even opt-in — adds registry writes and uninstall cleanup for no stated need.

**Running instances must be handled.** The original plan never mentioned them, though phase 13a already ships `stop --all`. Both outcomes of ignoring it are bad: Restart Manager hard-kills live guests mid-write, or (with phase 06's breakaway spawn) a VMM survives an *uninstall*, holding 4 GB and a handle to a deleted executable, with the reaper itself uninstalled.

## Related Code Files

- Create: `installer/mtplayer.wxs` — installer definition
- Create: `installer/build-installer.ps1` — reproducible packaging
- Create: `installer/components.toml` — bundled-component inventory: version, license, source URL, obligation
- Create: `NOTICE` — repo root, aggregating third-party attribution
- Create: `docs/third-party-licenses.md` — generated, shipped with the installer
- Create: `.github/workflows/release.yml` — tag -> build QEMU from pinned source (if QEMU) -> package -> sign -> publish binaries **and the QEMU source tarball**
- Modify: `assets/NOTICE` — finalize scrcpy and decoder attribution
- Modify: `crates/mtp-provider-vm/src/process.rs` — resolve the VMM from the install layout, never `PATH`

## Implementation Steps

1. **Freeze the component inventory** in `installer/components.toml` — every bundled binary with version, license, source URL, and obligation. It drives both the installer and the generated license doc, so it cannot drift. **Get crosvm's license right (BSD-3-Clause).**
2. **Resolve the decoder's copyright position.** If ffmpeg: obtain or produce an LGPL-only build and record its exact configure flags. If not confidently verifiable, use `openh264`. **Never ship an unverified ffmpeg build.**
3. **Resolve the decoder's patent position** via the three options above. Record the choice and the reasoning in `docs/third-party-licenses.md`.
4. **If QEMU: build from a pinned source tarball and publish it.** Ship the tarball (or a real 3-year written offer) with the release. Add a structural check — a CI assertion that no QEMU library is linked — so the process boundary is verifiable rather than asserted in a comment. State the constraint in `CLAUDE.md`/`CONTRIBUTING` where a future contributor would make the decision.
5. **Generate Rust crate attribution** with `cargo-about` into `docs/third-party-licenses.md`. **Keep the CI license gate** — the red team suggested dropping it as unrequested, but three separate license or patent errors in this plan were found by review rather than by tooling. Automated checking is more justified now, not less.
6. **Installer definition.** Per-user install, Start-menu shortcut, Add/Remove Programs entry. No `.apk` association.
7. **Stop instances before touching binaries.** An installer custom action (and an uninstall action) invoking `mtplayer-cli stop --all` and waiting; refuse to proceed if any instance will not stop. Document that a forced install kills guests.
8. **Uninstall.** Remove binaries; **ask** about `%APPDATA%/MTPlayer` (profiles, instances, snapshots, images — potentially tens of gigabytes). Default to retaining, and **state the size** so the choice is informed.
9. **Upgrade semantics.** An MSI upgrade replaces binaries and preserves user data. Test v1 -> v1.0.1 explicitly. Note that a VMM version change invalidates RAM-state snapshots — phase 11's `SnapshotMeta` now carries `vmm_version` so the mismatch is detectable rather than silent.
10. **VMM path resolution** from the install directory, never `PATH` — a user's unrelated QEMU must not be picked up, and MTPlayer must not require one.
11. **Signing.** Sign both executables and the installer. If unsigned, document the SmartScreen warning and publish the SHA-256.
12. **Clean-machine test.** A fresh Windows 11 VM with no Rust, no QEMU, **no Android SDK**, no Visual C++ redistributable.

## Success Criteria

- [ ] Installer produces a working install on a clean Windows 11 VM with no developer tooling
- [ ] **The install works with no Android SDK and no `adb.exe` anywhere on the machine** (G6)
- [ ] Per-user install completes without an admin prompt
- [ ] Both executables run from the installed location; the VMM resolves from the install directory, not `PATH`
- [ ] Installer size in the tens of MB — no images bundled
- [ ] Every bundled component appears in `docs/third-party-licenses.md` with correct license text — **crosvm listed as BSD-3-Clause**
- [ ] **If QEMU:** corresponding source for the exact bundled build is published at the release with its hash recorded; no QEMU library is linked (CI-verified)
- [ ] **The AVC/H.264 patent position is recorded** for the chosen decoder, with the option taken and the rationale
- [ ] The decoder's copyright position verified (LGPL-only ffmpeg with recorded flags, or openh264)
- [ ] CI fails if a dependency introduces an unexpected license
- [ ] **Upgrade with two instances running: instances stopped first, no guest killed mid-write, user data preserved**
- [ ] **Uninstall with an instance running: instance stopped first**; user data retained by default with its size stated
- [ ] v1 -> v1.0.1 upgrade preserves profiles, instances, and downloaded images
- [ ] No `.apk` file association is registered
- [ ] Release workflow produces a signed (or explicitly-documented-unsigned) artifact with a published SHA-256

## Risk Assessment

**Risk: building QEMU from source on Windows is a phase in itself.**
Signal: days lost to the QEMU build before the installer is touched.
Response: this is a newly-recognized cost, not a surprise to absorb silently. If it proves large, it is an argument for crosvm in phase 01's matrix — **feed it back there** rather than discovering it here. Shipping a prebuilt third-party GPLv2 binary is not an available shortcut.

**Risk: shipping a GPL-enabled ffmpeg build, making distribution GPL.**
Signal: the bundled ffmpeg's `--enable-gpl` status is unknown or assumed.
Response: step 2 is a gate. If the configuration cannot be verified confidently, use `openh264`. Invisible until someone checks.

**Risk: the AVC patent question is quietly dropped.**
Signal: `docs/third-party-licenses.md` shipping with no patent section.
Response: it is a success criterion. Any of the three options is defensible; silence is not. Note that "accept the exposure" is a legitimate choice for this project's scale — it just has to be a *choice*.

**Risk: the QEMU process boundary is asserted but not enforced.**
Signal: a future contributor adds a QEMU library binding for convenience.
Response: the CI assertion in step 4, plus documenting the constraint where the decision would be made.

**Risk: no code-signing certificate, so every download hits SmartScreen.**
Signal: "Windows protected your PC" reports.
Response: `plan.md` Open Question 4 needs an owner. If unsigned for v1, document it prominently with the SHA-256. Unsigned-and-undocumented is the only unacceptable outcome. Note this is a *different* key from phase 14's manifest signing key, though one owner could hold both.

**Risk: a missing Visual C++ redistributable breaks a clean install.**
Signal: works on the dev machine, fails on a fresh VM with a DLL error.
Response: the clean-machine test is the only reliable detector. Prefer static CRT linking, or bundle the redistributable.

**Risk: uninstall silently deletes tens of gigabytes of images.**
Signal: users losing downloads on a reinstall.
Response: ask, default to retain, state the size. Upgrades must never touch user data.
