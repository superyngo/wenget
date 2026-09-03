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
use super::store::InstalledStore;
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
    /// tests that call `init` or write package records would otherwise touch the
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

        Ok(())
    }

    /// Check if wenget is initialized
    pub fn is_initialized(&self) -> bool {
        self.paths.is_initialized()
    }

    /// A handle for reading and writing per-package records
    pub fn store(&self) -> InstalledStore {
        InstalledStore::new(self.paths.clone())
    }

    /// Load the set of installed packages from per-package records
    ///
    /// Migration from a legacy `installed.json`, quarantine of unparseable
    /// records, and skipping of future-version records all happen inside
    /// `InstalledStore::load`.
    pub fn load_installed(&self) -> Result<InstalledSet> {
        self.store().load()
    }

    /// Generic JSON loader (without repair - for internal use)
    #[allow(dead_code)]
    fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse JSON from: {}", path.display()))
    }

    /// Load the set of installed packages
    ///
    /// No longer initializes: with no file that must exist for reads to work, an
    /// absent root simply means nothing is installed. Initialization happens on
    /// `add` and on `wenget init`.
    pub fn get_or_create_installed(&self) -> Result<InstalledSet> {
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

    /// A minimal, valid installed package for tests.
    fn sample_installed_package() -> crate::core::manifest::InstalledPackage {
        crate::core::manifest::InstalledPackage {
            meta_version: crate::core::manifest::CURRENT_META_VERSION,
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: String::new(),
            executables: std::collections::HashMap::from([(
                "bin/rg".to_string(),
                "rg".to_string(),
            )]),
            source: crate::core::manifest::PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "search tool".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "ripgrep-linux.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        }
    }

    #[test]
    fn test_load_installed_reads_per_package_records() {
        let (config, tmp) = create_test_config();

        config
            .store()
            .save_package("ripgrep", &sample_installed_package())
            .unwrap();

        let set = config.load_installed().unwrap();
        assert!(set.get_package("ripgrep").is_some());
        assert!(
            !tmp.path().join("installed.json").exists(),
            "no global index is ever written"
        );
    }

    #[test]
    fn test_get_or_create_installed_does_not_initialize() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("absent");
        let config = Config::with_paths(WenPaths::with_root(root.clone()));

        assert!(config
            .get_or_create_installed()
            .unwrap()
            .packages
            .is_empty());
        assert!(!root.exists(), "a read must not create the root");
    }

    #[test]
    fn test_is_initialized_ignores_installed_json() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("fresh");
        let config = Config::with_paths(WenPaths::with_root(root.clone()));
        assert!(!config.is_initialized());

        config.init().unwrap();
        assert!(config.is_initialized());
        assert!(root.join("apps").exists());
        assert!(
            !root.join("installed.json").exists(),
            "init no longer creates a global index"
        );
    }
}
