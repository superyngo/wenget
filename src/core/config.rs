//! Configuration management for wenget
//!
//! This module handles:
//! - Loading and saving installed.json
//! - Loading and saving buckets.json
//! - Loading and saving manifest-cache.json
//! - Directory initialization

use super::manifest::{InstalledSet, SourceManifest};
use super::paths::WenPaths;
use super::preferences::Preferences;
use crate::bucket::BucketConfig;
use crate::cache::ManifestCache;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Configuration manager
pub struct Config {
    paths: WenPaths,
    #[allow(dead_code)]
    preferences: Preferences,
}

impl Config {
    /// Create a new Config instance
    pub fn new() -> Result<Self> {
        // First, create a temporary WenPaths to get the config file path
        let temp_paths = WenPaths::new()?;
        let config_path = temp_paths.config_toml();

        // Load preferences
        let preferences = Preferences::load(&config_path)?;

        // Validate preferences
        if let Err(e) = preferences.validate() {
            log::warn!("Invalid preferences in config.toml: {}", e);
            log::warn!("Using default preferences instead");
        }

        // Create WenPaths with custom bin directory if specified
        let paths = WenPaths::new_with_custom_bin(preferences.custom_bin_path.clone())?;

        Ok(Self { paths, preferences })
    }

    /// Create a Config instance backed by an explicit paths manager
    ///
    /// Intended for tests: `Config::new()` resolves the real `~/.wenget/`, so
    /// tests that call `init`/`save_installed` would otherwise overwrite the
    /// developer's actual installed-package records.
    #[cfg(test)]
    pub fn with_paths(paths: WenPaths) -> Self {
        Self {
            paths,
            preferences: Preferences::default(),
        }
    }

    /// Get the paths manager
    pub fn paths(&self) -> &WenPaths {
        &self.paths
    }

