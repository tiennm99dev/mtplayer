---
title: "Phase 3: mtp-adb — Native ADB Device Protocol"
status: todo
phase: 3
priority: P1
effort: "1.5-2 wk"
dependencies: [2]
---

# Phase 3: mtp-adb — Native ADB Device Protocol

## Overview

A Rust client speaking the ADB **device** protocol directly to the guest's `adbd` over TCP. Everything MTPlayer does to a guest — install, input, logcat, file transfer, scrcpy setup — rides this crate. It is the widest-fan-out crate in the project.

**This phase was rewritten after the red team.** The original design drove the ADB *server* on `127.0.0.1:5037` — but that server **is `adb.exe`**, an Android SDK artifact that the plan bundles nowhere and whose redistribution terms `plan.md` already rejects for SDK system images. On the clean no-SDK Windows host that phases 15 and 16 mandate testing on, nothing would have worked. Implementing the device protocol removes the dependency entirely (G6).

## Requirements

- [ ] Functional: `A_CNXN` handshake, `A_AUTH` authentication, `A_OPEN`/`A_WRTE`/`A_OKAY`/`A_CLSE` stream multiplexing
- [ ] Functional: `shell` (one-shot and streaming), `push`, `pull`, `install`, and raw stream open
- [ ] Functional: open a guest **`localabstract:`** socket — scrcpy's forward tunnel needs exactly this
- [ ] Functional: per-install RSA keypair; authenticate to the guest (S3)
- [ ] Functional: **no dependency on `adb.exe` or any ADB server** at any point
- [ ] Non-functional: no `unsafe`; all I/O async on one runtime; every op cancellable
- [ ] Non-functional: protocol tests against a fake `adbd`, runnable in CI with no device

## Architecture

The device protocol is a 24-byte message header plus payload, multiplexing many logical streams over one TCP connection.

```
header: command(4) arg0(4) arg1(4) length(4) crc32(4) magic(4)   [little-endian]
        magic == !command

AdbDevice::connect("127.0.0.1:<port>")
  ├─ A_CNXN  "host::mtplayer"           -> guest replies A_CNXN or A_AUTH
  ├─ A_AUTH  TOKEN -> SIGNATURE -> RSAPUBLICKEY   (S3)
  └─ streams, each a (local_id, remote_id) pair
        ├─ open("shell,v2,raw:<cmd>")   -> Stream
        ├─ open("sync:")                -> Stream (SEND/RECV/STAT/DONE)
        ├─ open("exec:cmd package install ...")
        └─ open("localabstract:scrcpy_<scid>")   <- phase 04's forward tunnel
```

**Stream flow control matters.** `A_WRTE` must be acknowledged with `A_OKAY` before the peer sends more on that stream. Ignoring this deadlocks under load — the classic failure when logcat and a `push` run concurrently. One reader task owns the socket, demultiplexes by `remote_id`, and dispatches to per-stream channels; writers acquire a per-stream send permit.

**`A_AUTH` and invariant S3.** The guest sends a token; the client signs it with its private key and replies, or offers its public key for the user to accept. AOSP `userdebug` images default `ro.adb.secure=0` and accept anything, so authentication must be *driven from the guest side* — phase 01 and 07 determine whether `ro.adb.secure=1` plus a pre-installed MTPlayer public key is achievable. This crate implements the client half either way; a guest that does not demand auth simply never sends `A_AUTH`.

Key storage: `%APPDATA%/MTPlayer/adbkey` (private) + `adbkey.pub`, generated on first run, file permissions restricted. This is a credential — never log it, never include it in the phase-16 diagnostics report (S8).

**Why this is also a security win.** With no ADB server there is no `adb reverse`, so phase 04 uses a **forward** tunnel: MTPlayer connects *out* to the guest's abstract socket. The red team found that scrcpy's default reverse tunnel makes the *host* listen, letting any local process — or a guest app, since the guest runs untrusted APKs — race to accept MTPlayer's control socket and receive everything typed into the device. **Forward tunneling eliminates that attack surface rather than mitigating it.**

**What this costs.** `mtp-adb` now owns framing, CRC, auth, stream flow control, and the sync subprotocol. Roughly double the original estimate. Accepted: it buys G6, S3, and the removal of D8.

## Related Code Files

- Create: `crates/mtp-adb/src/lib.rs` — public API
- Create: `crates/mtp-adb/src/wire.rs` — 24-byte header encode/decode, CRC32, magic check
- Create: `crates/mtp-adb/src/device.rs` — `AdbDevice`: connect, handshake, stream table, reader loop
- Create: `crates/mtp-adb/src/auth.rs` — RSA keypair generation/load, token signing, `RSAPUBLICKEY` encoding
- Create: `crates/mtp-adb/src/stream.rs` — `Stream` with `A_OKAY` flow control, cancel-safe drop
- Create: `crates/mtp-adb/src/sync.rs` — `SEND`/`RECV`/`STAT`/`DONE`, progress, **host-side path validation (S5)**
- Create: `crates/mtp-adb/src/install.rs` — streaming install + `install-multiple` session flow (phase 08 uses it)
- Create: `crates/mtp-adb/src/error.rs` — typed errors, guest reason strings preserved
- Create: `crates/mtp-adb/tests/wire.rs`, `tests/streams.rs`, `tests/sync.rs` — fake-`adbd` tests
- Modify: `crates/mtp-core/src/launch.rs` — hand the allocated loopback port to `AdbDevice::connect`

## Implementation Steps

