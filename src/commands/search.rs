//! Search command implementation

use crate::core::{Config, Platform};
use anyhow::Result;
use colored::Colorize;
use glob::Pattern;

/// Search for packages and scripts
pub fn run(patterns: Vec<String>) -> Result<()> {
    let config = Config::new()?;

    // Load cache
    let cache = config.get_or_rebuild_cache()?;

    if cache.packages.is_empty() && cache.scripts.is_empty() {
        println!("{}", "No packages or scripts in sources".yellow());
        println!("Add buckets with: wenget bucket add <name> <url>");
        return Ok(());
    }

    if patterns.is_empty() {
        println!("{}", "No search pattern provided".yellow());
        println!("Usage: wenget search <name>...");
        return Ok(());
    }

    // Get current platform
    let platform = Platform::current();
    let platform_ids = platform.possible_identifiers();

    // Compile glob patterns
    let glob_patterns: Vec<Pattern> = patterns
        .iter()
        .map(|p| Pattern::new(p))
        .collect::<Result<_, _>>()?;

    // Filter packages
    let matching_packages: Vec<_> = cache
        .packages
        .values()
        .filter(|cached_pkg| {
            let pkg = &cached_pkg.package;
            // Check if name matches any pattern
            let name_matches = glob_patterns
                .iter()
                .any(|pattern| pattern.matches(&pkg.name));

            // Check if supports current platform
            let platform_matches = platform_ids.iter().any(|id| pkg.platforms.contains_key(id));

            name_matches && platform_matches
        })
        .collect();

    // Filter scripts
    let matching_scripts: Vec<_> = cache
        .scripts
        .values()
        .filter(|cached_script| {
            let script = &cached_script.script;
            // Check if name matches any pattern
            let name_matches = glob_patterns
                .iter()
                .any(|pattern| pattern.matches(&script.name));

            // Check if supports current platform
            let platform_matches = script.is_compatible_with_current_platform();

            name_matches && platform_matches
        })
        .collect();

    if matching_packages.is_empty() && matching_scripts.is_empty() {
        println!(
            "{}",
            format!("No packages or scripts found matching: {:?}", patterns).yellow()
        );
        return Ok(());
    }

    // Print header
    println!("{}", format!("Search results for: {:?}", patterns).bold());
    println!();

    // Print packages
    if !matching_packages.is_empty() {
        println!("{}", "Binary Packages:".bold().cyan());
        println!(
            "{:<20} {:<10} {}",
            "NAME".bold(),
            "SIZE".bold(),
            "DESCRIPTION".bold()
        );
        println!("{}", "─".repeat(80));

        for cached_pkg in &matching_packages {
            let pkg = &cached_pkg.package;
            // Find the first matching platform and its first binary. Both come
            // from a remote bucket manifest, so an entry with a missing or empty
            // binary list must not abort the whole search.
            let first_binary = platform_ids
                .iter()
                .find_map(|id| pkg.platforms.get(id))
                .and_then(|binaries| binaries.first());
            let size_mb = first_binary.map_or(0.0, |b| b.size as f64 / 1_000_000.0);

            println!(
                "{:<20} {:>8.1} MB  {}",
                pkg.name.green(),
                size_mb,
                truncate(&pkg.description, 50)
            );
        }
        println!();
    }

    // Print scripts
    if !matching_scripts.is_empty() {
        println!("{}", "Scripts:".bold().cyan());
        println!(
            "{:<20} {:<10} {}",
            "NAME".bold(),
            "TYPE".bold(),
            "DESCRIPTION".bold()
        );
        println!("{}", "─".repeat(80));

        for cached_script in &matching_scripts {
            let script = &cached_script.script;
            // Get the best compatible script type for display
            let script_type = match script.get_compatible_script() {
                Some((st, _)) => st.display_name().to_string(),
                None => "script".to_string(),
            };

            println!(
                "{:<20} {:<10} {}",
                script.name.green(),
                script_type.yellow(),
                truncate(&script.description, 50)
            );
        }
        println!();
    }

    println!(
        "Found: {} package(s), {} script(s)",
        matching_packages.len(),
        matching_scripts.len()
    );

    Ok(())
}

/// Truncate string to max length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}
