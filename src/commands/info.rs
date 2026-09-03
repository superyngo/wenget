//! Info command implementation
//!
//! Shows detailed package information from cache (with glob support), GitHub URL,
//! or installed packages (for manually installed or non-bucket sources)

use crate::core::manifest::InstalledPackage;
use crate::core::Config;
use crate::package_resolver::{PackageInput, PackageResolver, ResolvedPackage};
use anyhow::Result;
use colored::Colorize;

/// Show package and script information
pub fn run(names: Vec<String>) -> Result<()> {
    let config = Config::new()?;

    if names.is_empty() {
        println!("{}", "No package names or URLs provided".yellow());
        println!("Usage: wenget info <name|url> [<name|url>...]");
        println!();
        println!("Examples:");
        println!("  wenget info ripgrep              # Query from cache");
        println!("  wenget info 'rip*'               # Glob pattern (cache only)");
        println!("  wenget info https://github.com/BurntSushi/ripgrep  # Direct URL");
        return Ok(());
    }

    // Load installed packages for status checking
    let installed = config.get_or_create_installed()?;

    // Load cache once for both script lookup and package resolution
    let cache = config.get_or_rebuild_cache()?;

    // Create resolver with shared cache reference
    let resolver = PackageResolver::new(&config, &cache)?;

    let mut total_found = 0;

    for name in &names {
        let input = PackageInput::parse(name);

        // First try to resolve as package from cache
        match resolver.resolve(&input) {
            Ok(packages) => {
                for resolved in packages {
                    if total_found > 0 {
                        println!();
                        println!("{}", "─".repeat(80));
                        println!();
                    }
                    display_package_info(&resolved, &installed, &resolver)?;
                    total_found += 1;
                }
            }
            Err(_) => {
                // If not found as package, try as script
                if let Some(cached_script) = cache.find_script(name) {
                    if total_found > 0 {
                        println!();
                        println!("{}", "─".repeat(80));
                        println!();
                    }
                    display_script_info(cached_script, &installed)?;
                    total_found += 1;
                } else if let Some(inst_pkg) = installed.get_package(name) {
                    // Check if it's an installed package not in cache (manual/direct install)
                    if total_found > 0 {
                        println!();
                        println!("{}", "─".repeat(80));
                        println!();
                    }
                    display_installed_only_info(name, inst_pkg)?;
                    total_found += 1;
                } else {
                    eprintln!("{} {}: Not found", "Error".red().bold(), name);
                }
            }
        }
    }

    if total_found == 0 {
        println!("{}", "No packages or scripts found".yellow());
    } else if total_found > 1 {
        println!();
        println!(
            "{}",
            format!("Found {} item(s)", total_found).green().bold()
        );
    }

    Ok(())
}

