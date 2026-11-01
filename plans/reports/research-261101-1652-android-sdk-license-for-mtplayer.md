# Android SDK License and MTPlayer: Google sources and community precedent

Researched 2026-11-01 by two Fable research agents: one on Google's own
sources, one on community and industry precedent. This is a research summary,
not legal advice.

## Contents

1. [Verdict](#verdict)
2. [What governs MTPlayer's two downloads](#what-governs-mtplayers-two-downloads)
3. [The clauses that matter](#the-clauses-that-matter)
4. [Google's own precedent](#googles-own-precedent)
5. [Community and industry precedent](#community-and-industry-precedent)
6. [Residual risks](#residual-risks)
7. [Changes this suggests for the plan](#changes-this-suggests-for-the-plan)
8. [Sources](#sources)
9. [Unresolved questions](#unresolved-questions)

## Verdict

MTPlayer's design holds up, and it is stricter than both Google's own tools and
the industry norm. The design: show the license, let the user accept, download
the pinned stable emulator and the `android-36;default;x86_64` image from
dl.google.com, and never redistribute or modify them.

- Both downloads fall under the plain `android-sdk-license`.
- That license has no non-commercial or internal-only limit. §2.4 expects
  people to accept it on behalf of an employer.
- Google's own Apache-2.0 container tool does the same thing MTPlayer plans:
  it reads the repository XML, shows the license, records acceptance and
  downloads the packages. Google then redistributes the results in public
  containers.
- Debian ships download-after-acceptance installers for the emulator.
  F-Droid wrote its own `sdkmanager`. Neither has been challenged.
- No sign of Google enforcing the license against anyone was found. That
  includes Docker images that have redistributed Google's system images in
  public since about 2017.

What remains is interpretive and untested, with no precedent either way:
§3.2 "develop another SDK", runtime identity spoofing, and whether the person
clicking "accept" at work has the authority to bind the company.

## What governs MTPlayer's two downloads

Read from Google's repository XML on 2026-11-01:

| Package | Revision | License id | Channel |
| --- | --- | --- | --- |
| `emulator` (Windows `emulator-windows_x64-16740729.zip`) | 37.2.13 | `android-sdk-license` | channel-0 (stable) |
| `emulator` (dev) | 37.3.6 | `android-sdk-preview-license` | channel-2 (dev) |
| `system-images;android-36;default;x86_64` (`x86_64-36_r02.zip`, 844,217,077 bytes) | 2 | `android-sdk-license` | channel-0 (stable) |
| `android-36` arm64-v8a `google_apis` and `playstore` images | n/a | `android-sdk-arm-dbt-license` | n/a |

- **The text a user accepts at download time is dated January 16, 2019.** It
  is embedded in `repository2-3.xml` and `sys-img2-3.xml`, and it is what
  `sdkmanager` and Android Studio show. The web page is dated April 28, 2026.
  Their §3 and §4 are word-for-word the same.
- **The preview license is stricter.** It limits use to "personally or
  internally within your company", and forbids "combine any part of the
  Preview with other software". It also keeps the old anti-fragmentation
  clause. MTPlayer must therefore never pin a dev or canary emulator.
- **The ARM DBT license** is the SDK License plus clause 4.7. That clause
  limits ARM execution to "application development and debug only on x86
  desktop, laptop, customer on-premise servers, and customer-procured
  cloud-based environments". It matters only to the later ARM plan.
- **Open-source notices ship with both packages.**
  - The emulator zip ships `LICENSE` (Apache-2.0), `NOTICE.txt` and
    `NOTICE.csv`, which list Apache, MIT, BSD, LGPL and GPL parts.
  - The image zip ships `x86_64/NOTICE.txt`. Its contents were checked only
    on a sibling Google image.
  - Google's REGISTRY.MD says: "The Android Emulator binary is released under
    the Apache 2.0 License."

## The clauses that matter

The clauses are the same in the 2019, 2021 and 2026 versions.

| Clause | Text (short) | Effect on MTPlayer |
| --- | --- | --- |
| §2.1 | You must agree before use | The license gate is required. |
| §2.4 | Accepting for an employer requires "full legal authority to bind your employer" | A work install needs someone with that authority to accept. |
| §3.1 | Use "solely to develop applications for compatible implementations of Android" | Testing your own apps at work fits. |
| §3.2 | Not "to develop another SDK" | No Google interpretation exists. No source treats a launcher as "another SDK". |
| §3.4 | No copying, modifying, redistributing or derivative works, "except to the extent required by applicable third party licenses" | MTPlayer never ships or changes the files. |
| §3.5 | Open-source components are "governed solely by" their own licenses | Most of the emulator and image is Apache, BSD or GPL, per their NOTICE files. |
| §3.8 | Don't "remove, obscure, or alter any proprietary rights notices" | Keep the NOTICE files, and don't hide them. |
| §4.2 | Use only for lawful purposes, including export law | Normal use; nothing extra to do. |

The 2012 version had clauses that would have been a real problem. It forbade
loading the SDK onto anything "except a personal computer", combining it
"with other software", and promoting "a software development kit derived from
the SDK". Most of the "the SDK is non-free" commentary online, from FSFE,
Replicant, LWN and F-Droid in 2013–2015, quotes that old wording. **None of
those clauses is in the current SDK License.** They survive only in the
preview license.

## Google's own precedent

- **android-emulator-container-scripts** (`github.com/google`, Apache-2.0):
  - it reads the repository XML and shows each `uses-license` text;
  - it asks "Do you accept the license?", and offers `licenses --accept` to
    skip the prompt;
  - it downloads the emulator and image zips from dl.google.com and builds
    containers;
  - Google hosts prebuilt containers in a public registry. REGISTRY.MD says:
    "By using these container images, you accept the Android SDK License
    Agreement."
- **Acceptance format** (Google's `License.java`): acceptance is stored as
  the SHA-1 of the license text, one line in `<sdk>/licenses/<license-id>`.
  Google's docs tell users to copy that `licenses/` folder to other machines,
  so Gradle can download packages there without a prompt.
- **Documented runtime features** cover everything MTPlayer does at runtime:
  - `-writable-system`, `-prop`, `-qemu` and `-selinux`;
  - `adb root` and `adb remount`;
  - userdebug builds are defined as "a user build with root access enabled".

  Nothing Google publishes calls runtime property changes, files pushed to
  `/data`, or qcow2 overlays a modification of the SDK.
- **Gradle Managed Devices** and the 2019 blog post on emulator CI show that
  Google expects automated, headless image downloads.

## Community and industry precedent

| Who | Model | License handling | Result |
| --- | --- | --- | --- |
| Debian `google-android-emulator-installer` | Downloads from dl.google.com at install time | Package in `non-free`. `google-android-licenses` pre-marks the licenses as accepted. | Shipping since 2017; no challenge |
| F-Droid `sdkmanager` (AGPLv3) | Own reimplementation of Google's installer, checks hashes | Packaged by Debian | Since 2021; no §3.2 challenge |
| thyrlian/AndroidSDK | Docker image with SDK tools, no images | README: "one may not redistribute the SDK"; the user accepts | Closest match to MTPlayer's position |
| ReactiveCircus/android-emulator-runner | Downloads at run time | `yes \| sdkmanager --licenses`, silently | Most popular Android CI action; no challenge |
| budtmo/docker-android | Redistributes Google's images on Docker Hub, and sells a Pro image | Never mentions the SDK License | No takedown found since about 2017. Being tolerated is not being permitted. |
| Sauce Labs | Paid service on Google's `google_apis_*` AVDs | States no license basis | No enforcement found |
| Magisk `avd_patch.sh`, rootAVD, MagiskOnEmulator | Patch the AVD's `ramdisk.img` | No license discussion | No takedown. Their changes go further than MTPlayer's runtime-only spoofing. |
| Genymotion | Its own images, with Google apps excluded "for Intellectual Property reasons" | — | Shows that GMS and Play are the real IP hot zone. MTPlayer's `default` image has no GMS. |
| Microsoft (VS Emulator, WSA) | Built its own AOSP | — | That was a GMS distribution choice, not an SDK License problem. |

Enforcement:
- The github/dmca search found no notice about Android SDK images, the
  emulator, libhoudini or libndk_translation. The search was partial,
  because GitHub's API rate limit cut it short.
- Against prop spoofing, Google's counter-measures are technical (Play
  Integrity attestation), not legal.

Auto-accepting the license is everywhere: `yes | sdkmanager --licenses`,
Google's own `--accept` flag, Debian's license package. The only credible
issue raised is §2.4 authority.

## Residual risks

1. **§3.2 "develop another SDK"**: interpretive risk, low. No Google
   statement exists, and every discussion of it concerns forking or
   rebuilding the SDK itself. MTPlayer only drives the unmodified emulator.
   The closest analogues, Debian's installers and F-Droid's `sdkmanager`,
   have stood for years unchallenged.
2. **Spoofing identity at runtime**: interpretive risk, low to medium. Two
   arguments could be made against it:
   - that `resetprop` "adapts" the SDK (§3.4);
   - that spoofing model, brand and fingerprint creates a non-"compatible
     implementation" (§3.1/§3.2).

   No Google text supports either argument, and Magisk and rootAVD change
   far more with no complaint. Using it to test your own apps is the
   strongest position. Using it to get around anti-fraud or attestation
   systems would be §4.4 territory ("accesses in an unauthorized manner the
   servers ... of any third party").
3. **§2.4 authority at work**: a practical risk, and the only real one for
   commercial use. The person who accepts on the work PC should be entitled
   to bind the company, or the company's usual policy for accepting
   developer-tool licenses should cover it. Android Studio installs raise
   the same question, so most companies already have an answer.
4. **Google can withdraw packages or change terms** (§3.6). Acceptance is
   tied to the exact license text by its SHA-1, so a changed text asks
   again.

## Changes this suggests for the plan

These are proposals and have not been applied:

- **Phase 2:**
  - pin channel-0 (stable) packages only, and refuse any package whose
    `uses-license` isn't `android-sdk-license`;
  - store acceptance in Google's format (the SHA-1 of the exact XML text in
    `<sdk>/licenses/<id>`), so the stock emulator tools recognize it;
  - keep the shipped `NOTICE` and `LICENSE` files, and show them in the GUI
    (§3.8);
  - word the license dialog so the user confirms they may accept for their
    employer (§2.4).
- **Phase 1, task 3:** `-prop` only sets properties labelled `qemu_prop`, so
  `ro.product.*` will most likely need `resetprop`. The spike should expect
  this.
- **The later ARM plan:** arm64 images carry the ARM DBT license, so the
  license gate has to handle more than one license id.
- **README:** state that MTPlayer is for testing apps, and that using its
  spoofing to get around third-party anti-fraud systems is out of scope and
  against §4.4.

## Sources

Google:
- Android SDK License, web page dated 2026-04-28:
  https://developer.android.com/studio/terms. Archive snapshots of the
  2012, 2019 and 2021 versions are on web.archive.org.
- Repository XML: https://dl.google.com/android/repository/repository2-3.xml
  and https://dl.google.com/android/repository/sys-img/android/sys-img2-3.xml
- Emulator archive and NOTICE: https://developer.android.com/studio/emulator_archive
- AOSP QEMU license: https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-master-dev/LICENSE
- Container scripts and REGISTRY.MD: https://github.com/google/android-emulator-container-scripts
- `sdkmanager` docs: https://developer.android.com/tools/sdkmanager
- Copying the `licenses/` folder: https://developer.android.com/studio/intro/update
- Gradle Managed Devices: https://developer.android.com/studio/test/gradle-managed-devices
- Emulator command line: https://developer.android.com/studio/run/emulator-commandline
- Emulator CI blog, 2019-10-02: https://android-developers.googleblog.com/2019/10/continuous-testing-with-new-android.html
- GSI no-redistribution note: https://developer.android.com/topic/generic-system-image/releases
- AOSP licenses: https://source.android.com/docs/setup/about/licenses

Community:
- Debian AndroidTools wiki and `google-android-*` packages: https://packages.debian.org/trixie/google-android-licenses
- F-Droid `sdkmanager`: https://gitlab.com/fdroid/sdkmanager
- https://github.com/thyrlian/AndroidSDK
- https://github.com/ReactiveCircus/android-emulator-runner
- https://github.com/budtmo/docker-android
- Sauce Labs emulators: https://docs.saucelabs.com/mobile-apps/android-emulators/
- Genymotion and Google apps: https://docs.genymotion.com/features/opengapps/
- Magisk AVD scripts: https://github.com/topjohnwu/Magisk/tree/master/scripts
- LWN, "Rebuilding the Android SDK", 2015: https://lwn.net/Articles/659116/
- Replicant and FSFE posts from 2013 (paulk.fr, HN item 5007565) and the
  F-Droid forum threads from 2013 and 2023.

## Unresolved questions

1. Is the person who accepts on `tiennm5-pc` entitled to bind the employer,
   or does an existing company policy cover developer-tool licenses?
2. The contents of the android-36 `default` image's `NOTICE.txt` were
   checked only on a sibling image. Phase 1 should read it from the
   downloaded zip.
3. The github/dmca search and the Stack Overflow thread on auto-accepting
   licenses (Q 42162231) were not fully checked.
4. Not covered: the emulator's telemetry terms, and the "Android" trademark
   rule beyond §3.7 (the plan already keeps "Android" out of the product
   name).
