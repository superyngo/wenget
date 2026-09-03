//! Update (Upgrade) command implementation

use crate::commands::add;
use crate::core::manifest::PackageSource;
use crate::core::{Config, Package};
use crate::providers::base::SourceProvider;
use crate::providers::GitHubProvider;
use anyhow::Result;
use colored::Colorize;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

/// Maximum number of concurrent GitHub API requests when checking for updates.
/// Capped to avoid hitting the unauthenticated rate limit (60 req/hour) too quickly.
const MAX_CONCURRENT_FETCHES: usize = 8;

/// A parallel fetch outcome: the repo name paired with its fetched package (or error).
type FetchResult = (String, Result<Package>);

/// Fetch package info for many repos in parallel, showing a progress bar.
///
/// Each job is `(repo_name, repo_url)`. Returns `(repo_name, Result<Package>)` in the
/// same order as the input jobs so downstream processing stays deterministic. All HTTP
/// work happens on worker threads; the caller applies any cache mutations or prompts
/// sequentially on the main thread afterwards.
///
/// If `existing_pb` is provided, uses that progress bar instead of creating a new one.
/// The caller is responsible for finishing/clearing an externally provided bar.
fn parallel_fetch_packages(
    github: &GitHubProvider,
    jobs: Vec<(String, String)>,
    existing_pb: Option<&indicatif::ProgressBar>,
) -> Vec<FetchResult> {
    let total = jobs.len();
    if total == 0 {
        return Vec::new();
    }

    let pb = match existing_pb {
        Some(pb) => pb.clone(),
        None => {
            let pb = indicatif::ProgressBar::new(total as u64);
            pb.set_style(
                indicatif::ProgressStyle::with_template(
                    "{spinner:.cyan} [{bar:30.cyan/blue}] {pos}/{len} checking for updates...",
                )
                .unwrap()
                .progress_chars("=>-"),
            );
            pb
        }
    };

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<FetchResult>>> = Mutex::new((0..total).map(|_| None).collect());

    let workers = total.min(MAX_CONCURRENT_FETCHES);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let github = github.clone();
            let jobs = &jobs;
            let next = &next;
            let results = &results;
            let pb = &pb;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= total {
                    break;
                }
                let (name, url) = &jobs[i];
                let res = github.fetch_package(url);
                results.lock().unwrap()[i] = Some((name.clone(), res));
                pb.inc(1);
            });
        }
    });

    if existing_pb.is_none() {
        pb.finish_and_clear();
    }

    Mutex::into_inner(results)
        .unwrap()
        .into_iter()
        .map(|slot| slot.expect("every job slot is filled by a worker"))
        .collect()
}

/// Compare two dot-separated version strings.
/// Returns true if `new` is strictly newer than `old`.
fn is_newer_version(old: &str, new: &str) -> bool {
    let parse_parts = |v: &str| {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse::<u64>().ok())
            .collect::<Vec<_>>()
    };
    let old_parts = parse_parts(old);
    let new_parts = parse_parts(new);
    for i in 0..old_parts.len().max(new_parts.len()) {
        let a = old_parts.get(i).unwrap_or(&0);
        let b = new_parts.get(i).unwrap_or(&0);
        if b > a {
            return true;
        }
        if b < a {
            return false;
        }
    }
    false
}

