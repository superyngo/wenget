//! Delete command implementation

use crate::core::{Config, WenPaths};
use anyhow::{Context, Result};
use colored::Colorize;
use glob::Pattern;
use std::env;
use std::fs;
use std::path::Path;

/// Delete installed packages
pub fn run(
    names: Vec<String>,
    yes: bool,
    force: bool,
    variant_filter: Option<String>,
) -> Result<()> {
    // Check for self-deletion request
    if names.len() == 1 && names[0].to_lowercase() == "self" {
        return delete_self(yes);
    }

    let config = Config::new()?;
    let paths = config.paths().clone();

    // Load the installed set
    let mut installed = config.load_installed()?;

    if installed.packages.is_empty() {
        println!("{}", "No packages installed".yellow());
        return Ok(());
    }

    if names.is_empty() {
        println!("{}", "No package names provided".yellow());
        println!("Usage: wenget del <name>...");
        return Ok(());
    }

    let matching_packages = match_installed(&installed, &names)?;
    if matching_packages.is_empty() {
        println!(
            "{}",
            format!("No installed packages found matching: {:?}", names).yellow()
        );
        return Ok(());
    }

    // Check for wenget self-deletion
    if matching_packages.contains(&"wenget".to_string()) && !force {
        println!("{}", "Cannot delete wenget itself".red());
        println!("Use --force if you really want to delete it");
        return Ok(());
    }

    // Group packages by repo: find repos and their variants
    // Support both repo names ("bun") and specific variants ("bun::baseline")
    let packages_to_delete = group_delete_candidates(
        &installed,
        &names,
        &matching_packages,
        variant_filter.as_deref(),
    );
    print_delete_plan(&installed, &packages_to_delete, variant_filter.as_deref());

    let final_to_delete = if yes {
        packages_to_delete
            .into_iter()
            .flat_map(|(_, v)| v)
            .collect()
    } else {
        choose_variants(&installed, &packages_to_delete)?
    };

    if final_to_delete.is_empty() {
        println!("No packages selected for deletion");
        return Ok(());
    }

    // Confirm deletion
    if !yes && !crate::utils::prompt::confirm_no_default("\nProceed with deletion?")? {
        println!("Deletion cancelled");
        return Ok(());
    }

    println!();

    let mut success_count = 0;
    let mut fail_count = 0;

    for name in final_to_delete {
        println!("{} {}...", "Deleting".cyan(), name);

        match delete_package(&config, &paths, &mut installed, &name) {
            Ok(()) => {
                println!("  {} Deleted successfully", "✓".green());
                success_count += 1;
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                fail_count += 1;
            }
        }
    }

    // No record write: `delete_package` removed each app directory, and the
    // package record lived inside it.

    // Summary
    println!();
    println!("{}", "Summary:".bold());
    if success_count > 0 {
        println!("  {} {} package(s) deleted", "✓".green(), success_count);
    }
    if fail_count > 0 {
        println!("  {} {} package(s) failed", "✗".red(), fail_count);
    }

    Ok(())
}

/// Installed keys whose key or repo name matches any of the glob patterns in `names`
fn match_installed(installed: &crate::core::InstalledSet, names: &[String]) -> Result<Vec<String>> {
    let glob_patterns: Vec<Pattern> = names
        .iter()
        .map(|p| Pattern::new(p))
        .collect::<Result<_, _>>()?;

    let mut keys: Vec<String> = installed
        .packages
        .iter()
        .filter(|(key, pkg)| {
            glob_patterns
                .iter()
                .any(|pattern| pattern.matches(key) || pattern.matches(&pkg.repo_name))
        })
        .map(|(key, _)| key.clone())
        .collect();
    keys.sort();
    Ok(keys)
}

/// Print each repo about to be deleted with its installed variants
fn print_delete_plan(
    installed: &crate::core::InstalledSet,
    packages_to_delete: &[(String, Vec<String>)],
    variant_filter: Option<&str>,
) {
    println!("{}", "Packages to delete:".bold());
    for (repo_name, variants) in packages_to_delete {
        if let Some(filter) = variant_filter {
            println!("  • {} (variant: {})", repo_name.red(), filter);
        } else {
            println!("  • {} (all variants)", repo_name.red());
        }
        for variant_key in variants {
            let var_pkg = installed.get_package(variant_key).unwrap();
            let variant_label = var_pkg.variant.as_deref().unwrap_or("(default)");
            println!("    └─ {} v{}", variant_label.dimmed(), var_pkg.version);
        }
    }
}

