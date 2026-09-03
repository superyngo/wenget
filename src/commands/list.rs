//! List command implementation

use crate::core::{Config, Platform};
use anyhow::Result;
use colored::Colorize;
use console::Term;

/// List installed packages or all available packages
pub fn run(all: bool) -> Result<()> {
    let config = Config::new()?;

    if all {
        // Show all available packages from cache
        list_all_packages(&config)?;
    } else {
        // Show only installed packages
        list_installed_packages(&config)?;
    }

    Ok(())
}

/// Get terminal width with fallback
fn term_width() -> usize {
    Term::stdout().size().1 as usize
}

/// Truncate description to fit within max_width, appending "..." if needed
fn truncate_desc(desc: &str, max_width: usize) -> String {
    if max_width <= 3 {
        return String::new();
    }
    let char_count = desc.chars().count();
    if char_count <= max_width {
        return desc.to_string();
    }
    let truncated: String = desc.chars().take(max_width - 3).collect();
    format!("{}...", truncated)
}

/// List only installed packages
fn list_installed_packages(config: &Config) -> Result<()> {
    // Load the installed set
    let manifest = config.get_or_create_installed()?;

    if manifest.packages.is_empty() {
        println!("{}", "No packages installed".yellow());
        println!("Install packages with: wenget add <name>");
        return Ok(());
    }

    // Column widths: NAME(20) + sp + VERSION(10) + sp + SOURCE(12) + sp = 45
    let width = term_width();
    let fixed_cols = 20 + 1 + 10 + 1 + 12 + 1;
    let desc_width = width.saturating_sub(fixed_cols);

    // Print header
    println!("{}", "Installed packages".bold());
    println!();
    println!(
        "{:<20} {:<10} {:<12} {}",
        "NAME".bold(),
        "VERSION".bold(),
        "SOURCE".bold(),
        "DESCRIPTION".bold()
    );
    println!("{}", "─".repeat(width.min(120)));

    // Group packages by repo_name
    let grouped = manifest.group_by_repo();

    // Sort repo names alphabetically
    let mut repo_names: Vec<_> = grouped.keys().collect();
    repo_names.sort();

    // Display packages with tree structure
    for repo_name in repo_names {
        let variants = &grouped[repo_name];

        // Sort variants: None (default) first, then alphabetically
        let mut sorted_variants = variants.clone();
        sorted_variants.sort_by(|a, b| {
            let a_variant = &a.1.variant;
            let b_variant = &b.1.variant;
            match (a_variant, b_variant) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(b),
            }
        });

        // Display the first (main/default) variant as the parent
        let (first_key, first_pkg) = sorted_variants[0];

        // Get source display
        let source_display = match &first_pkg.source {
            crate::core::manifest::PackageSource::Bucket { name } => name.clone(),
            crate::core::manifest::PackageSource::DirectRepo { .. } => "url".to_string(),
            crate::core::manifest::PackageSource::Script { script_type, .. } => {
                script_type.display_name().to_lowercase().to_string()
            }
        };

        let description = truncate_desc(&first_pkg.description, desc_width);

        // Display main package
        let display_name = if sorted_variants.len() == 1 {
            // Only one variant, show it normally
            if first_pkg.variant.is_none() {
                repo_name.green()
            } else {
                first_key.green()
            }
        } else {
            // Multiple variants, show repo name
            repo_name.green()
        };

        println!(
            "{:<20} {:<10} {:<12} {}",
            display_name,
            first_pkg.version,
            source_display.cyan(),
            description
        );

        // Display command for first variant
        if sorted_variants.len() == 1 {
            let cmd_display = format!("  [Command: {}]", first_pkg.get_command_names().join(", "));
            println!("{}", cmd_display.yellow().dimmed());
        } else {
            // Show first variant with tree structure
            let variant_label = first_pkg.variant.as_deref().unwrap_or("(default)");
            let cmd_display = format!("[Command: {}]", first_pkg.get_command_names().join(", "));
            println!(
                "  ├─ {:<30} {}",
                variant_label.dimmed(),
                cmd_display.yellow().dimmed()
            );

            // Display other variants (tree structure)
            for (i, (_var_key, var_pkg)) in sorted_variants.iter().skip(1).enumerate() {
                let is_last = i == sorted_variants.len() - 2; // -2 because we skipped first
                let prefix = if is_last { "└─" } else { "├─" };

                let variant_label = var_pkg.variant.as_deref().unwrap_or("(default)");
                let cmd_display = format!("[Command: {}]", var_pkg.get_command_names().join(", "));

                println!(
                    "  {} {:<30} {}",
                    prefix.dimmed(),
                    variant_label.dimmed(),
                    cmd_display.yellow().dimmed()
                );
            }
        }
    }

    // Calculate total
    let total_packages = manifest.packages.len();
    let total_repos = grouped.len();

    println!();
    if total_repos < total_packages {
        println!(
            "Total: {} package(s) installed from {} repositories",
            total_packages, total_repos
        );
    } else {
        println!("Total: {} package(s) installed", total_packages);
    }

    Ok(())
}

