//! Headless package installation logic
//!
//! Decisions that do not need a terminal: which release to install, which of a
//! platform's binaries apply. The `add` command renders and prompts around them.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;

use crate::core::manifest::{PackageSource, PlatformBinary};
use crate::core::platform::PlatformMatch;
use crate::core::{InstalledPackage, InstalledSet, Package, WenPaths};
use crate::downloader;
use crate::installer::extractor::ExecutableCandidate;
use crate::installer::{extract_archive, find_executable_candidates, normalize_command_name};

#[cfg(windows)]
use crate::installer::create_shim;
#[cfg(unix)]
use crate::installer::create_symlink;

/// The interaction an install needs from its front end.
///
/// The terminal implementation lives in `utils::prompt::TerminalUi`; tests script
/// the answers.
pub trait InstallUi {
    /// Show one progress or status line
    fn line(&self, msg: &str);
    /// Ask a yes/no question; `default` is the answer on empty input
    fn confirm(&self, prompt: &str, default: bool) -> Result<bool>;
    /// Pick one of `items`
    fn select(&self, prompt: &str, items: &[String], default: usize) -> Result<usize>;
    /// Pick any number of `items`
    fn multi_select(&self, prompt: &str, items: &[String]) -> Result<Vec<usize>>;
}

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

/// A single package install, bound to its front end and command-line choices
pub struct PackageInstaller<'a> {
    pub paths: &'a WenPaths,
    pub ui: &'a dyn InstallUi,
    /// `-c`: name for the first executable
    pub command_name: Option<&'a str>,
    pub yes: bool,
    pub no_suffix: bool,
    /// Keep the previously installed executables and command names
    pub update_mode: bool,
}

/// What to install: one binary of one release
#[derive(Clone, Copy)]
pub struct InstallRequest<'a> {
    pub package: &'a Package,
    pub platform_match: &'a PlatformMatch,
    pub binary: &'a PlatformBinary,
    pub version: &'a str,
    pub source: &'a PackageSource,
    pub installed_key: &'a str,
}

