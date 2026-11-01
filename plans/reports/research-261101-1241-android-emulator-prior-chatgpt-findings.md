# Android emulator: earlier ChatGPT findings

These findings come from two ChatGPT chats in the export
`chats/chatgpt-gmail/conversations-001.json`:

- "Android Emulator Market Share", 2026-05-25.
- "Emulator Work Use Research", 2026-09-11.

ChatGPT read the vendor terms and docs at the time; this summary has not
re-checked them.

## Licensing for work use (as of 2026-09)

| Emulator | Internal testing of your own app at work |
| --- | --- |
| BlueStacks (free) | Not allowed by the current ToS ("internal, personal, non-commercial use"; "any commercial use ... strictly prohibited" unless BlueStacks approves). A 2023 Reddit moderator said normal use was fine, but the current ToS supersedes earlier statements. Ask `eula@bluestacks.com` for written permission. |
| LDPlayer (free) | Not allowed. The Software Licensing Protocol limits it to individual users and forbids commercial use without authorization. |
| MuMu Player (free) | Not allowed. The 2025-12-01 ToS covers "internal and personal purposes" and forbids commercial use. |
| MuMu Developer Platform / SDK | Allowed. A separate agreement (effective 2026-03-16) licenses the SDK for developing and testing apps compatible with MuMu, for business users. |
| Genymotion Desktop free | Personal use only; the Business license is paid. |
| Android Emulator (AVD) | Allowed. It is free, made for app testing, and covered by the Android SDK license. |

## Compatibility with Hyper-V and WSL2 (as of 2026-05)

| Emulator | Runs with Hyper-V/WSL2 enabled |
| --- | --- |
| Android Emulator (AVD) | Yes, through the Windows Hypervisor Platform (WHPX). |
| Google Play Games on PC | Yes; supported games only. |
| BlueStacks 5 | Yes, in Hyper-V mode. |
| LDPlayer | Yes in the newer Hyper-V build, which is slower than non-Hyper-V mode. |
| MEmu | Mixed. |
| NoxPlayer, MuMu Player 12, GameLoop | No; their docs say to disable Hyper-V. |
| Windows Subsystem for Android | Dead; support ended 2025-03-05. |

## What this means for a new emulator

- No free, fast consumer emulator is licensed for work use and also runs
  with Hyper-V on. That is the gap a self-built emulator fills.
- It has to run on WHPX so it works alongside WSL2 and Docker.
- AVD's features are the bar for testing: fixed Android versions, ADB, the
  emulator console, GPS, network and sensor controls, snapshots, and CI use.
