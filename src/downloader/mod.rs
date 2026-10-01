//! Downloader module for wenget
//!
//! [`download_file`] streams into `<dest>.part` and renames it to `dest` only after the byte
//! count matches the advertised total. Transport errors, stalls (the shared client's 30 s
//! per-read timeout), 5xx and 429 are retried with `Range: bytes=N-` plus `If-Range`, so a
//! dropped connection resumes instead of restarting. Every retry re-requests the original URL
//! because GitHub redirects to short-lived signed asset URLs.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::StatusCode;

/// Consecutive attempts without new bytes before giving up
const MAX_STALLED_ATTEMPTS: u32 = 3;

/// Base retry delay, doubled per consecutive stalled attempt (1 s, 2 s, 4 s)
const BACKOFF_BASE_MS: u64 = if cfg!(test) { 5 } else { 1000 };

/// Removes a downloaded file or scratch directory when dropped
///
/// Bind it right after choosing the download path so every early return
/// (failed checksum, extraction error, ...) still cleans up.
pub struct CleanupGuard(PathBuf);

impl CleanupGuard {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let result = if self.0.is_dir() {
            std::fs::remove_dir_all(&self.0)
        } else if self.0.exists() {
            std::fs::remove_file(&self.0)
        } else {
            return;
        };
        if let Err(e) = result {
            log::warn!("Failed to clean up {}: {}", self.0.display(), e);
        }
    }
}

/// Warn when `url` is plaintext `http://`
///
/// The content can be altered in transit, and unless the release publishes a
/// checksum nothing downstream would notice.
pub fn warn_if_plaintext(url: &str) {
    if is_plaintext_http(url) {
        eprintln!(
            "  {} downloading over plain http (not encrypted, can be tampered with): {}",
            "Warning:".yellow(),
            url
        );
    }
}

fn is_plaintext_http(url: &str) -> bool {
    url.get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
}

/// Path of the in-progress download next to `dest`
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Outcome of one failed request attempt
enum AttemptError {
    /// Worth retrying (network error, stall, 5xx, 429, server restarted the body)
    Retry(anyhow::Error),
    /// Retrying cannot help (4xx, local I/O error, malformed response)
    Fatal(anyhow::Error),
}

/// Single-stream download state that survives across attempts
struct Stream<'a> {
    url: &'a str,
    file: File,
    /// Bytes already written to the `.part` file
    have: u64,
    /// Total size once a response told us
    total: Option<u64>,
    /// `ETag` (strong) or `Last-Modified` of the body in `.part`, sent as `If-Range`
    validator: Option<String>,
    pb: ProgressBar,
}

impl Stream<'_> {
    fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.have = 0;
        self.pb.set_position(0);
        Ok(())
    }

    fn set_total(&mut self, total: u64) {
        if self.total.is_none() && total > 0 {
            self.pb.set_length(total);
            self.pb.set_draw_target(ProgressDrawTarget::stderr());
        }
        self.total = Some(total);
    }

    /// One request: resume from `have` when possible, then stream the body to `.part`
    fn attempt(&mut self) -> std::result::Result<(), AttemptError> {
        let fatal = |e: anyhow::Error| AttemptError::Fatal(e);
        if self.have > 0 && self.validator.is_none() {
            // Without a validator a resumed body could belong to a different file
            self.truncate().map_err(fatal)?;
        }
        let mut req = crate::utils::http::shared_client().get(self.url);
        if let (true, Some(v)) = (self.have > 0, &self.validator) {
            req = req
                .header(RANGE, format!("bytes={}-", self.have))
                .header(IF_RANGE, v.as_str());
        }
        let resp = req
            .send()
            .map_err(|e| AttemptError::Retry(anyhow!(e).context("request failed")))?;
        let status = resp.status();
        match status {
            StatusCode::PARTIAL_CONTENT if self.have > 0 => {
                let (start, total) = content_range(&resp)
                    .ok_or_else(|| fatal(anyhow!("206 without a valid Content-Range")))?;
                if start != self.have {
                    self.truncate().map_err(fatal)?;
                    return Err(AttemptError::Retry(anyhow!(
                        "server resumed at {start} instead of {}",
                        self.have
                    )));
                }
                if let Some(t) = total {
                    self.set_total(t);
                }
            }
            StatusCode::OK => {
                if self.have > 0 {
                    log::info!("Server ignored the range request; restarting from 0");
                    self.truncate().map_err(fatal)?;
                }
                self.validator = validator(&resp);
                if let Some(t) = resp.content_length() {
                    self.set_total(t);
                }
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                self.truncate().map_err(fatal)?;
                return Err(AttemptError::Retry(anyhow!("HTTP 416; restarting")));
            }
            s if s.is_server_error() || s == StatusCode::TOO_MANY_REQUESTS => {
                return Err(AttemptError::Retry(anyhow!("HTTP {s} for {}", self.url)));
            }
            s => return Err(fatal(anyhow!("HTTP {s} for {}", self.url))),
        }
        self.copy_body(resp)
    }

    fn copy_body(&mut self, mut resp: impl Read) -> std::result::Result<(), AttemptError> {
        let mut buffer = vec![0; 65536];
        loop {
            let n = resp
                .read(&mut buffer)
                .map_err(|e| AttemptError::Retry(anyhow!(e).context("Failed to read response")))?;
            if n == 0 {
                return Ok(());
            }
            self.file
                .write_all(&buffer[..n])
                .context("Failed to write to file")
                .map_err(AttemptError::Fatal)?;
            self.have += n as u64;
            self.pb.set_position(self.have);
        }
    }
}

