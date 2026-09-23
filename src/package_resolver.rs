//! Package resolver - Smart detection and resolution of package inputs
//!
//! This module provides utilities for:
//! - Detecting whether an input is a package name or GitHub URL
//! - Fetching package information from cache or GitHub
//! - Determining the bucket source of cached packages

use crate::cache::ManifestCache;
use crate::core::manifest::{Package, PackageSource};
use crate::core::Config;
use crate::providers::GitHubProvider;
use anyhow::{anyhow, Context, Result};

/// Represents the type of package input
#[derive(Debug, Clone)]
pub enum PackageInput {
    /// Package name from cache (supports glob patterns)
    CacheName(String),
    /// Direct GitHub repository URL
    DirectUrl(String),
}

impl PackageInput {
    /// Parse an input string and detect if it's a URL or package name
    pub fn parse(input: &str) -> Self {
        // Check if input looks like a URL
        if input.starts_with("http://")
            || input.starts_with("https://")
            || input.starts_with("github.com/")
        {
            Self::DirectUrl(normalize_github_url(input))
        } else {
            Self::CacheName(input.to_string())
        }
    }
}

/// Normalize GitHub URL to standard format
fn normalize_github_url(url: &str) -> String {
    let mut url = url.trim().to_string();

    // Upgrade http:// to https://
    if url.starts_with("http://github.com/") {
        url = url.replacen("http://", "https://", 1);
    }

    // Add https:// if missing
    if url.starts_with("github.com/") {
        url = format!("https://{}", url);
    }

    // Remove trailing slash
    while url.ends_with('/') {
        url.pop();
    }

    // Remove .git suffix
    if url.ends_with(".git") {
        url.truncate(url.len() - 4);
    }

    url
}

/// Result of package resolution with source information
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    /// The package information
    pub package: Package,
    /// The source of this package
    pub source: PackageSource,
}

impl ResolvedPackage {
    /// Create a new resolved package
    pub fn new(package: Package, source: PackageSource) -> Self {
        Self { package, source }
    }
}

/// Package resolver for fetching package information
pub struct PackageResolver<'a> {
    config: &'a Config,
    cache: &'a ManifestCache,
    github: GitHubProvider,
}

impl<'a> PackageResolver<'a> {
    /// Create a new package resolver with pre-loaded cache
    pub fn new(config: &'a Config, cache: &'a ManifestCache) -> Result<Self> {
        let github = GitHubProvider::new()?;
        Ok(Self {
            config,
            cache,
            github,
        })
    }

    /// Resolve package(s) from input
    ///
    /// Returns a list of resolved packages with their sources.
    /// For cache names, supports glob patterns and may return multiple matches.
    /// For URLs, returns a single package.
    pub fn resolve(&self, input: &PackageInput) -> Result<Vec<ResolvedPackage>> {
        match input {
            PackageInput::CacheName(name) => self.resolve_from_cache(name),
            PackageInput::DirectUrl(url) => {
                let pkg = self.resolve_from_url(url)?;
                Ok(vec![pkg])
            }
        }
    }

    /// Resolve package from cache (supports glob patterns)
    /// Falls back to checking installed packages if not found in cache
    fn resolve_from_cache(&self, name: &str) -> Result<Vec<ResolvedPackage>> {
        // Handle repo::variant format - extract base name for cache lookup
        let base_name = if let Some(pos) = name.find("::") {
            &name[..pos]
        } else {
            name
        };

        // Filter packages by name pattern
        let matches: Vec<_> = if base_name.contains('*') {
            // Glob pattern matching
            self.cache
                .packages
                .values()
                .filter(|cached| glob_match(&cached.package.name, base_name))
                .collect()
        } else {
            // Exact name matching
            self.cache
                .packages
                .values()
                .filter(|cached| cached.package.name == base_name)
                .collect()
        };

        if !matches.is_empty() {
            // Found in cache - return these matches
            return Ok(matches
                .into_iter()
                .map(|cached| ResolvedPackage::new(cached.package.clone(), cached.source.clone()))
                .collect());
        }

        // Not found in cache - check if it's an installed package from direct URL
        // Note: Only check for exact name match, not glob patterns
        if !name.contains('*') {
            let installed = self.config.get_or_create_installed()?;
            if let Some(inst_pkg) = installed.get_package(name) {
                // Check if it's a DirectRepo source
                if let PackageSource::DirectRepo { url } = &inst_pkg.source {
                    // Fetch the package info from the URL
                    return self.resolve_from_url(url).map(|pkg| vec![pkg]);
                }
            }
        }

        // Provide more detailed error message
        let cache_pkg_count = self.cache.packages.len();
        let bucket_count = self.cache.sources.len();

        if cache_pkg_count == 0 {
            if bucket_count == 0 {
                Err(anyhow!(
                    "No packages found matching '{}'. No buckets configured. Run 'wenget bucket add' to add a bucket.",
                    name
                ))
            } else {
                Err(anyhow!(
                    "No packages found matching '{}'. Cache is empty. Run 'wenget bucket refresh' to rebuild cache.",
                    name
                ))
            }
        } else if name.contains('*') {
            Err(anyhow!(
                "No packages found matching pattern '{}'. {} packages available in cache.",
                name,
                cache_pkg_count
            ))
        } else {
            let suggestions = crate::core::fuzzy::suggest(
                base_name,
                self.cache
                    .packages
                    .values()
                    .map(|c| c.package.name.as_str()),
                3,
            );
            if suggestions.is_empty() {
                Err(anyhow!(
                    "Package '{}' not found. Use 'wenget search {}' to find similar packages.",
                    name,
                    name
                ))
            } else {
                Err(anyhow!(
                    "Package '{}' not found. Did you mean: {}?",
                    name,
                    suggestions.join(", ")
                ))
            }
        }
    }

