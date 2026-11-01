# Research: feature layer for a self-built Windows Android emulator (commercial internal testing)

Date: 2026-11-01 (Asia/Saigon). Scope: features and their commercial licensing. The VMM, graphics, and guest-image engine are covered by a separate report. Not legal advice; items marked **[UNVERIFIED]** or **[LEGAL-UNCERTAIN]** need confirmation by counsel or a primary source before you act on them.

## 1. Bottom line

1. **ARM translation is the only feature with no clean free option.** No free ARM-to-x86 translator can legally be redistributed inside your own image. Google's `libndk_translation` is licensed only inside Google's SDK system images. `libhoudini` is proprietary. AOSP Berberis is open source but translates only riscv64. The only commercially licensable translator is **Intel Bridge Technology**, a paid B2B product. Because the apps are internal and you build them yourself, the cheapest and cleanest answer is to **add `x86_64` to the test builds' ABIs**. Keep ARM-only coverage on physical devices.
2. **Root is easy and the licenses are fine for internal use.** A `userdebug` AOSP build gives `adb root`, which covers most testing. Magisk (GPL-3.0) and KernelSU (GPL-2 kernel part, GPL-3 for the rest) only trigger obligations when you distribute outside the company.
3. **Google Play/GMS cannot be shipped.** It requires MADA plus device certification, which you cannot get for a home-built emulator. Ship without GMS by default, offer microG (Apache-2.0) as an opt-in, and install apps with adb or bundletool. Play Integrity's Device and Strong verdicts will always fail on these instances.
4. **Most testing features already exist as free, permissive designs.** The goldfish console, the gRPC `EmulatorController`, adb, bundletool, scrcpy, UI Automator and Appium cover them. Copy their interfaces rather than inventing your own.
5. **Copy two things from MuMu and LDPlayer.** First, a read-only shared system disk with a per-instance copy-on-write data disk. Second, a CLI (`ldconsole`- and `MuMuManager`-style) that the GUI, sync-input and batch operations all drive.
6. **For the host UI, use Tauri 2 (Rust) for the manager**, with a **native Win32/D3D window per instance owned by the VMM process**. The webview should never carry guest video.

A strategic aside for the engine team, outside my scope: Google's own Android Emulator already runs on WHPX alongside Hyper-V. Its SDK x86_64 "Google APIs" images include ARM translation that Google explicitly permits for development and testing on on-premises machines ([Android Developers blog](https://android-developers.googleblog.com/2020/03/run-arm-apps-on-android-emulator.html)). If ARM coverage matters, that is the only legal free route, and it works only with Google's binaries, unmodified.

## 2. ARM app support on x86_64 guests

### Options compared

