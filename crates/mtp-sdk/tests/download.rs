//! The downloader against a loopback server: reuse, resume, restart, hash failure, redirects.

mod support;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mtp_sdk::download::{DownloadRequest, part_path};
use mtp_sdk::hash::{Digest, Sha1Hex, sha1_bytes, sha256_bytes};
use mtp_sdk::{Cancel, Client, Error, Event, SourcePolicy};
use support::fixtures::noise;
use support::http::{FixtureServer, Route};
use support::temp_dir;

const MIB: usize = 1 << 20;

fn client(server: &FixtureServer) -> Client {
    Client::new(SourcePolicy::loopback(server.port())).with_retries(1, Duration::from_millis(10))
}

fn request(server: &FixtureServer, path: &str, dest: &Path, bytes: &[u8]) -> DownloadRequest {
    DownloadRequest {
        url: server.url(path),
        dest: dest.to_path_buf(),
        size: bytes.len() as u64,
        digest: Digest::Sha1(sha1_bytes(bytes)),
    }
}

fn run(
    client: &Client,
    request: &DownloadRequest,
) -> mtp_sdk::Result<mtp_sdk::download::Downloaded> {
    client.download(request, &mut |_| {}, &Cancel::new())
}

#[test]
fn a_fresh_download_is_verified_and_a_second_makes_no_request() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 1);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, &bytes);

    let first = run(&client(&server), &req).unwrap();
    assert!(!first.reused);
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!part_path(&dest).exists());
    assert_eq!(server.request_count(), 1);

    let second = run(&client(&server), &req).unwrap();
    assert!(second.reused);
    assert_eq!(server.request_count(), 1);
}

#[test]
fn an_interrupted_transfer_resumes_with_a_range_request() {
    let server = FixtureServer::start();
    let bytes = noise(4 * MIB, 2);
    let closes = Arc::new(Mutex::new(vec![3 * MIB]));
    server.route("/a.zip", Route::CloseAfter(Arc::new(bytes.clone()), closes));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");

    let mut events = Vec::new();
    let result = client(&server).download(
        &request(&server, "/a.zip", &dest, &bytes),
        &mut |event| events.push(event),
        &Cancel::new(),
    );
    result.unwrap();

    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    let requests = server.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].range, None);
    assert_eq!(requests[1].range.as_deref(), Some("bytes=3145728-"));
    assert!(events.iter().any(|e| matches!(e, Event::Retry { .. })));
    assert!(events.iter().any(
        |e| matches!(e, Event::Download { resume_from: Some(from), .. } if *from == 3 * MIB as u64)
    ));
}

#[test]
fn when_retries_run_out_the_part_stays_and_a_later_run_resumes() {
    let server = FixtureServer::start();
    let bytes = noise(2 * MIB, 3);
    let closes = Arc::new(Mutex::new(vec![MIB]));
    server.route("/a.zip", Route::CloseAfter(Arc::new(bytes.clone()), closes));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, &bytes);
    let no_retries = client(&server).with_retries(0, Duration::ZERO);

    let err = run(&no_retries, &req).unwrap_err();
    assert!(matches!(err, Error::Interrupted { .. }), "{err}");
    assert_eq!(
        std::fs::metadata(part_path(&dest)).unwrap().len(),
        MIB as u64
    );

    run(&no_retries, &req).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(
        server.requests()[1].range.as_deref(),
        Some("bytes=1048576-")
    );
}

#[test]
fn a_cancel_keeps_the_part_for_resuming() {
    let server = FixtureServer::start();
    let bytes = noise(8 * MIB, 4);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, &bytes);
    let cancel = Cancel::new();
    let trigger = cancel.clone();
    let err = client(&server)
        .download(
            &req,
            &mut move |event| {
                if matches!(event, Event::Progress { received, .. } if received >= MIB as u64) {
                    trigger.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    let kept = std::fs::metadata(part_path(&dest)).unwrap().len();
    assert!(kept >= MIB as u64 && kept < bytes.len() as u64, "{kept}");

    run(&client(&server), &req).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(server.requests()[1].range, Some(format!("bytes={kept}-")));
}

#[test]
fn a_server_that_ignores_range_restarts_from_zero() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 5);
    server.route("/a.zip", Route::IgnoreRange(Arc::new(bytes.clone())));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes[..1000]).unwrap();

    run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(server.requests()[0].range.as_deref(), Some("bytes=1000-"));
}

