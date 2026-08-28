---
title: "Phase 10: File Transfer and Device Profiles (P1 EXIT)"
status: todo
phase: 10
priority: P1
effort: "5-7 d"
dependencies: [8, 9]
---

# Phase 10: File Transfer and Device Profiles (P1 EXIT)

## Overview

Two features that finish P1: getting files in and out of the guest, and making the guest configurable. **This phase is the P1 exit** — after it, MTPlayer is something a developer would plausibly choose over a physical device for a quick check.

## Requirements

- [ ] Functional: drag files onto the window to push; a pull action to retrieve
- [ ] Functional: a designated host folder synced **one-way host -> guest** by default, with an explicit pull-back action
- [ ] Functional: profiles covering resolution, DPI, RAM, cores, locale, timezone, boot timeout
- [ ] Functional: profiles persist as TOML under `%APPDATA%/MTPlayer/profiles/` — **as templates**
- [ ] Functional: resolution/DPI applied live; RAM/cores at next boot, stated as such in the UI
- [ ] Functional: **every guest-supplied path validated host-side (S5)** before any filesystem use
- [ ] Non-functional: transfer progress and real cancellation for large files
- [ ] Non-functional: ships with sensible default profiles

## Architecture

### File transfer

```
drop non-APK files  -> push to /sdcard/Shared/ (or a chosen path)
pull action          -> guest path -> host directory (S5-validated)
"Shared folder"      -> host dir -> /sdcard/Shared, explicit one-way sync
                        + a separate, explicit "pull changes back" action
```

**The shared folder is explicit sync, not a mount.** A live mount needs virtio-fs or 9p in the guest plus a host server — weeks of work, redundant given push/pull exists. **Do not imply live mirroring in the UI**, or users will lose data expecting writes to propagate.

**One-way by default, not bidirectional.** The original plan specified two-way size/mtime reconciliation. That is the hard version of a feature requested as "file push/pull, shared folder", it lands in the P1 exit phase where slack is thinnest, and it depends on mtime fidelity through `pull` that phase 03 only guarantees for `push`. v1 ships host -> guest sync plus a deliberate pull-back action. Two-way reconciliation is a v1.1 candidate.

**Guest -> host is a trust boundary (S1, S5).** The guest runs arbitrary untrusted APKs — that is the product's purpose. Any filename arriving from the guest is hostile input. Before use, every guest-supplied path must be validated and canonicalized host-side: reject `..`, path separators, `:` (NTFS alternate data streams), Windows reserved device names (`CON`, `NUL`, `LPT1`, …), leading/trailing dots and spaces, and over-long paths; then confirm the canonicalized result is inside the destination root. **Never apply guest file modes to host files.** Phase 03 implements the check in `sync.rs`; this phase must not bypass it with its own path joining.

Phase 08 owns the APK drop path; this phase handles everything else. One drop target, two routes, decided by extension.

### Device profiles — templates, not live state

```toml
# %APPDATA%/MTPlayer/profiles/pixel-api33.toml   (a TEMPLATE)
name        = "Pixel-ish (API 33)"
image       = "aosp_x86_64-api33"
width       = 1080
height      = 2400
dpi         = 420
ram_mb      = 4096
cores       = 4
locale      = "en-US"
timezone    = "Asia/Saigon"
boot_timeout_secs = 180
```

**Templates only.** Phase 06 establishes that `start()` copies the effective profile into the registry as immutable `resolved_profile` for the instance's lifetime. The original plan left ownership ambiguous — the registry recorded "the profile", these files were live-mutable, and phase 12's RAM guard summed "running profiles' ram". Editing a template mid-run would have made the guard subtract a number the VMM never had, refusing legitimate instances while reporting false figures.

So: editing a template affects **future** starts. For a running instance, the live-apply path writes to the guest *and* to the template, and the delta against `resolved_profile` is what drives the restart-required badge.

**Two classes of setting — conflating them confuses users:**

| Applied live over ADB | Applied at boot by the VMM |
|---|---|
| `wm size`, `wm density` | `-m` (RAM), `-smp` (cores) |
| `setprop persist.sys.locale` | |
| `setprop persist.sys.timezone` | |

Label boot-time fields explicitly ("takes effect on next start") rather than silently doing nothing until restart.

**Resolution changes ripple into phases 04, 05, and 12.** `wm size` triggers a scrcpy resolution change, invalidating the cached touch screen size and forcing texture reallocation. Both were built to handle it; this phase exercises it for real.

**Note on `cores`:** if phase 01 found `-smp` above 1 unstable under WHPX (see the open QEMU bug), the shipped defaults must respect that cap and the UI must explain it rather than letting users pick a value that hangs the guest.

## Related Code Files

- Create: `crates/mtp-core/src/profile.rs` — complete the phase-06 `DeviceProfile`; TOML load/save; defaults; validation
- Create: `crates/mtp-core/tests/profile_roundtrip.rs` — round-trip, missing-field defaults, invalid-value rejection
- Create: `crates/mtp-core/src/shared_folder.rs` — one-way sync + explicit pull-back, change detection
- Create: `crates/mtp-core/tests/guest_path_validation.rs` — **adversarial filename fixtures (S5)**
- Create: `crates/mtp-ui/src/profile_editor.rs` — live vs boot-time fields visually separated; template semantics explained
- Create: `crates/mtp-ui/src/file_transfer.rs` — drop routing, pull dialog, progress, cancel
- Create: `assets/profiles/` — default profiles shipped with the app
- Modify: `crates/mtp-ui/src/drop_target.rs` — route non-APK drops here
- Modify: `crates/mtp-provider-vm/src/lib.rs` — consume RAM/cores from **`resolved_profile`** at start
- Modify: `crates/mtp-adb/src/sync.rs` — cancellation, if phase 03 left it out

