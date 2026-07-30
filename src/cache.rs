//! Manifest cache management for wenget
//!
//! The cache fetches and merges bucket sources into a unified view.
//! This reduces GitHub API calls and improves performance.

use crate::bucket::Bucket;
use crate::core::manifest::{Package, PackageSource, ScriptItem, SourceManifest};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Package with source information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPackage {
    /// The package data
    #[serde(flatten)]
    pub package: Package,

    /// Source origin
    pub source: PackageSource,
}

/// Script with source information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedScript {
    /// The script data
    #[serde(flatten)]
    pub script: ScriptItem,

    /// Source origin
    pub source: PackageSource,
}

/// Source information in cache
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedSourceInfo {
    /// Source type
    #[serde(flatten)]
    pub source: PackageSource,

    /// Number of packages from this source
    pub package_count: usize,

    /// Last fetch time (for buckets)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fetched: Option<DateTime<Utc>>,

    /// Bucket URL (for buckets)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Manifest cache view
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestCache {
    /// Cache format version
    pub version: String,

    /// Last update timestamp
    pub last_updated: DateTime<Utc>,

    /// Time-to-live in seconds
    #[serde(default = "default_ttl")]
    pub ttl_seconds: i64,

    /// Source information
    pub sources: HashMap<String, CachedSourceInfo>,

    /// Cached packages (key: repo URL)
    pub packages: HashMap<String, CachedPackage>,

    /// Cached scripts (key: script name)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub scripts: HashMap<String, CachedScript>,
}

fn default_ttl() -> i64 {
    86400 // 24 hours
}

impl ManifestCache {
    /// Create a new empty cache
    pub fn new() -> Self {
        Self {
            version: "1.0".to_string(),
            last_updated: Utc::now(),
            ttl_seconds: default_ttl(),
            sources: HashMap::new(),
            packages: HashMap::new(),
            scripts: HashMap::new(),
        }
    }

    /// Load cache from file with automatic repair on parse errors
    pub fn load(path: &PathBuf) -> Result<Self> {
        use crate::core::repair::{
            print_repair_warning, try_parse_json, RepairAction, RepairSeverity,
        };

        // Handle missing file (existing behavior)
        if !path.exists() {
            return Ok(Self::new());
        }

        // Read file content
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read cache: {}", path.display()))?;

        // Try to parse JSON
        match try_parse_json::<Self>(&content, path) {
            Ok(cache) => Ok(cache),
            Err(parse_error) => {
                log::warn!("Failed to parse manifest-cache.json: {}", parse_error);

                // Delete corrupted cache file
                let _ = fs::remove_file(path);

                // Notify user (informational - cache will be rebuilt automatically)
                let action = RepairAction::Rebuilt {
                    source: "configured buckets".to_string(),
                };
                print_repair_warning(
                    "manifest-cache.json",
                    &action,
                    RepairSeverity::Info,
                    Some("Cache will be rebuilt from buckets on next operation."),
                );

                // Return empty cache (will trigger rebuild)
                Ok(Self::new())
            }
        }
    }

    /// Save cache to file
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create cache directory: {}", parent.display())
            })?;
        }

        let content = serde_json::to_string(self).context("Failed to serialize cache")?;

        fs::write(path, content)
            .with_context(|| format!("Failed to write cache: {}", path.display()))
    }

    /// Check if cache is valid (not expired)
    pub fn is_valid(&self) -> bool {
        let age = Utc::now() - self.last_updated;
        age.num_seconds() < self.ttl_seconds
    }

    /// Add a package to cache
    pub fn add_package(&mut self, package: Package, source: PackageSource) {
        let repo = package.repo.clone();
        self.packages
            .insert(repo, CachedPackage { package, source });
    }

    /// Add a script to cache
    pub fn add_script(&mut self, script: ScriptItem, source: PackageSource) {
        let name = script.name.clone();
        self.scripts.insert(name, CachedScript { script, source });
    }

    /// Get all packages as Vec (for compatibility with SourceManifest)
    pub fn get_packages(&self) -> Vec<Package> {
        self.packages
            .values()
            .map(|cp| cp.package.clone())
            .collect()
    }

    /// Get all scripts as Vec
    pub fn get_scripts(&self) -> Vec<ScriptItem> {
        self.scripts.values().map(|cs| cs.script.clone()).collect()
    }

    /// Convert cache to SourceManifest for compatibility
    pub fn to_source_manifest(&self) -> SourceManifest {
        SourceManifest {
            packages: self.get_packages(),
            scripts: self.get_scripts(),
        }
    }

    /// Find a package by name
    #[allow(dead_code)]
    pub fn find_package(&self, name: &str) -> Option<&CachedPackage> {
        self.packages.values().find(|cp| cp.package.name == name)
    }

    /// Build a name → cached package index for bulk name-based lookups.
    ///
    /// The `packages` map is keyed by repo URL, but update/check flows look
    /// packages up by name. Building this index once turns repeated O(n×m)
    /// linear scans into O(n + m). When duplicate names exist across buckets,
    /// the first one encountered wins (matching `find_package` semantics).
    pub fn packages_by_name(&self) -> HashMap<&str, &CachedPackage> {
        let mut index = HashMap::with_capacity(self.packages.len());
        for cp in self.packages.values() {
            index.entry(cp.package.name.as_str()).or_insert(cp);
        }
        index
    }

    /// Find a script by name
    #[allow(dead_code)]
    pub fn find_script(&self, name: &str) -> Option<&CachedScript> {
        self.scripts.get(name)
    }

    /// Get packages filtered by source
    #[allow(dead_code)]
    pub fn packages_by_source(&self, source_type: &PackageSource) -> Vec<&CachedPackage> {
        self.packages
            .values()
            .filter(|cp| &cp.source == source_type)
            .collect()
    }

    /// Get scripts filtered by source
    #[allow(dead_code)]
    pub fn scripts_by_source(&self, source_type: &PackageSource) -> Vec<&CachedScript> {
        self.scripts
            .values()
            .filter(|cs| &cs.source == source_type)
            .collect()
    }
}