#[test]
fn a_kept_file_that_fails_its_hash_is_replaced_and_the_user_is_told() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 12);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let mut kept = bytes.clone();
    kept[5] ^= 1;
    std::fs::write(&dest, &kept).unwrap();

    let mut events = Vec::new();
    let result = client(&server).download(
        &request(&server, "/a.zip", &dest, &bytes),
        &mut |event| events.push(event),
        &Cancel::new(),
    );

    assert!(!result.unwrap().reused);
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Warning { message } if message.contains("a.zip") && message.contains("sha1")
        )),
        "{events:?}"
    );
}

#[test]
fn a_stale_part_with_wrong_bytes_is_deleted_by_the_hash_check() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 6);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), vec![0u8; 1000]).unwrap();
    let req = request(&server, "/a.zip", &dest, &bytes);

    let err = run(&client(&server), &req).unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert!(!part_path(&dest).exists());
    assert!(!dest.exists());

    run(&client(&server), &req).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
}

#[test]
fn a_tampered_body_is_rejected_and_nothing_is_kept() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 7);
    let mut tampered = bytes.clone();
    tampered[1234] ^= 1;
    server.body("/a.zip", tampered);
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");

    let err = run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap_err();
    assert!(matches!(err, Error::HashMismatch { .. }), "{err}");
    assert!(!dest.exists());
    assert!(!part_path(&dest).exists());
}

#[test]
fn sha256_pins_work_the_same_way() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 8);
    server.body("/m.apk", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("m.apk");
    let mut req = request(&server, "/m.apk", &dest, &bytes);

    req.digest = Digest::Sha256(sha256_bytes(b"something else"));
    assert!(matches!(
        run(&client(&server), &req),
        Err(Error::HashMismatch { .. })
    ));

    req.digest = Digest::Sha256(sha256_bytes(&bytes));
    run(&client(&server), &req).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
}

#[test]
fn a_wrong_size_on_a_200_fails_before_the_transfer() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 9);
    server.body("/a.zip", noise(2000, 9));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let err = run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap_err();
    assert!(
        matches!(err, Error::SizeMismatch { actual: 2000, .. }),
        "{err}"
    );
    assert!(!part_path(&dest).exists());
}

#[test]
fn a_body_shorter_than_its_length_counts_as_an_interruption() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 10);
    server.route(
        "/a.zip",
        Route::WrongLength(Arc::new(bytes[..400].to_vec()), 1000),
    );
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let no_retries = client(&server).with_retries(0, Duration::ZERO);
    let err = run(&no_retries, &request(&server, "/a.zip", &dest, &bytes)).unwrap_err();
    assert!(
        matches!(err, Error::Interrupted { received: 400, .. }),
        "{err}"
    );
}

#[test]
fn a_range_answer_for_another_file_discards_the_part_and_restarts() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 11);
    server.route("/a.zip", Route::WrongContentRange(Arc::new(bytes.clone())));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes[..1000]).unwrap();

    run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap();
    let requests = server.requests();
    assert_eq!(requests[0].range.as_deref(), Some("bytes=1000-"));
    assert_eq!(requests[1].range, None);
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
}

#[test]
fn a_range_past_the_end_restarts() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 12);
    server.body("/a.zip", bytes[..500].to_vec());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes[..600]).unwrap();
    let no_retries = client(&server).with_retries(0, Duration::ZERO);
    let err = run(&no_retries, &request(&server, "/a.zip", &dest, &bytes)).unwrap_err();
    assert!(
        matches!(err, Error::Interrupted { received: 0, .. }),
        "{err}"
    );
    assert!(!part_path(&dest).exists());
}

#[test]
fn a_complete_part_is_hashed_without_any_request() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 13);
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes).unwrap();
    run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap();
    assert_eq!(server.request_count(), 0);
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
}

#[test]
fn an_oversized_part_starts_over() {
    let server = FixtureServer::start();
    let bytes = noise(1000, 14);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), vec![0u8; 2000]).unwrap();
    run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert_eq!(server.requests()[0].range, None);
}

#[test]
fn a_redirect_to_an_allowed_url_is_followed_and_range_is_resent() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 15);
    server.route("/start", Route::Redirect("/middle".to_string()));
    server.route("/middle", Route::Redirect(server.url("/final.zip")));
    server.body("/final.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes[..500]).unwrap();

    run(&client(&server), &request(&server, "/start", &dest, &bytes)).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .iter()
            .all(|r| r.range.as_deref() == Some("bytes=500-"))
    );
}