impl PackageInstaller<'_> {
    /// Download, verify, stage and swap in one binary, then create its launchers.
    ///
    /// `installed` is the caller's in-memory snapshot of the installed set, used for
    /// executable reuse in update mode and command-name conflicts. It must NOT be
    /// re-read from disk here; the caller persists the returned record.
    pub fn install(
        &self,
        installed: &InstalledSet,
        req: &InstallRequest,
    ) -> Result<InstalledPackage> {
        let InstallRequest {
            package: pkg,
            platform_match,
            binary,
            version,
            source,
            installed_key,
        } = *req;
        let (paths, ui) = (self.paths, self.ui);
        let custom_name = self.command_name;
        let (no_suffix, update_mode) = (self.no_suffix, self.update_mode);

        // Log if using fallback
        if let Some(fallback_type) = &platform_match.fallback_type {
            log::info!(
                "Using fallback platform {} ({})",
                platform_match.platform_id,
                fallback_type.description()
            );
        }

        // Download binary
        ui.line(&format!("  Downloading from {}...", binary.url));

        let download_dir = paths.downloads_dir();
        fs::create_dir_all(&download_dir)?;

        // The asset name is the release file name; sanitized so it stays in download_dir
        let filename = crate::core::paths::sanitize_path_component(&binary.asset_name);
        let download_path = download_dir.join(filename);

        // Removes the archive on every exit path, including errors below
        let _download_guard = downloader::CleanupGuard::new(&download_path);
        downloader::download_file(&binary.url, &download_path)?;

        crate::core::checksum::verify_download(&binary.url, &binary.asset_name, &download_path)?;

        // Sanitized directory names are lossy, so a different package may already
        // own the directory this key maps to.
        crate::core::InstalledStore::new(paths.clone()).ensure_dir_available(installed_key)?;

        // Stage the extraction beside the app directory and swap it in on success, so
        // a failed install leaves the previous install and its record untouched.
        let staged = crate::installer::StagedInstall::begin(paths, installed_key)?;
        let app_dir = staged.target().to_path_buf();

        ui.line(&format!("  Extracting to {}...", app_dir.display()));

        let extracted_files = extract_archive(&download_path, staged.path())?;

        // Find executable candidates (pass the staging dir for Unix permission checks)
        let candidates =
            find_executable_candidates(&extracted_files, &pkg.name, Some(staged.path()));

        if candidates.is_empty() {
            anyhow::bail!(
                "Failed to find executable in archive. Extracted files:\n{}",
                extracted_files.join("\n")
            );
        }

        let selected_executables =
            self.select_executables(installed, installed_key, &candidates)?;

        // Install all selected executables
        let mut executables: HashMap<String, String> = HashMap::new();

        // Extract repo_name and variant from installed_key for resolve_command_name
        // installed_key format: "repo_name" or "repo_name::variant"
        let (_, variant_opt) = if let Some(pos) = installed_key.find("::") {
            (
                installed_key[..pos].to_string(),
                if no_suffix {
                    None
                } else {
                    Some(installed_key[pos + 2..].to_string())
                },
            )
        } else {
            (installed_key.to_string(), None)
        };

        // If this package is already installed, grab old executables for command name reuse
        let old_executables = installed
            .get_package(installed_key)
            .map(|p| p.executables.clone());

        // Precompute the set of command names already in use (excluding this package)
        // so per-executable conflict checks are O(1) instead of scanning all packages
        // for every candidate suffix in `resolve_command_name`.
        let mut taken_names = installed.command_name_set(Some(installed_key));

        // Resolve every command name before the swap: anything that can fail here must
        // fail while the previous install and its record are still in place.
        let mut launchers: Vec<(String, String)> = Vec::new(); // (exe_relative, command name)
        for exe_relative in selected_executables {
            let staged_exe = staged.path().join(&exe_relative);

            if !staged_exe.exists() {
                anyhow::bail!("Executable not found: {}", exe_relative);
            }

            // When updating, try to reuse old command names
            let reused_name = if update_mode {
                if let Some(old_exes) = &old_executables {
                    let filename = staged_exe
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");

                    // Try path match first, then filename match
                    old_exes.get(&exe_relative).cloned().or_else(|| {
                        old_exes
                            .iter()
                            .find(|(old_path, _)| {
                                std::path::Path::new(old_path.as_str())
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    == Some(filename)
                            })
                            .map(|(_, name)| name.clone())
                    })
                } else {
                    None
                }
            } else {
                None
            };

            let resolved_name = if let Some(reused) = reused_name {
                ui.line(&format!("  Reusing command name: {}", reused));
                reused
            } else {
                // Extract the actual command name from the executable path
                let (base_name, is_custom) = if let Some(custom) = custom_name {
                    // Use custom name if provided (only for first executable)
                    if launchers.is_empty() {
                        (custom.to_string(), true)
                    } else {
                        // For additional executables, use auto-detected name
                        let raw_name = staged_exe
                            .file_name()
                            .and_then(|s| s.to_str())
                            .context("Failed to extract command name")?;
                        (normalize_command_name(raw_name), false)
                    }
                } else {
                    // Auto-detect and normalize command name
                    let raw_name = staged_exe
                        .file_name()
                        .and_then(|s| s.to_str())
                        .context("Failed to extract command name")?;

                    // Apply smart normalization to remove platform suffixes
                    (normalize_command_name(raw_name), false)
                };

                // Resolve command name with variant to avoid conflicts
                resolve_command_name(&base_name, variant_opt.as_deref(), &taken_names, is_custom)
            };

            ui.line(&format!(
                "  Command will be available as: {}",
                resolved_name
            ));

            // Record the name as taken so subsequent executables in the same package
            // don't resolve to a colliding name.
            taken_names.insert(resolved_name.clone());
            launchers.push((exe_relative, resolved_name));
        }

        // Every remaining step reads from the final location: swap now, so the
        // launchers point at the app directory rather than the staging path.
        let app_dir = staged.commit()?;

        // From here on the previous install is gone, so a launcher failure must not skip
        // writing the new record: collect failures and report them after saving it.
        let mut launcher_errors: Vec<String> = Vec::new();
        for (exe_relative, resolved_name) in launchers {
            let exe_path = app_dir.join(&exe_relative);
            let bin_path = paths.bin_shim_path(&resolved_name);

            ui.line(&format!("  Creating launcher at {}...", bin_path.display()));

            #[cfg(unix)]
            let created = create_symlink(&exe_path, &bin_path);

            #[cfg(windows)]
            let created = create_shim(&exe_path, &bin_path, &resolved_name);

            if let Err(e) = created {
                launcher_errors.push(format!("{}: {:#}", bin_path.display(), e));
            }

            executables.insert(exe_relative, resolved_name);
        }

        // Clean up symlinks/shims for old executables that no longer exist in the new version
        if let Some(ref old_exes) = old_executables {
            for old_cmd in old_exes.values() {
                if !executables.values().any(|n| n == old_cmd) {
                    let old_bin = paths.bin_shim_path(old_cmd);
                    if old_bin.exists() {
                        match fs::remove_file(&old_bin) {
                            Ok(()) => ui.line(&format!("  Removed obsolete command: {}", old_cmd)),
                            Err(e) => ui.line(&format!(
                                "  {} Could not remove obsolete command {}: {}",
                                "⚠".yellow(),
                                old_cmd,
                                e
                            )),
                        }
                    }
                }
            }
        }

        // Extract repo_name and variant from installed_key
        // installed_key format: "repo_name" or "repo_name::variant"
        let (repo_name, variant) = if let Some(pos) = installed_key.find("::") {
            (
                installed_key[..pos].to_string(),
                Some(installed_key[pos + 2..].to_string()),
            )
        } else {
            (installed_key.to_string(), None)
        };

        // Create installed package info
        let inst_pkg = InstalledPackage {
            schema_version: crate::core::manifest::CURRENT_SCHEMA_VERSION,
            repo_name,
            variant,
            version: version.to_string(),
            platform: platform_match.platform_id.clone(),
            installed_at: Utc::now(),
            install_path: app_dir.to_string_lossy().to_string(),
            executables,
            source: source.clone(),
            description: pkg.description.clone(),
            command_names: vec![],
            command_name: None,
            asset_name: binary.asset_name.clone(),
            parent_package: None, // Deprecated field
            download_url: None,
        };

        if !launcher_errors.is_empty() {
            // Keep the package tracked; `wenget repair` reports the missing launchers
            crate::core::InstalledStore::new(paths.clone())
                .save_package(installed_key, &inst_pkg)
                .with_context(|| {
                    format!("Failed to save the package record for {}", installed_key)
                })?;
            anyhow::bail!(
                "Installed {} but could not create its launcher(s): {}. Fix the path, then run \
             `wenget del {}` and `wenget add` again",
                installed_key,
                launcher_errors.join("; "),
                installed_key
            );
        }

        Ok(inst_pkg)
    }

    /// Choose which extracted executables get launchers.
    ///
    /// One candidate is taken as is. In update mode the previously installed ones are
    /// kept, a relocated one is matched by file name, and a vanished one is replaced
    /// by the user's pick (or dropped with `--yes`). Otherwise every scored candidate
    /// is taken when there are at most three (or `--yes`), else the user picks.
    fn select_executables(
        &self,
        installed: &InstalledSet,
        installed_key: &str,
        candidates: &[ExecutableCandidate],
    ) -> Result<Vec<String>> {
        let (ui, yes, update_mode) = (self.ui, self.yes, self.update_mode);
        Ok(if candidates.len() == 1 {
            // Single candidate - auto-select
            let selected = &candidates[0];
            ui.line(&format!(
                "  Found executable: {} ({})",
                selected.path, selected.reason
            ));
            vec![candidates[0].path.clone()]
        } else if update_mode {
            // Update mode: keep previously installed executables, ignore new ones,
            // prompt for replacement when old executables disappear
            let old_exes = installed
                .get_package(installed_key)
                .map(|p| p.executables.clone());

            if let Some(ref old) = old_exes {
                let old_paths: std::collections::HashSet<_> = old.keys().cloned().collect();

                // Separate: previously installed vs new candidates
                let mut kept: Vec<&ExecutableCandidate> = Vec::new();
                let mut new_candidates: Vec<&ExecutableCandidate> = Vec::new();

                for c in candidates {
                    if old_paths.contains(&c.path) {
                        kept.push(c);
                    } else if c.score > 0 {
                        new_candidates.push(c);
                    }
                }

                if kept.is_empty() && new_candidates.is_empty() {
                    ui.line(&format!(
                        "  {} No matching executables found for update, skipping {}",
                        "⚠".yellow(),
                        installed_key
                    ));
                    anyhow::bail!(
                        "No matching executables found for update of {}",
                        installed_key
                    );
                }

                let mut selected: Vec<String> = kept.iter().map(|c| c.path.clone()).collect();

                // Detect disappeared executables: old paths not found in any candidate
                let disappeared: Vec<(&String, &String)> = old
                    .iter()
                    .filter(|(path, _)| !kept.iter().any(|c| &c.path == *path))
                    .collect();

                if !disappeared.is_empty() {
                    for (old_path, old_cmd) in &disappeared {
                        let old_filename = Path::new(old_path).file_name().and_then(|s| s.to_str());

                        // Try auto-match by filename in new candidates
                        let auto_match = old_filename.and_then(|old_fname| {
                            new_candidates.iter().find(|c| {
                                Path::new(&c.path)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .map(|f| f == old_fname)
                                    .unwrap_or(false)
                            })
                        });

                        if let Some(matched) = auto_match {
                            // Auto-matched by filename — select silently
                            if !selected.contains(&matched.path) {
                                ui.line(&format!(
                                    "  {} Executable '{}' relocated to '{}' (auto-matched)",
                                    "ℹ".cyan(),
                                    old_path,
                                    matched.path
                                ));
                                selected.push(matched.path.clone());
                            }
                        } else if !new_candidates.is_empty() && !yes {
                            // No auto-match — prompt user to pick a replacement
                            ui.line(&format!(
                            "  {} Executable '{}' (command: {}) is no longer available in this release",
                            "⚠".yellow(),
                            old_path,
                            old_cmd
                        ));

                            let mut items: Vec<String> = new_candidates
                                .iter()
                                .filter(|c| !selected.contains(&c.path))
                                .map(|c| format!("{} ({})", c.path, c.reason))
                                .collect();
                            items.push("Skip (remove this command)".to_string());

                            let selection = ui.select(
                                &format!("    Select replacement for '{}'", old_cmd),
                                &items,
                                items.len() - 1,
                            )?;

                            if selection < items.len() - 1 {
                                // User picked a replacement from new candidates
                                let available: Vec<_> = new_candidates
                                    .iter()
                                    .filter(|c| !selected.contains(&c.path))
                                    .collect();
                                if selection < available.len() {
                                    selected.push(available[selection].path.clone());
                                }
                            }
                            // else: user chose "Skip" — old command will be cleaned up
                        } else {
                            // --yes mode or no new candidates: warn and auto-cleanup
                            ui.line(&format!(
                            "  {} Executable '{}' (command: {}) no longer available, will be removed",
                            "⚠".yellow(),
                            old_path,
                            old_cmd
                        ));
                        }
                    }
                }

                // New executables not in old install are silently ignored during updates

                ui.line(&format!(
                    "  Found {} executables (update mode):",
                    selected.len()
                ));
                for s in &selected {
                    let reason = candidates
                        .iter()
                        .find(|c| c.path == *s)
                        .map(|c| c.reason.as_str())
                        .unwrap_or("matched");
                    ui.line(&format!("    {} ({})", s, reason));
                }
                selected
            } else {
                // No old executables — fall through to normal auto-select
                let auto_select: Vec<_> = candidates.iter().filter(|c| c.score > 0).collect();
                ui.line(&format!("  Found {} executables:", auto_select.len()));
                for c in &auto_select {
                    ui.line(&format!("    {} ({})", c.path, c.reason));
                }
                auto_select.into_iter().map(|c| c.path.clone()).collect()
            }
        } else {
            // Multiple candidates - select all with valid scores (exec permission or name match)
            // On Unix, exec permission gives +35 score, name match gives +50
            // Files without any match get score 0 and should be filtered out
            let auto_select: Vec<_> = candidates
                .iter()
                .filter(|c| c.score > 0) // All valid candidates
                .collect();

            if auto_select.len() <= 3 || yes {
                // Auto-select if reasonable count (<=3) or --yes flag
                ui.line(&format!("  Found {} executables:", auto_select.len()));
                for c in &auto_select {
                    ui.line(&format!("    {} ({})", c.path, c.reason));
                }
                auto_select.into_iter().map(|c| c.path.clone()).collect()
            } else {
                // Too many candidates - show interactive selection
                ui.line(&format!(
                    "  Found {} possible executables:",
                    candidates.len()
                ));

                let items: Vec<String> = candidates
                    .iter()
                    .map(|c| format!("{} (score: {}, {})", c.path, c.score, c.reason))
                    .collect();

                let selections = ui.multi_select(
                    "Select executables to install (Space to select, Enter to confirm)",
                    &items,
                )?;

                if selections.is_empty() {
                    anyhow::bail!("No executables selected");
                }

                selections
                    .into_iter()
                    .map(|i| candidates[i].path.clone())
                    .collect()
            }
        })
    }
}

