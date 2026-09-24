//! Shim creation for Windows

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Create a .cmd shim (Windows only)
#[cfg(windows)]
pub fn create_shim(target: &Path, shim: &Path, _name: &str) -> Result<()> {
    log::debug!("Creating shim: {}", shim.display());

    // Create shim content
    let relative_path = pathdiff::diff_paths(target, shim.parent().unwrap())
        .context("Failed to calculate relative path")?;

    let shim_content = format!(
        "@echo off\r\n\"%~dp0{}\" %*\r\n",
        relative_path.display().to_string().replace('/', "\\")
    );

    // Create parent directory
    if let Some(parent) = shim.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write shim file
    fs::write(shim, shim_content)
        .with_context(|| format!("Failed to create shim: {}", shim.display()))?;

    Ok(())
}

/// Placeholder for Unix (uses symlink instead)
#[cfg(not(windows))]
pub fn create_shim(_target: &Path, _shim: &Path, _name: &str) -> Result<()> {
    // On Unix, we use symlinks instead of shims
    Ok(())
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_shim() {
        let temp_dir = TempDir::new().unwrap();

        let target = temp_dir
            .path()
            .join("apps")
            .join("test")
            .join("bin")
            .join("test.exe");
        let shim = temp_dir.path().join("bin").join("test.cmd");

        // Create target directory
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "").unwrap();

        // Create shim
        let result = create_shim(&target, &shim, "test");
        assert!(result.is_ok());
        assert!(shim.exists());

        // Verify shim content
        let content = fs::read_to_string(&shim).unwrap();
        assert!(content.contains("@echo off"));
        assert!(content.contains("test.exe"));
    }
}
