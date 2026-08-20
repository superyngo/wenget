//! Best-effort SHA-256 checksum verification for downloaded release assets.
//!
//! Wenget probes a small set of conventional checksum filenames published
//! alongside a GitHub Release asset (`<asset>.sha256`, `checksums.txt`,
//! `SHA256SUMS`), in the same release directory as the asset itself, and
//! verifies the download against a matching entry when one is found.
//!
//! This is deliberately asymmetric:
//! - A published checksum that *doesn't* match aborts the install outright.
//!   There is no override: a mismatch means either upstream shipped
//!   something different than it claims, or the download was tampered with
//!   in transit, and neither should be silently accepted.
//! - Everything else (no checksum published, or the probe itself failing)
//!   is a soft no-op — the install proceeds exactly as it would without
//!   this module. Most GitHub repos don't publish checksums at all, so
//!   treating their absence as fatal would break the common case.

use anyhow::{Context, Result};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use std::time::Duration;

/// Per-probe timeout. Kept short since most repos don't publish a checksum
/// file at all, and a slow/blackholed candidate shouldn't stall installs.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Result of probing every candidate checksum file for one asset.
#[derive(Debug)]
enum Lookup {
    /// A candidate was fetched and listed a hash for this asset.
    Found(String),
    /// Every candidate was a plain 404 (or equivalent) — nothing published.
    NotPublished,
    /// At least one candidate failed at the network layer (timeout,
    /// connection error, ...), so "not published" can't be concluded.
    ProbeFailed,
    /// `asset_url` isn't a GitHub Release download link; not probed.
    NotApplicable,
}

/// Outcome of fetching a single candidate checksum URL.
enum Probe {
    Found(String),
    NotFound,
    NetworkError,
}

fn probe(client: &reqwest::blocking::Client, url: &str) -> Probe {
    match client.get(url).send() {
        Ok(resp) if resp.status().is_success() => match resp.text() {
            Ok(text) => Probe::Found(text),
            Err(_) => Probe::NetworkError,
        },
        Ok(_) => Probe::NotFound,
        Err(_) => Probe::NetworkError,
    }
}

/// Probe the conventional checksum filenames next to `asset_url` for a hash
/// matching `asset_name`.
///
/// `<asset_name>.sha256` is a per-asset file scoped to `asset_name` by
/// construction (we only fetched it because we built its name from
/// `asset_name`); `checksums.txt` / `SHA256SUMS` are multi-entry files that
/// must be matched by filename.
fn lookup_checksum(asset_url: &str, asset_name: &str) -> Lookup {
    if !asset_url.contains("/releases/download/") {
        return Lookup::NotApplicable;
    }
    let Some((dir, _filename)) = asset_url.rsplit_once('/') else {
        return Lookup::NotApplicable;
    };

    let Ok(client) = reqwest::blocking::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
    else {
        return Lookup::ProbeFailed;
    };

    let candidates: [(String, bool); 3] = [
        (format!("{}.sha256", asset_name), true),
        ("checksums.txt".to_string(), false),
        ("SHA256SUMS".to_string(), false),
    ];

    let mut network_error = false;

    for (name, scoped_to_asset) in &candidates {
        let url = format!("{}/{}", dir, name);
        match probe(&client, &url) {
            Probe::Found(content) => {
                if let Some(hash) = extract_hash(&content, asset_name, *scoped_to_asset) {
                    return Lookup::Found(hash);
                }
                // File existed but didn't list this asset — try the next candidate.
            }
            Probe::NotFound => {}
            Probe::NetworkError => network_error = true,
        }
    }

    if network_error {
        Lookup::ProbeFailed
    } else {
        Lookup::NotPublished
    }
}

fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Extract the SHA-256 hex digest for `asset_name` from checksum file
/// content.
///
/// `scoped_to_asset` selects the parsing mode:
/// - `true` (per-asset file, e.g. `<asset>.sha256`): the first hex-64 token
///   on the first non-empty line is the digest, regardless of whether a
///   filename column follows.
/// - `false` (aggregate file, e.g. `checksums.txt`/`SHA256SUMS`): each line
///   is `<hex>  <filename>` (optionally `*filename` for binary mode); only
///   the line naming `asset_name` counts. A path-prefixed filename
///   (`dist/asset_name`) still matches via a suffix check.
fn extract_hash(content: &str, asset_name: &str, scoped_to_asset: bool) -> Option<String> {
    if scoped_to_asset {
        let first_line = content.lines().map(str::trim).find(|l| !l.is_empty())?;
        let hash = first_line.split_whitespace().next()?;
        return is_sha256_hex(hash).then(|| hash.to_lowercase());
    }

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(hash) = parts.next() else {
            continue;
        };
        if !is_sha256_hex(hash) {
            continue;
        }
        if let Some(name) = parts.next() {
            let name = name.trim_start_matches('*');
            if name == asset_name || name.ends_with(&format!("/{}", asset_name)) {
                return Some(hash.to_lowercase());
            }
        }
    }
    None
}

