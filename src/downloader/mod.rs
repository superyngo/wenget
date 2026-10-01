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
use reqwest::blocking::{RequestBuilder, Response};
use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::StatusCode;

mod resume;
mod segmented;

use resume::{Sidecar, State};

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

/// Download behaviour chosen by the caller (from `config.toml` preferences)
#[derive(Debug, Clone, Copy)]
pub struct DownloadOptions {
    /// Parallel connections for one file; 1 disables segmented downloads
    pub connections: u8,
}

impl DownloadOptions {
    /// Connections used when `download_connections` is unset
    pub const DEFAULT_CONNECTIONS: u8 = 4;

    /// Options from the user's preferences, defaulting unset values
    pub fn from_preferences(prefs: &crate::core::Preferences) -> Self {
        Self {
            connections: prefs
                .download_connections
                .unwrap_or(Self::DEFAULT_CONNECTIONS),
        }
    }
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connections: Self::DEFAULT_CONNECTIONS,
        }
    }
}

/// Size thresholds; tests shrink them
#[derive(Clone, Copy)]
struct Tuning {
    /// Files smaller than this always use one stream
    min_segmented: u64,
    /// Bytes per segment request
    chunk_size: u64,
}

const TUNING: Tuning = Tuning {
    min_segmented: 16 * 1024 * 1024,
    chunk_size: 8 * 1024 * 1024,
};

/// Outcome of one failed request attempt
enum AttemptError {
    /// Worth retrying (network error, stall, 5xx, 429, server restarted the body)
    Retry(anyhow::Error),
    /// Retrying cannot help (4xx, local I/O error, malformed response)
    Fatal(anyhow::Error),
    /// A segment got a full body (range unsupported or file changed): restart as one stream
    Fallback,
}

impl AttemptError {
    fn into_error(self) -> anyhow::Error {
        match self {
            Self::Retry(e) | Self::Fatal(e) => e,
            Self::Fallback => anyhow!("segmented download fell back unexpectedly"),
        }
    }
}

/// Map a non-success status to retry (5xx, 429) or fatal (everything else)
fn status_error(status: StatusCode, url: &str) -> AttemptError {
    let e = anyhow!("HTTP {status} for {url}");
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        AttemptError::Retry(e)
    } else {
        AttemptError::Fatal(e)
    }
}

fn send(req: RequestBuilder) -> std::result::Result<Response, AttemptError> {
    req.send()
        .map_err(|e| AttemptError::Retry(anyhow!(e).context("request failed")))
}

/// Run `attempt` until it succeeds, giving up after [`MAX_STALLED_ATTEMPTS`] in a row that left
/// the position (second tuple value) where it was; any progress resets the count.
fn retry_loop(
    url: &str,
    pb: &ProgressBar,
    mut position: u64,
    mut attempt: impl FnMut() -> (std::result::Result<(), AttemptError>, u64),
) -> std::result::Result<(), AttemptError> {
    let mut stalled = 0u32;
    loop {
        let (result, now) = attempt();
        let err = match result {
            Err(AttemptError::Retry(e)) => e,
            other => return other,
        };
        stalled = if now > position { 0 } else { stalled + 1 };
        position = now;
        if stalled >= MAX_STALLED_ATTEMPTS {
            return Err(AttemptError::Fatal(err.context(format!(
                "Download failed after {MAX_STALLED_ATTEMPTS} attempts without progress: {url}"
            ))));
        }
        log::warn!("Download interrupted: {err:#}");
        std::thread::sleep(Duration::from_millis(
            BACKOFF_BASE_MS << stalled.saturating_sub(1),
        ));
        pb.suspend(|| {
            eprintln!(
                "{} Resuming at {:.1} MB",
                "↻".yellow(),
                position as f64 / 1_048_576.0
            )
        });
    }
}

/// Single-stream download state that survives across attempts
struct Stream<'a> {
    url: &'a str,
    file: &'a File,
    /// Bytes already written to the `.part` file
    have: u64,
    /// Total size once a response told us
    total: Option<u64>,
    /// `ETag` (strong) or `Last-Modified` of the body in `.part`, sent as `If-Range`
    validator: Option<String>,
    pb: &'a ProgressBar,
    state: &'a State,
}