#[test]
fn a_redirect_to_another_host_is_refused_before_any_connection() {
    let server = FixtureServer::start();
    server.route(
        "/start",
        Route::Redirect("http://evil.example/x.zip".to_string()),
    );
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let err = run(&client(&server), &request(&server, "/start", &dest, b"abc")).unwrap_err();
    assert!(
        matches!(&err, Error::RedirectNotAllowed { host } if host == "evil.example"),
        "{err}"
    );
    assert_eq!(server.request_count(), 1);
}

#[test]
fn a_redirect_to_another_port_or_scheme_is_refused() {
    let server = FixtureServer::start();
    server.route("/port", Route::Redirect("http://127.0.0.1:1/x".to_string()));
    server.route(
        "/scheme",
        Route::Redirect(format!("https://127.0.0.1:{}/x", server.port())),
    );
    let dir = temp_dir();
    for path in ["/port", "/scheme"] {
        let dest = dir.path().join("a.zip");
        let err = run(&client(&server), &request(&server, path, &dest, b"abc")).unwrap_err();
        assert!(matches!(err, Error::InsecureUrl { .. }), "{path}: {err}");
    }
}

#[test]
fn a_redirect_loop_stops() {
    let server = FixtureServer::start();
    server.route("/loop", Route::Redirect("/loop".to_string()));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let err = run(&client(&server), &request(&server, "/loop", &dest, b"abc")).unwrap_err();
    assert!(matches!(err, Error::TooManyRedirects { .. }), "{err}");
    assert_eq!(server.request_count(), 6);
}

#[test]
fn the_production_policy_refuses_plain_http_with_no_socket() {
    let dir = temp_dir();
    let req = DownloadRequest {
        url: "http://dl.google.com/android/repository/x.zip".to_string(),
        dest: dir.path().join("x.zip"),
        size: 3,
        digest: Digest::Sha1(Sha1Hex::parse(&"a".repeat(40)).unwrap()),
    };
    let err = run(&Client::new(SourcePolicy::production()), &req).unwrap_err();
    assert!(matches!(err, Error::InsecureUrl { .. }), "{err}");
    assert!(!part_path(&req.dest).exists());
}

#[test]
fn http_errors_name_host_and_path_only() {
    let server = FixtureServer::start();
    server.route("/gone.zip", Route::Status(404));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let mut req = request(&server, "/gone.zip", &dest, b"abc");
    req.url.push_str("?sig=SECRET");
    let err = run(&client(&server), &req).unwrap_err();
    assert!(matches!(err, Error::Http { status: 404, .. }), "{err}");
    assert!(!err.to_string().contains("SECRET"), "{err}");
}

fn status_then(statuses: &[u16], bytes: &[u8]) -> Route {
    Route::StatusThen(
        Arc::new(Mutex::new(statuses.to_vec())),
        Arc::new(bytes.to_vec()),
    )
}

#[test]
fn a_status_that_can_pass_is_retried_and_then_succeeds() {
    for status in [408, 429, 500, 503] {
        let server = FixtureServer::start();
        let bytes = noise(4096, 8);
        server.route("/a.zip", status_then(&[status], &bytes));
        let dir = temp_dir();
        let dest = dir.path().join("a.zip");
        let req = request(&server, "/a.zip", &dest, &bytes);
        let mut events = Vec::new();
        client(&server)
            .download(&req, &mut |event| events.push(event), &Cancel::new())
            .unwrap();
        assert_eq!(server.request_count(), 2, "{status}");
        assert!(
            events.iter().any(|e| matches!(e, Event::Retry { .. })),
            "{status}: {events:?}"
        );
        assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    }
}

#[test]
fn a_status_that_stays_fails_when_the_retries_run_out() {
    let server = FixtureServer::start();
    server.route("/a.zip", Route::Status(503));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, b"abc");
    let err = run(&client(&server), &req).unwrap_err();
    assert!(matches!(err, Error::Http { status: 503, .. }), "{err}");
    assert_eq!(server.request_count(), 2);
}

#[test]
fn a_status_that_can_not_pass_is_not_retried() {
    let server = FixtureServer::start();
    server.route("/a.zip", Route::Status(403));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, b"abc");
    let err = run(&client(&server), &req).unwrap_err();
    assert!(matches!(err, Error::Http { status: 403, .. }), "{err}");
    assert_eq!(server.request_count(), 1);
}