/// Resolve command name to avoid conflicts
///
/// Priority:
/// 1. If variant exists, use base_name-{variant}
/// 2. Otherwise, use base_name
/// 3. If name is taken, try base_name-{number}
/// 4. If is_custom is true, skip variant suffix appending and go directly to conflict checking
///
/// `taken` is the precomputed set of command names already in use (excluding the
/// package being resolved). Callers build it once via
/// `InstalledSet::command_name_set` rather than scanning all packages for
/// every candidate suffix here.
pub fn resolve_command_name(
    base_name: &str,
    variant: Option<&str>,
    taken: &std::collections::HashSet<String>,
    is_custom: bool,
) -> String {
    // 1. A custom name skips the variant suffix
    if is_custom {
        return first_free(base_name, taken);
    }

    // 2. If it's a variant, construct the desired command name
    if let Some(var) = variant {
        // Check if base_name already ends with the variant suffix
        // This handles cases where the binary itself contains the variant name
        // e.g., base_name="bun-profile", variant="profile" -> keep as "bun-profile"
        // e.g., base_name="bun-profile", variant="baseline-profile" -> change to "bun-baseline-profile"

        let desired_name = if base_name.ends_with(&format!("-{}", var)) {
            // Base name already ends with variant, use as-is
            base_name.to_string()
        } else if let Some(base_stripped) = extract_repo_name_from_command(base_name, var) {
            // Base name contains part of the variant, reconstruct with full variant
            // e.g., "bun-profile" with variant "baseline-profile" -> "bun-baseline-profile"
            format!("{}-{}", base_stripped, var)
        } else {
            // Normal case: append variant to base name
            format!("{}-{}", base_name, var)
        };

        return first_free(&desired_name, taken);
    }

    // 3. No variant
    first_free(base_name, taken)
}

