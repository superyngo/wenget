//! Bucket management for wenget
//!
//! Buckets are remote manifest sources that can be added to wenget.
//! They use the same manifest format as local sources.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A bucket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Bucket name (unique identifier)
    pub name: String,

    /// URL to the manifest.json file
    pub url: String,

    /// Whether this bucket is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Priority (higher = higher priority, used for conflict resolution)
    #[serde(default = "default_priority")]
    pub priority: u32,
}

fn default_enabled() -> bool {
    true
}

fn default_priority() -> u32 {
    100
}

/// Bucket configuration file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketConfig {
    /// List of buckets
    pub buckets: Vec<Bucket>,
}

impl BucketConfig {
    /// Create a new empty bucket config
    pub fn new() -> Self {
        Self {
            buckets: Vec::new(),
        }
    }

    /// Load bucket config from file with automatic repair on parse errors
    pub fn load(path: &PathBuf) -> Result<Self> {
        use crate::core::repair::{
            create_backup, print_repair_warning, try_parse_json, RepairAction, RepairSeverity,
        };

        // Handle missing file (existing behavior)
        if !path.exists() {
            return Ok(Self::new());
        }

        // Read file content
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read bucket config: {}", path.display()))?;

        // Try to parse JSON
        match try_parse_json::<Self>(&content, path) {
            Ok(config) => Ok(config),
            Err(parse_error) => {
                log::warn!("Failed to parse buckets.json: {}", parse_error);

                // Create backup of corrupted file
                let backup_path = create_backup(path).ok();

                // Create new empty config
                let new_config = Self::new();

                // Save the new config
                let json_content = serde_json::to_string_pretty(&new_config)
                    .context("Failed to serialize bucket config")?;
                fs::write(path, json_content).with_context(|| {
                    format!("Failed to write bucket config: {}", path.display())
                })?;

                // Notify user
                let action = RepairAction::ResetToEmpty { backup_path };
                print_repair_warning(
                    "buckets.json",
                    &action,
                    RepairSeverity::Warning,
                    Some("Your bucket configuration was reset. Re-add buckets with 'wenget bucket add'."),
                );

                Ok(new_config)
            }
        }
    }

    /// Save bucket config to file
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize bucket config")?;

        fs::write(path, content)
            .with_context(|| format!("Failed to write bucket config: {}", path.display()))
    }

    /// Add a bucket
    pub fn add_bucket(&mut self, bucket: Bucket) -> bool {
        // Check if bucket with same name already exists
        if self.buckets.iter().any(|b| b.name == bucket.name) {
            return false;
        }

        self.buckets.push(bucket);
        true
    }

    /// Remove a bucket by name
    pub fn remove_bucket(&mut self, name: &str) -> bool {
        let original_len = self.buckets.len();
        self.buckets.retain(|b| b.name != name);
        self.buckets.len() < original_len
    }

    /// Find a bucket by name
    #[allow(dead_code)]
    pub fn find_bucket(&self, name: &str) -> Option<&Bucket> {
        self.buckets.iter().find(|b| b.name == name)
    }

    /// Find a bucket by name (mutable)
    #[allow(dead_code)]
    pub fn find_bucket_mut(&mut self, name: &str) -> Option<&mut Bucket> {
        self.buckets.iter_mut().find(|b| b.name == name)
    }

    /// Get all enabled buckets
    pub fn enabled_buckets(&self) -> Vec<&Bucket> {
        self.buckets.iter().filter(|b| b.enabled).collect()
    }

    /// Set bucket enabled state
    #[allow(dead_code)]
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        if let Some(bucket) = self.find_bucket_mut(name) {
            bucket.enabled = enabled;
            true
        } else {
            false
        }
    }
}

impl Default for BucketConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_config_new() {
        let config = BucketConfig::new();
        assert_eq!(config.buckets.len(), 0);
    }

    #[test]
    fn test_add_bucket() {
        let mut config = BucketConfig::new();

        let bucket = Bucket {
            name: "official".to_string(),
            url: "https://example.com/manifest.json".to_string(),
            enabled: true,
            priority: 100,
        };

        // First add should succeed
        assert!(config.add_bucket(bucket.clone()));
        assert_eq!(config.buckets.len(), 1);

        // Second add should fail (duplicate name)
        assert!(!config.add_bucket(bucket));
        assert_eq!(config.buckets.len(), 1);
    }

    #[test]
    fn test_remove_bucket() {
        let mut config = BucketConfig::new();

        let bucket = Bucket {
            name: "official".to_string(),
            url: "https://example.com/manifest.json".to_string(),
            enabled: true,
            priority: 100,
        };

        config.add_bucket(bucket);
        assert_eq!(config.buckets.len(), 1);

        assert!(config.remove_bucket("official"));
        assert_eq!(config.buckets.len(), 0);

        assert!(!config.remove_bucket("nonexistent"));
    }

    #[test]
    fn test_enabled_buckets() {
        let mut config = BucketConfig::new();

        config.add_bucket(Bucket {
            name: "bucket1".to_string(),
            url: "https://example.com/1.json".to_string(),
            enabled: true,
            priority: 100,
        });

        config.add_bucket(Bucket {
            name: "bucket2".to_string(),
            url: "https://example.com/2.json".to_string(),
            enabled: false,
            priority: 100,
        });

        let enabled = config.enabled_buckets();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "bucket1");
    }
}