/// Upgrade installed packages
pub fn run(names: Vec<String>, yes: bool, platform: Option<String>) -> Result<()> {
    // Check for wenget updates first
    if check_and_upgrade_self(yes)? {
        // On Windows, exit after self-update to avoid shell instability
        return Ok(());
    }

    let config = Config::new()?;
    let installed = config.get_or_create_installed()?;

    if installed.packages.is_empty() {
        println!("{}", "No packages installed".yellow());
        return Ok(());
    }

    // Force refresh bucket cache to ensure we have latest versions
    println!("{}", "Refreshing bucket cache...".cyan());
    let mut cache = config.rebuild_cache()?;

    // Create GitHub provider to fetch latest versions
    let github = GitHubProvider::new()?;

    // Determine which packages to upgrade
    let update_all = names.is_empty() || (names.len() == 1 && names[0] == "all");
    let to_upgrade: Vec<String> = if update_all {
        // List upgradeable packages (also syncs latest package info into the cache)
        let upgradeable = find_upgradeable(&installed, &github, &mut cache, yes)?;

        if upgradeable.is_empty() {
            println!("{}", "All packages are up to date".green());
            return Ok(());
        }

        println!("{}", "Packages to upgrade:".bold());
        for (name, current, latest) in &upgradeable {
            println!("  • {} {} -> {}", name, current.yellow(), latest.green());
        }
        println!();

        upgradeable.into_iter().map(|(name, _, _)| name).collect()
    } else {
        names
    };

    // Expand: include all installed variants when upgrading a repo
    let mut expanded = Vec::new();
    for name in &to_upgrade {
        // Check if this is a repo name or a specific variant
        if name.contains("::") {
            // Specific variant like "bun::baseline" — validate it exists
            if installed.is_installed(name) {
                expanded.push(name.clone());
            } else {
                eprintln!(
                    "{} '{}' is not installed, skipping (use 'wenget add' to install new packages)",
                    "Warning:".yellow(),
                    name
                );
            }
            continue;
        }

        // Find all installed variants of this repo
        let variants = installed.find_by_repo(name);
        if variants.is_empty() {
            if installed.is_installed(name) {
                // Standalone package or script
                expanded.push(name.clone());
            } else {
                eprintln!(
                    "{} '{}' is not installed, skipping (use 'wenget add' to install new packages)",
                    "Warning:".yellow(),
                    name
                );
            }
        } else {
            for (key, _pkg) in variants {
                expanded.push(key.clone());
            }
        }
    }

    if expanded.is_empty() {
        println!("{}", "No installed packages to update".yellow());
        return Ok(());
    }

    // For named updates, find_upgradeable was skipped, so sync the latest package info
    // for the targeted packages into the cache here.
    let mut to_run = expanded.clone();
    if !update_all {
        sync_bucket_packages_to_cache(&installed, &expanded, &github, &mut cache);

        // Filter out packages that are already up to date
        let mut filtered = Vec::new();
        let cache_by_name = cache.packages_by_name();
        for key in expanded {
            if let Some(inst_pkg) = installed.get_package(&key) {
                match &inst_pkg.source {
                    PackageSource::Bucket { .. } => {
                        if let Some(cached_pkg) = cache_by_name.get(inst_pkg.repo_name.as_str()) {
                            if let Some(cache_version) = &cached_pkg.package.version {
                                if is_newer_version(&inst_pkg.version, cache_version) {
                                    filtered.push(key);
                                } else {
                                    println!(
                                        "  • {} v{} is already up to date (latest: {})",
                                        inst_pkg.repo_name.bright_white(),
                                        inst_pkg.version.dimmed(),
                                        cache_version.green()
                                    );
                                }
                            } else {
                                filtered.push(key);
                            }
                        } else {
                            filtered.push(key);
                        }
                    }
                    PackageSource::Script { origin, .. } => {
                        if origin.starts_with("bucket:") {
                            if let Some(cached_script) = cache.find_script(&inst_pkg.repo_name) {
                                if let Some((_, platform_info)) =
                                    cached_script.script.get_installable_script()
                                {
                                    let cache_url = &platform_info.url;
                                    let needs_update = match &inst_pkg.download_url {
                                        Some(installed_url) => installed_url != cache_url,
                                        None => true,
                                    };
                                    if needs_update {
                                        filtered.push(key);
                                    } else {
                                        println!(
                                            "  • {} (script) is already up to date",
                                            inst_pkg.repo_name.bright_white()
                                        );
                                    }
                                } else {
                                    filtered.push(key);
                                }
                            } else {
                                filtered.push(key);
                            }
                        } else {
                            filtered.push(key);
                        }
                    }
                    _ => {
                        // For direct repo, we let add::run handle it/check live
                        filtered.push(key);
                    }
                }
            } else {
                filtered.push(key);
            }
        }
        to_run = filtered;
    }

    if to_run.is_empty() {
        return Ok(());
    }

    // Persist the API-synced package info so the add step (running in update_mode) reads
    // the latest version and download links from the cache, even if the GitHub API
    // becomes unavailable during installation.
    if let Err(e) = config.save_cache(&cache) {
        log::warn!("Failed to save synced cache: {}", e);
    }

    // Use add command to upgrade (reinstall). The platform override (if any)
    // is threaded through so updates honor an explicit `-p` target; when None,
    // the add path falls back to the `preferred_platform` config setting.
    add::run(to_run, yes, None, platform, None, None, false, true)
}

