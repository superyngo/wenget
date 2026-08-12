//! Rename command implementation

use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Input, Select};
use std::fs;
use std::path::Path;

use crate::core::{Config, InstalledManifest, InstalledPackage};
use crate::installer;

/// Run the rename command
///
/// Supports two modes:
/// 1. Direct: `wenget rename <old_cmd> <new_cmd>` - rename specified command
/// 2. Interactive: `wenget rename <package_name>` - select from multiple commands
pub fn run(old_name: String, new_name: Option<String>, config: &Config) -> Result<()> {
    let paths = config.paths();
    let mut installed = config.load_installed()?;

    // Find every command that could match `old_name`, across all installed
    // variants of a package (a repo name can resolve to several separately
    // installed variant packages, each contributing its own command(s)).
    let candidates = find_command_candidates(&installed, &old_name)?;

    let (pkg_key, old_cmd_name, _package) = if candidates.len() == 1 {
        let c = candidates.into_iter().next().unwrap();
        (c.pkg_key, c.cmd_name, c.package)
    } else if new_name.is_some() {
        // Direct mode requires an unambiguous target; a new name was given
        // but `old_name` still resolves to several commands.
        let names: Vec<String> = candidates.iter().map(|c| c.cmd_name.clone()).collect();
        anyhow::bail!(
            "'{}' matches multiple commands ({}); specify the exact command name to rename",
            old_name,
            names.join(", ")
        );
    } else {
        select_command_interactive(&candidates)?
    };

    // Get or prompt for new name
    let final_new_name = if let Some(new_name) = new_name {
        new_name
    } else {
        prompt_for_new_name(&old_cmd_name)?
    };

    // Validate new name doesn't conflict
    validate_new_name(&installed, &pkg_key, &final_new_name)?;

    println!(
        "{} Renaming command: {} → {}",
        "ℹ".cyan(),
        old_cmd_name.yellow(),
        final_new_name.green()
    );

    // Perform rename
    rename_command(
        paths,
        &mut installed,
        &pkg_key,
        &old_cmd_name,
        &final_new_name,
    )?;

    // Save updated manifest
    config.save_installed(&installed)?;

    println!("{} Successfully renamed command", "✓".green().bold());
    println!(
        "  {} New command: {}",
        "ℹ".cyan(),
        final_new_name.green().bold()
    );

    Ok(())
}

/// A command that could be the rename target, together with the installed
/// package (variant) it belongs to.
struct CommandCandidate {
    pkg_key: String,
    cmd_name: String,
    package: InstalledPackage,
}

