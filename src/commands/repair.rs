//! Repair command for wenget
//!
//! Reports and repairs drift between package records, app directories, command
//! launchers, and the global config files.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::bucket::BucketConfig;
use crate::cache::ManifestCache;
use crate::core::manifest::InstalledSet;
use crate::core::paths::WenPaths;
use crate::core::repair::{check_json_file, create_backup, FileStatus};
use crate::core::store::{InstalledStore, ScanEntry};
use crate::core::Config;

/// Run the repair command
pub fn run(force: bool) -> Result<()> {
    println!("{}", "Checking wenget state...".cyan());
    println!();

    let config = Config::new()?;
    let paths = config.paths().clone();
    let store = InstalledStore::new(paths.clone());

    let mut issues = 0usize;

    println!("{}", "Installed packages:".bold());
    let entries = store.scan_app_dirs()?;
    let mut residue = Vec::new();
    let mut package_count = 0usize;

    for entry in &entries {
        match entry {
            ScanEntry::Loaded { key, .. } => {
                package_count += 1;
                println!("  {} {}", "✓".green(), key);
            }
            ScanEntry::Untracked(dir) => {
                println!(
                    "  {} {} - untracked app directory (no package record)",
                    "!".yellow(),
                    dir.display()
                );
                println!(
                    "      Re-install to bring it under management: {}",
                    format!(
                        "wenget add {}",
                        dir.file_name().unwrap_or_default().to_string_lossy()
                    )
                    .cyan()
                );
                issues += 1;
            }
            ScanEntry::Corrupt(dir) => {
                println!(
                    "  {} {} - package record was corrupt and has been saved aside",
                    "✗".red(),
                    dir.display()
                );
                issues += 1;
            }
            ScanEntry::FutureVersion { dir, version } => {
                println!(
                    "  {} {} - record version {} is newer than this wenget; skipped",
                    "!".yellow(),
                    dir.display(),
                    version
                );
            }
            ScanEntry::Residue(path) => residue.push(path.clone()),
        }
    }

    if package_count == 0 {
        println!("  (none)");
    }

    for (key, dirs) in store.duplicate_keys()? {
        println!(
            "  {} {} is claimed by {} directories:",
            "✗".red(),
            key,
            dirs.len()
        );
        for dir in dirs {
            println!("      {}", dir.display());
        }
        issues += 1;
    }

    if !residue.is_empty() {
        println!();
        println!("{}", "Interrupted installs:".bold());
        for path in &residue {
            println!("  {} {}", "!".yellow(), path.display());
        }
        issues += residue.len();
    }

    let set = store.load()?;

    println!();
    println!("{}", "Command launchers:".bold());
    let orphans = provable_orphan_shims(&paths, &set)?;
    let missing = missing_shims(&paths, &set);
    if orphans.is_empty() && missing.is_empty() {
        println!("  {} all launchers match installed packages", "✓".green());
    }
    for path in &orphans {
        println!(
            "  {} {} - points into apps/ but no installed package owns it",
            "!".yellow(),
            path.display()
        );
        issues += 1;
    }
    for (command, path) in &missing {
        println!(
            "  {} {} - recorded command has no launcher at {}",
            "!".yellow(),
            command,
            path.display()
        );
        issues += 1;
    }

    // Global config files that legitimately remain global.
    println!();
    println!("{}", "Configuration files:".bold());
    let buckets_path = paths.buckets_json();
    let cache_path = paths.manifest_cache_json();
    let buckets_status = check_json_file::<BucketConfig>(&buckets_path);
    let cache_status = check_json_file::<ManifestCache>(&cache_path);
    println!("  buckets.json:        {}", buckets_status);
    println!("  manifest-cache.json: {}", cache_status);

    if matches!(buckets_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }
    if matches!(cache_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }

    println!();
    if issues == 0 && !force {
        println!("{}", "Everything looks healthy.".green());
        return Ok(());
    }

    if !residue.is_empty()
        && (force || crate::utils::prompt::confirm("Remove interrupted-install leftovers?")?)
    {
        for path in store.sweep_residue()? {
            println!("  {} Removed {}", "✓".green(), path.display());
        }
    }

    for path in &orphans {
        let question = format!("Remove orphaned launcher {}?", path.display());
        if force || crate::utils::prompt::confirm_no_default(&question)? {
            match std::fs::remove_file(path) {
                Ok(()) => println!("  {} Removed {}", "✓".green(), path.display()),
                Err(e) => println!("  {} {}: {}", "✗".red(), path.display(), e),
            }
        }
    }

    if force || matches!(buckets_status, FileStatus::Corrupted(_)) {
        repair_buckets(&config, &buckets_path, &buckets_status)?;
    }
    if force || matches!(cache_status, FileStatus::Corrupted(_)) {
        repair_cache(&config, &cache_path, &cache_status, force)?;
    }

    println!();
    println!("{}", "Repair complete.".green());

    Ok(())
}

