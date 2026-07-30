//! User preferences management
//!
//! This module handles persistent user configuration stored in config.toml

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// User preferences stored in config.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Preferences {
    /// Preferred platform override (e.g., "x86_64-unknown-linux-musl")
    ///
    /// When set, this overrides automatic platform detection.
    /// Useful for forcing musl builds on glibc systems, or testing cross-platform binaries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_platform: Option<String>,

    /// Custom bin directory path
    ///
    /// When set, symlinks/shims will be created here instead of the default location.
    /// Useful for custom PATH setups or when ~/.wenget/bin cannot be added to PATH.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_bin_path: Option<PathBuf>,
}

impl Preferences {
    /// Load preferences from config.toml
    ///
    /// Returns default preferences if the file doesn't exist.
    /// Creates parent directories if needed.
    pub fn load(config_path: &Path) -> Result<Self> {
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))
    }

    /// Save preferences to config.toml
    #[allow(dead_code)]
    pub fn save(&self, config_path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let content =
            toml::to_string_pretty(self).context("Failed to serialize preferences to TOML")?;

        fs::write(config_path, content)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))
    }

    /// Generate a default config.toml with helpful comments
    pub fn generate_default_file(config_path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let template = r#"# wenget Configuration File
# Edit with: wenget config
#
# This file allows you to customize wenget's behavior with persistent preferences.

# Preferred platform (overrides automatic detection)
#
# When set, wenget will prefer binaries for this platform instead of auto-detecting.
# Useful for:
# - Forcing musl builds on glibc systems (smaller, statically linked)
# - Testing cross-platform compatibility
# - Working around detection issues
#
# Common platform identifiers:
# - Linux x86_64 (glibc):  "x86_64-unknown-linux-gnu"
# - Linux x86_64 (musl):   "x86_64-unknown-linux-musl"
# - Linux ARM64 (glibc):   "aarch64-unknown-linux-gnu"
# - Linux ARM64 (musl):    "aarch64-unknown-linux-musl"
# - macOS Intel:           "x86_64-apple-darwin"
# - macOS Apple Silicon:   "aarch64-apple-darwin"
# - Windows x86_64:        "x86_64-pc-windows-msvc"
# - Windows ARM64:         "aarch64-pc-windows-msvc"
#
# Example:
# preferred_platform = "x86_64-unknown-linux-musl"

# Custom bin directory (overrides default)
#
# When set, symlinks/shims will be created in this directory instead of:
# - User install: ~/.wenget/bin
# - System install: /usr/local/bin (Linux) or C:\Program Files\wenget\bin (Windows)
#
# Useful if you want to use a custom location that's already in your PATH.
#
# Example:
# custom_bin_path = "/usr/local/bin"
"#;

        fs::write(config_path, template)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))
    }

    /// Validate preferences
    ///
    /// Checks that:
    /// - Platform string is reasonable (contains expected separators)
    /// - Custom bin path is absolute
    pub fn validate(&self) -> Result<()> {
        // Validate platform string format
        if let Some(ref platform) = self.preferred_platform {
            if !platform.contains('-') {
                anyhow::bail!(
                    "Invalid platform string: '{}' - Expected format: 'arch-vendor-os' or 'arch-vendor-os-abi'",
                    platform
                );
            }
        }

        // Validate custom bin path is absolute
        if let Some(ref path) = self.custom_bin_path {
            if !path.is_absolute() {
                anyhow::bail!("Custom bin path must be absolute, got: {}", path.display());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_preferences() {
        let prefs = Preferences::default();
        assert!(prefs.preferred_platform.is_none());
        assert!(prefs.custom_bin_path.is_none());
    }

    #[test]
    fn test_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let prefs = Preferences {
            preferred_platform: Some("x86_64-unknown-linux-musl".to_string()),
            custom_bin_path: Some(PathBuf::from("/usr/local/bin")),
        };

        prefs.save(&config_path).unwrap();
        let loaded = Preferences::load(&config_path).unwrap();

        assert_eq!(loaded.preferred_platform, prefs.preferred_platform);
        assert_eq!(loaded.custom_bin_path, prefs.custom_bin_path);
    }

    #[test]
    fn test_load_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent.toml");

        let prefs = Preferences::load(&config_path).unwrap();
        assert!(prefs.preferred_platform.is_none());
        assert!(prefs.custom_bin_path.is_none());
    }

    #[test]
    fn test_generate_default_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        Preferences::generate_default_file(&config_path).unwrap();
        assert!(config_path.exists());

        let content = fs::read_to_string(&config_path).unwrap();
        assert!(content.contains("wenget Configuration File"));
        assert!(content.contains("preferred_platform"));
        assert!(content.contains("custom_bin_path"));
    }

    #[test]
    fn test_validate_valid() {
        let prefs = Preferences {
            preferred_platform: Some("x86_64-unknown-linux-gnu".to_string()),
            custom_bin_path: Some(PathBuf::from("/usr/local/bin")),
        };
        assert!(prefs.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_platform() {
        let prefs = Preferences {
            preferred_platform: Some("invalid".to_string()),
            custom_bin_path: None,
        };
        assert!(prefs.validate().is_err());
    }

    #[test]
    fn test_validate_relative_path() {
        let prefs = Preferences {
            preferred_platform: None,
            custom_bin_path: Some(PathBuf::from("relative/path")),
        };
        assert!(prefs.validate().is_err());
    }
}
