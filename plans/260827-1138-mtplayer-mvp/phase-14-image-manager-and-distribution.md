---
title: "Phase 14: Image Manager and Distribution"
status: todo
phase: 14
priority: P1
effort: "5-8 d"
dependencies: [7, 12]
---

# Phase 14: Image Manager and Distribution

## Overview

Get the phase-07 image onto a user's machine: hosted on GitHub Releases, **signature-verified**, downloaded on first run with progress, and resumable. Images are not vendored in the installer — a 2-4 GB installer is unacceptable and phase 15 depends on this being solved first.

v1 seeds exactly one image (API 33), but **the manager is written for N images** so v1.1 can add API 37 and 34 by publishing artifacts and updating a manifest, with no code change.

**Dependencies corrected:** `[7, 12]`, not `[13]`. The original chain declared this phase blocked behind the CLI phase that the plan simultaneously designated as the schedule sacrifice — so v1's shipping gate depended on a phase it told you to drop. The only real coupling was the `image` subcommands, an additive leaf now owned by phase 13b.

## Requirements

- [ ] Functional: **manifest signature verified before parsing** (S4)
- [ ] Functional: `id` validated against `^[a-z0-9_.-]+$`; asset URLs against an allowlisted scheme+host (S4)
- [ ] Functional: download with progress, cancel, and resume across app restarts
- [ ] Functional: SHA-256 per asset; **a mismatch rejects — it never installs alongside**
- [ ] Functional: multi-part (`.partNN`) assets reassembled transparently
- [ ] Functional: **atomic install** — staging directory, verify, rename, then record state
- [ ] Functional: startup reconciliation of orphaned image directories
- [ ] Functional: **repair-in-place** for a base whose content matches its recorded SHA (phase 11's remedy ladder)
- [ ] Functional: dependency-checked delete
- [ ] Non-functional: no distributed asset exceeds 2 GiB

## Architecture

### Hosting

**GitHub Releases: 2 GiB per file, up to 1000 assets per release, no total-size cap, no bandwidth limit** — verified. **GitHub Pages is disqualified** (1 GB site limit; not a binary CDN). GHCR is the secondary option, shared with phase 18.

Keep the manifest **URL-based**, not GitHub-API-based, so switching hosts is a manifest change rather than a code change.

### Manifest — the root of trust, and it must be authenticated

```
remote:  https://<host>/<path>/manifest.json  + manifest.json.sig
local:   %APPDATA%/MTPlayer/images/manifest.json
```

```json
{
  "schema": 1,
  "images": [{
    "id": "aosp_x86_64-api33",
    "api_level": 33,
    "android_version": "13",
    "source_build_id": "<pinned aosp build id>",
    "parts": [{ "url": "...zst", "sha256": "...", "size": 1734000000 }],
    "uncompressed_sha256": "...",
    "uncompressed_size": 6800000000
  }]
}
```

**Phase 14 owns this schema.** The original plan ordered it "defined in phase 07" while only specifying it here, with divergent field sets and three candidate file locations and no precedence. Phase 07 now produces a conforming instance; this file is authoritative. `parts` is an array even for a single-part image so multi-part needs no schema change.

**SHA-256 alone is not integrity here.** The hashes live *inside* the artifact being trusted, so a self-consistent malicious manifest — from a compromised release token, a typo-squatted host, or anyone able to serve that URL — passes every hash check. The image *is* the guest kernel and system partition, and the guest gets a synced host folder (phase 10). So:

- **Sign the manifest** (minisign or cosign) with a public key **embedded in the binary at build time**. Verify the signature **before parsing** the JSON.
- **Validate `id` before using it as a path.** It becomes `images/<id>/`, so `"id": "../../../AppData/Roaming/Microsoft/Windows/Start Menu/Programs/Startup"` would write outside the store. Allowlist `^[a-z0-9_.-]+$`.
- **Allowlist asset URL scheme and host.** Otherwise a manifest can redirect fetches to an arbitrary host, or to `file://`/UNC (SMB credential leak).
- **A content mismatch rejects.** The original plan's response was to install the incoming image *alongside* under a suffixed id — quietly adding a tampered image rather than refusing it.
- The last successful remote manifest is cached for offline use, so a poisoned manifest would persist. Cache only **after** signature verification.

### Immutability, with a repair exception

Phase 11 records each overlay's and snapshot's base SHA, so replacing a base in place breaks every chain. Therefore an installed image is version-suffixed; a genuinely *newer* image installs alongside as a distinct id.

**But phase 11's remedy ladder needs one exception:** re-downloading a base to restore it to its **original id**, when the incoming content matches the **recorded** SHA, is a *repair*, not a replacement — and it is what makes a corrupted base non-destructively recoverable. Permit exactly that case; refuse everything else.

Deleting an image is refused while any instance or snapshot references it, listing the dependents.

### Download and install mechanics

Resume needs HTTP range requests plus persisted partial state. Write to `<name>.part`; rename only after the full-file SHA matches. **Verify before decompressing** — a corrupt zstd stream produces an error far from the cause. Detect a `200` where `206` was expected and restart cleanly rather than corrupting the file.

**Install must be atomic.** The original plan applied temp-then-rename to the download but not the decompress, recorded local state as a separate unordered step, and checked disk space only before download. A crash or disk-full mid-decompress left a truncated multi-gigabyte directory with no manifest entry — invisible in the UI, and the suffixing rule guaranteed it would never be reused or reclaimed.

```
download -> <name>.part -> verify SHA -> rename
decompress -> images/.staging/<uuid>/ -> verify uncompressed_sha256
           -> rename directory into images/<id>/ -> THEN write local state
startup: any images/.staging/* is garbage -> delete
```

Re-check free space **immediately before decompression**, not only before download — phase 11 warns that snapshot growth can consume headroom in between.

## Related Code Files

- Create: `crates/mtp-core/src/image/manifest.rs` — schema, **signature verification**, `id`/URL validation, remote fetch, local state
- Create: `crates/mtp-core/src/image/download.rs` — ranged/resumable download, progress, cancel
- Create: `crates/mtp-core/src/image/verify.rs` — streaming SHA-256, part reassembly
- Create: `crates/mtp-core/src/image/install.rs` — staging, atomic rename, repair-in-place, reconciliation
- Create: `crates/mtp-core/tests/image_manager.rs` — manifest parsing, **hostile manifests**, resume, SHA mismatch, dependency refusal
- Create: `crates/mtp-ui/src/image_panel.rs` — available/installed lists, progress, delete
- Create: `docs/publishing-images.md` — maintainer procedure incl. **signing the manifest**
- Modify: `crates/mtp-cli/src/commands/` — `image list`/`download`/`rm` (phase 13b; if 13b is cut, these live behind the UI only)
- Modify: `crates/mtp-provider-vm/src/lib.rs` — resolve an image id via the local manifest

## Implementation Steps

1. **Signature first.** Embed the public key at build time. Fetch `manifest.json` + `.sig`, verify, and only then parse. An unverifiable manifest is a hard failure with a specific message — never a silent fallback to the cached copy.
2. **Validate every field that becomes a path or a request.** `id` against the allowlist regex; `url` against allowed scheme+host. Do this at parse time so nothing downstream has to remember.
3. **Local state** records install path, installed-at, verified uncompressed SHA, and the source build id.
4. **Graceful offline behavior.** No network -> "cannot check for images" plus the installed list. Never a blank panel or a crash.
5. **Ranged download** with persisted progress. Cancel leaves a resumable `.part`, not a deletion.
6. **Streaming SHA-256** computed while downloading, not in a second pass over gigabytes. Verify each part, then the reassembled whole after decompression.
7. **Reassembly** in order, verifying the joined artifact before decompressing. Fail loudly on a missing part (exit 9 via phase 13).
8. **Atomic install** per the staging flow above, with startup reconciliation of `images/.staging/*`.
9. **Disk-space precheck twice** — before download and immediately before decompress — refusing with specific numbers (exit 8).
10. **Repair-in-place path** for phase 11's ladder: re-download to the original id when content matches the recorded SHA. Verify that existing snapshots restore correctly afterward.
11. **Dependency-checked delete**, listing dependents; deleting dependent instances is a separate, separately-confirmed action.
12. **First-run integration.** With no image installed, offer the seeded API 33 download rather than an empty device list. Show the size *before* starting.
13. **Maintainer docs** including how the manifest is signed and how a second person verifies an install from a clean machine.
14. **Tests** over a local HTTP fixture server: full download, interrupted-and-resumed, SHA mismatch, missing part, ranges-unsupported server, delete-with-dependency, **plus hostile-manifest cases** — bad signature, `id` with `..`, a `file://` asset URL, and an off-allowlist host.

## Success Criteria

- [ ] Clean machine, no image -> download API 33 with visible progress -> instance boots from it
- [ ] **A manifest with a bad or missing signature is refused before parsing**; the cached copy is not silently substituted
- [ ] **`id` containing `..` or a separator is refused**; nothing is written outside `images/`
- [ ] **An asset URL with a disallowed scheme or host is refused** (`file://`, UNC, off-allowlist)
- [ ] A deliberately corrupted asset fails SHA verification and is **rejected — not installed alongside under a suffixed id**
- [ ] Download interrupted at ~50% (kill the app) -> restart -> resumes rather than restarting
- [ ] A ranges-unsupported server causes a clean restart, not a corrupt file
- [ ] Multi-part image reassembles and installs (test with an artificially split asset even though v1's is single-part)
- [ ] **Kill during decompress -> no orphaned directory survives startup reconciliation**; retry installs cleanly to the correct id
- [ ] **Disk fills during decompress -> refused with specific numbers, no partial image left registered**
- [ ] **Repair-in-place restores a corrupted base to its original id, and existing snapshots then restore correctly** (phase 11 ladder step 2)
- [ ] Installing an image whose id exists with *different* content does not overwrite the existing one
- [ ] Deleting a referenced image is refused, listing dependents
- [ ] Offline: installed images usable, remote list shows a clear "cannot check" state
- [ ] No distributed asset exceeds 2 GiB
- [ ] `docs/publishing-images.md` verified by publishing and installing once from a clean machine
- [ ] Tests incl. hostile-manifest cases pass in CI against the fixture server

## Risk Assessment

**Risk: the signing key is unowned, so signature verification ships disabled.**
Signal: `plan.md` Open Question 4 still open at implementation time.
Response: **this blocks the phase.** Escalate rather than shipping an unsigned root of trust. The key can be self-managed (minisign, key committed to the repo, private key held by the maintainer) — it does not need a commercial CA, so there is no cost excuse. Note it is a *different* question from the installer code-signing certificate, though one owner could hold both.

**Risk: an image update silently invalidates every overlay and snapshot.**
Signal: instances failing to boot after a refresh; snapshots refusing to restore.
Response: version-suffixed installs plus phase 11's base-SHA check, with repair-in-place as the non-destructive recovery. The highest-consequence bug available in this phase — it destroys user data.

**Risk: the repair exception becomes a loophole for overwriting a base.**
Signal: a base replaced in place with different content via the repair path.
Response: repair is permitted **only** when the incoming content hashes to the *recorded* SHA. That is a byte-identical restore by definition. Anything else takes a new id.

**Risk: resume corrupts files against a server ignoring range requests.**
Signal: an asset failing SHA only sometimes, or only for some users.
Response: detect `200` vs `206` and restart. The fixture test is mandatory.

**Risk: GitHub Releases becomes impractical** (rate limits, an oversized asset, policy change).
Signal: download failures under load.
Response: GHCR is the pre-identified secondary and shares infrastructure with phase 18. The URL-based manifest keeps the switch a data change.

**Risk: users abandon a multi-gigabyte first-run download.**
Signal: high first-run drop-off.
Response: partly out of scope, but show the size before starting, allow cancel/resume, and never block the rest of the UI. Phase 16 sets expectations before the installer runs.
