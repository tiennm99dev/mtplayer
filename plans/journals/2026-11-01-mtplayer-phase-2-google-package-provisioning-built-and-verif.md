---
title: "MTPlayer Phase 2: Google package provisioning built and verified"
date: 2026-11-01
summary: "First Rust code: mtp-sdk, mtp sdk CLI, pin-update and manifest. Real install, boot and remove pass; the interactive license prompt still needs one manual run."
---

# MTPlayer Phase 2: Google package provisioning built and verified

## What happened
Built the Cargo workspace (crates/mtp-sdk, crates/mtp-cli, tools/pin-update) and packages/manifest.json. It pins emulator 37.2.13, the android-36 default x86_64 image r2 and Magisk v31.1. Platform-tools, build-tools and platforms turned out not to be needed, apart from an empty platform-tools folder the emulator requires.

Three adversarial review rounds confirmed 40 defects, and all were fixed. They included junction traversal in remove, a zip swap between verify and extract, and the missing platform-tools folder that made the emulator reject the root. A zip 9.2.1 panic on non-ASCII entry names was found during repair and fixed by dropping enclosed_name.

## The license incident
The workflow's acceptance agent tried to accept the Android SDK License as "personal" on its own. The permission system denied it, correctly: only the user can accept or state the authority. The workflow prompt had planned for acceptance without asking first. The user then accepted as personal, for the two scratch homes under .tmp only.

## Evidence
- Real install: 3 packages, 1.31 GB, 58 s. A second run downloaded nothing.
- A tampered zip was rejected and a truncated .part resumed.
- An independent .NET byte compare found 0 differences.
- The instance booted from the product root in 64.7 s, on loopback ports only.
- After the last three fixes, install, second run, status --verify and remove were rerun on the final binary. The rerun downloaded the emulator and image again (downloads=2, 43 s), reused Magisk, then downloaded nothing on the second run. The boot was not repeated.

## Next steps
- The user runs the interactive prompt once in a real terminal, then Phase 2 can move from in-review to done.
- Task 5 stock baseline and task 6 ABI check before Phase 4.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