    /// Resolve package from GitHub URL
    fn resolve_from_url(&self, url: &str) -> Result<ResolvedPackage> {
        let package = self
            .github
            .fetch_package(url, None, None)
            .with_context(|| format!("Failed to fetch package from: {}", url))?;

        let source = PackageSource::DirectRepo {
            url: url.to_string(),
        };

        Ok(ResolvedPackage::new(package, source))
    }

    /// Get the latest version from GitHub for a package
    pub fn fetch_latest_version(&self, repo_url: &str) -> Result<String> {
        self.github.fetch_latest_version(repo_url)
    }
}

/// Simple glob pattern matching (supports * wildcard)
///
/// Examples:
/// - `glob_match("ripgrep", "rip*")` -> true
/// - `glob_match("ripgrep", "*grep")` -> true
/// - `glob_match("ripgrep", "r*g*p")` -> true
/// - `glob_match("ripgrep", "*")` -> true
/// - `glob_match("ab", "a*b*")` -> true
fn glob_match(text: &str, pattern: &str) -> bool {
    // Split pattern by '*'
    let parts: Vec<&str> = pattern.split('*').collect();

    if parts.len() == 1 {
        // No wildcard, exact match
        return text == pattern;
    }

    let mut pos = 0;
    let mut first_non_empty = true;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }

        let is_last = i == parts.len() - 1;
        let is_first_match = first_non_empty;
        first_non_empty = false;

        if is_first_match && !pattern.starts_with('*') {
            // First non-empty part and pattern doesn't start with '*'
            // -> must match at the start
            if !text.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if is_last && !pattern.ends_with('*') {
            // Last non-empty part and pattern doesn't end with '*'
            // -> must match at the end
            if !text[pos..].ends_with(part) {
                return false;
            }
            // Also check that the part can be found after current position
            if let Some(found_pos) = text[pos..].rfind(part) {
                // Ensure the found position allows the suffix match
                if pos + found_pos + part.len() != text.len() {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            // Middle parts or parts with trailing wildcard - just need to exist in order
            if let Some(found_pos) = text[pos..].find(part) {
                pos += found_pos + part.len();
            } else {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_package_input() {
        assert!(matches!(
            PackageInput::parse("ripgrep"),
            PackageInput::CacheName(_)
        ));
        assert!(matches!(
            PackageInput::parse("https://github.com/user/repo"),
            PackageInput::DirectUrl(_)
        ));
        assert!(matches!(
            PackageInput::parse("github.com/user/repo"),
            PackageInput::DirectUrl(_)
        ));
        assert!(matches!(
            PackageInput::parse("http://github.com/user/repo"),
            PackageInput::DirectUrl(_)
        ));
    }

    #[test]
    fn test_normalize_github_url() {
        // Basic cases
        assert_eq!(
            normalize_github_url("github.com/user/repo"),
            "https://github.com/user/repo"
        );
        assert_eq!(
            normalize_github_url("https://github.com/user/repo"),
            "https://github.com/user/repo"
        );

        // HTTP upgrade to HTTPS
        assert_eq!(
            normalize_github_url("http://github.com/user/repo"),
            "https://github.com/user/repo"
        );

        // Trailing slash removal
        assert_eq!(
            normalize_github_url("https://github.com/user/repo/"),
            "https://github.com/user/repo"
        );
        assert_eq!(
            normalize_github_url("https://github.com/user/repo///"),
            "https://github.com/user/repo"
        );

        // .git suffix removal
        assert_eq!(
            normalize_github_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo"
        );

        // Combined: trailing slash and .git
        assert_eq!(
            normalize_github_url("github.com/user/repo.git"),
            "https://github.com/user/repo"
        );

        // Whitespace trimming
        assert_eq!(
            normalize_github_url("  https://github.com/user/repo  "),
            "https://github.com/user/repo"
        );
    }

    #[test]
    fn test_glob_match() {
        // Exact match
        assert!(glob_match("ripgrep", "ripgrep"));

        // Trailing wildcard
        assert!(glob_match("ripgrep", "rip*"));
        assert!(glob_match("ripgrep", "ripgrep*"));
        assert!(glob_match("ab", "a*"));

        // Leading wildcard
        assert!(glob_match("ripgrep", "*grep"));
        assert!(glob_match("ripgrep", "*ripgrep"));
        assert!(glob_match("ab", "*b"));

        // Middle wildcard
        assert!(glob_match("ripgrep", "r*p"));
        assert!(glob_match("ripgrep", "rip*rep"));

        // Multiple wildcards
        assert!(glob_match("ripgrep", "r*p*p"));
        assert!(glob_match("ripgrep", "*i*g*"));
        assert!(glob_match("ab", "a*b*"));
        assert!(glob_match("abc", "a*b*c"));
        assert!(glob_match("aXbYc", "a*b*c"));

        // Match all
        assert!(glob_match("ripgrep", "*"));
        assert!(glob_match("", "*"));

        // No match cases
        assert!(!glob_match("ripgrep", "rip"));
        assert!(!glob_match("ripgrep", "grep"));
        assert!(!glob_match("ripgrep", "bat*"));
        assert!(!glob_match("ripgrep", "*bat"));
        assert!(!glob_match("abc", "a*d*c"));

        // Edge cases
        assert!(glob_match("aaa", "a*a"));
        assert!(glob_match("abab", "*ab"));
        assert!(glob_match("abab", "ab*"));
    }
}