/// Ask which variants to remove for every repo that has more than one
fn choose_variants(
    installed: &crate::core::InstalledSet,
    packages_to_delete: &[(String, Vec<String>)],
) -> Result<Vec<String>> {
    use dialoguer::MultiSelect;

    let mut chosen = Vec::new();
    for (repo_name, variants) in packages_to_delete {
        if variants.len() == 1 {
            chosen.push(variants[0].clone());
            continue;
        }

        let items: Vec<String> = variants
            .iter()
            .map(|key| {
                let pkg = installed.get_package(key).unwrap();
                let variant_label = pkg.variant.as_deref().unwrap_or("(default)");
                format!("{} ({})", variant_label, pkg.asset_name)
            })
            .collect();

        println!(
            "\nFound {} variant(s) of '{}'. Select which to remove:",
            variants.len(),
            repo_name
        );

        let selections = MultiSelect::new()
            .with_prompt("Space to select, Enter to confirm")
            .items(&items)
            .defaults(&vec![true; items.len()]) // Default: all selected
            .interact()?;

        if selections.is_empty() {
            println!("  Skipped {}", repo_name);
            continue;
        }
        chosen.extend(selections.into_iter().map(|idx| variants[idx].clone()));
    }
    Ok(chosen)
}

/// Delete a single package
fn delete_package(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    name: &str,
) -> Result<()> {
    // Get package info to find all command names
    let pkg = installed
        .get_package(name)
        .context(format!("Package '{}' is not installed", name))?;

    // Remove symlinks/shims for all command names
    for command_name in pkg.executables.values() {
        let bin_path = paths.bin_shim_path(command_name);
        if bin_path.exists() {
            fs::remove_file(&bin_path)
                .with_context(|| format!("Failed to remove shim/symlink for '{}'", command_name))?;
        }
    }

    // Also remove symlinks for legacy command_names (pre-migration packages)
    for command_name in &pkg.command_names {
        let bin_path = paths.bin_shim_path(command_name);
        if bin_path.exists() {
            fs::remove_file(&bin_path).ok();
        }
    }

    // Also remove old single-name shim/symlink if it exists (for packages installed with old version)
    let bin_path = paths.bin_shim_path(name);
    if bin_path.exists() {
        fs::remove_file(&bin_path).ok(); // Ignore errors here
    }

    // Remove app directory
    let app_dir = paths.app_dir(name);
    if app_dir.exists() {
        fs::remove_dir_all(&app_dir)?;
    }

    // Remove from the in-memory installed set
    installed.remove_package(name);

    Ok(())
}

/// Removal options for self-deletion
#[derive(Debug, Clone, Copy)]
struct RemovalOptions {
    remove_data: bool,
    remove_path: bool,
    remove_binary: bool,
}

impl RemovalOptions {
    fn all() -> Self {
        Self {
            remove_data: true,
            remove_path: true,
            remove_binary: true,
        }
    }
}

/// Show interactive menu for selecting what to remove
fn show_removal_menu() -> Result<RemovalOptions> {
    use dialoguer::MultiSelect;

    let items = vec![
        "Apps & data (~/.wenget/)",
        "PATH configuration",
        "wenget binary",
    ];

    let defaults = vec![true, true, true];

    let selections = MultiSelect::new()
        .with_prompt("What would you like to remove?")
        .items(&items)
        .defaults(&defaults)
        .interact()
        .context("Failed to get user selection")?;

    Ok(RemovalOptions {
        remove_data: selections.contains(&0),
        remove_path: selections.contains(&1),
        remove_binary: selections.contains(&2),
    })
}