/// `base` if free, else the first free `base-1` .. `base-99`, else `base`
fn first_free(base: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    (1..=99)
        .map(|i| format!("{}-{}", base, i))
        .find(|numbered| !taken.contains(numbered))
        .unwrap_or_else(|| base.to_string())
}

/// Extract repo name from a command name that may contain partial variant info
/// e.g., "bun-profile" with variant "baseline-profile" -> Some("bun")
/// e.g., "bun" with variant "baseline" -> None
fn extract_repo_name_from_command(command_name: &str, variant: &str) -> Option<String> {
    // Split variant by '-' to get all parts
    let variant_parts: Vec<&str> = variant.split('-').collect();

    // Check if command_name ends with any part of the variant
    for part in &variant_parts {
        if command_name.ends_with(&format!("-{}", part)) {
            // Strip this part and return the base
            if let Some(stripped) = command_name.strip_suffix(&format!("-{}", part)) {
                return Some(stripped.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_resolve_command_name_no_conflict() {
        let taken = std::collections::HashSet::new();
        // No variant, name free -> base name returned as-is.
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg");
    }

    #[test]
    fn test_resolve_command_name_numeric_suffix_on_conflict() {
        let mut taken = std::collections::HashSet::new();
        taken.insert("rg".to_string());

        // "rg" taken -> "rg-1"
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg-1");

        // "rg" and "rg-1" taken -> "rg-2"
        taken.insert("rg-1".to_string());
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg-2");
    }

    #[test]
    fn test_resolve_command_name_with_variant() {
        let taken = std::collections::HashSet::new();
        // Variant appends "-variant" when base doesn't already end with it.
        assert_eq!(
            resolve_command_name("bun", Some("baseline"), &taken, false),
            "bun-baseline"
        );
    }

    #[test]
    fn test_resolve_command_name_variant_already_suffixed() {
        let taken = std::collections::HashSet::new();
        // Base already ends with "-profile" variant -> kept as-is.
        assert_eq!(
            resolve_command_name("bun-profile", Some("profile"), &taken, false),
            "bun-profile"
        );
    }

    #[test]
    fn test_resolve_command_name_custom_takes_numeric_suffix() {
        let mut taken = std::collections::HashSet::new();
        taken.insert("mytool".to_string());
        // Custom name that conflicts falls through to numeric suffix.
        assert_eq!(
            resolve_command_name("mytool", None, &taken, true),
            "mytool-1"
        );
    }

    /// Replays queued answers; any prompt without a queued answer fails the test
    #[derive(Default)]
    struct ScriptedUi {
        selects: std::cell::RefCell<Vec<usize>>,
        multi: std::cell::RefCell<Vec<Vec<usize>>>,
        prompts: std::cell::RefCell<Vec<String>>,
    }

    impl InstallUi for ScriptedUi {
        fn line(&self, _msg: &str) {}
        fn confirm(&self, prompt: &str, _default: bool) -> Result<bool> {
            panic!("unexpected confirm: {}", prompt)
        }
        fn select(&self, prompt: &str, items: &[String], _default: usize) -> Result<usize> {
            self.prompts
                .borrow_mut()
                .push(format!("{} {:?}", prompt.trim(), items));
            Ok(self.selects.borrow_mut().remove(0))
        }
        fn multi_select(&self, prompt: &str, _items: &[String]) -> Result<Vec<usize>> {
            self.prompts.borrow_mut().push(prompt.to_string());
            Ok(self.multi.borrow_mut().remove(0))
        }
    }

    fn cand(path: &str, score: u32) -> ExecutableCandidate {
        ExecutableCandidate {
            path: path.to_string(),
            score,
            reason: "r".to_string(),
        }
    }

    fn installed_with(exes: &[(&str, &str)]) -> InstalledSet {
        let mut pkg: InstalledPackage = serde_json::from_value(serde_json::json!({
            "version": "1.0.0",
            "platform": "p",
            "installed_at": "2020-01-01T00:00:00Z",
            "source": {"type": "bucket", "name": "b"},
            "description": "",
            "asset_name": "a.tar.gz",
        }))
        .unwrap();
        pkg.executables = exes
            .iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect();
        let mut set = InstalledSet::new();
        set.upsert_package("tool".to_string(), pkg);
        set
    }

    fn installer<'a>(
        paths: &'a WenPaths,
        ui: &'a ScriptedUi,
        update: bool,
    ) -> PackageInstaller<'a> {
        PackageInstaller {
            paths,
            ui,
            command_name: None,
            yes: false,
            no_suffix: false,
            update_mode: update,
        }
    }

    #[test]
    fn update_keeps_old_and_matches_relocated_by_filename() {
        let paths = WenPaths::with_root(std::env::temp_dir().join("wg-cl8-unit"));
        let ui = ScriptedUi::default();
        let installed = installed_with(&[("v1/tool", "tool"), ("v1/helper", "helper")]);
        let got = installer(&paths, &ui, true)
            .select_executables(
                &installed,
                "tool",
                &[
                    cand("v1/tool", 50),
                    cand("v2/helper", 35),
                    cand("v2/extra", 35),
                ],
            )
            .unwrap();
        assert_eq!(got, vec!["v1/tool", "v2/helper"]);
        assert!(ui.prompts.borrow().is_empty());
    }

    #[test]
    fn update_prompts_for_vanished_executable() {
        let paths = WenPaths::with_root(std::env::temp_dir().join("wg-cl8-unit"));
        let ui = ScriptedUi {
            selects: vec![1].into(),
            ..Default::default()
        };
        let installed = installed_with(&[("tool", "tool"), ("old", "old")]);
        let got = installer(&paths, &ui, true)
            .select_executables(
                &installed,
                "tool",
                &[cand("tool", 50), cand("new-a", 35), cand("new-b", 35)],
            )
            .unwrap();
        assert_eq!(got, vec!["tool", "new-b"]);
        assert_eq!(
            ui.prompts.borrow()[0],
            "Select replacement for 'old' [\"new-a (r)\", \"new-b (r)\", \"Skip (remove this command)\"]"
        );
    }

    #[test]
    fn add_many_candidates_asks_user() {
        let paths = WenPaths::with_root(std::env::temp_dir().join("wg-cl8-unit"));
        let ui = ScriptedUi {
            multi: vec![vec![0, 3]].into(),
            ..Default::default()
        };
        let cands: Vec<_> = ["a", "b", "c", "d"].iter().map(|p| cand(p, 35)).collect();
        let got = installer(&paths, &ui, false)
            .select_executables(&InstalledSet::new(), "tool", &cands)
            .unwrap();
        assert_eq!(got, vec!["a", "d"]);

        // Three or fewer are taken without asking
        let got = installer(&paths, &ui, false)
            .select_executables(&InstalledSet::new(), "tool", &cands[..3])
            .unwrap();
        assert_eq!(got, vec!["a", "b", "c"]);
        assert_eq!(ui.prompts.borrow().len(), 1);
    }
}