impl Default for ManifestCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_cache_from_results(
    buckets_with_results: Vec<(Bucket, Result<SourceManifest>)>,
) -> ManifestCache {
    let mut cache = ManifestCache::new();
    cache.last_updated = Utc::now();

    for (bucket, result) in buckets_with_results {
        let source_key = format!("bucket:{}", bucket.name);
        let now = Utc::now();

        match result {
            Ok(manifest) => {
                let package_count = manifest.packages.len();
                let script_count = manifest.scripts.len();
                let total_count = package_count + script_count;

                for package in manifest.packages {
                    cache.add_package(
                        package,
                        PackageSource::Bucket {
                            name: bucket.name.clone(),
                        },
                    );
                }

                for script in manifest.scripts {
                    cache.add_script(
                        script,
                        PackageSource::Bucket {
                            name: bucket.name.clone(),
                        },
                    );
                }

                cache.sources.insert(
                    source_key,
                    CachedSourceInfo {
                        source: PackageSource::Bucket {
                            name: bucket.name.clone(),
                        },
                        package_count: total_count,
                        last_fetched: Some(now),
                        url: Some(bucket.url.clone()),
                    },
                );
            }
            Err(e) => {
                log::warn!("Failed to fetch bucket '{}': {}", bucket.name, e);
            }
        }
    }

    cache
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_cache_new() {
        let cache = ManifestCache::new();
        assert_eq!(cache.version, "1.0");
        assert_eq!(cache.packages.len(), 0);
        assert_eq!(cache.sources.len(), 0);
    }

    #[test]
    fn test_add_package() {
        let mut cache = ManifestCache::new();

        let package = Package {
            name: "test".to_string(),
            description: "Test package".to_string(),
            repo: "https://github.com/test/test".to_string(),
            homepage: None,
            license: None,
            version: None,
            platforms: HashMap::new(),
        };

        let source = PackageSource::Bucket {
            name: "test-bucket".to_string(),
        };
        cache.add_package(package.clone(), source.clone());
        assert_eq!(cache.packages.len(), 1);

        let cached = cache.find_package("test").unwrap();
        assert_eq!(cached.package.name, "test");
        assert_eq!(cached.source, source);
    }

    #[test]
    fn test_is_valid() {
        let mut cache = ManifestCache::new();

        // Fresh cache should be valid
        assert!(cache.is_valid());

        // Expired cache should be invalid
        cache.last_updated = Utc::now() - chrono::Duration::days(2);
        assert!(!cache.is_valid());
    }
}