/// Print what will be removed based on selected options
fn print_self_delete_plan(options: &RemovalOptions, paths: &WenPaths, exe_path: &Path) {
    println!("{}", "The following will be removed:".yellow());
    println!();

    let mut step_num = 1;
    if options.remove_data {
        println!(
            "  {} wenget launchers in {}",
            format!("{}.", step_num).bold(),
            paths.bin_dir().display()
        );
        println!();
        step_num += 1;
    }

    if options.remove_data {
        println!(
            "  {} All wenget directories and files:",
            format!("{}.", step_num).bold()
        );
        println!("     {}", paths.root().display());
        println!();
        step_num += 1;
    }

    if options.remove_path {
        println!(
            "  {} wenget from PATH environment variable",
            format!("{}.", step_num).bold()
        );
        println!();
        step_num += 1;
    }

    if options.remove_binary {
        println!(
            "  {} The wenget executable itself",
            format!("{}.", step_num).bold()
        );
        println!("     {}", exe_path.display());
        println!();
    }
}

/// Execute the removal steps for self-deletion
fn execute_self_deletion(
    options: &RemovalOptions,
    paths: &WenPaths,
    exe_path: &Path,
) -> Result<()> {
    println!();
    println!("{}", "Proceeding with uninstallation...".cyan());
    println!();

    let exe_in_wenget = exe_path.starts_with(paths.root());
    let mut step_num = 1;

    // Step: Remove from PATH (if selected)
    if options.remove_path {
        println!("{} Removing from PATH...", format!("{}.", step_num).bold());
        if let Err(e) = remove_from_path(paths) {
            println!("   {} Failed to update PATH: {}", "⚠".yellow(), e);
        }
        println!();
        step_num += 1;
    }

    // Step: Remove launchers before the root they point into disappears
    if options.remove_data {
        println!("{} Removing launchers...", format!("{}.", step_num).bold());
        for path in remove_launchers(paths) {
            println!("   {} Removed: {}", "✓".green(), path.display());
        }
        println!();
        step_num += 1;
    }

    // Step: Delete wenget directories (if selected)
    if options.remove_data {
        println!(
            "{} Deleting wenget directories...",
            format!("{}.", step_num).bold()
        );
        if exe_in_wenget && options.remove_binary {
            println!(
                "   {} Scheduled for deletion (executable is inside .wenget)",
                "✓".yellow()
            );
            println!("      Directory will be deleted after wenget exits");
        } else if paths.root().exists() {
            match fs::remove_dir_all(paths.root()) {
                Ok(()) => println!("   {} Deleted: {}", "✓".green(), paths.root().display()),
                Err(e) => println!("   {} Failed to delete directory: {}", "✗".red(), e),
            }
        } else {
            println!("   {} Directory already removed", "✓".green());
        }
        println!();
        step_num += 1;
    }

    // Step: Delete the executable (if selected)
    if options.remove_binary {
        println!(
            "{} Deleting wenget executable...",
            format!("{}.", step_num).bold()
        );
        // A WENGET_ROOT sandbox must not delete an executable living outside it
        if paths.is_root_override() && !exe_in_wenget {
            println!(
                "   {} WENGET_ROOT is set; keeping executable outside the root: {}",
                "⚠".yellow(),
                exe_path.display()
            );
        } else {
            delete_executable(exe_path, exe_in_wenget, paths.root())?;
        }
    }

    Ok(())
}

/// Remove bin entries that provably launch something inside the wenget root
///
/// Covers package launchers (including legacy names) and the `wenget` launcher
/// written by `install.sh` / `init`. Anything else in the bin directory,
/// including user files and links elsewhere, is left alone.
fn remove_launchers(paths: &WenPaths) -> Vec<std::path::PathBuf> {
    let bin_dir = paths.bin_dir();
    let root = paths.root();
    if bin_dir.starts_with(root) {
        return Vec::new(); // removed together with the root
    }
    let Ok(entries) = fs::read_dir(&bin_dir) else {
        return Vec::new();
    };
    let mut removed = Vec::new();
    for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
        if !crate::commands::repair::points_into_apps(&path, root) {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => removed.push(path),
            Err(e) => println!("   {} {}: {}", "⚠".yellow(), path.display(), e),
        }
    }
    removed.sort();
    removed
}

