---
title: "MTPlayer Phase 1 spike: stock emulator passes, review decisions recorded"
date: 2026-11-01
summary: "Google's stock emulator 37.2.13 with the android-36 default image passed boot, multi-instance and runtime identity gates; six review decisions recorded and pushed."
---

# MTPlayer Phase 1 spike: stock emulator passes, review decisions recorded

## What happened
The Phase 1 workflow provisioned emulator 37.2.13 and system-images;android-36;default;x86_64 into .tmp, then booted, cloned and ran four instances. Quick Boot took 3.4-6.9 s, a normal cold start 23.8 s and a first boot 60-66 s. All ports were loopback-only and the per-instance gRPC token worked. Model, brand, device, fingerprint, serial, Android ID, phone number and IMSI/ICCID changed per instance at runtime. The IMEI is compiled into the emulator, and eth0's MAC is fixed. Google's files were byte-identical to their zips afterwards.

## Problems found
- The license acceptance hash (efa68a6b...) does not match what Android Studio writes for the same 2019 text (24333f8a...); cause unknown.
- The console token is per Windows user, not per instance.
- `adb reboot` froze 37.2.13; `-android-serialno` hung it.
- The Android ID reset needs network isolation and a cold restart.

## Decisions
- Cold boot means starting an existing instance; first boot has its own 90 s target.
- WSL2 and Docker re-measure before Phase 4.
- IMEI and eth0 MAC are listed as fixed, with an optional RAM-only MAC change.
- The daemon controls instances over gRPC only.
- MTPlayer keeps its own license acceptance record.
- No MuMu comparison: criterion 3 means staying within 10% of a plain stock-emulator launch.

Commits c62279c and c41b1fd were pushed to main.

## Next steps
- Phase 2 (package provisioning).
- Task 5, the stock baseline benchmark.
- Task 6, the ABI check, which needs the work APKs.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