/// Find upgradeable packages by checking their sources
fn find_upgradeable(
    installed: &crate::core::InstalledSet,
    github: &GitHubProvider,
    cache: &mut crate::cache::ManifestCache,
    yes: bool,
) -> Result<Vec<(String, String, String)>> {
    let mut upgradeable = Vec::new();

    let grouped = installed.group_by_repo();
    let total = grouped.len();

    let pb = indicatif::ProgressBar::new(total as u64);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.cyan} [{bar:30.cyan/blue}] {pos}/{len} checking for updates...",
        )
        .unwrap()
        .progress_chars("=>-"),
    );

    // Phase 1 (sequential): resolve each repo's URL, handle local-only sources (scripts)
    // that need no API call, and collect the rest into `jobs` for parallel fetching.
    // `job_meta` keeps the installed source/version snapshot needed when applying results.
    //
    // Build a name → cached package index once: the loop below and the cache-fallback
    // path in Phase 3 both look packages up by repo name, which is O(cache) per lookup
    // against the URL-keyed `cache.packages` map.
    let cache_by_name = cache.packages_by_name();
    let mut jobs: Vec<(String, String)> = Vec::new();
    let mut job_meta: HashMap<String, (PackageSource, String)> = HashMap::new();

    for (repo_name, variants) in grouped {
        // Use the first variant to get version and source info
        let (_key, inst_pkg) = variants[0];

        let repo_url = match &inst_pkg.source {
            PackageSource::Bucket { name: bucket_name } => {
                // Get package info from cache for bucket packages
                let found = cache_by_name.get(repo_name.as_str());

                if let Some(cached_pkg) = found {
                    cached_pkg.package.repo.clone()
                } else {
                    eprintln!(
                        "{} Package {} not found in bucket {} cache, skipping update check",
                        "Warning:".yellow(),
                        repo_name,
                        bucket_name
                    );
                    pb.inc(1);
                    continue;
                }
            }
            PackageSource::DirectRepo { url } => {
                // Use the stored repo URL directly
                url.clone()
            }
            PackageSource::Script { origin, .. } => {
                // Check if this is a bucket-sourced script
                if !origin.starts_with("bucket:") {
                    log::debug!(
                        "Skipping non-bucket script '{}' - no update source",
                        repo_name
                    );
                    pb.inc(1);
                    continue;
                }

                // Look up the script in the refreshed cache
                let cached_script = cache.find_script(&repo_name);
                if cached_script.is_none() {
                    log::debug!("Script '{}' not found in cache, skipping update", repo_name);
                    pb.inc(1);
                    continue;
                }

                let cached_script = cached_script.unwrap();

                // Get the installable script URL for current platform
                if let Some((_script_type, platform_info)) =
                    cached_script.script.get_installable_script()
                {
                    let cache_url = &platform_info.url;

                    // Compare with installed download_url
                    let needs_update = match &inst_pkg.download_url {
                        Some(installed_url) => installed_url != cache_url,
                        None => true, // No stored URL = always update (legacy installs)
                    };

                    if needs_update {
                        upgradeable.push((
                            repo_name,
                            inst_pkg.download_url.clone().unwrap_or_default(),
                            cache_url.clone(),
                        ));
                    }
                }

                pb.inc(1);
                continue;
            }
        };

        jobs.push((repo_name.clone(), repo_url));
        job_meta.insert(
            repo_name,
            (inst_pkg.source.clone(), inst_pkg.version.clone()),
        );
    }

    // Phase 2 (parallel): fetch latest package info from GitHub for all collected jobs.
    let results = parallel_fetch_packages(github, jobs, Some(&pb));

    // Phase 3 (sequential): apply results — mutate the cache and resolve any prompts on the
    // main thread, where it is safe to do so.
    for (repo_name, result) in results {
        let (source, inst_version) = match job_meta.get(&repo_name) {
            Some(meta) => meta.clone(),
            None => continue,
        };

        match result {
            Ok(latest_pkg) => {
                let latest_version = latest_pkg
                    .version
                    .clone()
                    .unwrap_or_else(|| inst_version.clone());

                // Persist the fresh package info (version + download links) into the cache so
                // the install step reads the latest data even if the API later becomes
                // unavailable. Only bucket packages are stored in the cache.
                if matches!(source, PackageSource::Bucket { .. }) {
                    cache.add_package(latest_pkg, source.clone());
                }

                if inst_version != latest_version {
                    upgradeable.push((repo_name, inst_version, latest_version));
                }
            }
            Err(e) => {
                // API failed - try to use cache version as fallback
                log::debug!(
                    "GitHub API failed for {}: {}. Trying cache fallback...",
                    repo_name,
                    e
                );

                // Find package in cache by repo_name.
                // A linear scan is acceptable here: this branch only runs on API
                // failure (rare), and `cache` may have been mutated by prior Ok
                // results in this loop, so a pre-built name index would be stale.
                if let Some(cached_pkg) = cache
                    .packages
                    .values()
                    .find(|p| p.package.name == repo_name)
                {
                    if let Some(cache_version) = &cached_pkg.package.version {
                        let should_upgrade = if inst_version == "local" {
                            if yes {
                                true
                            } else {
                                eprintln!(
                                    "{} {} is locally installed, cache has version {} available",
                                    "Info:".cyan(),
                                    repo_name,
                                    cache_version
                                );
                                crate::utils::prompt::confirm(&format!(
                                    "  Overwrite local {} with cached version {}?",
                                    repo_name, cache_version
                                ))?
                            }
                        } else {
                            is_newer_version(&inst_version, cache_version)
                        };

                        if should_upgrade {
                            eprintln!(
                                "{} Using cached version for {}: {} (API unavailable)",
                                "Info:".cyan(),
                                repo_name,
                                cache_version
                            );
                            upgradeable.push((repo_name, inst_version, cache_version.clone()));
                        }
                    } else {
                        eprintln!(
                            "{} No version info in cache for {}, skipping",
                            "Warning:".yellow(),
                            repo_name
                        );
                    }
                } else {
                    eprintln!(
                        "{} Failed to check updates for {}: API error and no cache available",
                        "Warning:".yellow(),
                        repo_name
                    );
                }
            }
        }
    }

    pb.finish();
    println!();

    Ok(upgradeable)
}