    /// Get the preferences
    #[allow(dead_code)]
    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }

    /// Initialize wenget (create directories if needed)
    pub fn init(&self) -> Result<()> {
        self.paths.init_dirs()?;

        // Create empty manifests if they don't exist
        if !self.paths.installed_json().exists() {
            self.save_installed(&InstalledSet::new())?;
        }

        Ok(())
    }

    /// Check if wenget is initialized
    pub fn is_initialized(&self) -> bool {
        self.paths.is_initialized() && self.paths.installed_json().exists()
    }

    /// Load installed manifest with automatic repair on parse errors
    pub fn load_installed(&self) -> Result<InstalledSet> {
        use super::repair::{
            create_backup, print_repair_warning, try_parse_json, RepairAction, RepairSeverity,
        };

        let path = self.paths.installed_json();

        // Handle missing file
        if !path.exists() {
            return Ok(InstalledSet::new());
        }

        // Read file content
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        // Try to parse JSON
        match try_parse_json::<InstalledSet>(&content, &path) {
            Ok(mut manifest) => {
                // Migrate old format to new format
                manifest.migrate();
                Ok(manifest)
            }
            Err(parse_error) => {
                log::error!("CRITICAL: Failed to parse installed.json: {}", parse_error);

                // This is critical - create backup
                let backup_path = create_backup(&path)
                    .map_err(|e| {
                        log::warn!("Failed to create backup of corrupted file: {}", e);
                        e
                    })
                    .ok();

                // Create new empty manifest
                let new_manifest = InstalledSet::new();

                // Save the new manifest
                self.save_installed(&new_manifest)?;

                // Notify user with critical warning
                let action = RepairAction::ResetToEmpty {
                    backup_path: backup_path.clone(),
                };
                print_repair_warning(
                    "installed.json",
                    &action,
                    RepairSeverity::Critical,
                    Some("Your installed package records were corrupted. wenget cannot track previously installed packages. You may need to reinstall them."),
                );

                Ok(new_manifest)
            }
        }
    }

    /// Save installed manifest
    pub fn save_installed(&self, manifest: &InstalledSet) -> Result<()> {
        let path = self.paths.installed_json();
        Self::save_json(&path, manifest).context("Failed to save installed.json")
    }

    /// Generic JSON loader (without repair - for internal use)
    #[allow(dead_code)]
    fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON from: {}", path.display()))
    }

    /// Generic JSON saver
    fn save_json<T: serde::Serialize>(path: &Path, data: &T) -> Result<()> {
        let json =
            serde_json::to_string_pretty(data).context("Failed to serialize data to JSON")?;

        fs::write(path, json)
            .with_context(|| format!("Failed to write file: {}", path.display()))?;

        Ok(())
    }

    /// Get or create installed manifest (auto-initialize if needed)
    pub fn get_or_create_installed(&self) -> Result<InstalledSet> {
        if !self.is_initialized() {
            self.init()?;
        }
        self.load_installed()
    }

    /// Load bucket config
    pub fn load_buckets(&self) -> Result<BucketConfig> {
        let path = self.paths.buckets_json();
        BucketConfig::load(&path)
    }

    /// Save bucket config
    pub fn save_buckets(&self, config: &BucketConfig) -> Result<()> {
        let path = self.paths.buckets_json();
        config.save(&path)
    }

    /// Get or create bucket config
    pub fn get_or_create_buckets(&self) -> Result<BucketConfig> {
        if !self.is_initialized() {
            self.init()?;
        }
        self.load_buckets()
    }

    /// Load manifest cache
    pub fn load_cache(&self) -> Result<ManifestCache> {
        let path = self.paths.manifest_cache_json();
        ManifestCache::load(&path)
    }

    /// Save manifest cache
    pub fn save_cache(&self, cache: &ManifestCache) -> Result<()> {
        let path = self.paths.manifest_cache_json();
        cache.save(&path)
    }

    /// Invalidate cache (delete the cache file)
    pub fn invalidate_cache(&self) -> Result<()> {
        let path = self.paths.manifest_cache_json();
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to remove cache file: {}", path.display()))?;
        }
        Ok(())
    }

    /// Get or rebuild manifest cache
    /// Returns the cache if valid, otherwise rebuilds it
    pub fn get_or_rebuild_cache(&self) -> Result<ManifestCache> {
        let cache = self.load_cache()?;

        // Check if cache is valid
        if cache.is_valid() && !cache.packages.is_empty() {
            return Ok(cache);
        }

        // Rebuild cache
        self.rebuild_cache()
    }

    /// Force rebuild manifest cache from buckets only
    pub fn rebuild_cache(&self) -> Result<ManifestCache> {
        use crate::bucket::Bucket;
        use crate::cache::build_cache_from_results;
        use crate::utils::HttpClient;
        use std::time::Duration;

        let bucket_config = self.get_or_create_buckets()?;
        let enabled_buckets: Vec<Bucket> = bucket_config
            .enabled_buckets()
            .into_iter()
            .cloned()
            .collect();

        if enabled_buckets.is_empty() {
            let cache = ManifestCache::new();
            self.save_cache(&cache)?;
            return Ok(cache);
        }

        let results: Vec<(Bucket, Result<SourceManifest>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = enabled_buckets
                .into_iter()
                .map(|bucket| {
                    scope.spawn(move || {
                        let name = bucket.name.clone();
                        let url = bucket.url.clone();
                        log::debug!("Fetching bucket '{}' from {}", name, url);

                        let fetch_result = (|| -> Result<SourceManifest> {
                            let http = HttpClient::with_timeout(Duration::from_secs(10))?;
                            let content = http
                                .get_text(&url)
                                .with_context(|| format!("Failed to fetch bucket from {}", url))?;
                            serde_json::from_str(&content).with_context(|| {
                                format!("Failed to parse bucket manifest from {}", url)
                            })
                        })();

                        (
                            Bucket {
                                name,
                                url,
                                enabled: bucket.enabled,
                                priority: bucket.priority,
                            },
                            fetch_result,
                        )
                    })
                })
                .collect();

            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });

        let cache = build_cache_from_results(results);
        self.save_cache(&cache)?;
        Ok(cache)
    }

    /// Get packages from cache
    /// This is the recommended way to get packages for read operations
    pub fn get_packages_from_cache(&self) -> Result<SourceManifest> {
        let cache = self.get_or_rebuild_cache()?;
        Ok(cache.to_source_manifest())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a Config rooted in a fresh temporary directory.
    ///
    /// The returned TempDir must stay alive for the duration of the test.
    fn create_test_config() -> (Config, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = Config::with_paths(WenPaths::with_root(temp_dir.path().to_path_buf()));
        (config, temp_dir)
    }

    #[test]
    fn test_config_creation() {
        let (config, _tmp) = create_test_config();
        assert!(config.paths().root().exists());
    }

    #[test]
    fn test_init() {
        let (config, _tmp) = create_test_config();
        config.init().unwrap();
        assert!(config.paths().root().exists());
        assert!(config.paths().apps_dir().exists());
    }

    #[test]
    fn test_manifest_round_trip() {
        let (config, tmp) = create_test_config();
        config.init().unwrap();

        let manifest = InstalledSet::new();
        config.save_installed(&manifest).unwrap();

        let loaded = config.load_installed().unwrap();
        assert_eq!(loaded.packages.len(), manifest.packages.len());

        // The round trip must stay inside the temp root and never touch the
        // developer's real ~/.wenget/installed.json.
        assert!(config.paths().installed_json().starts_with(tmp.path()));
    }
}
