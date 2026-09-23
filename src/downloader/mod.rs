//! Downloader module for wenget

use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn shared_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(format!("wenget/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("Failed to create HTTP client")
    })
}

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

/// Download a file from URL to a local path with progress bar
pub fn download_file(url: &str, dest: &Path) -> Result<()> {
    warn_if_plaintext(url);
    log::info!("Downloading: {}", url);
    log::debug!("Destination: {}", dest.display());

    let client = shared_client();

    // Send GET request
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("Failed to download from {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP {} for {}", response.status(), url);
    }

    // Get file size for progress bar
    let total_size = response.content_length().unwrap_or(0);

    // Create progress bar
    let pb = if total_size > 0 {
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Some(pb)
    } else {
        None
    };

    // Create destination file
    let mut file =
        File::create(dest).with_context(|| format!("Failed to create file: {}", dest.display()))?;

    // Download and write with progress
    let mut downloaded = 0u64;
    let mut buffer = vec![0; 65536];

    let mut reader = std::io::BufReader::new(response);
    loop {
        let n = std::io::Read::read(&mut reader, &mut buffer).context("Failed to read response")?;

        if n == 0 {
            break;
        }

        file.write_all(&buffer[..n])
            .context("Failed to write to file")?;

        downloaded += n as u64;

        if let Some(pb) = &pb {
            pb.set_position(downloaded);
        }
    }

    if let Some(pb) = pb {
        pb.finish_with_message("Download complete");
    }

    log::info!("Downloaded {} bytes", downloaded);

    Ok(())
}

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
}
