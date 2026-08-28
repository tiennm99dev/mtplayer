---
title: "Phase 8: APK Install and ABI Gate"
status: todo
phase: 8
priority: P1
effort: "4-6 d"
dependencies: [7]
---

# Phase 8: APK Install and ABI Gate

## Overview

Drag an APK onto the window and have it install — and when the APK cannot possibly work, say so clearly instead of installing something that crashes on launch. **The loud-failure requirement (G5) is the point of this phase**, not the install itself.

MTPlayer runs x86_64 Android with no ARM translation layer. `libhoudini` is discontinued; `libndk_translation` has no redistribution license. This is the same limitation as the official Android Studio x86_64 emulator, and it must be communicated at install time.

## Requirements

- [ ] Functional: drag-and-drop one or more APKs onto the window installs them
- [ ] Functional: ABI pre-check before install, based on reading the APK, not on a failed install
- [ ] Functional: split-APK sets install together via `install-multiple`
- [ ] Functional: the ARM-only refusal message names the ABIs found and what to do instead
- [ ] Non-functional: pre-check reads only the zip central directory — no full extraction of a 200 MB APK
- [ ] Non-functional: install progress visible; a failure surfaces the installer's actual reason

## Architecture

An APK is a zip. Native libraries live at `lib/<abi>/`. The ABI decision needs only the central directory, so it is fast and needs no unzip to disk.

```
APK dropped
   │
   ├─ read zip central directory -> collect lib/<abi>/ prefixes
   │
   ├─ no lib/ entries               -> pure-Java, PROCEED
   ├─ contains x86_64 (or x86)      -> PROCEED
   └─ only arm64-v8a / armeabi-v7a  -> REFUSE, explain
   │
   ├─ single APK       -> pm install
   └─ split APK set    -> install-multiple (all parts in one transaction)
```

**Split APKs are the subtle case.** A base APK may contain no `lib/` at all while an `.._config.arm64_v8a.apk` split carries the natives. Evaluating each file in isolation would pass the base and refuse the split, or vice versa. **The ABI decision must be made over the union of the whole dropped set**, and the set must install as one `install-multiple` transaction.

**Message wording matters** — this string is the product's answer to its most-hit limitation:

> `foo.apk` ships native libraries for arm64-v8a only. MTPlayer runs x86_64 Android and has no ARM translation layer. Rebuild with an x86_64 ABI split, or test this APK on a physical device.

## Related Code Files

- Create: `crates/mtp-core/src/apk.rs` — zip central-directory reader, ABI extraction, `AbiVerdict`
- Create: `crates/mtp-core/tests/apk_abi.rs` — table-driven tests over synthetic APK fixtures
- Modify: `crates/mtp-adb/src/install.rs` — created in phase 03; add split-session handling and progress reporting here
- Create: `crates/mtp-ui/src/drop_target.rs` — drag-drop handling, set grouping, verdict dialog
- Modify: `crates/mtp-ui/src/app.rs` — wire the drop target and the install progress surface
- Modify: `README.md` — the ARM limitation stated up front, matching the in-app message

## Implementation Steps

1. **Zip central-directory reader.** Use a zip crate that can list entries without extracting. Collect the distinct `<abi>` segments under `lib/`. Ignore directory entries and zero-length placeholders.
2. **`AbiVerdict` as a three-state enum** — `PureJava`, `Compatible { abis }`, `Incompatible { abis }` — never a bool. The UI needs to distinguish "no natives" from "x86_64 present" for its own messaging.
3. **Group the dropped set first.** Detect a split set by package name and the `config.` naming convention, or simply treat everything dropped in one gesture as one set. Compute the verdict over the **union** of ABIs across the set.
4. **Refuse before touching the guest.** The verdict is computed host-side; an incompatible set never reaches `pm install`. This is faster and gives a much better message than parsing an installer failure.
5. **Install via ADB.** Prefer streaming install (`exec:cmd package install`) over push-then-install: no `/data/local/tmp` cleanup, and progress comes from the bytes written. For split sets use the `install-multiple` session flow — create session, write each part, commit.
6. **Preserve installer errors verbatim.** `INSTALL_FAILED_INSUFFICIENT_STORAGE`, `INSTALL_FAILED_VERSION_DOWNGRADE`, `INSTALL_FAILED_UPDATE_INCOMPATIBLE` are all actionable, and paraphrasing them destroys information. Show the code and add a plain-language hint alongside it, not instead of it.
7. **Launch after install** — resolve the launchable activity (`cmd package resolve-activity`) and offer a one-click launch. A dev who just dropped an APK almost always wants to see it run.
8. **Fixture tests.** Synthesize small APKs (they are just zips) covering: no `lib/`, `x86_64` only, `arm64-v8a` only, both, `armeabi-v7a` only, and a split set whose base has no natives while a split has `arm64-v8a` only. That last case is the one a naive implementation gets wrong.

## Success Criteria

- [ ] Dropping a pure-Java APK installs it and it launches
- [ ] Dropping an x86_64 APK installs it and it launches
- [ ] Dropping an ARM-only APK produces the exact refusal message, names the ABIs found, and **does not touch the guest**
- [ ] Dropping a split set whose natives live only in a split is refused correctly (the trap case)
- [ ] Dropping a valid split set installs as one `install-multiple` transaction and launches
- [ ] A 200 MB APK's verdict reads **only** the zip central directory — verified by a byte counter or I/O trace showing bytes read is a small fraction of file size, not by wall-clock time (a full read of a local 200 MB file also finishes "well under a second", so the original timing criterion could not detect the failure it targeted)
- [ ] `INSTALL_FAILED_*` codes appear verbatim in the UI alongside a plain-language hint
- [ ] Install progress is visible for a large APK, not a frozen window
- [ ] `README.md`'s ARM limitation wording matches the in-app message
- [ ] ABI fixture tests pass in CI with no device

## Risk Assessment

**Risk: split-APK ABI evaluation is done per-file and gives the wrong verdict.**
Signal: an ARM-only app installs successfully and then crashes on launch — exactly the mystery crash G5 exists to prevent.
Response: the union rule in step 3 and the specific fixture test in step 8. This is the single most likely correctness bug in the phase; treat the fixture as non-negotiable.

**Risk: an APK declares `x86_64` but ships a broken or stub library and still crashes.**
Signal: verdict says compatible, app dies on launch with a linker error.
Response: out of scope to detect — the check is a necessary, not sufficient, condition. Say so in the docs. When a compatible-verdict app crashes, the logcat pane (phase 09) is the tool, and the two phases should be usable together.

**Risk: an obscure ABI directory name is misclassified.**
Signal: an entry like `lib/x86_64/` alongside `lib/mips/`, or a nonstandard directory.
Response: allowlist the four real ABIs (`x86_64`, `x86`, `arm64-v8a`, `armeabi-v7a`); treat anything else as ignorable rather than as a refusal trigger. Refusing on an unknown directory would produce false negatives on valid APKs.

**Risk: streaming install is unsupported on the guest and the fallback is untested.**
Signal: works on API 33, would fail on a future API 37 image (v1.1).
Response: probe support and keep the push-then-install fallback exercised at least once, with cleanup of `/data/local/tmp`.
