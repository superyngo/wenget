//! Staged install/swap
//!
//! A package's record lives inside its app directory, and every install path
//! wipes that directory before writing. Extracting into a staging directory and
//! swapping by `rename` means every crash point leaves either the complete old
//! install or the complete new one.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::core::paths::{sanitize_path_component, WenPaths};

/// An in-progress install, extracted aside and not yet swapped into place
pub struct StagedInstall {
    staging_path: PathBuf,
    app_dir: PathBuf,
    committed: bool,
}

impl StagedInstall {
    /// Create an empty staging directory for `key`
    pub fn begin(paths: &WenPaths, key: &str) -> Result<Self> {
        let staging_root = paths.staging_dir();
        fs::create_dir_all(&staging_root)
            .with_context(|| format!("Failed to create {}", staging_root.display()))?;

        let staging_path = staging_root.join(format!(
            "{}-{}",
            sanitize_path_component(key),
            std::process::id()
        ));
        if staging_path.exists() {
            fs::remove_dir_all(&staging_path).with_context(|| {
                format!(
                    "Failed to clear stale staging dir {}",
                    staging_path.display()
                )
            })?;
        }
        fs::create_dir_all(&staging_path)
            .with_context(|| format!("Failed to create {}", staging_path.display()))?;

        Ok(Self {
            staging_path,
            app_dir: paths.app_dir(key),
            committed: false,
        })
    }

    /// Where to extract files and write the package record
    pub fn path(&self) -> &Path {
        &self.staging_path
    }

    /// The app directory this will become, for messages before the swap
    pub fn target(&self) -> &Path {
        &self.app_dir
    }

    /// Swap the staged directory into place, returning the app directory
    pub fn commit(mut self) -> Result<PathBuf> {
        if let Some(parent) = self.app_dir.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let retired = if self.app_dir.exists() {
            let retired = self.app_dir.with_file_name(format!(
                "{}.old-{}",
                self.app_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                Utc::now().timestamp()
            ));
            fs::rename(&self.app_dir, &retired).with_context(|| {
                format!(
                    "Failed to move the previous install aside: {} -> {}",
                    self.app_dir.display(),
                    retired.display()
                )
            })?;
            Some(retired)
        } else {
            None
        };

        if let Err(e) = fs::rename(&self.staging_path, &self.app_dir) {
            // Put the previous install back before reporting failure.
            if let Some(retired) = &retired {
                let _ = fs::rename(retired, &self.app_dir);
            }
            return Err(e).with_context(|| {
                format!(
                    "Failed to move the new install into {}",
                    self.app_dir.display()
                )
            });
        }

        self.committed = true;

        if let Some(retired) = retired {
            if let Err(e) = fs::remove_dir_all(&retired) {
                log::warn!(
                    "Installed successfully, but could not remove {}: {}. \
                     `wenget repair` will sweep it.",
                    retired.display(),
                    e
                );
            }
        }

        Ok(self.app_dir.clone())
    }
}

impl Drop for StagedInstall {
    fn drop(&mut self) {
        if !self.committed && self.staging_path.exists() {
            if let Err(e) = fs::remove_dir_all(&self.staging_path) {
                log::warn!(
                    "Could not clean up staging dir {}: {}",
                    self.staging_path.display(),
                    e
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_commit_swaps_staged_content_into_place() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());

        let staged = StagedInstall::begin(&paths, "tool").unwrap();
        std::fs::write(staged.path().join("tool"), b"new").unwrap();
        let app_dir = staged.commit().unwrap();

        assert_eq!(app_dir, paths.app_dir("tool"));
        assert_eq!(std::fs::read(app_dir.join("tool")).unwrap(), b"new");
        assert!(!paths.staging_dir().join("tool").exists());
    }

    #[test]
    fn test_commit_replaces_a_previous_install_completely() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        let app_dir = paths.app_dir("tool");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("stale"), b"old").unwrap();

        let staged = StagedInstall::begin(&paths, "tool").unwrap();
        std::fs::write(staged.path().join("tool"), b"new").unwrap();
        staged.commit().unwrap();

        assert!(
            !app_dir.join("stale").exists(),
            "old payload must not survive"
        );
        assert!(app_dir.join("tool").exists());

        let residue: Vec<_> = std::fs::read_dir(paths.apps_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".old-"))
            .collect();
        assert!(
            residue.is_empty(),
            "committed swaps clean up their own .old- dir"
        );
    }

    #[test]
    fn test_abandoned_staging_leaves_the_previous_install_intact() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        let app_dir = paths.app_dir("tool");
        std::fs::create_dir_all(app_dir.join(".wenget")).unwrap();
        std::fs::write(app_dir.join(".wenget").join("package.json"), b"{}").unwrap();
        std::fs::write(app_dir.join("tool"), b"old").unwrap();

        {
            let staged = StagedInstall::begin(&paths, "tool").unwrap();
            std::fs::write(staged.path().join("half"), b"partial").unwrap();
            // Dropped without commit, as an extraction failure would.
        }

        assert_eq!(std::fs::read(app_dir.join("tool")).unwrap(), b"old");
        assert!(app_dir.join(".wenget").join("package.json").exists());
        assert!(!paths.staging_dir().join("tool").exists());
    }
}
