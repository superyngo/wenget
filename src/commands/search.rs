//! Search command implementation

use anyhow::Result;
use colored::Colorize;

use crate::core::cache::{CachedPackage, CachedScript, ManifestCache};
use crate::core::{fuzzy, Config, Platform};

/// Scored matches for packages and scripts
struct SearchResults<'a> {
    matching_packages: Vec<&'a CachedPackage>,
    matching_scripts: Vec<&'a CachedScript>,
    hidden_by_platform: usize,
}

/// Score and sort matching packages and scripts against search patterns
fn score_matches<'a>(
    cache: &'a ManifestCache,
    patterns: &[String],
    platform_ids: &[String],
) -> SearchResults<'a> {
    // Best score across all terms (terms are OR'ed)
    let best_score = |name: &str, description: &str, repo: &str| {
        patterns
            .iter()
            .filter_map(|p| fuzzy::score(p, name, description, repo))
            .max()
    };

    let mut hidden_by_platform = 0usize;

    // Score packages
    let mut matching_packages: Vec<_> = cache
        .packages
        .values()
        .filter_map(|cached_pkg| {
            let pkg = &cached_pkg.package;
            let score = best_score(&pkg.name, &pkg.description, &pkg.repo)?;
            if !platform_ids.iter().any(|id| pkg.platforms.contains_key(id)) {
                hidden_by_platform += 1;
                return None;
            }
            Some((score, cached_pkg))
        })
        .collect();
    matching_packages.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.package.name.cmp(&b.1.package.name))
    });
    let matching_packages: Vec<_> = matching_packages.into_iter().map(|(_, p)| p).collect();

    // Score scripts
    let mut matching_scripts: Vec<_> = cache
        .scripts
        .values()
        .filter_map(|cached_script| {
            let script = &cached_script.script;
            let score = best_score(&script.name, &script.description, &script.repo)?;
            if !script.is_compatible_with_current_platform() {
                hidden_by_platform += 1;
                return None;
            }
            Some((score, cached_script))
        })
        .collect();
    matching_scripts.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.script.name.cmp(&b.1.script.name))
    });
    let matching_scripts: Vec<_> = matching_scripts.into_iter().map(|(_, s)| s).collect();

    SearchResults {
        matching_packages,
        matching_scripts,
        hidden_by_platform,
    }
}

/// Print matching binary packages in tabular format
fn print_packages(packages: &[&CachedPackage], platform_ids: &[String]) {
    if packages.is_empty() {
        return;
    }

    println!("{}", "Binary Packages:".bold().cyan());
    println!(
        "{:<20} {:<10} {}",
        "NAME".bold(),
        "SIZE".bold(),
        "DESCRIPTION".bold()
    );
    println!("{}", "─".repeat(80));

    for cached_pkg in packages {
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

/// Print matching scripts in tabular format
fn print_scripts(scripts: &[&CachedScript]) {
    if scripts.is_empty() {
        return;
    }

    println!("{}", "Scripts:".bold().cyan());
    println!(
        "{:<20} {:<10} {}",
        "NAME".bold(),
        "TYPE".bold(),
        "DESCRIPTION".bold()
    );
    println!("{}", "─".repeat(80));

    for cached_script in scripts {
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

/// Print a notice when matching items were excluded by platform
fn print_hidden(hidden_by_platform: usize) {
    if hidden_by_platform > 0 {
        println!(
            "{}",
            format!(
                "{} more match(es) not available for this platform",
                hidden_by_platform
            )
            .dimmed()
        );
    }
}

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

    let results = score_matches(&cache, &patterns, &platform_ids);

    if results.matching_packages.is_empty() && results.matching_scripts.is_empty() {
        println!(
            "{}",
            format!("No packages or scripts found matching: {:?}", patterns).yellow()
        );
        print_hidden(results.hidden_by_platform);
        return Ok(());
    }

    // Print header
    println!("{}", format!("Search results for: {:?}", patterns).bold());
    println!();

    print_packages(&results.matching_packages, &platform_ids);
    print_scripts(&results.matching_scripts);

    println!(
        "Found: {} package(s), {} script(s)",
        results.matching_packages.len(),
        results.matching_scripts.len()
    );
    print_hidden(results.hidden_by_platform);

    Ok(())
}

/// Truncate string to at most `max_len` characters (char-safe)
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_multibyte() {
        let s = "中文描述".repeat(20);
        let t = truncate(&s, 50);
        assert_eq!(t.chars().count(), 50);
        assert!(t.ends_with("..."));
        assert_eq!(truncate("short", 50), "short");
    }
}