/// Bin entries wenget can prove it created and no loaded package owns
///
/// `bin_dir` is `~/.local/bin` (or `/usr/local/bin`), shared with unrelated
/// software, so absence from the loaded set is not evidence. Proof means a
/// symlink resolving under `{root}/apps/` (Unix), or a wenget-generated `.cmd`
/// launcher whose `%~dp0` target resolves there (Windows). Anything
/// unresolvable is left silent.
pub fn provable_orphan_shims(paths: &WenPaths, set: &InstalledSet) -> Result<Vec<PathBuf>> {
    let bin_dir = paths.bin_dir();
    if !bin_dir.exists() {
        return Ok(Vec::new());
    }

    let owned: std::collections::HashSet<String> = set
        .packages
        .values()
        .flat_map(|p| p.executables.values().cloned())
        .collect();

    let apps_dir = paths.apps_dir();
    let mut orphans = Vec::new();

    for entry in std::fs::read_dir(&bin_dir)
        .with_context(|| format!("Failed to read {}", bin_dir.display()))?
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let command = match path.file_stem().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if owned.contains(&command) {
            continue;
        }
        if points_into_apps(&path, &apps_dir) {
            orphans.push(path);
        }
    }

    orphans.sort();
    Ok(orphans)
}

/// Whether this bin entry is provably a wenget launcher into `apps_dir`
fn points_into_apps(path: &Path, apps_dir: &Path) -> bool {
    #[cfg(unix)]
    {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return false;
        };
        if !meta.file_type().is_symlink() {
            return false;
        }
        let Ok(target) = std::fs::read_link(path) else {
            return false;
        };
        let absolute = if target.is_absolute() {
            target
        } else {
            match path.parent() {
                Some(parent) => parent.join(target),
                None => return false,
            }
        };
        // Compare lexically: the target may not exist, which is exactly the
        // dangling case worth reporting.
        normalize_lexically(&absolute).starts_with(normalize_lexically(apps_dir))
    }

    #[cfg(windows)]
    {
        // Both `installer::shim` and `installer::script` write a `.cmd` whose
        // target is relative to the shim's own directory via `%~dp0`.
        let Ok(content) = std::fs::read_to_string(path) else {
            return false;
        };
        let Some(parent) = path.parent() else {
            return false;
        };
        for (_, tail) in content
            .match_indices("%~dp0")
            .map(|(i, m)| (i, &content[i + m.len()..]))
        {
            let relative = match tail.split('"').next() {
                Some(rel) if !rel.is_empty() => rel,
                _ => continue,
            };
            let resolved = normalize_lexically(&parent.join(relative));
            if resolved.starts_with(normalize_lexically(apps_dir)) {
                return true;
            }
        }
        false
    }
}

/// Resolve `.` and `..` without touching the filesystem
///
/// Launcher targets are relative and may dangle, so `canonicalize` is not an
/// option: a removed app directory is exactly the case worth reporting.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Packages whose recorded command has no launcher in `bin_dir`
pub fn missing_shims(paths: &WenPaths, set: &InstalledSet) -> Vec<(String, PathBuf)> {
    let mut missing = Vec::new();
    for package in set.packages.values() {
        for command in package.executables.values() {
            let shim = paths.bin_shim_path(command);
            if !shim.exists() {
                missing.push((command.clone(), shim));
            }
        }
    }
    missing.sort();
    missing
}

/// Repair buckets.json
fn repair_buckets(config: &Config, path: &std::path::Path, status: &FileStatus) -> Result<()> {
    print!("  Repairing buckets.json... ");

    match status {
        FileStatus::Corrupted(_) => {
            // Create backup before repair
            if let Ok(backup_path) = create_backup(path) {
                println!(
                    "{}",
                    format!("backup created: {}", backup_path.display()).yellow()
                );
            }

            // Reset to empty
            let new_config = BucketConfig::new();
            config.save_buckets(&new_config)?;

            println!(
                "  {} Reset to empty (use 'wenget bucket add' to re-add buckets)",
                "!".yellow()
            );
        }
        FileStatus::Missing => {
            // Create new file
            let new_config = BucketConfig::new();
            config.save_buckets(&new_config)?;
            println!("{}", "created".green());
        }
        FileStatus::Ok => {
            println!("{}", "skipped (already OK)".green());
        }
    }

    Ok(())
}

