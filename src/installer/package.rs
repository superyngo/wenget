//! Headless package installation logic
//!
//! Decisions that do not need a terminal: which release to install, which of a
//! platform's binaries apply. The `add` command renders and prompts around them.

use anyhow::Result;

use crate::core::manifest::{PackageSource, PlatformBinary};
use crate::core::Package;

/// How the target release was obtained
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetStatus {
    /// From the GitHub API, or data already trusted (update mode, direct repo, derived URLs)
    Fresh,
    /// The GitHub API failed; installing from the cached (bucket) download links
    Cached,
    /// No usable release: the error to report at install time
    Failed(String),
}

/// The release chosen for one resolved package
#[derive(Debug, Clone)]
pub struct Target {
    pub package: Package,
    pub version: String,
    pub status: TargetStatus,
}

fn version_or_unknown(pkg: &Package) -> String {
    pkg.version.clone().unwrap_or_else(|| "unknown".to_string())
}

/// Choose the release to install for `cached`, calling `fetch(version)` for the
/// GitHub API (`None` = latest).
///
/// - `custom_version`: fetch that release; if the API fails, derive its URLs from the
///   cached ones; if that is impossible the target is `Failed`.
/// - Update mode on a bucket package, or a direct repo URL: the cached data was just
///   refreshed, so no API call.
/// - Otherwise: the latest release, falling back to the cached links.
pub fn target_package(
    cached: &Package,
    source: &PackageSource,
    custom_version: Option<&str>,
    update_mode: bool,
    fetch: impl Fn(Option<&str>) -> Result<Package>,
) -> Target {
    if let Some(custom) = custom_version {
        let version = custom.trim_start_matches('v').to_string();
        return match fetch(Some(custom)) {
            Ok(package) => Target {
                package,
                version,
                status: TargetStatus::Fresh,
            },
            Err(e) => match derive_versioned_package(cached, custom) {
                Some(package) => Target {
                    package,
                    version,
                    status: TargetStatus::Fresh,
                },
                None => Target {
                    package: cached.clone(),
                    version,
                    status: TargetStatus::Failed(e.to_string()),
                },
            },
        };
    }

    let trusted = matches!(source, PackageSource::DirectRepo { .. })
        || (update_mode && matches!(source, PackageSource::Bucket { .. }));
    if trusted {
        return Target {
            package: cached.clone(),
            version: version_or_unknown(cached),
            status: TargetStatus::Fresh,
        };
    }

    match fetch(None) {
        Ok(package) => Target {
            version: version_or_unknown(&package),
            package,
            status: TargetStatus::Fresh,
        },
        Err(e) => {
            log::warn!(
                "Failed to fetch latest package info from GitHub API for {}: {}",
                cached.name,
                e
            );
            Target {
                package: cached.clone(),
                version: version_or_unknown(cached),
                status: TargetStatus::Cached,
            }
        }
    }
}

/// Narrow a platform's binaries to the ones this install applies to.
///
/// In update mode the previously installed asset name (`stored_asset`) is matched as
/// a version-free template first; this survives stale or misdetected variant names.
/// Then the named `variant` filter applies; with neither, every binary is kept.
pub fn filter_binaries(
    binaries: &[PlatformBinary],
    pkg_name: &str,
    stored_asset: Option<&str>,
    variant: Option<&str>,
) -> Vec<PlatformBinary> {
    if let Some(template) = stored_asset.map(normalize_asset_for_matching) {
        let matched: Vec<_> = binaries
            .iter()
            .filter(|b| normalize_asset_for_matching(&b.asset_name) == template)
            .cloned()
            .collect();
        if !matched.is_empty() {
            return matched;
        }
    }
    match variant {
        Some(filter) => binaries
            .iter()
            .filter(|b| {
                crate::core::manifest::extract_variant_from_asset(&b.asset_name, pkg_name)
                    .as_deref()
                    == Some(filter)
            })
            .cloned()
            .collect(),
        None => binaries.to_vec(),
    }
}