## Implementation Steps

1. **Complete `DeviceProfile`.** Serde with `#[serde(default)]` on **every** field so an old profile never fails to load after a schema addition. Validate ranges (RAM >= 2048, cores >= 1 and within any phase-01 cap, DPI 120-640) and reject with a specific message rather than clamping silently.
2. **Ship defaults** in `assets/profiles/` — a phone, a tablet, a low-RAM one. Copy to `%APPDATA%` on first run if absent; never overwrite a user's edited copy.
3. **Profile editor UI.** Two visually distinct groups: "Applies now" and "Applies on next start". Show a restart-required badge when a boot-time field differs from `resolved_profile` on a running instance. Make the template semantics visible — an edit does not retroactively change a running instance.
4. **Live application.** `wm size`, `wm density`, `setprop persist.sys.locale`, `setprop persist.sys.timezone`. Locale and timezone may need a settings-service nudge or a restart on some builds — verify on the API 33 image and say so honestly in the UI if a restart is required.
5. **Boot-time application** flows into the phase-06 provider's VMM args from `resolved_profile`. Reject RAM exceeding available host memory (phase 12 generalizes the guard).
6. **Drop routing.** `.apk`/`.apks` -> phase 08; everything else -> push. Directories push recursively. Show the destination path before transferring.
7. **Pull.** A "pull path" input plus a host save dialog satisfies the requirement; a `/sdcard` browser is optional. Prefer the simple version for v1. **Route every returned name through the S5 validator** — a guest can name a file anything.
8. **Progress and cancel.** Wire phase 03's progress callback. Cancellation must actually abort the transfer, and **push to a temporary name, renaming on completion**, so a cancelled transfer never leaves a partial file that looks complete.
9. **One-way shared-folder sync.** Compare by size and mtime host -> guest, transfer differences, show last-synced time. Label it "Sync", never "Mount". Provide a separate explicit "pull changes from guest" action that runs the same S5 validation.
10. **Exercise a resolution change end to end** on a running instance: pane resizes, texture reallocates, touch coordinates still land correctly.
11. **P1 exit walkthrough** — the full scenario from `plan.md` in one session, on the API 33 image.

## Success Criteria

- [ ] Drop a file -> lands in `/sdcard/Shared/`, verified with `shell ls`
- [ ] Drop a directory -> pushed recursively
- [ ] Pull a file the app wrote -> byte-identical on the host
- [ ] **A guest file named `../../evil.txt`, `NUL`, `a:b`, or with a 300-char name is refused on pull; nothing is written outside the destination root** (S5)
- [ ] Guest file modes are not applied to host files
- [ ] Cancel a large transfer mid-flight -> transfer actually aborts, and **no partial file is left at the destination path**
- [ ] One-way sync moves host changes to the guest; last-synced time shown; the UI never implies live mirroring
- [ ] The explicit pull-back action works and applies S5 validation
- [ ] Profile round-trips through TOML; a file missing new fields still loads via defaults
- [ ] Invalid values (RAM 64 MB, DPI 5000, cores above the phase-01 cap) rejected with specific messages
- [ ] Live fields apply immediately; boot-time fields show a restart-required badge against `resolved_profile`
- [ ] **Editing a template while an instance runs does not change that instance's recorded RAM or cores**
- [ ] Resolution change on a running instance: pane resizes, texture reallocates, touches land correctly
- [ ] Default profiles present on first run; a user-edited profile survives an app restart
- [ ] Adversarial-filename and profile tests pass in CI
- [ ] **P1 EXIT:** on the API 33 image — drag in an APK, see it launch, read its logcat, pull a file it wrote, restart the instance, and get a clear error when dropping an ARM-only APK

## Risk Assessment

**Risk: users expect the shared folder to be a live mount and lose work.**
Signal: "I saved a file in the guest and it did not appear on my host."
Response: wording plus the one-way default. "Sync", an explicit button, a visible timestamp, and a separate pull action make the direction obvious. A real mount is a v1.1 investigation, not a v1 feature.

**Risk: S5 validation is bypassed by this phase's own path joining.**
Signal: a traversal lands via the pull dialog even though `sync.rs` validates.
Response: validation belongs in one place (phase 03) and this phase must call it, not reimplement joining. The adversarial fixtures are success criteria here as well as there, precisely to catch a bypass.

**Risk: `wm size` leaves the guest broken.**
Signal: black screen, wrong-density UI, launcher failure after a resolution change.
Response: keep `wm size reset` / `wm density reset` reachable as an escape hatch. Test the specific resolutions the shipped profiles use rather than assuming all values are safe.

**Risk: locale/timezone props do not take effect without a reboot.**
Signal: prop set, guest UI unchanged.
Response: verify on the API 33 image and be honest in the UI. Marking them restart-required is acceptable; silently doing nothing is not.

**Risk: profile schema changes break existing files.**
Signal: users' profiles failing to load after an update.
Response: `#[serde(default)]` on every field from the start, plus the explicit missing-field test. Never require a field added after v1. Note that phase 21 wants to add a display-path field — it must follow this rule.

**Risk: template-vs-resolved semantics confuse users.**
Signal: "I changed the RAM and nothing happened."
Response: the restart-required badge and the explicit two-group layout. The confusion is preferable to the alternative, which silently corrupted phase 12's resource accounting.