/// `(start, total)` from `Content-Range: bytes start-end/total`; total is `None` for `*`
fn content_range(resp: &reqwest::blocking::Response) -> Option<(u64, Option<u64>)> {
    let value = resp.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Strong `ETag`, else `Last-Modified`: the only values `If-Range` accepts
fn validator(resp: &reqwest::blocking::Response) -> Option<String> {
    let header = |name| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };
    header(ETAG)
        .filter(|e| !e.starts_with("W/"))
        .or_else(|| header(LAST_MODIFIED))
}

fn new_progress_bar() -> ProgressBar {
    let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::hidden());
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );
    pb
}

/// Download a file from URL to a local path with progress bar, retrying and resuming
///
/// Gives up after [`MAX_STALLED_ATTEMPTS`] consecutive attempts that added no bytes; any
/// progress resets the count. On failure neither `dest` nor `<dest>.part` is left behind.
pub fn download_file(url: &str, dest: &Path) -> Result<()> {
    warn_if_plaintext(url);
    log::info!("Downloading: {}", url);
    log::debug!("Destination: {}", dest.display());

    let part = part_path(dest);
    let guard = CleanupGuard::new(&part);
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&part)
        .with_context(|| format!("Failed to create file: {}", part.display()))?;
    let mut stream = Stream {
        url,
        file,
        have: 0,
        total: None,
        validator: None,
        pb: new_progress_bar(),
    };
    run_with_retries(&mut stream)?;

    if let Some(total) = stream.total.filter(|&t| t != stream.have) {
        anyhow::bail!(
            "Download incomplete: got {} of {} bytes from {}",
            stream.have,
            total,
            url
        );
    }
    stream.file.flush()?;
    drop(stream.file);
    std::fs::rename(&part, dest)
        .with_context(|| format!("Failed to move download to {}", dest.display()))?;
    drop(guard);
    stream.pb.finish_and_clear();
    log::info!("Downloaded {} bytes", stream.have);
    Ok(())
}

fn run_with_retries(stream: &mut Stream) -> Result<()> {
    let mut stalled = 0u32;
    loop {
        let before = stream.have;
        let err = match stream.attempt() {
            Ok(()) => return Ok(()),
            Err(AttemptError::Fatal(e)) => return Err(e),
            Err(AttemptError::Retry(e)) => e,
        };
        stalled = if stream.have > before { 0 } else { stalled + 1 };
        if stalled >= MAX_STALLED_ATTEMPTS {
            return Err(err.context(format!(
                "Download failed after {MAX_STALLED_ATTEMPTS} attempts without progress: {}",
                stream.url
            )));
        }
        log::warn!("Download interrupted: {err:#}");
        std::thread::sleep(Duration::from_millis(
            BACKOFF_BASE_MS << stalled.saturating_sub(1),
        ));
        stream.pb.suspend(|| {
            eprintln!(
                "{} Resuming at {:.1} MB",
                "↻".yellow(),
                stream.have as f64 / 1_048_576.0
            )
        });
    }
}