/// Normalize an asset filename for template-based matching across versions.
///
/// Strips file extensions and version-like segments so that the same binary
/// across different releases produces the same template string.
///
/// # Examples
/// - `uv-x86_64-unknown-linux-gnu.tar.gz` → `uv-x86-64-unknown-linux-gnu`
/// - `gh_copilot_1.0.22_linux_amd64.tar.gz` → `gh-copilot-linux-amd64`
/// - `ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz` → `ripgrep-x86-64-unknown-linux-musl`
pub fn normalize_asset_for_matching(asset_name: &str) -> String {
    let name = asset_name
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".zip")
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".exe")
        .trim_end_matches(".7z");

    // Split on both - and _, filter out version segments, rejoin
    name.split(['-', '_'])
        .filter(|seg| {
            if seg.is_empty() {
                return false;
            }
            let s = seg.trim_start_matches('v');
            // A version segment starts with a digit AND contains a dot (e.g. 1.0.22, v0.11.6)
            !(s.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && s.contains('.'))
        })
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Derive a package for a specific version by rewriting the cached download URLs.
///
/// GitHub release assets always live at `.../releases/download/{tag}/{asset_name}`,
/// so replacing the cached version substring with the requested version yields the URL
/// for that version. This covers both the version in the tag path and any version
/// embedded in the asset name, and preserves a `v` prefix because only the numeric
/// part is swapped (e.g. `v0.8.1/foo` -> `v2.0.0/foo`).
///
/// This is a best-effort fallback used only when the GitHub API is unavailable: it
/// fails (the download later 404s) if the project uses a tag scheme that doesn't
/// contain the version, or changed its asset naming between versions.
///
/// Returns `None` when the cached version can't be used as a substitution anchor.
pub fn derive_versioned_package(
    cached: &crate::core::Package,
    requested_version: &str,
) -> Option<crate::core::Package> {
    let old_ver = cached.version.as_deref()?.trim_start_matches('v');
    let new_ver = requested_version.trim_start_matches('v');

    if old_ver.is_empty() || old_ver == "unknown" || old_ver == "local" {
        return None;
    }

    let platforms = cached
        .platforms
        .iter()
        .map(|(platform_id, binaries)| {
            let rewritten = binaries
                .iter()
                .map(|b| crate::core::manifest::PlatformBinary {
                    url: b.url.replace(old_ver, new_ver),
                    size: 0,        // unknown for a derived URL
                    checksum: None, // cached checksum is for a different version
                    asset_name: b.asset_name.replace(old_ver, new_ver),
                })
                .collect();
            (platform_id.clone(), rewritten)
        })
        .collect();

    Some(crate::core::Package {
        name: cached.name.clone(),
        description: cached.description.clone(),
        repo: cached.repo.clone(),
        homepage: cached.homepage.clone(),
        license: cached.license.clone(),
        version: Some(new_ver.to_string()),
        platforms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cached_pkg(version: &str, url: &str, asset_name: &str) -> Package {
        let mut platforms = HashMap::new();
        platforms.insert(
            "linux-armv7".to_string(),
            vec![PlatformBinary {
                url: url.to_string(),
                size: 123,
                checksum: Some("abc".to_string()),
                asset_name: asset_name.to_string(),
            }],
        );
        Package {
            name: "sshi".to_string(),
            description: "desc".to_string(),
            repo: "https://github.com/superyngo/sshi".to_string(),
            homepage: None,
            license: None,
            version: Some(version.to_string()),
            platforms,
        }
    }

    #[test]
    fn test_derive_versioned_package_version_in_tag_only() {
        // sshi-style: version only in the tag path, not the asset name.
        let cached = cached_pkg(
            "1.2.0",
            "https://github.com/superyngo/sshi/releases/download/v1.2.0/sshi-linux-armv7.tar.gz",
            "sshi-linux-armv7.tar.gz",
        );
        let derived = derive_versioned_package(&cached, "1.3.0").unwrap();
        let bin = &derived.platforms["linux-armv7"][0];
        assert_eq!(
            bin.url,
            "https://github.com/superyngo/sshi/releases/download/v1.3.0/sshi-linux-armv7.tar.gz"
        );
        assert_eq!(bin.asset_name, "sshi-linux-armv7.tar.gz");
        assert_eq!(derived.version.as_deref(), Some("1.3.0"));
        assert!(bin.checksum.is_none());
    }

    #[test]
    fn test_derive_versioned_package_version_in_asset_name() {
        // Nexus-style: version in both the tag and the asset name.
        let cached = cached_pkg(
            "0.8.1",
            "https://github.com/x/y/releases/download/v0.8.1/Setup_0.8.1.exe",
            "Setup_0.8.1.exe",
        );
        let derived = derive_versioned_package(&cached, "v2.0.0").unwrap();
        let bin = &derived.platforms["linux-armv7"][0];
        assert_eq!(
            bin.url,
            "https://github.com/x/y/releases/download/v2.0.0/Setup_2.0.0.exe"
        );
        assert_eq!(bin.asset_name, "Setup_2.0.0.exe");
    }

    #[test]
    fn test_derive_versioned_package_rejects_unusable_version() {
        let cached = cached_pkg(
            "local",
            "https://github.com/x/y/releases/download/v1/y.tar.gz",
            "y.tar.gz",
        );
        assert!(derive_versioned_package(&cached, "1.0.0").is_none());

        let mut no_version = cached_pkg(
            "1.0.0",
            "https://github.com/x/y/releases/download/v1.0.0/y.tar.gz",
            "y.tar.gz",
        );
        no_version.version = None;
        assert!(derive_versioned_package(&no_version, "1.0.0").is_none());
    }

    #[test]
    fn test_normalize_asset_for_matching() {
        // Same binary across versions should produce identical templates
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            "uv-x86-64-unknown-linux-gnu"
        );
        assert_eq!(
            normalize_asset_for_matching("gh_copilot_1.0.21_linux_amd64.tar.gz"),
            normalize_asset_for_matching("gh_copilot_1.0.22_linux_amd64.tar.gz")
        );
        assert_eq!(
            normalize_asset_for_matching("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"),
            normalize_asset_for_matching("ripgrep-14.1.2-x86_64-unknown-linux-musl.tar.gz")
        );
        // Same asset produces same template
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz")
        );
        // Different arch should NOT match
        assert_ne!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-unknown-linux-gnu.tar.gz")
        );
        // zip extension
        assert_eq!(
            normalize_asset_for_matching("bun-linux-x64.zip"),
            "bun-linux-x64"
        );
        // apple stays — template matching uses full normalized name for comparison
        assert_eq!(
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz")
        );
    }

    fn bin(asset: &str) -> PlatformBinary {
        PlatformBinary {
            url: format!("https://x/{}", asset),
            size: 1,
            checksum: None,
            asset_name: asset.to_string(),
        }
    }

    fn bucket() -> PackageSource {
        PackageSource::Bucket {
            name: "b".to_string(),
        }
    }

    #[test]
    fn target_uses_latest_release() {
        let cached = cached_pkg("1.0.0", "u", "a");
        let t = target_package(&cached, &bucket(), None, false, |v| {
            assert_eq!(v, None);
            Ok(cached_pkg("2.0.0", "u", "a"))
        });
        assert_eq!(
            (t.version.as_str(), t.status),
            ("2.0.0", TargetStatus::Fresh)
        );
    }

    #[test]
    fn target_falls_back_to_cache_when_api_fails() {
        let cached = cached_pkg("1.0.0", "u", "a");
        let t = target_package(&cached, &bucket(), None, false, |_| anyhow::bail!("rate"));
        assert_eq!(
            (t.version.as_str(), t.status),
            ("1.0.0", TargetStatus::Cached)
        );
    }

    #[test]
    fn target_update_mode_bucket_skips_api() {
        let cached = cached_pkg("1.0.0", "u", "a");
        let t = target_package(&cached, &bucket(), None, true, |_| panic!("no API call"));
        assert_eq!(t.version, "1.0.0");
    }

    #[test]
    fn target_custom_version_derives_then_fails() {
        let cached = cached_pkg("1.0.0", "https://x/v1.0.0/a", "a");
        let t = target_package(&cached, &bucket(), Some("v1.1.0"), false, |v| {
            assert_eq!(v, Some("v1.1.0"));
            anyhow::bail!("down")
        });
        assert_eq!(t.version, "1.1.0");
        assert_eq!(
            t.package.platforms["linux-armv7"][0].url,
            "https://x/v1.1.0/a"
        );

        let unusable = cached_pkg("unknown", "u", "a");
        let t = target_package(&unusable, &bucket(), Some("1.1.0"), false, |_| {
            anyhow::bail!("down")
        });
        assert_eq!(t.status, TargetStatus::Failed("down".to_string()));
    }

    #[test]
    fn filter_prefers_stored_asset_template() {
        let bins = [bin("bun-linux-x64-baseline.zip"), bin("bun-linux-x64.zip")];
        let got = filter_binaries(&bins, "bun", Some("bun-linux-x64.zip"), Some("baseline"));
        assert_eq!(got, vec![bins[1].clone()]);
    }

    #[test]
    fn filter_by_variant_or_all() {
        let bins = [bin("bun-linux-x64-baseline.zip"), bin("bun-linux-x64.zip")];
        let got = filter_binaries(&bins, "bun", Some("gone-1.0.zip"), Some("baseline"));
        assert_eq!(got, vec![bins[0].clone()]);
        assert!(filter_binaries(&bins, "bun", None, Some("nope")).is_empty());
        assert_eq!(filter_binaries(&bins, "bun", None, None).len(), 2);
    }
}
