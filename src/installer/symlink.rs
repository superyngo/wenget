//! Symlink creation for Unix systems

use anyhow::Result;
use std::path::Path;

#[cfg(unix)]
use anyhow::Context;

/// Create a symlink (Unix only)
#[cfg(unix)]
pub fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    log::debug!(
        "Creating symlink: {} -> {}",
        link.display(),
        target.display()
    );

    // Remove existing symlink if it exists
    if link.exists() || link.is_symlink() {
        std::fs::remove_file(link)
            .with_context(|| format!("Failed to remove existing symlink: {}", link.display()))?;
    }

    // Create parent directory
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }

    symlink(target, link)
        .with_context(|| format!("Failed to create symlink: {}", link.display()))?;

    Ok(())
}

/// Placeholder for Windows (uses shim instead)
#[cfg(not(unix))]
pub fn create_symlink(_target: &Path, _link: &Path) -> Result<()> {
    // On Windows, we use .cmd shims instead of symlinks
    Ok(())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_symlink() {
        let temp_dir = TempDir::new().unwrap();

        let target = temp_dir.path().join("target.txt");
        let link = temp_dir.path().join("link.txt");

        // Create target file
        std::fs::write(&target, "test").unwrap();

        // Create symlink
        let result = create_symlink(&target, &link);
        assert!(result.is_ok());
        assert!(link.exists());
        assert!(link.is_symlink());
    }
}