#[cfg(test)]
mod test_server;

#[cfg(test)]
mod tests {
    #[test]
    fn test_is_plaintext_http() {
        assert!(super::is_plaintext_http("http://example.com/a.tar.gz"));
        assert!(super::is_plaintext_http("HTTP://example.com/a"));
        assert!(!super::is_plaintext_http("https://example.com/a"));
        assert!(!super::is_plaintext_http("./http"));
    }

    #[test]
    fn test_cleanup_guard_removes_file_and_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("a.tar.gz");
        let dir = tmp.path().join("scratch");
        std::fs::write(&file, b"x").unwrap();
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        {
            let _f = super::CleanupGuard::new(&file);
            let _d = super::CleanupGuard::new(&dir);
        }
        assert!(!file.exists());
        assert!(!dir.exists());
    }

    use super::*;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Requires network access
    fn test_download_file() {
        let temp_dir = TempDir::new().unwrap();
        let dest = temp_dir.path().join("test.txt");

        // Download a small file
        let result = download_file("https://httpbin.org/bytes/1024", &dest);
        assert!(result.is_ok());
        assert!(dest.exists());
    }

    use test_server::{payload, Fault, Server};

    fn fetch(srv: &Server) -> (TempDir, PathBuf, Result<()>) {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        let res = download_file(&srv.url, &dest);
        (tmp, dest, res)
    }

    #[test]
    fn test_resumes_after_drop_with_range() {
        let data = payload(300_000);
        let srv = Server::start(data.clone(), true);
        srv.fault(Fault::Drop(100_000));
        let (_t, dest, res) = fetch(&srv);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!part_path(&dest).exists());
        let seen = srv.ranges_seen();
        assert_eq!(seen[0], None);
        assert_eq!(seen[1].as_deref(), Some("bytes=100000-"));
    }

    #[test]
    fn test_restarts_when_range_ignored() {
        let data = payload(200_000);
        let srv = Server::start(data.clone(), false);
        srv.fault(Fault::Drop(50_000));
        let (_t, dest, res) = fetch(&srv);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn test_restarts_when_file_changed() {
        let srv = Server::start(payload(200_000), true);
        srv.fault(Fault::Drop(50_000));
        let new = payload(150_000)
            .iter()
            .map(|b| b ^ 0xff)
            .collect::<Vec<_>>();
        srv.fault(Fault::Swap(new.clone(), "\"v2\"".into()));
        let (_t, dest, res) = fetch(&srv);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), new);
    }

    #[test]
    fn test_retries_server_errors() {
        let data = payload(10_000);
        let srv = Server::start(data.clone(), true);
        srv.fault(Fault::Status(503));
        srv.fault(Fault::Status(429));
        let (_t, dest, res) = fetch(&srv);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn test_404_is_not_retried() {
        let srv = Server::start(payload(10), true);
        srv.fault(Fault::Status(404));
        let (_t, dest, res) = fetch(&srv);
        assert!(res.unwrap_err().to_string().contains("404"));
        assert_eq!(srv.ranges_seen().len(), 1);
        assert!(!dest.exists() && !part_path(&dest).exists());
    }

    #[test]
    fn test_gives_up_without_progress() {
        let srv = Server::start(payload(10_000), true);
        for _ in 0..3 {
            srv.fault(Fault::Drop(0));
        }
        let (_t, dest, res) = fetch(&srv);
        assert!(format!("{:#}", res.unwrap_err()).contains("without progress"));
        assert_eq!(srv.ranges_seen().len(), 3);
        assert!(!dest.exists() && !part_path(&dest).exists());
    }

    #[test]
    fn test_progress_resets_stall_count() {
        let data = payload(100_000);
        let srv = Server::start(data.clone(), true);
        srv.fault(Fault::Drop(0));
        srv.fault(Fault::Drop(0));
        srv.fault(Fault::Drop(10_000));
        srv.fault(Fault::Drop(0));
        srv.fault(Fault::Drop(0));
        let (_t, dest, res) = fetch(&srv);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }
}