/// Find every command matching `name`.
///
/// Resolution order:
/// 1. Exact package key (e.g. `confy-desktop-64`): every command of that package.
/// 2. Repo name (e.g. `confy`): every command of every installed variant
///    sharing that repo name.
/// 3. Exact command name (e.g. `confyd`): that single command.
fn find_command_candidates(
    installed: &InstalledManifest,
    name: &str,
) -> Result<Vec<CommandCandidate>> {
    // 1. Direct package key lookup
    if let Some(package) = installed.packages.get(name) {
        let cmds = package.get_command_names();
        if cmds.is_empty() {
            anyhow::bail!("Package '{}' has no commands", name);
        }
        return Ok(cmds
            .into_iter()
            .map(|c| CommandCandidate {
                pkg_key: name.to_string(),
                cmd_name: c.to_string(),
                package: package.clone(),
            })
            .collect());
    }

    // 2. Match by repo_name across all installed variants
    let variants = installed.find_by_repo(name);
    if !variants.is_empty() {
        let candidates: Vec<CommandCandidate> = variants
            .into_iter()
            .flat_map(|(key, package)| {
                package
                    .get_command_names()
                    .into_iter()
                    .map(|c| CommandCandidate {
                        pkg_key: key.clone(),
                        cmd_name: c.to_string(),
                        package: package.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        if candidates.is_empty() {
            anyhow::bail!("Package '{}' has no commands", name);
        }
        return Ok(candidates);
    }

    // 3. Match by exact command name
    for (key, package) in &installed.packages {
        if package.get_command_names().contains(&name) {
            return Ok(vec![CommandCandidate {
                pkg_key: key.clone(),
                cmd_name: name.to_string(),
                package: package.clone(),
            }]);
        }
    }

    anyhow::bail!(
        "Package or command '{}' not found. Use 'wenget ls' to see installed packages.",
        name
    )
}

/// Interactively select a command when multiple candidates match
fn select_command_interactive(
    candidates: &[CommandCandidate],
) -> Result<(String, String, InstalledPackage)> {
    println!("{} Multiple commands found:", "ℹ".cyan());

    let items: Vec<String> = candidates
        .iter()
        .map(|c| match &c.package.variant {
            Some(variant) => format!("{} (variant: {})", c.cmd_name, variant),
            None => c.cmd_name.clone(),
        })
        .collect();

    let selection = Select::new()
        .with_prompt("Select command to rename")
        .items(&items)
        .default(0)
        .interact()
        .context("Failed to get user selection")?;

    let chosen = &candidates[selection];
    Ok((
        chosen.pkg_key.clone(),
        chosen.cmd_name.clone(),
        chosen.package.clone(),
    ))
}

/// Prompt user for new command name
fn prompt_for_new_name(old_name: &str) -> Result<String> {
    let new_name: String = Input::new()
        .with_prompt(format!("New name for '{}'", old_name))
        .interact_text()
        .context("Failed to get user input")?;

    if new_name.trim().is_empty() {
        anyhow::bail!("New name cannot be empty");
    }

    Ok(new_name.trim().to_string())
}

/// Validate that new name doesn't conflict with existing commands
fn validate_new_name(
    installed: &InstalledManifest,
    exclude_key: &str,
    new_name: &str,
) -> Result<()> {
    for (key, package) in &installed.packages {
        if key == exclude_key {
            continue; // Skip the package we're renaming
        }

        if package.get_command_names().contains(&new_name) {
            anyhow::bail!(
                "Command name '{}' is already used by package '{}'",
                new_name,
                key
            );
        }
    }

    Ok(())
}

/// Perform the actual rename operation
///
/// Updates symlink/shim and modifies InstalledPackage.executables
fn rename_command(
    paths: &crate::core::WenPaths,
    installed: &mut InstalledManifest,
    pkg_key: &str,
    old_cmd: &str,
    new_cmd: &str,
) -> Result<()> {
    let package = installed
        .packages
        .get(pkg_key)
        .context("Package not found in manifest")?;

    // Find the executable path for the old command name
    let _exe_path_key = package
        .get_exe_path_for_command(old_cmd)
        .map(|s| s.to_string())
        .or_else(|| {
            // Fallback: check legacy command_names
            package
                .command_names
                .iter()
                .position(|c| c == old_cmd)
                .map(|_| old_cmd.to_string())
        })
        .context("Command not found in package")?;

    // Get install path
    let install_path = Path::new(&package.install_path);
    if !install_path.exists() {
        anyhow::bail!("Install path does not exist: {}", install_path.display());
    }

    // Read the target of the old symlink/shim before removing it
    let old_shim = paths.bin_shim_path(old_cmd);

    #[cfg(unix)]
    let target_binary = if old_shim.exists() {
        // Read symlink target
        fs::read_link(&old_shim)
            .with_context(|| format!("Failed to read symlink: {}", old_shim.display()))?
    } else {
        anyhow::bail!("Old symlink does not exist: {}", old_shim.display());
    };

    #[cfg(windows)]
    let target_binary = if old_shim.exists() {
        // Read shim target from .cmd file
        read_shim_target(&old_shim)?
    } else {
        anyhow::bail!("Old shim does not exist: {}", old_shim.display());
    };

    // Remove old symlink/shim
    if old_shim.exists() {
        fs::remove_file(&old_shim)
            .with_context(|| format!("Failed to remove old shim: {}", old_shim.display()))?;
        log::info!("Removed old shim: {}", old_shim.display());
    }

    // Create new symlink/shim pointing to the same target
    #[cfg(unix)]
    {
        installer::create_symlink(&target_binary, &paths.bin_dir().join(new_cmd))
            .context("Failed to create new symlink")?;
    }

    #[cfg(windows)]
    {
        installer::create_shim(
            &target_binary,
            &paths.bin_dir().join(format!("{}.cmd", new_cmd)),
            new_cmd,
        )
        .context("Failed to create new shim")?;
    }

    log::info!("Created new shim/symlink: {}", new_cmd);

    // Update executables map in the package
    let package_mut = installed
        .packages
        .get_mut(pkg_key)
        .context("Package disappeared during rename")?;

    // Update executables map if the command is there
    if let Some(value) = package_mut
        .executables
        .values_mut()
        .find(|v| v.as_str() == old_cmd)
    {
        *value = new_cmd.to_string();
    }
    // Also update legacy command_names if present
    if let Some(pos) = package_mut.command_names.iter().position(|c| c == old_cmd) {
        package_mut.command_names[pos] = new_cmd.to_string();
    }

    Ok(())
}

/// Read the target binary path from a Windows shim (.cmd file)
#[cfg(windows)]
fn read_shim_target(shim_path: &Path) -> Result<std::path::PathBuf> {
    let content = fs::read_to_string(shim_path)
        .with_context(|| format!("Failed to read shim file: {}", shim_path.display()))?;

    // Parse the shim to extract the target binary path.
    // Shims are generated (see installer::shim / installer::init) as two lines:
    //   @echo off
    //   "%~dp0relative\path\to\binary.exe" %*
    // (or an absolute path instead of %~dp0). The quoted path is not on the
    // same line as "@echo off", so skip that line rather than requiring '@'.
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("@echo off") {
            continue;
        }
        if let Some(start) = trimmed.find('"') {
            if let Some(end) = trimmed[start + 1..].find('"') {
                let path_str = &trimmed[start + 1..start + 1 + end];
                // Resolve %~dp0 (directory of the shim)
                let resolved = if path_str.contains("%~dp0") {
                    let shim_dir = shim_path.parent().context("Shim has no parent directory")?;
                    let relative = path_str.replace("%~dp0", "");
                    shim_dir.join(relative)
                } else {
                    std::path::PathBuf::from(path_str)
                };
                return Ok(resolved);
            }
        }
    }

    anyhow::bail!("Failed to parse shim target from: {}", shim_path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_validate_new_name_success() {
        let mut manifest = InstalledManifest::new();
        let mut exe1 = HashMap::new();
        exe1.insert("bin/oldcmd".to_string(), "oldcmd".to_string());

        let package = InstalledPackage {
            repo_name: "pkg1".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: "/path/to/pkg1".to_string(),
            executables: exe1,
            source: crate::core::manifest::PackageSource::Bucket {
                name: "test".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "pkg1.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };
        manifest.packages.insert("pkg1".to_string(), package);

        assert!(validate_new_name(&manifest, "pkg1", "newcmd").is_ok());
    }

    #[test]
    fn test_validate_new_name_conflict() {
        let mut manifest = InstalledManifest::new();

        // First package
        let mut exe1 = HashMap::new();
        exe1.insert("bin/cmd1".to_string(), "cmd1".to_string());

        let package1 = InstalledPackage {
            repo_name: "pkg1".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: "/path/to/pkg1".to_string(),
            executables: exe1,
            source: crate::core::manifest::PackageSource::Bucket {
                name: "test".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "pkg1.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };
        manifest.packages.insert("pkg1".to_string(), package1);

        // Second package
        let mut exe2 = HashMap::new();
        exe2.insert("bin/cmd2".to_string(), "cmd2".to_string());

        let package2 = InstalledPackage {
            repo_name: "pkg2".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: "/path/to/pkg2".to_string(),
            executables: exe2,
            source: crate::core::manifest::PackageSource::Bucket {
                name: "test".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "pkg2.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };
        manifest.packages.insert("pkg2".to_string(), package2);

        // Try to rename pkg1's cmd to "cmd2" which is already used
        assert!(validate_new_name(&manifest, "pkg1", "cmd2").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn test_read_shim_target_relative() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        // Matches the format produced by installer::shim::create_shim
        let shim_path = bin_dir.join("confy-desktop-64.cmd");
        std::fs::write(
            &shim_path,
            "@echo off\r\n\"%~dp0..\\apps\\confy\\confy-desktop.exe\" %*\r\n",
        )
        .unwrap();

        let target = read_shim_target(&shim_path).unwrap();
        assert_eq!(target, bin_dir.join("..\\apps\\confy\\confy-desktop.exe"));
    }

    #[cfg(windows)]
    #[test]
    fn test_read_shim_target_absolute() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        // Matches the format produced by installer::init
        let shim_path = bin_dir.join("foo.cmd");
        std::fs::write(
            &shim_path,
            "@echo off\r\n\"C:\\Program Files\\wenget\\apps\\foo\\foo.exe\" %*\r\n",
        )
        .unwrap();

        let target = read_shim_target(&shim_path).unwrap();
        assert_eq!(
            target,
            std::path::PathBuf::from("C:\\Program Files\\wenget\\apps\\foo\\foo.exe")
        );
    }

    #[test]
    fn test_find_command_candidates_multiple_variants() {
        // Reproduces `wenget rn confy` when confy is installed as two
        // separate variant packages (confy-64 -> confy-64, confy-desktop-64
        // -> confyd), each contributing exactly one command of its own.
        let mut manifest = InstalledManifest::new();

        let mut exe_cli = HashMap::new();
        exe_cli.insert("bin/confy-64".to_string(), "confy-64".to_string());
        let pkg_cli = InstalledPackage {
            repo_name: "confy".to_string(),
            variant: Some("64".to_string()),
            version: "0.19.1".to_string(),
            platform: "windows-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: "C:\\apps\\confy-64".to_string(),
            executables: exe_cli,
            source: crate::core::manifest::PackageSource::Bucket {
                name: "wenget".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "confy-windows-x86_64.exe".to_string(),
            parent_package: None,
            download_url: None,
        };
        manifest.packages.insert("confy-64".to_string(), pkg_cli);

        let mut exe_desktop = HashMap::new();
        exe_desktop.insert("bin/confyd".to_string(), "confyd".to_string());
        let pkg_desktop = InstalledPackage {
            repo_name: "confy".to_string(),
            variant: Some("desktop-64".to_string()),
            version: "0.19.1".to_string(),
            platform: "windows-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: "C:\\apps\\confy-desktop-64".to_string(),
            executables: exe_desktop,
            source: crate::core::manifest::PackageSource::Bucket {
                name: "wenget".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "confy-desktop-windows-x86_64.exe".to_string(),
            parent_package: None,
            download_url: None,
        };
        manifest
            .packages
            .insert("confy-desktop-64".to_string(), pkg_desktop);

        // Repo-name lookup must surface commands from BOTH variant packages.
        let candidates = find_command_candidates(&manifest, "confy").unwrap();
        let mut cmd_names: Vec<&str> = candidates.iter().map(|c| c.cmd_name.as_str()).collect();
        cmd_names.sort();
        assert_eq!(cmd_names, vec!["confy-64", "confyd"]);

        // Exact package key still resolves to just that variant's command(s).
        let single = find_command_candidates(&manifest, "confy-desktop-64").unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].cmd_name, "confyd");

        // Exact command name still resolves unambiguously.
        let by_cmd = find_command_candidates(&manifest, "confy-64").unwrap();
        // "confy-64" is both a package key and a command name here; package
        // key match takes precedence and still yields exactly one command.
        assert_eq!(by_cmd.len(), 1);
        assert_eq!(by_cmd[0].cmd_name, "confy-64");
    }
}