1. **Wire format first.** `wire.rs`: little-endian header, `magic == !command`, CRC32 over the payload. Reject a header whose magic does not match rather than trying to resync — a desynced stream is unrecoverable and silent corruption is worse than a hard error.
2. **`A_CNXN` handshake.** Send `host::mtplayer` with the protocol version and max payload. Parse the guest's banner (`device::ro.product.name=...`) — it carries features, which replaces the server's `host:features` query for deciding shell v2 support.
3. **`A_AUTH`.** Generate or load the keypair. On `AUTH_TOKEN`, sign with PKCS#1 v1.5 / SHA-1 (what adbd expects) and reply `AUTH_SIGNATURE`; if rejected, send `AUTH_RSAPUBLICKEY`. Surface "guest has not accepted this key" as a distinct, actionable error — phase 16 needs it as a `Remedy`.
4. **Stream table and reader loop.** One task owns the socket. Maintain `HashMap<local_id, StreamHandle>`. Dispatch `A_OKAY`/`A_WRTE`/`A_CLSE` by id. Allocate `local_id` monotonically, never reuse an id while a close is in flight.
5. **Flow control, properly.** A stream may not send another `A_WRTE` until the peer's `A_OKAY` for the previous one arrives. Implement as a one-permit semaphore per stream. **Test it by running a 100 MB push concurrently with a live logcat stream** — that is the case that deadlocks a naive implementation.
6. **`shell` one-shot and streaming.** `shell,v2,raw:<cmd>` where supported (real exit code, stdout/stderr separated), legacy `shell:<cmd>` otherwise. Streaming returns the `Stream`; dropping it sends `A_CLSE` and stops the guest-side process.
7. **Sync subprotocol.** `sync:` then `SEND`/`RECV` in 64 KiB chunks, `DONE` + mtime, expect `OKAY`. Progress callback. **On `RECV` (guest -> host), validate every guest-supplied path host-side per S5** — reject `..`, path separators, `:`, Windows reserved device names (`CON`, `NUL`, `LPT1`…), trailing dots/spaces, over-long paths; canonicalize and confirm the result is inside the destination root. **Do not apply guest file modes to host files.**
8. **Streaming install.** `exec:cmd package install -S <size>` and the `install-multiple` session flow (create / write parts / commit). Preserve `INSTALL_FAILED_*` verbatim — phase 08 depends on the exact codes.
9. **`open(service)`** for arbitrary destinations, including `localabstract:`. Hand the raw `Stream` to the caller; phase 04 owns what comes next.
10. **Fake-`adbd` tests.** A `tokio` listener implementing enough of the protocol to exercise: successful handshake, `A_AUTH` accept and reject, an `A_WRTE` never acknowledged (must time out, not hang forever), a stream closed mid-transfer, a bad CRC, a bad magic, a 0-byte file, a file exactly 64 KiB, and a `RECV` offering `../../evil.txt` and `NUL` (both must be refused).

## Success Criteria

- [ ] Connects to the phase-01 guest and `shell "getprop ro.build.version.sdk"` returns `33` — **with no `adb.exe` present on the machine** (verify: rename/remove any ADB on `PATH` first)
- [ ] `A_AUTH` completes against a guest demanding authentication; an unaccepted key produces a distinct actionable error
- [ ] Private key stored with restricted permissions and never appears in any log
- [ ] `push` then `pull` a 100 MB file; SHA-256 matches; progress monotonic
- [ ] 0-byte and exactly-64-KiB files round-trip correctly
- [ ] **100 MB push concurrent with a live logcat stream: both complete, no deadlock** (flow control verified)
- [ ] `shell_stream "logcat"` runs 60 s; dropping the handle closes the stream within 1 s and stops the guest process
- [ ] `open("localabstract:...")` returns a usable stream (phase 04's prerequisite)
- [ ] A guest-offered path of `../../evil.txt`, `NUL`, or a 300-char name is **refused**, and nothing is written outside the destination root
- [ ] Guest file modes are not applied to host files
- [ ] `INSTALL_FAILED_*` reasons surface unchanged
- [ ] Bad CRC and bad magic produce hard errors, not attempted resync
- [ ] Protocol tests pass in CI with no device and no ADB installed
- [ ] `cargo clippy -- -D warnings` clean; zero `unsafe`

## Risk Assessment

**Risk: the device protocol is more work than estimated.**
Signal: >2 weeks with flow control still unreliable.
Response: this is the accepted cost of removing `adb.exe`. If it overruns badly, the fallback is bundling platform-tools with a license determination (`plan.md` Open Question, and it contradicts the SDK-redistribution reasoning) — escalate rather than deciding silently, because it reverses a plan-level decision.

**Risk: stream flow control deadlocks only under concurrency.**
Signal: works in isolation, hangs when logcat and a transfer overlap.
Response: the concurrent 100 MB + logcat test is a success criterion for exactly this reason. Never accept "it works" from sequential testing.

**Risk: `A_AUTH` signing details are wrong** (padding scheme, key encoding, token length).
Signal: guest rejects every signature.
Response: adbd expects PKCS#1 v1.5 over the raw 20-byte token, and its own base64 `RSAPUBLICKEY` structure. Read adbd's source as the authority; test against a real guest early, since a fake `adbd` will happily accept a wrong implementation.

**Risk: S5 path validation is incomplete and a traversal slips through.**
Signal: a guest-written file lands outside the destination root.
Response: validate *and* canonicalize, then assert containment — never rely on rejection patterns alone. The three adversarial fixtures are mandatory. This crosses a real trust boundary: the guest runs arbitrary untrusted APKs by design (S1).

**Risk: removing the ADB server breaks phase-12 auto-registration.**
Signal: instances cannot appear in the user's `adb devices`.
Response: known and accepted. Auto-registration becomes **conditional** — if `adb.exe` is detected on the host, MTPlayer may register with it; otherwise the feature is absent and the endpoint is still shown and copyable. Phase 12 must not treat it as guaranteed.
