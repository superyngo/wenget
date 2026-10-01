//! Cross-run resume state for one download
//!
//! A failed or interrupted download keeps `<dest>.part` plus a `<dest>.part.json` sidecar that
//! records the URL, the server's validator (`ETag`/`Last-Modified`), the total size and, for
//! segmented downloads, which chunks are complete. Single-stream downloads write sequentially,
//! so the `.part` length is their resume offset. The sidecar is replaced atomically
//! (write-to-temp + rename) so a kill mid-write never leaves it half written.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Leftover `.part` files older than this are deleted when a download starts
const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Contents of `<dest>.part.json`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Sidecar {
    pub url: String,
    pub validator: Option<String>,
    pub total: Option<u64>,
    /// Segment size; 0 means a single sequential stream (resume from the `.part` length)
    #[serde(default)]
    pub chunk_size: u64,
    /// Completed segment indices
    #[serde(default)]
    pub done_chunks: Vec<usize>,
}

impl Sidecar {
    /// Bytes from offset 0 that are known good, given the current `.part` length
    pub fn contiguous_prefix(&self, part_len: u64) -> u64 {
        let have = if self.chunk_size == 0 {
            part_len
        } else {
            let mut n = 0;
            while self.done_chunks.contains(&n) {
                n += 1;
            }
            n as u64 * self.chunk_size
        };
        self.total.map_or(have, |t| have.min(t)).min(part_len)
    }

    /// Which `chunk_size` segments of a `total`-byte file are already complete
    pub fn done_mask(&self, part_len: u64, total: u64, chunk_size: u64) -> Vec<bool> {
        let chunks = total.div_ceil(chunk_size) as usize;
        if self.chunk_size == chunk_size {
            (0..chunks).map(|i| self.done_chunks.contains(&i)).collect()
        } else {
            // From a sequential stream: chunks wholly inside the written prefix
            let have = self.contiguous_prefix(part_len);
            (0..chunks)
                .map(|i| ((i as u64 + 1) * chunk_size).min(total) <= have)
                .collect()
        }
    }
}

/// `<dest>.part.json` on disk plus its in-memory copy, shared by segment workers
pub(super) struct State {
    path: PathBuf,
    data: Mutex<Option<Sidecar>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl State {
    /// Sidecar next to `part`; returns the previous run's record if it was for `url`
    pub fn open(part: &Path, url: &str) -> (Self, Option<Sidecar>) {
        let mut name = part.as_os_str().to_owned();
        name.push(".json");
        let path = PathBuf::from(name);
        let previous = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Sidecar>(&b).ok())
            .filter(|s| s.url == url && s.validator.is_some());
        let state = Self {
            path,
            data: Mutex::new(None),
        };
        (state, previous)
    }

    /// Replace the record
    pub fn save(&self, sidecar: Sidecar) {
        let mut data = lock(&self.data);
        self.write(&sidecar);
        *data = Some(sidecar);
    }

    /// Mark a segment complete
    pub fn chunk_done(&self, index: usize) {
        let mut data = lock(&self.data);
        if let Some(s) = data.as_mut() {
            s.done_chunks.push(index);
            self.write(s);
        }
    }

    /// Whether this run recorded resumable state with at least some fetched bytes
    pub fn has_progress(&self, part_len: u64) -> bool {
        lock(&self.data).as_ref().is_some_and(|s| {
            if s.chunk_size == 0 {
                part_len > 0
            } else {
                !s.done_chunks.is_empty()
            }
        }) && self.path.exists()
    }

    /// Delete the record (download finished or abandoned)
    pub fn remove(&self) {
        *lock(&self.data) = None;
        let _ = fs::remove_file(&self.path);
    }

    /// Best effort: losing the sidecar only costs the ability to resume
    fn write(&self, sidecar: &Sidecar) {
        let mut tmp = self.path.clone().into_os_string();
        tmp.push(".tmp");
        let result = serde_json::to_vec(sidecar)
            .map_err(std::io::Error::from)
            .and_then(|b| fs::write(&tmp, b))
            .and_then(|()| fs::rename(&tmp, &self.path));
        if let Err(e) = result {
            log::warn!("Failed to save resume state {}: {}", self.path.display(), e);
        }
    }
}

/// Delete `*.part` / `*.part.json` files in `dir` not modified for [`STALE_AFTER`]
pub(super) fn purge_stale(dir: &Path) {
    purge_older_than(dir, STALE_AFTER);
}

fn purge_older_than(dir: &Path, max_age: Duration) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.ends_with(".part") || name.ends_with(".part.json")) {
            continue;
        }
        let age = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok());
        if age.is_some_and(|a| a >= max_age) {
            log::info!("Removing stale partial download {}", entry.path().display());
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sidecar(chunk_size: u64, done: &[usize]) -> Sidecar {
        Sidecar {
            url: "u".into(),
            validator: Some("v".into()),
            total: Some(100),
            chunk_size,
            done_chunks: done.to_vec(),
        }
    }

    #[test]
    fn test_contiguous_prefix() {
        assert_eq!(sidecar(0, &[]).contiguous_prefix(40), 40);
        assert_eq!(sidecar(0, &[]).contiguous_prefix(400), 100);
        assert_eq!(sidecar(30, &[0, 1, 3]).contiguous_prefix(100), 60);
        assert_eq!(sidecar(30, &[1]).contiguous_prefix(100), 0);
    }

    #[test]
    fn test_done_mask() {
        assert_eq!(
            sidecar(30, &[0, 3]).done_mask(100, 100, 30),
            [true, false, false, true]
        );
        // Sequential prefix of 65 bytes covers chunks 0 and 1 of size 30
        assert_eq!(
            sidecar(0, &[]).done_mask(65, 100, 30),
            [true, true, false, false]
        );
    }

    #[test]
    fn test_state_roundtrip_and_url_filter() {
        let tmp = TempDir::new().unwrap();
        let part = tmp.path().join("a.zip.part");
        let (state, prev) = State::open(&part, "u");
        assert!(prev.is_none() && !state.has_progress(0));
        state.save(sidecar(30, &[]));
        state.chunk_done(2);
        assert!(state.has_progress(0));
        assert_eq!(State::open(&part, "u").1, Some(sidecar(30, &[2])));
        assert_eq!(State::open(&part, "other").1, None);
        state.remove();
        assert_eq!(State::open(&part, "u").1, None);
    }

    #[test]
    fn test_purge_only_old_part_files() {
        let tmp = TempDir::new().unwrap();
        for f in ["a.part", "a.part.json", "keep.zip"] {
            fs::write(tmp.path().join(f), b"x").unwrap();
        }
        purge_older_than(tmp.path(), Duration::from_secs(3600));
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 3);
        purge_older_than(tmp.path(), Duration::ZERO);
        let left: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["keep.zip"]);
    }
}