/// Delete wenget itself (complete uninstallation)
fn delete_self(yes: bool) -> Result<()> {
    println!("{}", "wenget Self-Deletion".bold().red());
    println!("{}", "═".repeat(60));
    println!();

    // Honour `custom_bin_path` so the launcher step looks in the right place
    let paths = Config::new()?.paths().clone();
    let exe_path = env::current_exe().context("Failed to get current executable path")?;

    // Determine removal options
    let options = if yes {
        // When -y flag is used, remove everything (current behavior)
        RemovalOptions::all()
    } else {
        // Show interactive menu
        show_removal_menu()?
    };

    // Check if user selected nothing
    if !options.remove_data && !options.remove_path && !options.remove_binary {
        println!();
        println!(
            "{}",
            "Nothing selected for removal. Deletion cancelled.".yellow()
        );
        return Ok(());
    }

    print_self_delete_plan(&options, &paths, &exe_path);

    // Confirm deletion (only if -y not used)
    if !yes {
        println!("{}", "═".repeat(60));
        println!();
        println!("{}", "Are you sure you want to proceed?".bold().red());

        if !crate::utils::prompt::confirm_no_default("")? {
            println!();
            println!("{}", "Deletion cancelled".green());
            return Ok(());
        }
    }

    execute_self_deletion(&options, &paths, &exe_path)?;

    println!();
    println!("{}", "═".repeat(60));
    println!();
    println!("{}", "wenget uninstallation completed.".green().bold());
    println!();
    println!("{}", "Thank you for using wenget!".cyan());
    println!();

    Ok(())
}

/// Remove exactly the PATH entries `wenget init` recorded (see `core::path_record`)
///
/// Without a record PATH is left alone: the bin directory may be one the user
/// put on PATH themselves.
fn remove_from_path(paths: &WenPaths) -> Result<()> {
    use crate::core::path_record::{PathEntry, PathRecord};

    // A WENGET_ROOT sandbox never edited the real rc files or registry PATH
    if paths.is_root_override() {
        println!("   {} WENGET_ROOT is set; PATH untouched", "ℹ".cyan());
        return Ok(());
    }

    let record = PathRecord::load(paths);
    if record.entries.is_empty() {
        println!(
            "   {} No PATH change recorded by `wenget init`; PATH untouched",
            "ℹ".cyan()
        );
        println!(
            "      If {} is on your PATH, remove it manually",
            paths.bin_dir().display()
        );
        return Ok(());
    }

    for entry in &record.entries {
        let (what, result) = match entry {
            PathEntry::ShellFile { file, dir } => (
                format!("{} from {}", dir.display(), file.display()),
                remove_shell_block(file, dir),
            ),
            PathEntry::UserRegistry { dir } => (
                format!("{} from user PATH", dir.display()),
                remove_registry_entry(dir, false),
            ),
            PathEntry::SystemRegistry { dir } => (
                format!("{} from system PATH", dir.display()),
                remove_registry_entry(dir, true),
            ),
        };
        match result {
            Ok(()) => println!("   {} Removed {}", "✓".green(), what),
            Err(e) => println!("   {} Failed to remove {}: {}", "⚠".yellow(), what, e),
        }
    }

    Ok(())
}

/// Remove the PATH block `init` appended to a shell rc file
fn remove_shell_block(file: &Path, dir: &Path) -> Result<()> {
    if !file.exists() {
        return Ok(());
    }
    let content =
        fs::read_to_string(file).with_context(|| format!("Failed to read {}", file.display()))?;
    let stripped = crate::core::path_record::strip_shell_block(&content, dir);
    if stripped != content {
        fs::write(file, stripped)
            .with_context(|| format!("Failed to write to {}", file.display()))?;
    }
    Ok(())
}

/// Remove `dir` from the Windows user or system PATH
#[cfg(windows)]
fn remove_registry_entry(dir: &Path, system: bool) -> Result<()> {
    if system {
        crate::core::registry::remove_from_system_path(dir)?;
    } else {
        crate::core::registry::remove_from_user_path(dir)?;
    }
    Ok(())
}

/// Registry entries are only ever recorded on Windows
#[cfg(not(windows))]
fn remove_registry_entry(_dir: &Path, _system: bool) -> Result<()> {
    Ok(())
}

/// Delete the executable (platform-specific implementation)
fn delete_executable(exe_path: &Path, exe_in_wenget: bool, wenget_root: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        delete_executable_windows(exe_path, exe_in_wenget, wenget_root)
    }

    #[cfg(not(windows))]
    {
        delete_executable_unix(exe_path, exe_in_wenget, wenget_root)
    }
}