/// List all available packages from cache
fn list_all_packages(config: &Config) -> Result<()> {
    // Get packages from cache
    let manifest = config.get_packages_from_cache()?;

    // Load installed packages for marking
    let installed = config.get_or_create_installed()?;

    // Get current platform
    let platform = Platform::current();
    let platform_ids = platform.possible_identifiers();

    // Filter packages that support current platform
    let mut packages: Vec<_> = manifest
        .packages
        .iter()
        .filter(|pkg| {
            platform_ids
                .iter()
                .any(|pid| pkg.platforms.contains_key(pid))
        })
        .collect();

    // Filter scripts that are compatible with current OS
    let scripts: Vec<_> = manifest
        .scripts
        .iter()
        .filter(|script| script.is_compatible_with_current_platform())
        .collect();

    if packages.is_empty() && scripts.is_empty() {
        println!("{}", "No packages available in buckets".yellow());
        println!("Add a bucket with: wenget bucket add <name> <url>");
        return Ok(());
    }

    // Sort alphabetically
    packages.sort_by(|a, b| a.name.cmp(&b.name));

    // Column widths: NAME(30) + sp + TYPE(12) + sp = 44
    let width = term_width();
    let fixed_cols = 30 + 1 + 12 + 1;
    let desc_width = width.saturating_sub(fixed_cols);

    // Print header
    println!("{}", "Available packages".bold());
    println!();
    println!(
        "{:<30} {:<12} {}",
        "NAME".bold(),
        "TYPE".bold(),
        "DESCRIPTION".bold()
    );
    println!("{}", "─".repeat(width.min(120)));

    // Print packages
    for pkg in &packages {
        let description = truncate_desc(&pkg.description, desc_width);

        if installed.is_installed(&pkg.name) {
            // For installed packages, calculate padding manually to account for "(installed)"
            let name_width = 30;
            let installed_suffix = " (installed)";
            let visible_length = pkg.name.len() + installed_suffix.len();
            let padding = if visible_length < name_width {
                name_width - visible_length
            } else {
                1
            };

            print!("{} {}", pkg.name, "(installed)".green());
            print!("{}", " ".repeat(padding));
            println!("{:<12} {}", "binary".cyan(), description);
        } else {
            println!("{:<30} {:<12} {}", pkg.name, "binary".cyan(), description);
        }
    }

    // Print scripts
    for script in &scripts {
        // Get the best compatible script type for display
        let script_type_display = script
            .get_compatible_script()
            .map(|(st, _)| st.display_name().to_lowercase())
            .unwrap_or_else(|| script.platforms_display().to_lowercase());

        let description = truncate_desc(&script.description, desc_width);

        if installed.is_installed(&script.name) {
            // For installed scripts, calculate padding manually to account for "(installed)"
            let name_width = 30;
            let installed_suffix = " (installed)";
            let visible_length = script.name.len() + installed_suffix.len();
            let padding = if visible_length < name_width {
                name_width - visible_length
            } else {
                1
            };

            print!("{} {}", script.name, "(installed)".green());
            print!("{}", " ".repeat(padding));
            println!("{:<12} {}", script_type_display.magenta(), description);
        } else {
            println!(
                "{:<30} {:<12} {}",
                script.name,
                script_type_display.magenta(),
                description
            );
        }
    }

    println!();
    println!(
        "Total: {} package(s), {} script(s) available ({} installed)",
        packages.len(),
        scripts.len(),
        installed.packages.len()
    );

    Ok(())
}
