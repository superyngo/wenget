//! Segmented (multi-connection) download of one file
//!
//! A `Range: bytes=0-` probe decides the mode: a 206 with a known total, a validator and a size
//! above the threshold splits the file into fixed-size chunks that `connections` workers pull
//! from a shared counter and write in place with positional writes. Each chunk retries on its
//! own; any full-body (200) answer or a moved range means the server cannot segment (or the file
//! changed), so the caller restarts as one stream.

use std::fs::File;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use anyhow::anyhow;
use indicatif::ProgressBar;
use reqwest::blocking::Response;
use reqwest::header::{IF_RANGE, RANGE};
use reqwest::StatusCode;

use super::resume::State;
use super::{content_range, retry_loop, send, status_error, validator, AttemptError};

/// What a segmented download needs to know about the remote file
pub(super) struct Plan {
    pub total: u64,
    /// Sent as `If-Range` so a changed file answers 200 instead of mixing versions
    pub validator: String,
    pub chunk_size: u64,
}

/// Result of the initial request
pub(super) enum Probe {
    Segmented(Plan),
    /// Use one stream; the response (if any) is the start of the body to consume
    Single(Option<Response>),
}

/// Probe `url` with an open-ended range; never fails (errors fall back to the stream's retries)
pub(super) fn probe(url: &str, connections: u8, min_segmented: u64) -> Probe {
    if connections <= 1 {
        return Probe::Single(None);
    }
    let req = crate::utils::http::shared_client()
        .get(url)
        .header(RANGE, "bytes=0-");
    let Ok(resp) = send(req) else {
        return Probe::Single(None);
    };
    if resp.status() == StatusCode::PARTIAL_CONTENT {
        if let (Some((0, Some(total))), Some(validator)) = (content_range(&resp), validator(&resp))
        {
            if total >= min_segmented {
                return Probe::Segmented(Plan {
                    total,
                    validator,
                    chunk_size: 0,
                });
            }
        }
    }
    Probe::Single(Some(resp))
}

/// Shared state of one segmented download
struct Job<'a> {
    url: &'a str,
    file: &'a File,
    plan: &'a Plan,
    pb: &'a ProgressBar,
    state: &'a State,
    /// Chunk indices still to fetch
    pending: Vec<usize>,
    /// Next position in `pending`
    next: AtomicUsize,
    /// Set by the first worker that fails, so the others stop early
    stop: AtomicBool,
}

/// Fill `file` (resized to `plan.total`) using up to `connections` parallel range requests
///
/// Chunks marked in `done` (from a previous run) are skipped; each finished chunk is
/// recorded in `state`.
pub(super) fn download(
    url: &str,
    file: &File,
    plan: &Plan,
    connections: u8,
    pb: &ProgressBar,
    done: &[bool],
    state: &State,
) -> Result<(), AttemptError> {
    file.set_len(plan.total)
        .map_err(|e| AttemptError::Fatal(anyhow!(e).context("Failed to allocate file")))?;
    let chunks = plan.total.div_ceil(plan.chunk_size) as usize;
    let job = Job {
        url,
        file,
        plan,
        pb,
        state,
        pending: (0..chunks)
            .filter(|&i| !done.get(i).copied().unwrap_or(false))
            .collect(),
        next: AtomicUsize::new(0),
        stop: AtomicBool::new(false),
    };
    let workers = usize::from(connections).min(job.pending.len());
    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers).map(|_| s.spawn(|| job.worker())).collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(AttemptError::Fatal(anyhow!("download worker panicked")))
                })
            })
            .collect()
    });
    let mut fallback = false;
    for result in results {
        match result {
            Ok(()) => {}
            Err(AttemptError::Fallback) => fallback = true,
            Err(e) => return Err(e),
        }
    }
    if fallback {
        Err(AttemptError::Fallback)
    } else {
        Ok(())
    }
}

impl Job<'_> {
    fn worker(&self) -> Result<(), AttemptError> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let Some(&i) = self.pending.get(self.next.fetch_add(1, Ordering::SeqCst)) else {
                return Ok(());
            };
            let start = i as u64 * self.plan.chunk_size;
            let end = (start + self.plan.chunk_size).min(self.plan.total) - 1;
            let mut pos = start;
            let result = retry_loop(self.url, self.pb, start, || {
                let r = self.attempt(&mut pos, end);
                (r, pos)
            });
            if result.is_err() {
                self.stop.store(true, Ordering::SeqCst);
                return result;
            }
            if pos > end {
                self.state.chunk_done(i);
            }
        }
    }

    /// Request `pos..=end` and write it in place, advancing `pos`
    fn attempt(&self, pos: &mut u64, end: u64) -> Result<(), AttemptError> {
        let req = crate::utils::http::shared_client()
            .get(self.url)
            .header(RANGE, format!("bytes={pos}-{end}"))
            .header(IF_RANGE, self.plan.validator.as_str());
        let mut resp = send(req)?;
        match resp.status() {
            StatusCode::PARTIAL_CONTENT => {}
            StatusCode::OK => return Err(AttemptError::Fallback),
            s => return Err(status_error(s, self.url)),
        }
        if content_range(&resp).map(|(start, _)| start) != Some(*pos) {
            return Err(AttemptError::Fallback);
        }
        let mut buffer = vec![0; 65536];
        while *pos <= end {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let n = resp
                .read(&mut buffer)
                .map_err(|e| AttemptError::Retry(anyhow!(e).context("Failed to read response")))?;
            if n == 0 {
                return Err(AttemptError::Retry(anyhow!(
                    "connection closed at byte {pos}"
                )));
            }
            let n = n.min((end + 1 - *pos) as usize);
            write_at(self.file, &buffer[..n], *pos)
                .map_err(|e| AttemptError::Fatal(anyhow!(e).context("Failed to write to file")))?;
            *pos += n as u64;
            self.pb.inc(n as u64);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn write_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_write(buf, offset)? {
            0 => return Err(std::io::ErrorKind::WriteZero.into()),
            n => {
                buf = &buf[n..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}