/// Sync the latest package info for the given installed keys into the cache.
///
/// Used for named updates (where `find_upgradeable` is skipped). Only bucket-sourced
/// packages are synced — direct-repo packages are not stored in the cache and are always
/// resolved live from the GitHub API.
fn sync_bucket_packages_to_cache(
    installed: &crate::core::InstalledSet,
    keys: &[String],
    github: &GitHubProvider,
    cache: &mut crate::cache::ManifestCache,
) {
    let mut synced = HashSet::new();
    let mut jobs: Vec<(String, String)> = Vec::new();
    let mut source_map: HashMap<String, PackageSource> = HashMap::new();

    // Name-keyed index of the cache so per-key lookups are O(1) instead of
    // an O(keys × cache) linear scan.
    let cache_by_name = cache.packages_by_name();

    for key in keys {
        let inst_pkg = match installed.get_package(key) {
            Some(p) => p,
            None => continue,
        };

        if !matches!(inst_pkg.source, PackageSource::Bucket { .. }) {
            continue;
        }

        // Refresh each repo only once even if multiple variants are listed.
        if !synced.insert(inst_pkg.repo_name.clone()) {
            continue;
        }

        // Look up the repo URL from the cached bucket entry.
        let repo_url = match cache_by_name.get(inst_pkg.repo_name.as_str()) {
            Some(cached) => cached.package.repo.clone(),
            None => continue,
        };

        jobs.push((inst_pkg.repo_name.clone(), repo_url));
        source_map.insert(inst_pkg.repo_name.clone(), inst_pkg.source.clone());
    }

    // Fetch in parallel, then apply cache mutations sequentially on the main thread.
    for (repo_name, result) in parallel_fetch_packages(github, jobs, None) {
        match result {
            Ok(pkg) => {
                if let Some(source) = source_map.remove(&repo_name) {
                    cache.add_package(pkg, source);
                }
            }
            Err(e) => log::debug!(
                "Failed to refresh cache for {} during update: {}",
                repo_name,
                e
            ),
        }
    }
}