/// Compute the lowercase hex SHA-256 digest of a file, streaming so memory
/// use stays constant regardless of file size.
fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open {} for checksum", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("Failed to read {} for checksum", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Look up and verify the published checksum (if any) for a freshly
/// downloaded release asset, printing the outcome.
///
/// Returns `Err` only when a checksum was found and didn't match the
/// downloaded file — callers must treat that as fatal (delete the download,
/// abort the install). Every other outcome (nothing published, probe
/// failed, or verified successfully) returns `Ok(())` and the install
/// should proceed.
pub fn verify_download(asset_url: &str, asset_name: &str, download_path: &Path) -> Result<()> {
    match lookup_checksum(asset_url, asset_name) {
        Lookup::Found(expected) => {
            let actual = sha256_file(download_path)?;
            if actual.eq_ignore_ascii_case(&expected) {
                println!("  {} Checksum verified (sha256)", "✓".green());
                Ok(())
            } else {
                anyhow::bail!(
                    "Checksum mismatch for {}: expected {}, got {}",
                    asset_name,
                    expected,
                    actual
                )
            }
        }
        Lookup::NotPublished => {
            println!(
                "  {} No checksum published, skipping verification",
                "ℹ".cyan()
            );
            Ok(())
        }
        Lookup::ProbeFailed => {
            println!(
                "  {} Checksum lookup failed (network error), skipping verification",
                "⚠".yellow()
            );
            Ok(())
        }
        Lookup::NotApplicable => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_hash_scoped_hash_only() {
        let hash64 = "a".repeat(64);
        let content = format!("{}\n", hash64);
        assert_eq!(
            extract_hash(&content, "app.tar.gz", true),
            Some(hash64.clone())
        );
        // Content with trailing filename column is still just the first token.
        let content_with_name = format!("{}  app.tar.gz\n", hash64);
        assert_eq!(
            extract_hash(&content_with_name, "app.tar.gz", true),
            Some(hash64)
        );
    }

    #[test]
    fn test_extract_hash_aggregate_exact_match() {
        let hash_a = "a".repeat(64);
        let hash_b = "b".repeat(64);
        let content = format!(
            "{}  app-linux-x86_64.tar.gz\n{}  app-macos-aarch64.tar.gz\n",
            hash_a, hash_b
        );
        assert_eq!(
            extract_hash(&content, "app-macos-aarch64.tar.gz", false),
            Some(hash_b)
        );
        assert_eq!(
            extract_hash(&content, "app-linux-x86_64.tar.gz", false),
            Some(hash_a)
        );
    }

    #[test]
    fn test_extract_hash_aggregate_path_prefix_suffix_match() {
        let hash = "c".repeat(64);
        let content = format!("{} *dist/app.tar.gz\n", hash);
        assert_eq!(extract_hash(&content, "app.tar.gz", false), Some(hash));
    }

    #[test]
    fn test_extract_hash_aggregate_no_match() {
        let hash = "d".repeat(64);
        let content = format!("{}  other-asset.tar.gz\n", hash);
        assert_eq!(extract_hash(&content, "app.tar.gz", false), None);
    }

    #[test]
    fn test_extract_hash_rejects_wrong_length() {
        // sha512-length digest must not be mistaken for a sha256 one.
        let hash512 = "e".repeat(128);
        let content = format!("{}  app.tar.gz\n", hash512);
        assert_eq!(extract_hash(&content, "app.tar.gz", false), None);
        assert_eq!(extract_hash(&content, "app.tar.gz", true), None);
    }

    #[test]
    fn test_sha256_file_known_digest() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("hello.txt");
        std::fs::write(&path, b"hello world").unwrap();
        // Well-known SHA-256 of "hello world".
        assert_eq!(
            sha256_file(&path).unwrap(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_lookup_not_applicable_for_non_release_url() {
        // Should short-circuit without making any network request.
        match lookup_checksum("https://example.com/random/file.zip", "file.zip") {
            Lookup::NotApplicable => {}
            _ => panic!("expected NotApplicable for a non-release URL"),
        }
    }

    #[test]
    #[ignore] // Requires network access
    fn test_lookup_real_wenget_release() {
        // wenget's own release workflow always publishes SHA256SUMS.
        let url =
            "https://github.com/superyngo/wenget/releases/download/v3.8.5/wenget-macos-aarch64.tar.gz";
        match lookup_checksum(url, "wenget-macos-aarch64.tar.gz") {
            Lookup::Found(hash) => assert_eq!(hash.len(), 64),
            other => panic!(
                "expected a checksum to be found in the real SHA256SUMS file, got {:?}",
                other
            ),
        }
    }

    #[test]
    #[ignore] // Requires network access
    fn test_verify_download_detects_mismatch() {
        // A file that doesn't match the real published SHA256SUMS entry
        // must hard-fail verify_download — this is the "no override" contract.
        let url =
            "https://github.com/superyngo/wenget/releases/download/v3.8.5/wenget-macos-aarch64.tar.gz";
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("wenget-macos-aarch64.tar.gz");
        std::fs::write(&path, b"not the real archive content").unwrap();

        let err = verify_download(url, "wenget-macos-aarch64.tar.gz", &path)
            .expect_err("mismatched content must be rejected");
        assert!(err.to_string().contains("mismatch"));
    }
}
