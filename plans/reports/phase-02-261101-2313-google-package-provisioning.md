# Phase 2 acceptance: Google package provisioning, 2026-11-02

This report closes the acceptance stage of
[Phase 2](../261101-1256-mtplayer-engine/phase-02-google-package-provisioning.md)
of the [MTPlayer engine plan](../261101-1256-mtplayer-engine/plan.md) on miti-b550.
The evidence files named below are in the git-ignored `.tmp/evidence/phase2/`
folder.

## Outcome

All 10 Phase 2 checks passed against the real endpoints with the built `mtp`
binary. The full install downloaded 3 packages (1.31 GB) in 58 s, and the resulting
root booted Android 16 to `sys.boot_completed=1` in 64.7 s. The acceptance stage
found no bug that made a check fail.

**The phase stays in review, not done.** The interactive license prompt was not
exercised end to end, because the user chose to accept by flags. The Verification
item that says the install asks for the license is therefore only partly proven.
[The user's one-minute check](#the-users-one-minute-check) closes it.

The final review by Fable found no blocking defect and named three small fixes,
which are now in the code. They came after the acceptance run, so the acceptance
evidence describes the binary before them, and the 1.3 GB install was not repeated.
Fixture tests cover the changed paths. The tree after the fixes passes fmt, clippy,
232 offline tests and 4 small live tests.

Check 7 has one caveat: during the boot, the NVIDIA driver updated three of its own
existing files outside the root. See [the NVIDIA caveat](#the-nvidia-caveat).

## The license acceptance on record

The user gave this acceptance on 2026-11-02. It is personal, and it applies only to
the scratch homes `.tmp/mtp-sdk` and `.tmp/mtp-check4` (the second is now deleted).
Acceptance went through `--accept` with the user's exact hashes and
`--on-behalf-of personal`. No record was written by hand.

| License | SHA-1 of the text | Authority | Via |
| --- | --- | --- | --- |
| `android-sdk-license` | `efa68a6b3c661d18699d5c026771d5911cdc2f83` | personal | flag |
| `gpl-3.0` | `12d81f50767d4e09aa7877da077ad9d1b915d75b` | personal | flag |

The records in `.tmp/mtp-sdk/licenses/` show `accepted_at` 2026-11-01 18:59:06 UTC,
which is 2026-11-02 01:59:06 in Asia/Saigon. This acceptance says nothing about work
use or `tiennm5-pc`, and `%LOCALAPPDATA%\MTPlayer` holds no acceptance, because that
folder still doesn't exist.

## The user's one-minute check

Run this in a real terminal, not through a pipe, because the prompt requires both
stdin and stdout to be terminals.

```text
cargo run -q -p mtp-cli -- sdk install --home D:\tiennm99dev\mtplayer\.tmp\mtp-prompt
```

1. Read the Android SDK License text, type `I accept`, and answer `N` to the employer
   question.
2. Read the GPL-3.0 text that follows, and type anything other than `I acknowledge`.
3. Confirm that `$LASTEXITCODE` is 3 and that `.tmp\mtp-prompt\downloads` holds no zip
   or `.part` file.
4. Delete `.tmp\mtp-prompt`, then set the phase to done in the phase file and in
   `plan.md`.

The two texts appear one after the other, so you have to accept the first to see the
second. Declining at the first question also exits with 3 and downloads nothing, but
it shows only the Android text (read from `run_gate` and `gate_one` in
`crates/mtp-sdk/src/install.rs`, not observed live). Accepting the first text writes
its record in that scratch home only, which step 4 removes.

## Acceptance checks

| # | Check | Result |
| --- | --- | --- |
| 1 | Non-interactive install without acceptance is refused and downloads nothing | Passed |
| 2 | Accepted install downloads, verifies, extracts and compares every pinned package | Passed |
| 3 | A second run downloads nothing | Passed |
| 4 | A tampered zip is rejected before extraction, and a truncated part resumes | Passed |
| 5 | Independent byte-for-byte comparison with the zips | Passed |
| 6 | Boot test from the product root | Passed |
| 7 | No writes outside the root and the user's temp folder, other than the known stock files | Passed, with the caveat below |
| 8 | `mtp sdk remove` removes only what it should, and status reports not installed | Passed |
| 9 | No Google or Magisk binary tracked or staged, and no secrets | Passed |
| 10 | No process left, and ports free | Passed |
| - | Quality gates on the final source | Passed |
| - | Interactive license prompt | Skipped, as the user chose |

### Evidence for each check

1. **Refusal** (`check1-refused.txt`). Install with no `--accept` gave exit 3 in
   0.01 s. `--accept` without `--on-behalf-of` gave exit 2, and a wrong hash gave
   exit 3. After all refusals the home held only a 0-byte `mtp-sdk.lock`. The live
   hashes matched the user's decision.
2. **Install** (`check2-install.txt`, `check2-result.txt`). It started with an empty
   root and no seeding from `.tmp/dl`, and it ended with exit 0, 58 s and
   `downloads=3`. The zips were emulator 455,344,678 B, image 844,217,077 B and Magisk
   APK 10,679,283 B. Installed were the emulator (386 files, 1,121,594,689 B), the
   image (20 files, 2,049,707,357 B), an empty `platform-tools`, and Magisk
   (410,576 B). The crate's compare reported 386 and 20 files matching, and
   `status --verify` reported ok with 406 files and 2.95 GiB.
3. **Second run** (`check3-second-run.txt`). It gave `downloads=0` and exit 0 in
   0.02 s, also with `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` pointed at a closed
   port. As a control, a command that needs the network failed under that proxy with
   exit 1. The file list, sizes and mtimes of the whole home showed 0 differences.
4. **Tamper and resume** (`check4-tamper-resume.txt`), in a separate home with a
   development manifest pinning `platform-tools` first.
   - Case A: a complete byte-flipped part gave exit 1 with expected and actual hashes.
     The part was deleted, and no package directory or staging folder existed.
   - Case B: a truncated part with a flipped byte printed `resume from 3 MiB`, then
     failed the hash with exit 1 and extracted nothing.
   - Case C: a byte-flipped final zip plus a good 3 MiB part. The bad zip was discarded
     unextracted, the download resumed, the SHA-1 matched, and the install finished
     with exit 0. No message said the kept zip was bad at that time; the final review
     fix below adds one.
   - The live test `a_cancelled_download_resumes_with_a_ranged_request_and_verifies`
     asserts the HTTP 206 itself, because the CLI doesn't print the status.
5. **Byte compare** (`check5-byte-compare.txt`), using a .NET zip reader with no crate
   code. The emulator had 386 entries and the image 20, with 0 differences over
   1,121,594,689 and 2,049,707,357 bytes. Zip hashes matched the pins. The Magisk APK
   (`89cefac0...`) and the installed binary (`19e7c1d3...`) matched their pins, and the
   binary was byte-identical to the APK member. The compare gave the same result after
   the boot (`check6-root-after-boot.txt`).
6. **Boot** (`check6-boot.txt`). `spike/spike-env.ps1` gained optional
   `MTP_SPIKE_SDK_DIR` and `MTP_SPIKE_ADB_EXE` overrides, and its defaults are
   unchanged (`spike-env-defaults.txt`). The emulator ran from the product root, and
   `adb` from `.tmp/sdk` was used for probing only. A first boot reached
   `sys.boot_completed=1` in 64.7 s. Ports 5137, 5600, 5601 and 8600 were loopback
   only. Two minutes after boot the screen was awake. The root was unchanged afterwards
   and `platform-tools` was still empty.
7. **Writes outside** (`check7-writes-outside.txt`, `snapshot-diff.txt`), from
   whole-disk snapshots of C: and D: (2.07 million files). There were no changes under
   `%LOCALAPPDATA%\MTPlayer` (still absent), `%LOCALAPPDATA%\Android`,
   `%USERPROFILE%\.android` or `.emulator_console_auth_token`. In the user's temp folder,
   `adb.log` changed, which is allowed. Every other difference belongs to apps running at
   the same time, to cargo's cache, or to the agent harness, apart from the NVIDIA files.
   The Android Studio vmoptions file changed at 02:13:06, after the emulator had
   stopped and alongside Toolbox activity at 02:13:09; that link is inferred from
   timing.
8. **Remove** (`check8-remove.txt`). A lease blocked the remove with exit 1, naming the
   owner. After the lease was removed, `remove <set>` deleted the root and its record, and
   status reported no root installed. `remove --downloads` deleted only MTPlayer's zips.
   Test files in the home, `downloads/` and `sdk/not-a-set` survived. The spike SDK and
   the user's own Android SDK were untouched.
9. **Repo scan** (`check9-repo-scan.txt`). There were 0 staged files and 88 files scanned,
   with no binary names, no file over 1 MiB, no binary magic and no hash match with
   cached downloads. The live console token and `adb` private key values were not in any
   file. The one pattern hit is a false positive: `spike/test-grpc-auth.ps1` parses the
   `grpc.token=` line and prints it as `[redacted]`.
10. **Processes** (`check10-processes-ports.txt`). No process runs from the repo or
    `.tmp`, and the four ports have no connections. I rechecked both after the final
    fixes. Three `crashpad_handler` processes (PIDs 8660, 9956, 12476) had unreadable
    paths and start times. None was in this run's recorded emulator tree, and they were
    not touched.

## The NVIDIA caveat

Three NVIDIA files were updated in place during the boot, and no new files appeared in
those locations.

- `C:\ProgramData\NVIDIA Corporation\Drs\nvAppTimestamps`, at 02:03:27.967, about 1 s
  after the emulator started.
- `%LOCALAPPDATA%\NVIDIA\GLCache\...\f452c563f8c73bef.bin` and `.toc`, at 02:04:33
  during boot.

They aren't on criterion 8's list of known stock files. The acceptance stage counted the
check as passed and raised it for the user. The final review then judged that it needs no
user decision: the files pre-existed and belong to the driver, which refreshes them for
any OpenGL program, and no uninstaller should touch them. The link to the emulator rests
on timing only. Criterion 8 now says this in one sentence. A future snapshot showing new
per-application files there would reopen the question, and they would then be listed in
the README, still without deleting them.

## Final review fixes

| Fix | File | Behavior now |
| --- | --- | --- |
| A kept zip that fails its check | `crates/mtp-sdk/src/download.rs` | It is discarded with a warning and downloaded again, so the user learns the kept file was bad. |
| Removing a folder without a record | `crates/mtp-sdk/src/error.rs`, `install.rs` | The new `NotOurs` error names the path, in place of a misleading `invalid name` message. The exit code stays 1. |
| Proxy variables and `cargo test` | `crates/mtp-sdk/src/source.rs` | A loopback client uses no proxy, so `cargo test` passes with `HTTPS_PROXY` set. |

I differed from the review on one detail. It asked for the warning assertion in
`a_part_with_wrong_bytes_is_not_taken_for_a_kept_zip`, but that test only calls `plan`
and writes a part, so it never reaches the warning. I added
`a_kept_file_that_fails_its_hash_is_replaced_and_the_user_is_told` to
`crates/mtp-sdk/tests/download.rs` instead.

Results after the fixes, saved in `final-review-*.txt`:

- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  are clean.
- `cargo test --workspace` passes 232 tests (2+18+29+35+32+82+9+17+8), up from 231. It
  passes the same with `HTTPS_PROXY` pointed at a closed port, and `.tmp/test` is left
  empty.
- All 4 live tests pass against the real endpoints, including that every pin matches
  Google's XML and that the license hash is still the reviewed one.
- The product still honours the proxy: with `HTTPS_PROXY` at a closed port,
  `mtp sdk license android-sdk-license` failed with a network error, and without the
  variable it reached `dl.google.com`.

## Design decisions

I took these from the code and from the final review, which checked each against the
requirements. They refine the phase text where noted.

1. **One root per set.** The set id names both zip hashes, so any new pin lands in a new
   root, and a ready root is never changed. Instances lease a root while they exist.
2. **No `adb.exe`.** A root holds an empty `platform-tools`, because the emulator refuses
   a root without one, and `adb.exe` would write a key to the profile folder.
3. **No way to skip the prompt.** There is no `--yes`, environment variable or
   configuration key. A script has to name the exact text hash and say on whose behalf,
   with no default. `mtp sdk license <id> --text` is the only way to get a hash.
4. **A narrow offline exception.** An existing acceptance stands only when no XML answered
   at all and every failure was a network error. A server that answers with an error is
   not an offline machine.
5. **Kept zips still go through the gate.** They read the live text and the stable rule
   when reachable. A pin the XML no longer lists doesn't stop a kept zip, but one listed
   under another channel or license does.
6. **A fixed host list.** Redirects are followed by hand and each hop is checked against
   the same list. No setting widens it; the loopback policy exists for tests.
7. **Compare by size and CRC-32.** Every file is checked against its zip entry after
   extraction, and extra files fail the compare.
8. **The GPL is acknowledged, not accepted.** Its text is embedded from
   `packages/licenses/gpl-3.0.txt`, with a hash the manifest binds. The employer question
   belongs to Google's license alone.
9. **A home folder override.** `--home` and `MTPLAYER_HOME` exist so that tests and
   development stay under `.tmp` on D:. The phase text now says `<home>`.
10. **Records outlive a set.** License records and the current Magisk binary stay after
    `mtp sdk remove`. Uninstall has to delete them.
11. **Final review choices.** The proxy fix lives in the client rather than in tests that
    clear variables, the NVIDIA paths stay out of remove and uninstall logic, no license
    record is hand-written to satisfy the prompt check, and the 1.3 GB acceptance isn't
    repeated for the fixes.

## Rust dependency licenses

`cargo metadata` shows 85 third-party crates compiled into the binaries, and nothing is
incompatible with Apache-2.0. None is copyleft, and no crate lacks a license field. The
summary is in [`LICENSES.md`](../../LICENSES.md#rust-dependencies). A binary release has
to carry the notices that MIT, BSD, ISC and similar licenses require, which is Phase 7's
packaging work.

## Remaining concerns

- The Verification item on the machine says "no Android SDK", and this machine does have
  `%LOCALAPPDATA%\Android\Sdk`. It showed no change, and no `ANDROID_*` variable is set,
  but a truly clean machine such as a fresh `tiennm5-pc` profile hasn't been tried.
- No release exists yet, so the "release artifacts" half of the binary check is for Phase 7.
- Check 7's snapshots don't cover the registry, `C:\Windows` or Program Files, and the
  baseline was taken after `cargo install` had built the binary.
- `mtp sdk status` still prints `magisk v31.1 sha256 ok` after `remove <set>`, and no CLI
  command removes the current Magisk binary or the license records.
- Git warns that it will convert LF to CRLF on the modified tracked files. The final review
  suggests committing as it is, or adding `* text=auto eol=lf` at the repo root. I did
  neither.
- Left in place, all git-ignored: `.tmp/mtp-sdk` (license records, the Magisk binary and an
  empty `sdk` folder) and the two 277 MB `snapshot-*.tsv` files in `.tmp/evidence/phase2`,
  which the review says can go. Nothing asked this stage to delete them.

No process from this work is left running. Nothing was committed or pushed.

## Unresolved questions

1. Will you run the one-minute terminal check? The phase can't move to done without it.
2. Does the criterion 8 sentence about GPU driver caches read as you want? The final review
   recommended it and I applied it, so tell me if you would rather word it differently.
3. Should `.claude/` go into the commit? It is untracked and unstaged. It holds agent memory
   files with no secrets, according to the final review.
4. Do you want `* text=auto eol=lf` at the repo root to stop the line-ending churn?