| Option | Licence for your use | Redistribute in your image? | ARM64 | AMD hosts | Verdict |
|---|---|---|---|---|---|
| Build internal apps with `x86_64` ABI | Your code | n/a | n/a | yes | **Rank 1** |
| Intel Bridge Technology (Intel BT, the commercial successor to Houdini) | Paid B2B licence: per-unit royalty, per-core/year for servers, or per-MAU for PC clients ([Intel BT page](https://www.intel.com/content/www/us/en/developer/topic-technology/bridge-technology.html), [IBT SLA v1.0](https://www.intel.com/content/www/us/en/content-details/817127/intel-r-bridge-technology-software-license-agreement-ver-1-0.html)) | Yes, under the licence | yes | **[UNVERIFIED]** not stated by Intel | **Rank 2** if ARM is a must |
| Google `libndk_translation` inside Google SDK system images | Android SDK License §3.1 allows use "solely to develop applications"; §3.4 forbids copying, modifying, redistributing or making derivative works ([SDK terms](https://developer.android.com/studio/terms)). Google's blog limits it to "application development and debugging only… customer on-premises servers" and excludes "commercial hosted services" | **No** | yes | yes | Rank 3, and only if you run Google's image unmodified. **[LEGAL-UNCERTAIN]**: §3.2 bars using the SDK "to develop another SDK", and an emulator product may count |
| `libndk_translation` or `libhoudini` extracted from WSA, ChromeOS or Pixel firmware (the redroid and waydroid_script approach) | None granted. These are proprietary binaries ([redroid notes](https://hub.docker.com/r/erstt/redroid), [libhoudini package from WSA](https://www.github.com/natsumerinchan/libhoudini-package)) | No | yes | yes | **Reject.** Infringement risk in a commercial setting |
| AOSP Berberis | AOSP open source (Apache-2.0 per the AOSP default; the README states no licence, **[UNVERIFIED]**) | Yes | **No.** riscv64 guest only ("run Android apps with riscv64 native code on x86_64") ([README](https://android.googlesource.com/platform/frameworks/libs/binary_translation/+/refs/heads/main/README.md)) | yes | Not usable for ARM today |
| ARM64 guest under QEMU TCG | QEMU GPL-2, fine | Yes | yes | yes | Legal but impractically slow for UI testing. WHPX only accelerates x86 guests |

### What competitors use

- **WSA** (shut down 5 Mar 2025, per [ElevenForum quoting Microsoft Learn](https://www.elevenforum.com/t/windows-subsystem-for-android-wsa-is-being-deprecated-beginning-march-5-2025.23205/post-424712)) shipped Intel BT/libhoudini. Community repacks extract it from WSA.
- **Google Play Games on PC** uses "Intel Bridge Technology" for ARM games, and ARM-only games are offered only on Intel CPUs ([Play Games PC docs](https://developer.android.com/games/playgames/start?hl=zh-tw)).
- **LDPlayer** announced an Intel BT integration on 25 Jun 2026 ([GameDeveloper press release](https://www.gamedeveloper.com/press-release/ldplayer-improves-cross-architecture-compatibility-with-intel-bridge-technology)). This is a vendor claim, and Intel's notice disclaims endorsement.
- **MuMu and BlueStacks**: I found no primary source. They are widely reported to use houdini-derived or in-house translators. **[UNVERIFIED]**

### Recommendation

Make x86_64 the contract. For internal Gradle builds, set `ndk { abiFilters "arm64-v8a", "x86_64" }` on the test flavour. Most commercial native SDKs (Unity, Unreal, Firebase, ML Kit) publish x86_64 libraries. Have the manager show an "ARM-only APK, not supported" error at install time by inspecting the `lib/` folders. Reopen Intel BT only if a must-test third-party ARM-only binary appears, and email IBT@intel.com about AMD host support before you commit.

## 3. Root

| Mechanism | Licence | How to bake in | Per-instance toggle |
|---|---|---|---|
| `userdebug` or `eng` AOSP build plus `adb root` | AOSP (Apache-2.0) | Build the `*-userdebug` lunch target. `ro.debuggable=1` makes adbd honour `adb root` | Inherent. Apps see no `su`, so this is the "root off" state for app-visible root checks |
| Magisk | GPL-3.0 ([repo](https://github.com/topjohnwu/Magisk)) | Patch the guest ramdisk with `magiskinit`. Magisk ships emulator scripts (`scripts/emulator.sh`, `avd_patch.sh`, `live_setup.sh`; [dev docs](https://magisk.readthedocs.io/en/latest/developers/deployment.html)) for API 23–36. Community rootAVD does the same for AVDs ([rootAVD](https://gitlab.com/newbit/rootAVD/-/issues/113)) | Keep two ramdisks (stock and Magisk-patched) beside the shared system image. The instance config picks one at boot |
| KernelSU (or the KernelSU-Next fork) | `kernel/` GPL-2, rest GPL-3 ([KernelSU](https://kernelsu.org/llms.txt), [KernelSU-Next](https://github.com/KernelSU-Next/KernelSU-Next/)) | Build it into your x86_64 GKI-style guest kernel, or load it as an LKM. Note: x86_64 syscall-table hardening breaks the hook, and Next warns of x86_64 panics | Kernel cmdline flag or module load per instance. Riskier than Magisk on x86_64 |

Recommendation: ship `userdebug` everywhere, plus an optional Magisk-patched ramdisk selected per instance. Skip KernelSU unless you need to test kernel-level root hiding. GPL source obligations apply only if you convey binaries outside the company. Keep the build scripts in-repo regardless.

## 4. Device spoofing (per-instance identity)

How the commercial emulators do it: their settings UI offers editable model and brand presets, IMEI, phone number and a "randomize" button for each instance. **[UNVERIFIED]** I did not confirm the mechanism from vendor documentation. Use AOSP's own channels:

| Identity | Where it lives in AOSP | Per-instance mechanism (recommended) |
|---|---|---|
| Model, brand, manufacturer, device, fingerprint | `ro.product.{,system.,vendor.,odm.}*` and `ro.build.fingerprint` in each partition's `build.prop`. `ro.*` properties are write-once | Inject through bootconfig or the kernel cmdline as `androidboot.*` (which becomes `ro.boot.*`), plus a small init `.rc` or `property_service` patch in your AOSP tree that copies them into `ro.product.*` at early init. Alternative: a tiny per-instance `odm` disk holding `build.prop` overrides. Magisk `resetprop` works but needs root |
| IMEI, IMSI, ICCID, phone number, operator | The RIL HAL talks to an emulated modem. The goldfish emulator has a modem simulator with the `phonenumber` console command ([console](https://developer.android.com/studio/run/emulator-console)). Cuttlefish's `modem_simulator` has `SetPhoneNumber` and `LoadNvramConfig` ([docs](https://google.github.io/android-cuttlefish/cvd/classcuttlefish_1_1ModemSimulator.html)) | Reuse Cuttlefish's `modem_simulator` (Apache-2.0) as a host-side process per instance, with an NVRAM and ICC profile file per instance. **[UNVERIFIED]** the exact IMEI and ICCID keys; read `LoadNvramConfig` |
| Android ID | Since Android 8 it is scoped per signing key, user and device ([API ref](https://learn.microsoft.com/en-us/dotnet/api/android.provider.settings.secure.androidid)) and stored in `/data/system/users/0/settings_ssaid.xml` **[UNVERIFIED]** path | **Clone must regenerate it.** Delete or reseed `settings_ssaid.xml` on a cloned data disk before first boot. Otherwise every clone shares the same IDs |
| Serial | `ro.serialno` comes from `androidboot.serialno` | Bootconfig per instance. The manager generates it |
| MAC (Wi-Fi/Ethernet) | virtio-net MAC set by the host. Guest Wi-Fi MAC randomisation also applies | The VMM assigns a stable MAC per instance (locally administered range) |
| GPS | Goldfish GNSS HAL fed by `geo fix` / `geo nmea` or gRPC `setGps` (sampled at about 1 Hz) | Host-pushed per instance. Support GPX/KML route playback |
| Advertising ID | GMS only | Not applicable without GMS. microG provides its own |

Provide a "device profile" JSON (brand, model, fingerprint, screen density, Android ID seed, IMEI, MSISDN, MAC, serial, locale and timezone) per instance, plus a "randomize" helper that emits valid Luhn IMEIs and real TAC prefixes. Spoofing your own test devices is lawful. Using it to defeat third-party fraud controls may breach those parties' terms. **[LEGAL-UNCERTAIN]**, depending on intended use.

## 5. App-testing tools

**Copy goldfish's interfaces.** The console groups are `geo`, `gsm`/`cdma` (calls, signal, voice/data state), `sms send`, `power` (ac/status/health/capacity), `network delay|speed|capture`, `sensor get|set`, `finger touch`, `rotate`, `avd snapshot save|load`, `screenrecord`, `multidisplay`, `proxy`, `phonenumber` and `automation record|play` ([emulator console](https://developer.android.com/studio/run/emulator-console)). The gRPC [`emulator_controller.proto`](https://android.googlesource.com/platform/tools/base/+/studio-master-dev/emulator/proto/emulator_controller.proto) adds sensors, GPS, battery, screenshot and screen streaming, input injection, clipboard, and audio. Google marks it experimental and liable to change, so treat it as a template rather than a stable contract. Recommendation: implement a subset of the gRPC service in your VMM. Existing clients such as Android Studio's embedded emulator view and the Rust crate [android-emulator](https://docs.rs/android-emulator) then partly work for free. Also expose the telnet console, because `adb emu <cmd>` depends on it.

| Feature | Implementation | Licence |
|---|---|---|
| APK install | `adb install` / `install-multiple` | AOSP Apache-2.0 |
| AAB install | `bundletool build-apks --connected-device` then `install-apks` | Apache-2.0 |
| XAPK/APKS/APKM | A non-standard zip of split APKs plus OBB. Unzip, `adb install-multiple`, then push the OBB to `/sdcard/Android/obb/<pkg>/` | n/a (format **[UNVERIFIED]**, de facto only) |
| Logcat, bugreport | adb | Apache-2.0 |
| Screenshot, recording | Host-side capture from the framebuffer (fast, no guest cost), or `adb exec-out screencap` / `screenrecord` | — |
| File transfer | `adb push`/`pull`, plus drag-and-drop in the UI that calls push | — |
| Network throttle | Console `network delay/speed` semantics implemented in the VMM's virtio-net backend | — |
| Battery, sensors, GPS, calls, SMS | Console and gRPC surface above, backed by goldfish HALs in the guest image | Apache-2.0 |
| ADB for many instances | Each instance gets a console port at even 5554+2n and adbd at 5555+2n, which the adb server auto-discovers. Beyond the default scan range, use `adb connect 127.0.0.1:<port>` or raise `ADB_LOCAL_TRANSPORT_MAX_PORT` **[UNVERIFIED]** env var name; check adb source. Bind to 127.0.0.1 only. Ports are deterministic per instance index | — |
| Automation | UI Automator, Espresso and the Appium UiAutomator2 driver all work over adb unchanged | Apache-2.0 |
| Streaming/remote | Native window per instance. For remote or headless use, embed or port scrcpy (Apache-2.0), or stream from the gRPC display | Apache-2.0 |
| Snapshots | `avd snapshot` semantics: save/load VM state per instance | VMM work (engine report) |

## 6. Google Play / GMS

- **The licensing reality.** GMS is available only to OEMs who sign the MADA and pass device certification (CTS/GTS). The licence is non-transferable and cannot be sublicensed ([XDA](https://www.xda-developers.com/huawei-mate-30-google-play-store-challenges/amp/), [eInfochips](https://www.einfochips.com/blog/how-to-obtain-googles-gms-license-for-android-devices), [DOJ exhibit](https://www.justice.gov/atr/media/1322106/dl)). Small vendors are generally not offered a MADA ([Hexnode](https://www.hexnode.com/blogs/gms-certification-is-it-really-necessary-for-android-devices/)). Extracting GApps (OpenGApps, MindTheGapps) into your image is unlicensed. The commercial emulators presumably hold their own agreements. **[UNVERIFIED]**
- **microG** (Apache-2.0) supplies the GMS APIs for FCM-style push, location and SafetyNet/Play Integrity stubs. On non-certified hardware it reaches only the Basic Play Integrity verdict at best, never Device or Strong ([/e/OS docs](https://doc.e.foundation/os/apps/microg)). April 2026 reports show even Basic failing at times ([e/OS forum](https://community.e.foundation/tag/microg/22)). microG needs signature spoofing in the image, which is a patch to your AOSP framework. Note that an app's FCM push through microG still uses Google's servers under your app's Firebase project, which is fine for your own apps.
- **Aurora Store** (GPL-3.0) downloads from Google Play using anonymous or personal Google accounts ([APKMirror listing](https://www.apkmirror.com/?p=15067386)). **[LEGAL-UNCERTAIN]**: this likely conflicts with Google Play's Terms of Service. Do not make it a standard corporate workflow. Allow it only if counsel approves.
- **What to expect.** Instances have no Play Store, no Play Billing (test purchases need a real certified device or Google's emulator image), no Play Integrity device verdict, and no Google sign-in through GMS. Distribute internal builds through adb, bundletool, or your CI artifact store. Keep a small pool of real devices or Google's official emulator for Play-dependent tests.
- **Ranking:** (1) no GMS by default; (2) microG as a per-instance opt-in image variant; (3) Aurora only with legal sign-off.

## 7. Multi-instance manager

What MuMu documents: **Clone** (or "Copy") duplicates an instance with its settings and data. Performance profiles set CPU and RAM per instance. A **read-only system disk** shared across instances improves multi-open performance ([MuMu help](https://mumuglobal.com/mac/tutorials/multi-instance.html), [MuMu multi-instance](https://mumuglobal.com/mac/function/20240319/40268_1144076.html)). MuMu also has an App Cloner (up to 9 copies, [FAQ](https://www.mumuglobal.com/faq/app-cloner-guide.html)) and lists a "sync function" without documenting it ([release notes](https://mumuglobal.com/update/android11/index.html)). LDPlayer has a Multi-Instance Manager and an "Operation Sync" synchronizer. That comes from secondary sources only **[UNVERIFIED]**. Both ship CLIs (`ldconsole.exe`, `MuMuManager.exe`) that are widely used for scripting **[UNVERIFIED]** command sets.

Design to copy:

1. **Storage.** One immutable base (system, vendor, kernel and ramdisk variants) per Android version. Per instance, a differencing data disk (VHDX differencing or qcow2) plus a small config disk. A clone copies the data disk, or uses CoW from a snapshot, and regenerates the identity: Android ID seed, serial, MAC, IMEI.
2. **Control plane.** A single local daemon (Rust) owns instance state, ports and processes. The GUI, a CLI (`emuctl list|create|clone|start|stop|install|shell|sync`) and an optional local gRPC/HTTP API are thin clients. This is DRY: batch operations and sync-input are just fan-out over that API.
3. **Sync input.** Capture input in the "leader" window as normalised coordinates (0–1) and keycodes, then inject them into followers through the VMM's virtio-input channel, which has lower latency than `adb shell input`. Keep it best-effort, because it drifts as soon as followers' UIs diverge.
4. **Batch operations.** Install, uninstall, push, start and stop across selected instances, with bounded concurrency so 30 simultaneous adb installs do not saturate the disk.
5. **Resource limits.** vCPU, RAM and frame-rate cap per instance. Put each VMM process in a **Windows Job Object** (CPU rate and memory limit) and set a "max running instances" guard. Track PID and port per instance, and reap orphans on daemon start.
6. **Window arrangement.** Tile or cascade instances, and pin a leader.

## 8. Host launcher/manager app stack

| Stack | Licence | Fit for this tool | Risk |
|---|---|---|---|
| **Tauri 2 + Rust** | MIT/Apache-2.0 | Small binary on WebView2. Shares a language and crates with a Rust daemon or VMM. Fine for lists, forms and logs | Webview is not suited to guest video. Keep the display native |
| Qt 6 (C++/QML) | LGPL-3 (dynamic linking is fine internally) or commercial ([licensing debate example](https://github.com/tauri-apps/tauri/issues/4554)) | Best for embedding native GPU surfaces inside one app window | Heavier toolchain. Licence handling. C++ only, unless you use PySide |
| WinUI 3 (C#/.NET) | MIT ([MS docs](https://learn.microsoft.com/is-is/windows/apps/winui)) | Native Windows look, and a `SwapChainPanel` for D3D video | Windows-only (acceptable here). Packaging friction. Two languages if the VMM is Rust |
| Electron | MIT | Fastest UI iteration | 150 MB+ per app and higher memory, which is bad when the RAM is meant for VMs |

Recommendation (ranked): **(1) Tauri 2** for the manager and CLI companion, with each instance's display as a native top-level Win32 window rendered by the VMM process (D3D11/Vulkan swapchain). **(2) WinUI 3** if the team is C#-first or wants instance video docked inside the manager. **(3) Qt 6** if cross-platform hosting ever matters. Avoid Electron. Secondary sources only for comparisons ([2026 survey](https://www.youngju.dev/blog/culture/2026-05-14-desktop-app-frameworks-2026-tauri-electron-wails-compose-multiplatform-maui-flutter-comparison-deep-dive-2026)). Confirm Tauri 2's current release status yourself.

## 9. Adoption risk summary

| Component | Maturity | Main risk |
|---|---|---|
| adb, bundletool, UI Automator, Appium, scrcpy | Very mature | Low |
| Goldfish console and gRPC proto | Mature, but the proto is "experimental" | API drift. Pin a revision |
| Cuttlefish modem_simulator | Maintained by Google | Porting effort outside Cuttlefish |
| Magisk | Mature, single maintainer | Bus factor. Breaks on new Android releases |
| KernelSU on x86_64 | Fragile | Kernel panics and hook breakage |
| microG | Active, small team | Play Integrity regressions |
| Intel BT | Commercial, 100M+ deployments per Intel | Cost, AMD support unknown, vendor lock-in |
| Berberis | Active, riscv64 only | Not applicable to ARM |

## 10. Limitations

- No access to MuMu, LDPlayer or BlueStacks binaries or internal docs. Their translation engines, spoofing mechanisms and CLIs are from secondary or community sources.
- Intel BT pricing, AMD support and Android-version support were not published. You must ask Intel directly.
- I did not read the AOSP source for the exact property-override order, `settings_ssaid.xml` handling, or the modem_simulator NVRAM keys. These are implementation-time checks.
- This is not legal review. Every row marked [LEGAL-UNCERTAIN] needs counsel, especially Google SDK §3.2/§3.4 and Aurora Store.

## Unresolved questions

1. Are any must-test apps or third-party SDKs ARM-only, with no x86_64 build? The answer decides whether Intel BT is needed at all.
2. Does Intel BT support AMD hosts, and what does it cost for an internal-only deployment of N seats?
3. Would counsel accept running Google's unmodified SDK x86_64 images (with ndk_translation) inside a non-Google VMM for internal testing, given SDK License §3.2?
4. Will the VMM be written in Rust? That strengthens the Tauri choice.
5. Is Play Billing or Play Integrity testing in scope? If yes, real devices remain mandatory.