impl<'a> Stream<'a> {
    fn new(url: &'a str, file: &'a File, pb: &'a ProgressBar, state: &'a State) -> Self {
        Self {
            url,
            file,
            have: 0,
            total: None,
            validator: None,
            pb,
            state,
        }
    }

    /// Continue a previous run's sequential prefix of `have` bytes
    fn resume_from(&mut self, prev: Sidecar, have: u64) -> Result<()> {
        self.file.set_len(have)?;
        self.file.seek(SeekFrom::Start(have))?;
        self.have = have;
        self.validator.clone_from(&prev.validator);
        if let Some(t) = prev.total {
            self.set_total(t);
        }
        self.pb.set_position(have);
        self.state.save(Sidecar {
            chunk_size: 0,
            done_chunks: Vec::new(),
            ..prev
        });
        Ok(())
    }

    fn truncate(&mut self) -> Result<()> {
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.have = 0;
        self.pb.set_position(0);
        Ok(())
    }

    fn set_total(&mut self, total: u64) {
        show_total(self.pb, total);
        self.total = Some(total);
    }

    /// One request: resume from `have` when possible, then stream the body to `.part`
    fn attempt(&mut self) -> std::result::Result<(), AttemptError> {
        if self.have > 0 && self.validator.is_none() {
            // Without a validator a resumed body could belong to a different file
            self.truncate().map_err(AttemptError::Fatal)?;
        }
        let mut req = crate::utils::http::shared_client().get(self.url);
        if let (true, Some(v)) = (self.have > 0, &self.validator) {
            req = req
                .header(RANGE, format!("bytes={}-", self.have))
                .header(IF_RANGE, v.as_str());
        }
        let resp = send(req)?;
        self.accept(resp)
    }

