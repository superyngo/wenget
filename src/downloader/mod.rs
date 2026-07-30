//! Downloader module for wenget

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::Write;
use std::path::Path;
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

/// Download a file from URL to a local path with progress bar
pub fn download_file(url: &str, dest: &Path) -> Result<()> {
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