#[test]
fn a_link_that_keeps_dropping_still_finishes_because_progress_resets_the_budget() {
    let server = FixtureServer::start();
    let bytes = noise(5 * MIB, 9);
    let closes = Arc::new(Mutex::new(vec![MIB; 4]));
    server.route("/a.zip", Route::CloseAfter(Arc::new(bytes.clone()), closes));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, &bytes);
    let client = client(&server).with_retries(2, Duration::from_millis(1));
    run(&client, &req).unwrap();
    assert_eq!(server.request_count(), 5);
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
}

#[test]
fn drops_with_no_progress_use_up_the_budget() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 10);
    let closes = Arc::new(Mutex::new(vec![0; 10]));
    server.route("/a.zip", Route::CloseAfter(Arc::new(bytes.clone()), closes));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let req = request(&server, "/a.zip", &dest, &bytes);
    let client = client(&server).with_retries(2, Duration::from_millis(1));
    let err = run(&client, &req).unwrap_err();
    assert!(matches!(err, Error::Interrupted { .. }), "{err}");
    assert_eq!(server.request_count(), 3);
}

#[test]
fn a_text_fetch_retries_what_can_pass_and_nothing_else() {
    let server = FixtureServer::start();
    server.route("/a.xml", status_then(&[503], b"<xml/>"));
    server.route("/b.xml", Route::Status(404));
    let client = client(&server);
    assert_eq!(
        client.get_text(&server.url("/a.xml"), 100).unwrap(),
        "<xml/>"
    );
    assert_eq!(server.request_count(), 2);
    server.clear_requests();
    assert!(matches!(
        client.get_text(&server.url("/b.xml"), 100),
        Err(Error::Http { status: 404, .. })
    ));
    assert_eq!(server.request_count(), 1);
}

#[test]
fn text_fetches_have_a_size_limit() {
    let server = FixtureServer::start();
    server.body("/big.xml", vec![b'a'; 2000]);
    let client = client(&server);
    assert_eq!(
        client
            .get_text(&server.url("/big.xml"), 2000)
            .unwrap()
            .len(),
        2000
    );
    assert!(matches!(
        client.get_text(&server.url("/big.xml"), 1999),
        Err(Error::BadResponse { .. })
    ));
}

#[test]
fn a_complete_part_is_promoted_with_no_request_and_counts_as_reused() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 21);
    server.body("/a.zip", bytes.clone());
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    std::fs::write(part_path(&dest), &bytes).unwrap();

    let mut events = Vec::new();
    let got = client(&server)
        .download(
            &request(&server, "/a.zip", &dest, &bytes),
            &mut |event| events.push(event),
            &Cancel::new(),
        )
        .unwrap();

    assert!(got.reused);
    assert_eq!(server.request_count(), 0);
    assert!(events.iter().any(|e| matches!(e, Event::Verified { .. })));
    assert!(!events.iter().any(|e| matches!(e, Event::Download { .. })));
    assert_eq!(std::fs::read(&dest).unwrap(), bytes);
    assert!(!part_path(&dest).exists());
}

#[test]
fn bytes_received_before_a_retry_still_count_as_a_transfer() {
    let server = FixtureServer::start();
    let bytes = noise(MIB, 22);
    let closes = Arc::new(Mutex::new(vec![MIB / 2]));
    server.route("/a.zip", Route::CloseAfter(Arc::new(bytes.clone()), closes));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let got = run(&client(&server), &request(&server, "/a.zip", &dest, &bytes)).unwrap();
    assert!(!got.reused);
}

#[test]
fn a_cancel_ends_the_wait_between_download_attempts() {
    let server = FixtureServer::start();
    server.route("/a.zip", Route::Status(503));
    let dir = temp_dir();
    let dest = dir.path().join("a.zip");
    let slow =
        Client::new(SourcePolicy::loopback(server.port())).with_retries(3, Duration::from_secs(60));
    let cancel = Cancel::new();
    let flag = cancel.clone();

    let started = std::time::Instant::now();
    let err = slow
        .download(
            &request(&server, "/a.zip", &dest, b"bytes"),
            &mut |event| {
                if matches!(event, Event::Retry { .. }) {
                    flag.cancel();
                }
            },
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_cancel_stops_a_text_fetch_before_any_request() {
    let server = FixtureServer::start();
    server.body("/a.xml", b"text".to_vec());
    let cancel = Cancel::new();
    let url = server.url("/a.xml");
    let got = client(&server)
        .get_text_cancellable(&url, 100, &cancel)
        .unwrap();
    assert_eq!(got, "text");
    server.clear_requests();

    cancel.cancel();
    let err = client(&server)
        .get_text_cancellable(&url, 100, &cancel)
        .unwrap_err();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert_eq!(server.request_count(), 0);
}