/// Check for wenget updates and prompt user
/// Returns true if wenget was updated on Windows (caller should exit)
fn check_and_upgrade_self(yes: bool) -> Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");

    println!("{}", "Checking for wenget updates...".dimmed());

    // Try to check latest version - don't fail the whole update if this fails
    let provider = match GitHubProvider::new() {
        Ok(p) => p,
        Err(e) => {
            log::debug!("Failed to create GitHub provider for self-check: {}", e);
            return Ok(false);
        }
    };

    let latest_version = match provider.fetch_latest_version("https://github.com/superyngo/wenget")
    {
        Ok(v) => v,
        Err(e) => {
            log::debug!("Failed to check wenget updates: {}", e);
            return Ok(false);
        }
    };

    if current_version == latest_version {
        return Ok(false);
    }

    println!(
        "{} {} -> {}",
        "New wenget version available:".yellow().bold(),
        current_version.yellow(),
        latest_version.green()
    );

    #[cfg(windows)]
    {
        if let Ok(current_exe) = std::env::current_exe() {
            if is_installed_via_winget(&current_exe) {
                println!(
                    "{}",
                    "⚠  wenget was installed via winget. Run 'winget upgrade WenanLin.wenget' to update."
                        .yellow()
                        .bold()
                );
                println!();
                return Ok(false);
            }
        }
    }

    let should_update = if yes {
        true
    } else {
        crate::utils::confirm("Update wenget first?")?
    };

    if !should_update {
        println!();
        return Ok(false);
    }

    // Perform self-update, passing provider and known version to avoid redundant API calls
    upgrade_self_with_provider(provider, &latest_version)?;

    // On Windows, recommend restarting shell
    #[cfg(windows)]
    {
        println!();
        println!(
            "{}",
            "⚠  Please restart your shell, then run 'wenget update' again to update packages."
                .yellow()
                .bold()
        );
        Ok(true) // Signal caller to exit
    }

    #[cfg(not(windows))]
    {
        println!();
        Ok(false) // Continue with package updates on Unix
    }
}

/// Whether the running executable lives under WinGet's portable-package
/// install directory (`...\Microsoft\WinGet\Packages\...`).
///
/// WinGet-managed installs should be updated via `winget upgrade`, not by
/// wenget replacing its own binary — winget tracks its own installed-version
/// state and a silent self-replace would desync it.
#[cfg(windows)]
fn is_installed_via_winget(exe_path: &std::path::Path) -> bool {
    exe_path
        .to_string_lossy()
        .to_lowercase()
        .contains(r"microsoft\winget\packages")
}