/// Display detailed information for a single package
fn display_package_info(
    resolved: &ResolvedPackage,
    installed: &crate::core::InstalledSet,
    resolver: &PackageResolver,
) -> Result<()> {
    let pkg = &resolved.package;

    // ═══════════════════════════════════════════════════════════════════
    // Beautiful Header (Box style)
    // ═══════════════════════════════════════════════════════════════════
    println!();
    println!("╭─────────────────────────────────────────────────────────╮");
    println!("│  {:<55}│", pkg.name.bold().cyan());
    println!("╰─────────────────────────────────────────────────────────╯");
    println!();

    // Basic info
    println!("  {} {}", "Repository:".bold(), pkg.repo);

    if let Some(ref homepage) = pkg.homepage {
        println!("  {} {}", "Homepage:".bold(), homepage);
    }

    if let Some(ref license) = pkg.license {
        println!("  {} {}", "License:".bold(), license);
    }

    println!("  {} {}", "Description:".bold(), pkg.description);
    println!();

    // Source
    match &resolved.source {
        crate::core::manifest::PackageSource::Bucket { name } => {
            println!("  {} {} ({})", "Source:".bold(), "Bucket".green(), name);
        }
        crate::core::manifest::PackageSource::DirectRepo { url: _ } => {
            println!("  {} {}", "Source:".bold(), "Direct URL".yellow());
        }
        crate::core::manifest::PackageSource::Script {
            origin,
            script_type,
        } => {
            println!(
                "  {} {} ({} from {})",
                "Source:".bold(),
                "Script".magenta(),
                script_type.display_name(),
                origin
            );
        }
    }

    // Latest version from GitHub
    if let Ok(version) = resolver.fetch_latest_version(&pkg.repo) {
        println!("  {} {}", "Latest version:".bold(), version.green());
    }

    // Installation status and variants
    let all_variants = installed.find_by_repo(&pkg.name);

    if !all_variants.is_empty() {
        println!("  {} {}", "Status:".bold(), "Installed".green());

        // Collect all command names from all variants
        let all_commands: Vec<String> = all_variants
            .iter()
            .flat_map(|(_, p)| p.get_command_names().into_iter().map(String::from))
            .collect();

        println!(
            "  {} {}",
            "Command name(s):".bold(),
            all_commands.join(", ").yellow()
        );

        // Show variant details
        println!(
            "  {} {} variant(s):",
            "Variants:".bold(),
            all_variants.len()
        );
        for (_key, inst_pkg) in &all_variants {
            let variant_label = inst_pkg.variant.as_deref().unwrap_or("(default)");
            println!(
                "    {} {} - v{} [{}]",
                "└─".dimmed(),
                variant_label.green(),
                inst_pkg.version,
                inst_pkg.get_command_names().join(", ").yellow()
            );
        }

        // Show first variant's details
        let (_key, first_pkg) = &all_variants[0];
        println!("  {} {}", "Installed at:".bold(), first_pkg.installed_at);
        println!("  {} {}", "Platform:".bold(), first_pkg.platform);
        println!("  {} {}", "Install path:".bold(), first_pkg.install_path);
    } else {
        println!("  {} {}", "Status:".bold(), "Not installed".yellow());
    }

    // Supported platforms (enhanced: show multiple packages per platform)
    println!();
    println!(
        "  {} {} platform(s)",
        "Supported:".bold(),
        pkg.platforms.len()
    );

    let mut platforms: Vec<_> = pkg.platforms.keys().collect();
    platforms.sort();

    for platform in platforms {
        let binaries = &pkg.platforms[platform];
        if binaries.len() == 1 {
            let b = &binaries[0];

            // Check if this binary is installed
            let variant =
                crate::core::manifest::extract_variant_from_asset(&b.asset_name, &pkg.name);
            let installed_key =
                crate::core::manifest::generate_installed_key(&pkg.name, variant.as_deref());
            let install_status = if let Some(inst_pkg) = installed.get_package(&installed_key) {
                // Only show [Installed] if the platform matches
                if inst_pkg.platform == *platform {
                    format!(" [Installed: {}]", inst_pkg.get_command_names().join(", "))
                        .green()
                        .to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            println!(
                "    {} {} ({:.2} MB){}",
                "•".cyan(),
                platform,
                b.size as f64 / 1_048_576.0,
                install_status
            );
        } else {
            println!(
                "    {} {} [{} packages]",
                "•".cyan(),
                platform,
                binaries.len()
            );
            for b in binaries {
                // Extract variant and check installation status
                let variant =
                    crate::core::manifest::extract_variant_from_asset(&b.asset_name, &pkg.name);
                let installed_key =
                    crate::core::manifest::generate_installed_key(&pkg.name, variant.as_deref());

                let variant_label = variant.as_deref().unwrap_or("(default)");
                let install_status = if let Some(inst_pkg) = installed.get_package(&installed_key) {
                    // Only show [Installed] if the platform matches
                    if inst_pkg.platform == *platform {
                        format!(" [Installed: {}]", inst_pkg.get_command_names().join(", "))
                            .green()
                            .to_string()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                println!(
                    "      {} {} ({:.2} MB) [{}]{}",
                    "─".dimmed(),
                    b.asset_name,
                    b.size as f64 / 1_048_576.0,
                    variant_label,
                    install_status
                );
            }
        }
    }

    Ok(())
}

/// Display detailed information for a single script
fn display_script_info(
    cached_script: &crate::cache::CachedScript,
    installed: &crate::core::InstalledSet,
) -> Result<()> {
    let script = &cached_script.script;

    // Header
    println!("{} {}", script.name.bold().cyan(), "[Script]".magenta());
    println!("{}", "─".repeat(60));

    // Basic info
    println!("{:<16} {}", "Repository:".bold(), script.repo);

    if let Some(ref homepage) = script.homepage {
        println!("{:<16} {}", "Homepage:".bold(), homepage);
    }

    if let Some(ref license) = script.license {
        println!("{:<16} {}", "License:".bold(), license);
    }

    println!("{:<16} {}", "Description:".bold(), script.description);

    // Source
    match &cached_script.source {
        crate::core::manifest::PackageSource::Bucket { name } => {
            println!("{:<16} {} ({})", "Source:".bold(), "Bucket".green(), name);
        }
        crate::core::manifest::PackageSource::DirectRepo { url: _ } => {
            println!("{:<16} {}", "Source:".bold(), "Direct URL".yellow());
        }
        crate::core::manifest::PackageSource::Script {
            origin,
            script_type,
        } => {
            println!(
                "{:<16} {} ({} from {})",
                "Source:".bold(),
                "Script".magenta(),
                script_type.display_name(),
                origin
            );
        }
    }

    // Installation status
    if let Some(inst_pkg) = installed.get_package(&script.name) {
        println!("{:<16} {}", "Status:".bold(), "Installed".green());
        println!(
            "{:<16} {}",
            "Command name:".bold(),
            inst_pkg.get_command_names().join(", ").yellow()
        );
        println!("{:<16} {}", "Installed at:".bold(), inst_pkg.installed_at);
        println!("{:<16} {}", "Install path:".bold(), inst_pkg.install_path);
    } else {
        println!("{:<16} {}", "Status:".bold(), "Not installed".yellow());
    }

    // Available platforms (multi-platform support)
    println!();
    println!(
        "{} {} platform(s):",
        "Available platforms:".bold(),
        script.platforms.len()
    );
    for (script_type, platform_info) in &script.platforms {
        let supported = script_type.is_supported_on_current_platform();
        let status = if supported {
            "✓ compatible".green()
        } else {
            "✗ not supported".red()
        };
        println!(
            "  {} {:<12} {} {}",
            "•".cyan(),
            script_type.display_name(),
            status,
            format!("({})", platform_info.url).dimmed()
        );
    }

    // Show if current platform is compatible
    println!();
    if script.is_compatible_with_current_platform() {
        if let Some((best_type, _)) = script.get_compatible_script() {
            println!(
                "{} {} ({})",
                "Current platform:".bold(),
                "Supported".green(),
                format!("will use {}", best_type.display_name()).dimmed()
            );
        }
    } else {
        println!("{} {}", "Current platform:".bold(), "Not supported".red());
    }

    Ok(())
}

/// Display information for an installed package not found in cache
/// (e.g., manually installed, direct URL install, or local script)
fn display_installed_only_info(name: &str, inst_pkg: &InstalledPackage) -> Result<()> {
    // Determine type label based on source
    let type_label = match &inst_pkg.source {
        crate::core::manifest::PackageSource::Script { script_type, .. } => {
            format!("[{} Script]", script_type.display_name())
        }
        crate::core::manifest::PackageSource::DirectRepo { .. } => "[Direct Install]".to_string(),
        crate::core::manifest::PackageSource::Bucket { name } => {
            format!("[Bucket: {}]", name)
        }
    };

    // Header
    println!("{} {}", name.bold().cyan(), type_label.magenta());
    println!("{}", "─".repeat(60));

    // Description
    if !inst_pkg.description.is_empty() {
        println!("{:<16} {}", "Description:".bold(), inst_pkg.description);
    }

    // Source information
    match &inst_pkg.source {
        crate::core::manifest::PackageSource::Bucket { name } => {
            println!("{:<16} {} ({})", "Source:".bold(), "Bucket".green(), name);
        }
        crate::core::manifest::PackageSource::DirectRepo { url } => {
            println!("{:<16} {}", "Source:".bold(), "Direct URL".yellow());
            println!("{:<16} {}", "Repository:".bold(), url);
        }
        crate::core::manifest::PackageSource::Script {
            origin,
            script_type,
        } => {
            println!(
                "{:<16} {} ({})",
                "Source:".bold(),
                "Script".magenta(),
                script_type.display_name()
            );
            println!("{:<16} {}", "Origin:".bold(), origin);
        }
    }

    // Installation status (always installed since we found it in installed.json)
    println!(
        "{:<16} {} (v{})",
        "Status:".bold(),
        "Installed".green(),
        inst_pkg.version
    );
    println!(
        "{:<16} {}",
        "Command name:".bold(),
        {
            let names = inst_pkg.get_command_names();
            if names.is_empty() {
                "-".to_string()
            } else {
                names.join(", ")
            }
        }
        .yellow()
    );
    println!("{:<16} {}", "Installed at:".bold(), inst_pkg.installed_at);
    println!("{:<16} {}", "Platform:".bold(), inst_pkg.platform);
    println!("{:<16} {}", "Install path:".bold(), inst_pkg.install_path);

    // Show executables
    if !inst_pkg.executables.is_empty() {
        println!();
        println!(
            "{} {} executable(s)",
            "Executables:".bold(),
            inst_pkg.executables.len()
        );
        for (exe_path, cmd_name) in &inst_pkg.executables {
            println!("  {} {} → {}", "•".cyan(), exe_path.dimmed(), cmd_name);
        }
    } else if !inst_pkg.get_command_names().is_empty() {
        println!();
        println!(
            "{} {}",
            "Command names:".bold(),
            inst_pkg.get_command_names().join(", ").cyan()
        );
    }

    Ok(())
}