    /// Check a response against the current position, then stream its body
    fn accept(&mut self, resp: Response) -> std::result::Result<(), AttemptError> {
        let fatal = AttemptError::Fatal;
        match resp.status() {
            StatusCode::PARTIAL_CONTENT => {
                let (start, total) = content_range(&resp)
                    .ok_or_else(|| fatal(anyhow!("206 without a valid Content-Range")))?;
                if start != self.have {
                    self.truncate().map_err(fatal)?;
                    return Err(AttemptError::Retry(anyhow!(
                        "server resumed at {start} instead of {}",
                        self.have
                    )));
                }
                if self.have == 0 {
                    self.validator = validator(&resp);
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
            s => return Err(status_error(s, self.url)),
        }
        if self.have == 0 {
            // A fresh body: record what a later run needs to resume it
            match &self.validator {
                Some(_) => self.state.save(Sidecar {
                    url: self.url.to_string(),
                    validator: self.validator.clone(),
                    total: self.total,
                    ..Sidecar::default()
                }),
                None => self.state.remove(),
            }
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

    /// Retry until the body is complete; `first` is an already-sent response to consume first
    fn run(&mut self, mut first: Option<Response>) -> Result<u64> {
        let (url, pb) = (self.url, self.pb);
        retry_loop(url, pb, self.have, || {
            let r = match first.take() {
                Some(resp) => self.accept(resp),
                None => self.attempt(),
            };
            (r, self.have)
        })
        .map_err(AttemptError::into_error)?;
        if let Some(total) = self.total.filter(|&t| t != self.have) {
            anyhow::bail!(
                "Download incomplete: got {} of {} bytes from {}",
                self.have,
                total,
                url
            );
        }
        Ok(self.have)
    }
}

/// Switch the bar on once the total size is known
fn show_total(pb: &ProgressBar, total: u64) {
    if total > 0 && pb.length() != Some(total) {
        pb.set_length(total);
        pb.set_draw_target(ProgressDrawTarget::stderr());
    }
}

/// `(start, total)` from `Content-Range: bytes start-end/total`; total is `None` for `*`
fn content_range(resp: &Response) -> Option<(u64, Option<u64>)> {
    let value = resp.headers().get(CONTENT_RANGE)?.to_str().ok()?;
    let (range, total) = value.strip_prefix("bytes ")?.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

/// Strong `ETag`, else `Last-Modified`: the only values `If-Range` accepts
fn validator(resp: &Response) -> Option<String> {
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
/// With `opts.connections > 1`, a server that honours `Range` and a file of at least 16 MiB,
/// the file is fetched as 8 MiB segments over that many connections. Each request gives up
/// after [`MAX_STALLED_ATTEMPTS`] consecutive attempts that added no bytes; any progress
/// resets the count. A failed download keeps `<dest>.part` and `<dest>.part.json` when the
/// server supports resuming; the next call for the same URL and unchanged file continues it.
pub fn download_file(url: &str, dest: &Path, opts: &DownloadOptions) -> Result<()> {
    fetch(url, dest, opts, TUNING)
}

fn fetch(url: &str, dest: &Path, opts: &DownloadOptions, tuning: Tuning) -> Result<()> {
    warn_if_plaintext(url);
    log::info!("Downloading: {}", url);
    log::debug!("Destination: {}", dest.display());

    if let Some(dir) = dest.parent() {
        resume::purge_stale(dir);
    }
    let part = part_path(dest);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&part)
        .with_context(|| format!("Failed to create file: {}", part.display()))?;
    if let Err(e) = file.try_lock() {
        anyhow::bail!(
            "{} is being downloaded by another wenget process ({e})",
            dest.display()
        );
    }
    let (state, prev) = resume::State::open(&part, url);
    let part_len = file.metadata()?.len();
    let pb = new_progress_bar();
    let result = transfer(url, &file, opts, tuning, &pb, &state, prev, part_len);

    let len_now = file.metadata().map_or(0, |m| m.len());
    drop(file); // releases the lock; Windows cannot rename an open file
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(e) => {
            pb.abandon();
            if state.has_progress(len_now) {
                eprintln!(
                    "{} Partial download kept; run the same command again to resume",
                    "↻".yellow()
                );
            } else {
                state.remove();
                let _ = std::fs::remove_file(&part);
            }
            return Err(e);
        }
    };
    std::fs::rename(&part, dest)
        .with_context(|| format!("Failed to move download to {}", dest.display()))?;
    state.remove();
    pb.finish_and_clear();
    log::info!("Downloaded {} bytes", bytes);
    Ok(())
}

/// Probe, then fill the locked `.part` file (segmented or single stream), resuming `prev`
#[allow(clippy::too_many_arguments)]
fn transfer(
    url: &str,
    file: &File,
    opts: &DownloadOptions,
    tuning: Tuning,
    pb: &ProgressBar,
    state: &State,
    prev: Option<Sidecar>,
    part_len: u64,
) -> Result<u64> {
    let mut stream = Stream::new(url, file, pb, state);
    let bytes = match segmented::probe(url, opts.connections, tuning.min_segmented) {
        segmented::Probe::Segmented(plan) => {
            let plan = segmented::Plan {
                chunk_size: tuning.chunk_size,
                ..plan
            };
            let done = prev
                .filter(|p| {
                    p.validator.as_ref() == Some(&plan.validator) && p.total == Some(plan.total)
                })
                .map(|p| p.done_mask(part_len, plan.total, plan.chunk_size))
                .unwrap_or_default();
            start_segmented(url, &plan, pb, state, &done);
            match segmented::download(url, file, &plan, opts.connections, pb, &done, state) {
                Ok(()) => plan.total,
                Err(AttemptError::Fallback) => {
                    log::info!("Segmented download not possible; using one stream");
                    stream.truncate()?;
                    stream.run(None)?
                }
                Err(e) => return Err(e.into_error()),
            }
        }
        segmented::Probe::Single(resp) => {
            let have = prev.as_ref().map_or(0, |p| p.contiguous_prefix(part_len));
            match prev {
                Some(prev) if have > 0 => {
                    // The probe body starts at 0; resume with a range request instead
                    drop(resp);
                    announce_resume(pb, have);
                    stream.resume_from(prev, have)?;
                    stream.run(None)?
                }
                _ => {
                    stream.truncate()?;
                    stream.run(resp)?
                }
            }
        }
    };
    file.sync_all().ok();
    Ok(bytes)
}

/// Record the segmented plan and show the bytes a previous run already fetched
fn start_segmented(
    url: &str,
    plan: &segmented::Plan,
    pb: &ProgressBar,
    state: &State,
    done: &[bool],
) {
    let done_chunks: Vec<usize> = done
        .iter()
        .enumerate()
        .filter_map(|(i, &d)| d.then_some(i))
        .collect();
    let have: u64 = done_chunks
        .iter()
        .map(|&i| (plan.chunk_size).min(plan.total - i as u64 * plan.chunk_size))
        .sum();
    show_total(pb, plan.total);
    if have > 0 {
        announce_resume(pb, have);
    }
    pb.set_position(have);
    state.save(Sidecar {
        url: url.to_string(),
        validator: Some(plan.validator.clone()),
        total: Some(plan.total),
        chunk_size: plan.chunk_size,
        done_chunks,
    });
}

fn announce_resume(pb: &ProgressBar, have: u64) {
    pb.suspend(|| {
        eprintln!(
            "{} Resuming previous download at {:.1} MB",
            "↻".yellow(),
            have as f64 / 1_048_576.0
        )
    });
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
        let result = download_file(
            "https://httpbin.org/bytes/1024",
            &dest,
            &DownloadOptions::default(),
        );
        assert!(result.is_ok());
        assert!(dest.exists());
    }

    use test_server::{payload, Fault, Server};

    /// Small thresholds so test payloads are segmented
    const SMALL: Tuning = Tuning {
        min_segmented: 1000,
        chunk_size: 4096,
    };

    fn fetch_with(srv: &Server, connections: u8) -> (TempDir, PathBuf, Result<()>) {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        let res = super::fetch(&srv.url, &dest, &DownloadOptions { connections }, SMALL);
        (tmp, dest, res)
    }

    /// Single-stream download
    fn fetch(srv: &Server) -> (TempDir, PathBuf, Result<()>) {
        fetch_with(srv, 1)
    }

    #[test]
    fn test_segmented_reassembles_file() {
        let data = payload(50_000);
        let srv = Server::start(data.clone(), true);
        let (_t, dest, res) = fetch_with(&srv, 4);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        let seen = srv.ranges_seen();
        assert_eq!(seen[0].as_deref(), Some("bytes=0-"));
        assert_eq!(seen.len(), 1 + 50_000_usize.div_ceil(4096));
        assert!(seen
            .iter()
            .any(|r| r.as_deref() == Some("bytes=49152-49999")));
    }

    #[test]
    fn test_segmented_chunk_resumes_mid_chunk() {
        let data = payload(50_000);
        let srv = Server::start(data.clone(), true);
        srv.fault(Fault::Drop(0)); // the probe body is never read
        srv.fault(Fault::Drop(1000));
        let (_t, dest, res) = fetch_with(&srv, 2);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        // One chunk was re-requested from 1000 bytes past its start
        assert!(srv.ranges_seen().iter().flatten().any(|r| r
            .trim_start_matches("bytes=")
            .split('-')
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            % 4096
            == 1000));
    }

    #[test]
    fn test_no_range_support_reuses_probe() {
        let data = payload(50_000);
        let srv = Server::start(data.clone(), false);
        let (_t, dest, res) = fetch_with(&srv, 4);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(srv.ranges_seen().len(), 1);
    }

    #[test]
    fn test_small_file_single_request() {
        let data = payload(500);
        let srv = Server::start(data.clone(), true);
        let (_t, dest, res) = fetch_with(&srv, 4);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(srv.ranges_seen().len(), 1);
    }

    #[test]
    fn test_segmented_falls_back_when_file_changes() {
        let srv = Server::start(payload(50_000), true);
        let new = payload(30_000).iter().map(|b| b ^ 0xff).collect::<Vec<_>>();
        srv.fault(Fault::Drop(0));
        srv.fault(Fault::Swap(new.clone(), "\"v2\"".into()));
        let (_t, dest, res) = fetch_with(&srv, 4);
        res.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), new);
    }

    #[test]
    fn test_segmented_404_fails_cleanly() {
        let srv = Server::start(payload(50_000), true);
        srv.fault(Fault::Drop(0));
        srv.fault(Fault::Status(404));
        let (_t, dest, res) = fetch_with(&srv, 4);
        assert!(res.unwrap_err().to_string().contains("404"));
        // Chunks finished before the 404 may be kept for resuming; the target never appears
        assert!(!dest.exists());
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

    fn fetch_into(srv: &Server, dest: &Path, connections: u8) -> Result<()> {
        super::fetch(&srv.url, dest, &DownloadOptions { connections }, SMALL)
    }

    fn sidecar_path(dest: &Path) -> PathBuf {
        let mut p = part_path(dest).into_os_string();
        p.push(".json");
        PathBuf::from(p)
    }

    #[test]
    fn test_single_stream_resumes_next_run() {
        let data = payload(100_000);
        let srv = Server::start(data.clone(), true);
        srv.fault(Fault::Drop(30_000));
        for _ in 0..3 {
            srv.fault(Fault::Drop(0));
        }
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        assert!(fetch_into(&srv, &dest, 1).is_err());
        assert_eq!(std::fs::metadata(part_path(&dest)).unwrap().len(), 30_000);
        assert!(sidecar_path(&dest).exists());

        let before = srv.ranges_seen().len();
        fetch_into(&srv, &dest, 1).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(srv.ranges_seen()[before].as_deref(), Some("bytes=30000-"));
        assert!(!part_path(&dest).exists() && !sidecar_path(&dest).exists());
    }

    /// Leave a `.part` whose first two 4096-byte chunks are done, as a killed run would
    fn seed_segmented(dest: &Path, data: &[u8], validator: &str) {
        let mut part = vec![0u8; data.len()];
        part[..8192].copy_from_slice(&data[..8192]);
        std::fs::write(part_path(dest), part).unwrap();
        let sidecar = Sidecar {
            url: String::new(),
            validator: Some(validator.into()),
            total: Some(data.len() as u64),
            chunk_size: 4096,
            done_chunks: vec![1, 0],
        };
        std::fs::write(sidecar_path(dest), serde_json::to_vec(&sidecar).unwrap()).unwrap();
    }

    fn seeded_url(srv: &Server, dest: &Path) {
        let path = sidecar_path(dest);
        let mut s: Sidecar = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        s.url = srv.url.clone();
        std::fs::write(path, serde_json::to_vec(&s).unwrap()).unwrap();
    }

    #[test]
    fn test_segmented_resumes_next_run() {
        let data = payload(50_000);
        let srv = Server::start(data.clone(), true);
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        seed_segmented(&dest, &data, "\"v1\"");
        seeded_url(&srv, &dest);
        fetch_into(&srv, &dest, 4).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        let seen = srv.ranges_seen();
        assert!(!seen.iter().any(|r| r.as_deref() == Some("bytes=0-4095")));
        assert!(!seen.iter().any(|r| r.as_deref() == Some("bytes=4096-8191")));
        assert_eq!(seen.len(), 1 + 50_000_usize.div_ceil(4096) - 2);
    }

    #[test]
    fn test_changed_validator_discards_previous_run() {
        let data = payload(50_000);
        let srv = Server::start(data.clone(), true);
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        // Stale bytes under an old validator must not survive
        seed_segmented(&dest, &vec![0xAA; 50_000], "\"old\"");
        seeded_url(&srv, &dest);
        fetch_into(&srv, &dest, 4).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(srv
            .ranges_seen()
            .iter()
            .any(|r| r.as_deref() == Some("bytes=0-4095")));
    }

    #[test]
    fn test_locked_part_is_refused() {
        let srv = Server::start(payload(100), true);
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("file.bin");
        let held = File::create(part_path(&dest)).unwrap();
        held.lock().unwrap();
        let err = fetch_into(&srv, &dest, 1).unwrap_err();
        assert!(err.to_string().contains("another wenget"), "{err}");
        assert!(srv.ranges_seen().is_empty());
    }
}
