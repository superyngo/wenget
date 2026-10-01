//! Minimal HTTP/1.1 server for downloader tests
//!
//! Serves one body with an `ETag`, honours `Range: bytes=a-[b]` (+ `If-Range`) when enabled,
//! and pops scripted [`Fault`]s per request to simulate drops, errors and file changes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// Lock ignoring poisoning (a panicking handler thread must not cascade)
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Scripted misbehaviour applied to the next request
pub enum Fault {
    /// Send headers for the full response, then close after this many body bytes
    Drop(usize),
    /// Reply with this status and an empty body
    Status(u16),
    /// Replace the body and ETag, then serve normally
    Swap(Vec<u8>, String),
}

pub struct State {
    pub data: Mutex<Vec<u8>>,
    pub etag: Mutex<String>,
    pub ranges: AtomicBool,
    pub faults: Mutex<VecDeque<Fault>>,
    /// `Range` header of every request, in arrival order
    pub log: Mutex<Vec<Option<String>>>,
}

pub struct Server {
    pub url: String,
    pub state: Arc<State>,
}

impl Server {
    pub fn start(data: Vec<u8>, ranges: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file.bin", listener.local_addr().unwrap());
        let state = Arc::new(State {
            data: Mutex::new(data),
            etag: Mutex::new("\"v1\"".into()),
            ranges: AtomicBool::new(ranges),
            faults: Mutex::new(VecDeque::new()),
            log: Mutex::new(Vec::new()),
        });
        let shared = Arc::clone(&state);
        std::thread::spawn(move || {
            for conn in listener.incoming().flatten() {
                let st = Arc::clone(&shared);
                std::thread::spawn(move || handle(conn, &st));
            }
        });
        Self { url, state }
    }

    pub fn fault(&self, f: Fault) {
        lock(&self.state.faults).push_back(f);
    }

    pub fn ranges_seen(&self) -> Vec<Option<String>> {
        lock(&self.state.log).clone()
    }
}

/// Deterministic, non-repeating-ish test payload
pub fn payload(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

fn handle(mut conn: TcpStream, st: &State) {
    let mut reader = BufReader::new(conn.try_clone().unwrap());
    let (mut range, mut if_range) = (None, None);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            match k.to_ascii_lowercase().as_str() {
                "range" => range = Some(v.trim().to_string()),
                "if-range" => if_range = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    lock(&st.log).push(range.clone());
    let fault = lock(&st.faults).pop_front();
    let mut drop_after = None;
    match fault {
        Some(Fault::Status(code)) => {
            let _ = write!(
                conn,
                "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            return;
        }
        Some(Fault::Swap(d, e)) => {
            *lock(&st.data) = d;
            *lock(&st.etag) = e;
        }
        Some(Fault::Drop(n)) => drop_after = Some(n),
        None => {}
    }
    let data = lock(&st.data).clone();
    let etag = lock(&st.etag).clone();
    let total = data.len();
    let use_range =
        st.ranges.load(Ordering::SeqCst) && if_range.as_ref().is_none_or(|v| *v == etag);
    let span = range
        .filter(|_| use_range)
        .and_then(|r| parse_range(&r, total));
    let (status, body, extra) = match span {
        Some((a, b)) => (
            "206 Partial Content",
            &data[a..=b],
            format!("Content-Range: bytes {a}-{b}/{total}\r\n"),
        ),
        None => ("200 OK", &data[..], String::new()),
    };
    let _ = write!(
        conn,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nETag: {etag}\r\nAccept-Ranges: bytes\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let body = drop_after.map_or(body, |n| &body[..n.min(body.len())]);
    let _ = conn.write_all(body);
    let _ = conn.flush();
}

fn parse_range(r: &str, total: usize) -> Option<(usize, usize)> {
    let (a, b) = r.strip_prefix("bytes=")?.split_once('-')?;
    let a: usize = a.parse().ok()?;
    let b = if b.is_empty() {
        total - 1
    } else {
        b.parse::<usize>().ok()?.min(total - 1)
    };
    (a <= b).then_some((a, b))
}
