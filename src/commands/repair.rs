//! Repair command for wenget
//!
//! Checks and repairs corrupted configuration files.

use crate::bucket::BucketConfig;
use crate::cache::ManifestCache;
use crate::core::manifest::InstalledManifest;
use crate::core::repair::{check_json_file, create_backup, FileStatus};
use crate::core::Config;
use anyhow::Result;
use colored::Colorize;

/// Run the repair command
pub fn run(force: bool) -> Result<()> {
    println!("{}", "Checking wenget configuration files...".cyan());
    println!();

    let config = Config::new()?;
    let paths = config.paths();

    // Check all config files
    let installed_path = paths.installed_json();
    let buckets_path = paths.buckets_json();
    let cache_path = paths.manifest_cache_json();

    let installed_status = check_json_file::<InstalledManifest>(&installed_path);
    let buckets_status = check_json_file::<BucketConfig>(&buckets_path);
    let cache_status = check_json_file::<ManifestCache>(&cache_path);

    // Display status
    println!("{}", "Configuration File Status:".bold());
    println!("  installed.json:      {}", installed_status);
    println!("  buckets.json:        {}", buckets_status);
    println!("  manifest-cache.json: {}", cache_status);
    println!();

    // Count issues
    let mut issues = 0;
    if matches!(installed_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }
    if matches!(buckets_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }
    if matches!(cache_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }

    if issues == 0 && !force {
        println!("{}", "All configuration files are OK.".green());
        return Ok(());
    }

    if force {
        println!("{}", "Force mode: Rebuilding all files...".yellow());
        println!();
    } else {
        println!(
            "{} {} corrupted file(s) found. Repairing...",
            "!".yellow(),
            issues
        );
        println!();
    }

    // Repair installed.json if corrupted or force mode
    if force || matches!(installed_status, FileStatus::Corrupted(_)) {
        repair_installed(&config, &installed_path, &installed_status)?;
    }

    // Repair buckets.json if corrupted or force mode
    if force || matches!(buckets_status, FileStatus::Corrupted(_)) {
        repair_buckets(&config, &buckets_path, &buckets_status)?;
    }

    // Repair cache if corrupted or force mode
    if force || matches!(cache_status, FileStatus::Corrupted(_)) {
        repair_cache(&config, &cache_path, &cache_status, force)?;
    }

    println!();
    println!("{}", "Repair complete.".green());

    Ok(())
}

/// Repair installed.json
fn repair_installed(config: &Config, path: &std::path::Path, status: &FileStatus) -> Result<()> {
    print!("  Repairing installed.json... ");

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
            let new_manifest = InstalledManifest::new();
            config.save_installed(&new_manifest)?;

            println!(
                "  {} Reset to empty (previous package records lost)",
                "!".red()
            );
        }
        FileStatus::Missing => {
            // Create new file
            let new_manifest = InstalledManifest::new();
            config.save_installed(&new_manifest)?;
            println!("{}", "created".green());
        }
        FileStatus::Ok => {
            println!("{}", "skipped (already OK)".green());
        }
    }

    Ok(())
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