/// Whether a `preferred_platform` override targets the same OS and architecture
/// as the host.
///
/// Only such overrides are safe to apply during self-update: they merely select
/// a libc/compiler variant (e.g. musl on glibc) while keeping OS+arch. A
/// cross-OS/arch override would replace the running binary with one that cannot
/// execute on this machine.
fn override_matches_host(override_str: &str, host: crate::core::Platform) -> bool {
    let parsed = crate::core::platform::ParsedAsset::from_filename(override_str);
    match (parsed.os, parsed.arch) {
        (Some(os), Some(arch)) => os == host.os && arch == host.arch,
        // OS matches and arch unspecified → treat as host arch (compatible).
        (Some(os), None) => os == host.os,
        _ => false,
    }
}

/// Upgrade wenget itself
fn upgrade_self_with_provider(provider: GitHubProvider, latest_version: &str) -> Result<()> {
    use crate::core::{Platform, WenPaths};
    use crate::downloader::download_file;
    use crate::installer::{extract_archive, find_executable};
    use colored::Colorize;
    use std::env;
    use std::fs;

    println!("{}", "Upgrading wenget...".cyan());

    // Get package information including binaries
    let package = provider.fetch_package("https://github.com/superyngo/wenget")?;

    // Select binary for current platform
    // Note: Uses same platform matching logic as add command (see add.rs).
    // This handles libc detection (musl vs glibc), compiler variants, and fallbacks.
    let current_platform = Platform::current();

    // Honor `preferred_platform` from config for self-update, but ONLY when it
    // targets the same OS+arch as the host. The self-update binary must run on
    // this machine, so a cross-OS/arch override would brick wenget; a pure
    // libc/compiler choice (e.g. musl on glibc) keeps OS+arch and is safe.
    let preferred = Config::new()
        .ok()
        .and_then(|c| c.preferences().preferred_platform.clone());

    let matches = match preferred.as_deref() {
        Some(pref) if override_matches_host(pref, current_platform) => {
            let m = Platform::match_override(pref, &package.platforms);
            if m.is_empty() {
                current_platform.find_best_match(&package.platforms)
            } else {
                m
            }
        }
        Some(pref) => {
            println!(
                "  {} Ignoring preferred_platform '{}' for self-update (different OS/arch than host)",
                "ℹ".cyan(),
                pref
            );
            current_platform.find_best_match(&package.platforms)
        }
        None => current_platform.find_best_match(&package.platforms),
    };

    if matches.is_empty() {
        anyhow::bail!(
            "No binary available for platform: {}. Available platforms: {}",
            current_platform,
            package
                .platforms
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let best_match = &matches[0];
    let binaries = &package.platforms[&best_match.platform_id];

    // Show fallback information if using compatible binary
    if let Some(fallback_type) = &best_match.fallback_type {
        println!(
            "  {} Using compatible binary: {} ({})",
            "ℹ".cyan(),
            best_match.platform_id,
            fallback_type.description()
        );
    }

    // For self-update, just use the first binary if multiple exist
    let binary = binaries
        .first()
        .ok_or_else(|| anyhow::anyhow!("No binaries found for platform"))?;

    println!("Downloading: {}", binary.url);

    // Determine download file name from URL
    let filename = binary
        .url
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("Invalid download URL"))?;

    // Download to temporary directory
    let paths = WenPaths::new()?;
    let temp_dir = paths.cache_dir().join("self-upgrade");
    fs::create_dir_all(&temp_dir)?;

    let download_path = temp_dir.join(filename);
    download_file(&binary.url, &download_path)?;

    if let Err(e) =
        crate::core::checksum::verify_download(&binary.url, &binary.asset_name, &download_path)
    {
        fs::remove_file(&download_path).ok();
        return Err(e);
    }

    // Extract archive
    let extract_dir = temp_dir.join("extracted");
    fs::create_dir_all(&extract_dir)?;

    println!("{}", "Extracting...".cyan());
    let extracted_files = extract_archive(&download_path, &extract_dir)?;

    // Find the wenget executable
    let exe_relative_path = find_executable(&extracted_files, "wenget")
        .ok_or_else(|| anyhow::anyhow!("Could not find wenget executable in archive"))?;

    let new_exe_path = extract_dir.join(&exe_relative_path);

    if !new_exe_path.exists() {
        anyhow::bail!("Extracted executable not found: {}", new_exe_path.display());
    }

    // Get current executable path
    let current_exe = env::current_exe()?;

    println!("{}", "Installing new version...".cyan());

    // Platform-specific replacement logic
    #[cfg(windows)]
    {
        replace_exe_windows(&current_exe, &new_exe_path)?;
    }

    #[cfg(not(windows))]
    {
        replace_exe_unix(&current_exe, &new_exe_path)?;
    }

    // Clean up temporary files
    if let Err(e) = fs::remove_dir_all(&temp_dir) {
        log::warn!(
            "Failed to cleanup temp directory: {}: {}",
            temp_dir.display(),
            e
        );
    }

    println!();
    println!(
        "{}",
        format!("✓ Successfully upgraded to v{}!", latest_version).green()
    );
    println!("Please restart your terminal or run 'wenget --version' to verify.");

    Ok(())
}

/// Replace executable on Windows
///
/// Windows locks running executables, so we use a multi-step process:
/// 1. Rename current exe to .old
/// 2. Copy new exe to original location
/// 3. Create a cleanup script to delete .old file
#[cfg(windows)]
fn replace_exe_windows(
    current_exe: &std::path::PathBuf,
    new_exe: &std::path::PathBuf,
) -> Result<()> {
    use std::fs;
    use std::process::Command;

    let old_exe = current_exe.with_extension("exe.old");

    // Rename current executable
    if old_exe.exists() {
        fs::remove_file(&old_exe)?;
    }
    fs::rename(current_exe, &old_exe)?;

    // Copy new executable to the original location
    fs::copy(new_exe, current_exe)?;

    // Create cleanup script
    let cleanup_script = current_exe.parent().unwrap().join("wenget_cleanup.cmd");

    let script_content = format!(
        r#"@echo off
timeout /t 2 /nobreak >nul
del /f /q "{}"
del /f /q "%~f0"
"#,
        old_exe.display()
    );

    fs::write(&cleanup_script, script_content)?;

    // Start cleanup script in background
    let _ = Command::new("cmd")
        .args(["/C", "start", "/B", cleanup_script.to_str().unwrap()])
        .spawn();

    Ok(())
}

/// Replace executable on Unix (Linux/macOS)
///
/// This function uses a robust strategy to replace the running executable:
/// 1. The new executable is made executable (`chmod 755`).
/// 2. The current running executable is renamed to `*.old`.
/// 3. An atomic `fs::rename` is attempted to move the new executable into place.
/// 4. If `rename` fails (e.g., cross-device link), it falls back to `fs::copy`.
/// 5. The `*.old` file is removed on a best-effort basis.
#[cfg(not(windows))]
fn replace_exe_unix(current_exe: &std::path::PathBuf, new_exe: &std::path::PathBuf) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // Set permissions on the new executable before doing anything else.
    fs::set_permissions(new_exe, fs::Permissions::from_mode(0o755))?;

    let old_exe = current_exe.with_extension("old");

    // Remove any existing .old file to avoid confusion.
    if old_exe.exists() {
        if let Err(e) = fs::remove_file(&old_exe) {
            log::warn!(
                "Failed to remove old executable: {}: {}",
                old_exe.display(),
                e
            );
        }
    }

    // 1. Rename the currently running executable.
    if let Err(e) = fs::rename(current_exe, &old_exe) {
        return Err(anyhow::anyhow!(
            "Failed to rename running executable: {}. Try running with sudo.",
            e
        ));
    }

    // 2. Move the new executable into place. Try atomic rename first.
    if let Err(rename_err) = fs::rename(new_exe, current_exe) {
        // Rename failed, likely a cross-device link error (EXDEV). Fall back to copying.
        log::warn!(
            "Atomic rename failed: {}. Falling back to copy.",
            rename_err
        );
        match fs::copy(new_exe, current_exe) {
            Ok(_) => {
                // Permissions may not be preserved by `copy`, so set them again.
                fs::set_permissions(current_exe, fs::Permissions::from_mode(0o755))?;
            }
            Err(copy_err) => {
                // Copy failed. Try to restore the original executable.
                log::error!("Failed to copy new executable: {}", copy_err);
                if let Err(restore_err) = fs::rename(&old_exe, current_exe) {
                    log::error!(
                        "CRITICAL: Failed to restore original executable: {}",
                        restore_err
                    );
                }
                return Err(copy_err.into());
            }
        }
    }

    // 3. Clean up the old executable (best-effort).
    if let Err(e) = fs::remove_file(&old_exe) {
        log::warn!(
            "Failed to remove old executable: {}. It can be removed manually.",
            e
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::platform::{Arch, Os};
    use crate::core::Platform;

    #[test]
    fn test_override_matches_host() {
        let host = Platform::new(Os::Linux, Arch::X86_64);

        // Same OS+arch, libc variant only → safe to apply.
        assert!(override_matches_host("x86_64-unknown-linux-musl", host));
        assert!(override_matches_host("linux-x86_64-gnu", host));
        // OS matches, arch unspecified → treated as host arch.
        assert!(override_matches_host("linux", host));

        // Different arch → unsafe.
        assert!(!override_matches_host("aarch64-unknown-linux-musl", host));
        // Different OS → unsafe.
        assert!(!override_matches_host("x86_64-pc-windows-msvc", host));
        // Unparseable → unsafe.
        assert!(!override_matches_host("not-a-platform", host));
    }

    #[cfg(windows)]
    #[test]
    fn test_is_installed_via_winget() {
        assert!(is_installed_via_winget(std::path::Path::new(
            r"C:\Users\wen\AppData\Local\Microsoft\WinGet\Packages\WenanLin.wenget_Microsoft.Winget.Source_8wekyb3d8bbwe\wenget.exe"
        )));
        // Case-insensitive
        assert!(is_installed_via_winget(std::path::Path::new(
            r"C:\Users\wen\AppData\Local\microsoft\winget\packages\foo\wenget.exe"
        )));
        assert!(!is_installed_via_winget(std::path::Path::new(
            r"C:\Users\wen\.wenget\bin\wenget.exe"
        )));
        assert!(!is_installed_via_winget(std::path::Path::new(
            r"C:\Program Files\wenget\wenget.exe"
        )));
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("1.0.0", "2.0.0"));
        assert!(is_newer_version("1.73.2", "1.73.3"));
        assert!(is_newer_version("0.10.12", "0.11.2"));
        assert!(is_newer_version("0.9.0", "0.14.1"));
        assert!(is_newer_version("1.0", "1.0.1"));
        assert!(is_newer_version("1.3.0", "1.3.6"));

        // Not newer — same version
        assert!(!is_newer_version("1.0.0", "1.0.0"));

        // Not newer — cache is older (the bug case)
        assert!(!is_newer_version("1.73.3", "1.73.2"));
        assert!(!is_newer_version("2.61.0", "2.60.0"));
        assert!(!is_newer_version("0.14.1", "0.9.0"));
        assert!(!is_newer_version("2.89.0", "2.88.1"));

        // Handles v prefix
        assert!(is_newer_version("1.0.0", "v2.0.0"));
        assert!(!is_newer_version("v2.0.0", "1.0.0"));
    }
}
