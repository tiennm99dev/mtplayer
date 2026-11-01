---
title: "MTPlayer image re-plan: stock emulator and SDK image"
date: 2026-11-01
summary: "No goldfish image exists on ci.android.com and source builds were dropped, so v1 runs Google's stock emulator and android-36 default image, set up at runtime"
---

# MTPlayer image re-plan: stock emulator and SDK image

## What happened

The cook workflow's image gate checked all 41 public build IDs on ci.android.com, and every frozen branch. No public build has ever published a goldfish `sdk_phone64_x86_64` image, so the plan's image source didn't exist. The Fable advisor stopped and asked the user, as instructed.

## Decision

- The user dropped all source builds, both AOSP and the engine.
- v1 uses Google's stock emulator and `system-images;android-36;default;x86_64`.
- Each user downloads both through MTPlayer, after accepting the Android SDK License.
- No Google file is modified or redistributed (SDK License §3.4).
- Root is `adb root`. Identity is set at runtime, through boot properties and `magisk resetprop` from `/data` followed by a framework restart.
- The own engine fork (Phase 3) is deferred until the user funds a cloud build.
- `C:\mtp` was deleted, and work files moved to the repo's git-ignored `.tmp/`.

## Next steps

- Run the Phase 1 spike on the stock packages.
- Before anything else, prove that runtime identity reaches `Build.*` in an app, and find which telephony and MAC fields the stock emulator lets us change.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
