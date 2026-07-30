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
    let paths = WenPaths::new()?;

    // Load installed manifest
    let mut installed = config.get_or_create_installed()?;

    if installed.packages.is_empty() {
        println!("{}", "No packages installed".yellow());
        return Ok(());
    }

    if names.is_empty() {
        println!("{}", "No package names provided".yellow());
        println!("Usage: wenget del <name>...");
        return Ok(());
    }

    // Compile glob patterns
    let glob_patterns: Vec<Pattern> = names
        .iter()
        .map(|p| Pattern::new(p))
        .collect::<Result<_, _>>()?;

    // Find matching packages (match against both key and repo_name)
    let matching_packages: Vec<String> = installed
        .packages
        .iter()
        .filter(|(key, pkg)| {
            glob_patterns
                .iter()
                .any(|pattern| pattern.matches(key) || pattern.matches(&pkg.repo_name))
        })
        .map(|(key, _)| key.clone())
        .collect();

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
    let mut packages_to_delete: Vec<(String, Vec<String>)> = Vec::new();
    let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut final_to_delete: Vec<String> = Vec::new();

    for name in &matching_packages {
        if processed.contains(name) {
            continue;
        }

        // Get the package to find repo_name
        let pkg = installed.get_package(name).unwrap();
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
        let variants: Vec<_> = if let Some(ref filter) = variant_filter {
            all_variants
                .into_iter()
                .filter(|(_, pkg)| pkg.variant.as_deref() == Some(filter.as_str()))
                .collect()
        } else {
            all_variants
        };

        if variants.is_empty() {
            // No variants match the filter
            if let Some(ref filter) = variant_filter {
                println!(
                    "  {} No variant '{}' found for package '{}'",
                    "✗".yellow(),
                    filter,
                    name
                );
            }
            continue;
        }

        // Collect all variant keys
        let variant_keys: Vec<String> = variants.iter().map(|(key, _)| (*key).clone()).collect();

        for key in &variant_keys {
            processed.insert(key.clone());
        }

        packages_to_delete.push((name.clone(), variant_keys));
    }

    // Show packages to delete
    println!("{}", "Packages to delete:".bold());
    for (repo_name, variants) in &packages_to_delete {
        // Show repo name with variant filter info if applicable
        if let Some(ref filter) = variant_filter {
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

    // If there are variants and not using -y, ask which ones to delete
    if !yes {
        for (repo_name, variants) in &packages_to_delete {
            if variants.len() == 1 {
                // Only one variant, just add it
                final_to_delete.push(variants[0].clone());
            } else {
                // Has multiple variants, show selection dialog
                use dialoguer::MultiSelect;

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

                for &idx in &selections {
                    final_to_delete.push(variants[idx].clone());
                }
            }
        }
    } else {
        // -y flag: delete all
        for (_repo_name, variants) in &packages_to_delete {
            final_to_delete.extend(variants.clone());
        }
    }

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

    // Delete each package
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

    // Save updated manifest
    config.save_installed(&installed)?;

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

/// Delete a single package
fn delete_package(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledManifest,
    name: &str,
) -> Result<()> {
    // Get package info to find all command names
    let pkg = installed.get_package(name).context(format!(
        "Package '{}' not found in installed manifest",
        name
    ))?;

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

    // Remove from installed manifest
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

/// Delete wenget itself (complete uninstallation)
fn delete_self(yes: bool) -> Result<()> {
    println!("{}", "wenget Self-Deletion".bold().red());
    println!("{}", "═".repeat(60));
    println!();

    let paths = WenPaths::new()?;
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

    // Show what will be removed
    println!("{}", "The following will be removed:".yellow());
    println!();

    let mut step_num = 1;

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

    println!();
    println!("{}", "Proceeding with uninstallation...".cyan());
    println!();

    let exe_in_wenget = exe_path.starts_with(paths.root());
    let mut step_num = 1;

    // Step: Remove from PATH (if selected)
    if options.remove_path {
        println!("{} Removing from PATH...", format!("{}.", step_num).bold());
        match remove_from_path(&paths.bin_dir()) {
            Ok(()) => println!("   {} PATH updated", "✓".green()),
            Err(e) => println!("   {} Failed to update PATH: {}", "⚠".yellow(), e),
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
        delete_executable(&exe_path, exe_in_wenget, paths.root())?;
    }

    println!();
    println!("{}", "═".repeat(60));
    println!();
    println!("{}", "wenget uninstallation completed.".green().bold());
    println!();
    println!("{}", "Thank you for using wenget!".cyan());
    println!();

    Ok(())
}

/// Remove wenget bin directory from PATH
fn remove_from_path(bin_dir: &Path) -> Result<()> {
    let bin_dir_str = bin_dir.to_string_lossy();

    #[cfg(windows)]
    {
        remove_from_path_windows(&bin_dir_str)?;
    }

    #[cfg(not(windows))]
    {
        remove_from_path_unix(&bin_dir_str)?;
    }

    Ok(())
}

/// Remove from PATH on Windows
#[cfg(windows)]
fn remove_from_path_windows(bin_dir: &str) -> Result<()> {
    use std::process::Command;

    let ps_script = format!(
        r#"
        $oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($oldPath -like '*{}*') {{
            $newPath = ($oldPath -split ';' | Where-Object {{ $_ -ne '{}' }}) -join ';'
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Write-Output 'Removed'
        }} else {{
            Write-Output 'Not found'
        }}
        "#,
        bin_dir, bin_dir
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
        .context("Failed to execute PowerShell command")?;

    let result = String::from_utf8_lossy(&output.stdout);

    if !result.contains("Removed") && !result.contains("Not found") && !output.status.success() {
        return Err(anyhow::anyhow!("PowerShell command failed"));
    }

    Ok(())
}

/// Remove from PATH on Unix-like systems
#[cfg(not(windows))]
fn remove_from_path_unix(bin_dir: &str) -> Result<()> {
    let home = dirs::home_dir().context("Failed to determine home directory")?;

    let shell_configs = vec![
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".zshrc"),
        home.join(".profile"),
    ];

    for config_path in shell_configs {
        if config_path.exists() {
            if let Err(e) = remove_from_shell_config(&config_path, bin_dir) {
                log::warn!("Failed to update {}: {}", config_path.display(), e);
            }
        }
    }

    Ok(())
}

/// Remove wenget PATH entry from a shell configuration file
#[cfg(not(windows))]
fn remove_from_shell_config(config_path: &Path, bin_dir: &str) -> Result<()> {
    let content = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    // Remove lines containing the wenget PATH entry
    let new_content: String = content
        .lines()
        .filter(|line| {
            // Skip lines that contain the wenget bin directory or wenget comment
            !line.contains(bin_dir) && !line.contains("# wenget")
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Only write if content changed
    if new_content != content {
        fs::write(config_path, new_content.trim_end())
            .with_context(|| format!("Failed to write to {}", config_path.display()))?;
    }

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
    use std::process::Command;

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
    Command::new("cmd")
        .args(["/C", "start", "/min", script_path.to_str().unwrap()])
        .spawn()
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_specific_variant_not_duplicated_in_final_to_delete() {
        // Simulate the variant resolution logic
        let names = ["opencode::desktop.app".to_string()];
        let matching_packages = vec!["opencode::desktop.app".to_string()];

        let mut packages_to_delete: Vec<(String, Vec<String>)> = Vec::new();
        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut final_to_delete: Vec<String> = Vec::new();

        for name in &matching_packages {
            if processed.contains(name) {
                continue;
            }

            let is_specific_variant_request = names
                .iter()
                .any(|user_input| user_input.contains("::") && user_input == name);

            if is_specific_variant_request {
                packages_to_delete.push((name.clone(), vec![name.clone()]));
                // BUG WAS HERE: final_to_delete.push(name.clone());
                processed.insert(name.clone());
                continue;
            }
        }

        // Simulate the -y flag path
        for (_repo_name, variants) in &packages_to_delete {
            final_to_delete.extend(variants.clone());
        }

        // Should only appear ONCE
        assert_eq!(
            final_to_delete
                .iter()
                .filter(|x| *x == "opencode::desktop.app")
                .count(),
            1,
            "Specific variant should only appear once in final_to_delete"
        );
    }
}
