//! A small HTTP/1.1 fixture server on loopback, with `Range`, redirects, early closes,
//! wrong lengths and a request counter per path.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Route {
    /// Serves the bytes, honoring `Range`.
    Body(Arc<Vec<u8>>),
    /// Serves the bytes, but closes the connection after this many on the first requests
    /// (one element per request, consumed in order).
    CloseAfter(Arc<Vec<u8>>, Arc<Mutex<Vec<usize>>>),
    /// Serves the bytes with `200` even when asked for a range.
    IgnoreRange(Arc<Vec<u8>>),
    /// Answers a range with a `Content-Range` total other than the body's length.
    WrongContentRange(Arc<Vec<u8>>),
    /// Declares this `Content-Length` on a 200, whatever the body is.
    WrongLength(Arc<Vec<u8>>, u64),
    Redirect(String),
    Status(u16),
    /// Answers the first requests with these statuses (one per request, consumed in order),
    /// and then serves the bytes.
    StatusThen(Arc<Mutex<Vec<u16>>>, Arc<Vec<u8>>),
}

#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub range: Option<String>,
}

#[derive(Default)]
struct State {
    routes: HashMap<String, Route>,
    seen: Vec<Seen>,
}

pub struct FixtureServer {
    port: u16,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let state = state.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let state = state.clone();
                            std::thread::spawn(move || handle(stream, &state));
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        Self {
            port,
            state,
            stop,
            thread: Some(thread),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn route(&self, path: &str, route: Route) {
        self.state
            .lock()
            .unwrap()
            .routes
            .insert(path.to_string(), route);
    }

    pub fn body(&self, path: &str, bytes: Vec<u8>) {
        self.route(path, Route::Body(Arc::new(bytes)));
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.state.lock().unwrap().seen.clone()
    }

    pub fn request_count(&self) -> usize {
        self.state.lock().unwrap().seen.len()
    }

    pub fn clear_requests(&self) {
        self.state.lock().unwrap().seen.clear();
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn handle(mut stream: TcpStream, state: &Mutex<State>) {
    stream.set_nonblocking(false).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let path = first.split(' ').nth(1).unwrap_or("/").to_string();
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("range")
        {
            range = Some(value.trim().to_string());
        }
    }
    let route = {
        let mut state = state.lock().unwrap();
        state.seen.push(Seen {
            path: path.clone(),
            range: range.clone(),
        });
        state.routes.get(&path).cloned()
    };
    let _ = match route {
        None => respond(&mut stream, "404 Not Found", &[], b""),
        Some(Route::Status(code)) => respond(&mut stream, &format!("{code} Fixture"), &[], b""),
        Some(Route::StatusThen(statuses, bytes)) => {
            let status = {
                let mut statuses = statuses.lock().unwrap();
                (!statuses.is_empty()).then(|| statuses.remove(0))
            };
            match status {
                Some(code) => respond(&mut stream, &format!("{code} Fixture"), &[], b""),
                None => serve_range(&mut stream, &bytes, range.as_deref(), bytes.len(), None),
            }
        }
        Some(Route::Redirect(to)) => {
            respond(&mut stream, "302 Found", &[("Location".into(), to)], b"")
        }
        Some(Route::Body(bytes)) => {
            serve_range(&mut stream, &bytes, range.as_deref(), bytes.len(), None)
        }
        Some(Route::IgnoreRange(bytes)) => respond(&mut stream, "200 OK", &[], &bytes),
        Some(Route::WrongContentRange(bytes)) => {
            serve_range(&mut stream, &bytes, range.as_deref(), bytes.len() + 1, None)
        }
        Some(Route::WrongLength(bytes, length)) => write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        )
        .and_then(|()| stream.write_all(&bytes)),
        Some(Route::CloseAfter(bytes, closes)) => {
            let close = {
                let mut closes = closes.lock().unwrap();
                if closes.is_empty() {
                    None
                } else {
                    Some(closes.remove(0))
                }
            };
            serve_range(&mut stream, &bytes, range.as_deref(), bytes.len(), close)
        }
    };
    let _ = stream.flush();
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    )?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body)
}

fn serve_range(
    stream: &mut TcpStream,
    bytes: &[u8],
    range: Option<&str>,
    reported_total: usize,
    close_after: Option<usize>,
) -> std::io::Result<()> {
    let start = range
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.strip_suffix('-'))
        .and_then(|r| r.parse::<usize>().ok());
    let (status, body, extra) = match start {
        Some(start) if start >= bytes.len() => {
            return respond(stream, "416 Range Not Satisfiable", &[], b"");
        }
        Some(start) => (
            "206 Partial Content",
            &bytes[start..],
            format!(
                "Content-Range: bytes {start}-{}/{reported_total}\r\n",
                bytes.len() - 1
            ),
        ),
        None => ("200 OK", bytes, String::new()),
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    )?;
    let sent = close_after.unwrap_or(body.len()).min(body.len());
    stream.write_all(&body[..sent])
}