/// Delete executable on Windows
/// On Windows, we can't delete a running executable directly,
/// so we use a batch script that waits and then deletes it
#[cfg(windows)]
fn delete_executable_windows(
    exe_path: &Path,
    exe_in_wenget: bool,
    wenget_root: &Path,
) -> Result<()> {
    // Create a temporary batch script to delete the executable after exit
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join("wenget_uninstall.bat");

    let exe_path_str = exe_path.to_string_lossy();
    let script_content = if exe_in_wenget {
        // If executable is inside .wenget, delete the entire directory
        let wenget_root_str = wenget_root.to_string_lossy();
        format!(
            r#"@echo off
timeout /t 2 /nobreak >nul
rd /s /q "{}"
del /f /q "%~f0"
"#,
            wenget_root_str
        )
    } else {
        // Otherwise just delete the executable
        format!(
            r#"@echo off
timeout /t 2 /nobreak >nul
del /f /q "{}"
del /f /q "%~f0"
"#,
            exe_path_str
        )
    };

    fs::write(&script_path, script_content).context("Failed to create uninstall script")?;

    // Launch the script in background
    crate::utils::process::spawn_script_detached(&script_path, "/min")
        .context("Failed to launch uninstall script")?;

    println!(
        "   {} Scheduled for deletion (will be removed in 2 seconds)",
        "✓".green()
    );

    Ok(())
}

/// Delete executable on Unix
#[cfg(not(windows))]
fn delete_executable_unix(exe_path: &Path, exe_in_wenget: bool, wenget_root: &Path) -> Result<()> {
    use std::process::Command;

    // Create a shell script to delete the executable after exit
    let temp_dir = env::temp_dir();
    let script_path = temp_dir.join("wenget_uninstall.sh");

    let exe_path_str = exe_path.to_string_lossy();
    let script_content = if exe_in_wenget {
        // If executable is inside .wenget, delete the entire directory
        let wenget_root_str = wenget_root.to_string_lossy();
        format!(
            r#"#!/bin/sh
sleep 2
rm -rf "{}"
rm -f "$0"
"#,
            wenget_root_str
        )
    } else {
        // Otherwise just delete the executable
        format!(
            r#"#!/bin/sh
sleep 2
rm -f "{}"
rm -f "$0"
"#,
            exe_path_str
        )
    };

    fs::write(&script_path, script_content).context("Failed to create uninstall script")?;

    // Make script executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)?;
    }

    // Launch the script in background
    Command::new("sh")
        .arg(&script_path)
        .spawn()
        .context("Failed to launch uninstall script")?;

    println!(
        "   {} Scheduled for deletion (will be removed in 2 seconds)",
        "✓".green()
    );

    Ok(())
}