/// Repair manifest-cache.json
fn repair_cache(
    config: &Config,
    path: &std::path::PathBuf,
    status: &FileStatus,
    force: bool,
) -> Result<()> {
    print!("  Repairing manifest-cache.json... ");

    // In force mode, always rebuild; otherwise only repair corrupted/missing
    let should_rebuild = force || !matches!(status, FileStatus::Ok);

    if should_rebuild {
        // Delete existing file if exists
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        // Rebuild from buckets
        match config.rebuild_cache() {
            Ok(cache) => {
                println!(
                    "{} ({} packages cached)",
                    "rebuilt".green(),
                    cache.packages.len()
                );
            }
            Err(e) => {
                println!("{} ({})", "rebuild failed".yellow(), e);
                println!("    Cache will be rebuilt on next operation");
            }
        }
    } else {
        println!("{}", "skipped (already OK)".green());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::manifest::{InstalledPackage, PackageSource, CURRENT_META_VERSION};
    use tempfile::TempDir;

    fn package_with_command(command: &str) -> InstalledPackage {
        InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "tool".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: String::new(),
            executables: std::collections::HashMap::from([(
                "bin/tool".to_string(),
                command.to_string(),
            )]),
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "d".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "tool.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_orphan_detection_requires_provenance() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        std::fs::create_dir_all(paths.apps_dir()).unwrap();

        // 1. An unrelated regular file in bin_dir. Not ours, no provenance.
        std::fs::write(paths.bin_dir().join("system-tool"), b"#!/bin/sh\n").unwrap();

        // 2. A symlink to somewhere outside the wenget root. Not ours.
        let foreign = tmp.path().join("elsewhere");
        std::fs::write(&foreign, b"x").unwrap();
        std::os::unix::fs::symlink(&foreign, paths.bin_dir().join("foreign")).unwrap();

        // 3. A symlink into apps/ with no owning package. Provably ours, orphaned.
        let dangling_target = paths.apps_dir().join("ghost").join("bin").join("ghost");
        std::fs::create_dir_all(dangling_target.parent().unwrap()).unwrap();
        std::fs::write(&dangling_target, b"x").unwrap();
        std::os::unix::fs::symlink(&dangling_target, paths.bin_dir().join("ghost")).unwrap();

        let set = InstalledStore::new(paths.clone()).load().unwrap();
        let orphans = provable_orphan_shims(&paths, &set).unwrap();

        let names: Vec<String> = orphans
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["ghost".to_string()], "got {names:?}");
    }

    #[test]
    #[cfg(unix)]
    fn test_relative_symlink_into_apps_is_provable() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        std::fs::create_dir_all(paths.apps_dir().join("ghost")).unwrap();

        // The launcher target expressed relative to bin_dir, as `..` traversal.
        std::os::unix::fs::symlink("../apps/ghost/ghost", paths.bin_dir().join("ghost")).unwrap();

        let set = InstalledStore::new(paths.clone()).load().unwrap();
        let orphans = provable_orphan_shims(&paths, &set).unwrap();
        assert_eq!(orphans.len(), 1, "got {orphans:?}");
    }

    #[test]
    fn test_owned_launcher_is_not_an_orphan() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        let store = InstalledStore::new(paths.clone());
        store
            .save_package("tool", &package_with_command("tool"))
            .unwrap();

        #[cfg(unix)]
        {
            let target = paths.app_dir("tool").join("bin").join("tool");
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(&target, b"x").unwrap();
            std::os::unix::fs::symlink(&target, paths.bin_shim_path("tool")).unwrap();
        }

        let set = store.load().unwrap();
        assert!(provable_orphan_shims(&paths, &set).unwrap().is_empty());
    }

    #[test]
    fn test_missing_shim_is_reported() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        let store = InstalledStore::new(paths.clone());
        store
            .save_package("tool", &package_with_command("tool"))
            .unwrap();

        let set = store.load().unwrap();
        let missing = missing_shims(&paths, &set);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].0, "tool");
    }
}