/// Group matched keys into `(label, variant keys)` deletion entries
///
/// An input containing `::` that matches a key selects just that variant; any
/// other match expands to every installed variant of the repo (optionally
/// narrowed by `variant_filter`). Each key appears in at most one entry.
fn group_delete_candidates(
    installed: &crate::core::InstalledSet,
    names: &[String],
    matching_packages: &[String],
    variant_filter: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let mut packages_to_delete: Vec<(String, Vec<String>)> = Vec::new();
    let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();

    for name in matching_packages {
        if processed.contains(name) {
            continue;
        }

        // Get the package to find repo_name
        let Some(pkg) = installed.get_package(name) else {
            continue;
        };
        let repo_name = &pkg.repo_name;

        // Check if user explicitly requested this specific variant
        // (i.e., user input contained "::" AND matched this exact key)
        let is_specific_variant_request = names.iter().any(|user_input| {
            user_input.contains("::")
                && (user_input == name
                    || Pattern::new(user_input)
                        .map(|p| p.matches(name))
                        .unwrap_or(false))
        });

        if is_specific_variant_request {
            // User explicitly requested this variant - show it individually
            packages_to_delete.push((name.clone(), vec![name.clone()]));
            processed.insert(name.clone());
            continue;
        }

        // This is a repo-level request - find all variants
        let all_variants = installed.find_by_repo(repo_name);

        if all_variants.is_empty() {
            continue;
        }

        // Apply variant filter if specified
        let variants: Vec<_> = if let Some(filter) = variant_filter {
            all_variants
                .into_iter()
                .filter(|(_, pkg)| pkg.variant.as_deref() == Some(filter))
                .collect()
        } else {
            all_variants
        };

        if variants.is_empty() {
            // No variants match the filter
            if let Some(filter) = variant_filter {
                println!(
                    "  {} No variant '{}' found for package '{}'",
                    "✗".yellow(),
                    filter,
                    name
                );
            }
            continue;
        }

        // Collect all variant keys, in a stable order
        let mut variant_keys: Vec<String> =
            variants.iter().map(|(key, _)| (*key).clone()).collect();
        variant_keys.sort();

        for key in &variant_keys {
            processed.insert(key.clone());
        }

        // Label the group with the repo, not whichever variant matched first
        let label = if repo_name.is_empty() {
            name.split("::").next().unwrap_or(name).to_string()
        } else {
            repo_name.clone()
        };
        packages_to_delete.push((label, variant_keys));
    }

    packages_to_delete
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::manifest::{InstalledPackage, PackageSource, CURRENT_SCHEMA_VERSION};

    #[cfg(unix)]
    #[test]
    fn test_remove_launchers_only_touches_wenget_links() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().join("root");
        let bin = tmp.path().join("bin");
        let target = root.join("apps/rg/rg");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::create_dir_all(&bin).unwrap();
        fs::write(&target, "").unwrap();
        symlink(&target, bin.join("rg")).unwrap();
        symlink(root.join("bin/wenget"), bin.join("wenget")).unwrap();
        symlink("/bin/ls", bin.join("userlink")).unwrap();
        fs::write(bin.join("userfile"), "").unwrap();

        let paths = WenPaths::with_root_and_bin(root, bin.clone());
        let removed = remove_launchers(&paths);

        assert_eq!(removed, vec![bin.join("rg"), bin.join("wenget")]);
        assert!(bin.join("userlink").symlink_metadata().is_ok());
        assert!(bin.join("userfile").exists());
    }

    fn pkg(variant: Option<&str>) -> InstalledPackage {
        InstalledPackage {
            schema_version: CURRENT_SCHEMA_VERSION,
            repo_name: "opencode".to_string(),
            variant: variant.map(str::to_string),
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: String::new(),
            executables: Default::default(),
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: String::new(),
            download_url: None,
        }
    }

    fn set() -> crate::core::InstalledSet {
        let mut set = crate::core::InstalledSet::default();
        set.upsert_package("opencode".to_string(), pkg(None));
        set.upsert_package(
            "opencode::desktop.app".to_string(),
            pkg(Some("desktop.app")),
        );
        set
    }

    #[test]
    fn test_specific_variant_not_duplicated_in_final_to_delete() {
        let set = set();
        let key = "opencode::desktop.app".to_string();
        let groups = group_delete_candidates(
            &set,
            std::slice::from_ref(&key),
            std::slice::from_ref(&key),
            None,
        );
        assert_eq!(groups, vec![(key.clone(), vec![key])]);
    }

    #[test]
    fn test_repo_request_expands_to_all_variants_once() {
        let set = set();
        let mut matching = vec!["opencode".to_string(), "opencode::desktop.app".to_string()];
        matching.sort();
        let groups = group_delete_candidates(&set, &["opencode*".to_string()], &matching, None);
        assert_eq!(groups.len(), 1);
        let mut keys = groups[0].1.clone();
        keys.sort();
        assert_eq!(keys, matching);
    }

    #[test]
    fn test_group_label_is_repo_and_variants_sorted() {
        let set = set();
        let matching = vec!["opencode::desktop.app".to_string(), "opencode".to_string()];
        let groups = group_delete_candidates(&set, &["opencode".to_string()], &matching, None);
        assert_eq!(
            groups,
            vec![(
                "opencode".to_string(),
                vec!["opencode".to_string(), "opencode::desktop.app".to_string()]
            )]
        );
        assert_eq!(
            match_installed(&set, &["opencode*".to_string()]).unwrap(),
            vec!["opencode".to_string(), "opencode::desktop.app".to_string()]
        );
    }

    #[test]
    fn test_variant_filter_narrows_group() {
        let set = set();
        let groups = group_delete_candidates(
            &set,
            &["opencode".to_string()],
            &["opencode".to_string()],
            Some("desktop.app"),
        );
        assert_eq!(
            groups,
            vec![(
                "opencode".to_string(),
                vec!["opencode::desktop.app".to_string()]
            )]
        );
    }
}
