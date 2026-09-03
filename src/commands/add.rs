//! Add (Install) command implementation

use crate::core::manifest::{PackageSource, ScriptType};
use crate::core::{Config, InstalledPackage, Platform, WenPaths};
use crate::downloader;
use crate::installer::{
    create_script_shim, detect_script_type, download_script, extract_archive, extract_script_name,
    find_executable_candidates,
    input_detector::{detect_input_type, InputType},
    install_script,
    local::install_local_file,
    normalize_command_name, read_local_script,
};
use crate::package_resolver::{PackageInput, PackageResolver, ResolvedPackage};
use crate::providers::{GitHubProvider, SourceProvider};
use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[cfg(windows)]
use crate::installer::create_shim;

#[cfg(unix)]
use crate::installer::create_symlink;

/// Install packages (smart detection: package names from cache or GitHub URLs)
#[allow(clippy::too_many_arguments)]
pub fn run(
    names: Vec<String>,
    yes: bool,
    script_name: Option<String>,
    platform: Option<String>,
    version: Option<String>,
    variant_filter: Option<String>,
    no_suffix: bool,
    update_mode: bool,
) -> Result<()> {
    let config = Config::new()?;
    let paths = WenPaths::new()?;

    // Ensure initialized
    if !config.is_initialized() {
        config.init()?;
    }

    let mut installed = config.get_or_create_installed()?;

    if names.is_empty() {
        println!("{}", "No package names or URLs provided".yellow());
        println!("Usage: wenget add <name|url>...");
        println!();
        println!("Examples:");
        println!("  wenget add ripgrep              # Install from cache");
        println!("  wenget add 'rip*'               # Install matching packages (glob)");
        println!("  wenget add https://github.com/BurntSushi/ripgrep  # Install from URL");
        println!("  wenget add ./script.ps1         # Install local script");
        println!(
            "  wenget add https://raw.githubusercontent.com/.../script.sh  # Install remote script"
        );
        println!("  wenget add ripgrep -p linux-x64 # Install for specific platform");
        return Ok(());
    }

    // Categorize inputs
    let mut script_inputs = Vec::new();
    let mut local_inputs = Vec::new();
    let mut url_inputs = Vec::new();
    let mut package_inputs = Vec::new();

    for name in &names {
        match detect_input_type(name) {
            InputType::Script => script_inputs.push(name),
            InputType::LocalFile => local_inputs.push(name),
            InputType::DirectUrl => url_inputs.push(name),
            InputType::PackageName => package_inputs.push(name),
        }
    }

    // Handle script installations
    if !script_inputs.is_empty() {
        install_scripts(
            &config,
            &paths,
            &mut installed,
            script_inputs,
            yes,
            script_name.as_deref(),
        )?;
    }

    // Handle local file installations
    if !local_inputs.is_empty() {
        install_local_files(
            &config,
            &paths,
            &mut installed,
            local_inputs,
            yes,
            script_name.as_deref(),
        )?;
    }

    // Handle direct URL installations
    if !url_inputs.is_empty() {
        install_from_urls(
            &config,
            &paths,
            &mut installed,
            url_inputs,
            yes,
            script_name.as_deref(),
        )?;
    }

    // Handle package installations (existing logic)
    if !package_inputs.is_empty() {
        install_packages(
            &config,
            &paths,
            &mut installed,
            package_inputs,
            yes,
            script_name.as_deref(),
            platform.as_deref(),
            version.as_deref(),
            variant_filter.as_deref(),
            no_suffix,
            update_mode,
        )?;
    }

    Ok(())
}

/// Resolve command name to avoid conflicts
///
/// Priority:
/// 1. If variant exists, use base_name-{variant}
/// 2. Otherwise, use base_name
/// 3. If name is taken, try base_name-{number}
/// 4. If is_custom is true, skip variant suffix appending and go directly to conflict checking
///
/// `taken` is the precomputed set of command names already in use (excluding the
/// package being resolved). Callers build it once via
/// `InstalledSet::command_name_set` rather than scanning all packages for
/// every candidate suffix here.
fn resolve_command_name(
    base_name: &str,
    variant: Option<&str>,
    taken: &std::collections::HashSet<String>,
    is_custom: bool,
) -> String {
    // 1. If custom name provided, skip variant suffix - just check for conflicts
    if is_custom {
        if !taken.contains(base_name) {
            return base_name.to_string();
        }
        // Custom name taken, try numeric suffixes
        for i in 1..=99 {
            let numbered = format!("{}-{}", base_name, i);
            if !taken.contains(&numbered) {
                return numbered;
            }
        }
        return base_name.to_string();
    }

    // 2. If it's a variant, construct the desired command name
    if let Some(var) = variant {
        // Check if base_name already ends with the variant suffix
        // This handles cases where the binary itself contains the variant name
        // e.g., base_name="bun-profile", variant="profile" -> keep as "bun-profile"
        // e.g., base_name="bun-profile", variant="baseline-profile" -> change to "bun-baseline-profile"

        let desired_name = if base_name.ends_with(&format!("-{}", var)) {
            // Base name already ends with variant, use as-is
            base_name.to_string()
        } else if let Some(base_stripped) = extract_repo_name_from_command(base_name, var) {
            // Base name contains part of the variant, reconstruct with full variant
            // e.g., "bun-profile" with variant "baseline-profile" -> "bun-baseline-profile"
            format!("{}-{}", base_stripped, var)
        } else {
            // Normal case: append variant to base name
            format!("{}-{}", base_name, var)
        };

        if !taken.contains(&desired_name) {
            return desired_name;
        }
        // Desired name taken, try numeric suffixes
        for i in 1..=99 {
            let numbered = format!("{}-{}", desired_name, i);
            if !taken.contains(&numbered) {
                return numbered;
            }
        }
        return desired_name;
    }

    // 3. No variant - try base_name first
    if !taken.contains(base_name) {
        return base_name.to_string();
    }

    // 4. Try numeric suffixes for non-variant
    for i in 1..=99 {
        let numbered = format!("{}-{}", base_name, i);
        if !taken.contains(&numbered) {
            return numbered;
        }
    }

    base_name.to_string()
}

/// Extract repo name from a command name that may contain partial variant info
/// e.g., "bun-profile" with variant "baseline-profile" -> Some("bun")
/// e.g., "bun" with variant "baseline" -> None
fn extract_repo_name_from_command(command_name: &str, variant: &str) -> Option<String> {
    // Split variant by '-' to get all parts
    let variant_parts: Vec<&str> = variant.split('-').collect();

    // Check if command_name ends with any part of the variant
    for part in &variant_parts {
        if command_name.ends_with(&format!("-{}", part)) {
            // Strip this part and return the base
            if let Some(stripped) = command_name.strip_suffix(&format!("-{}", part)) {
                return Some(stripped.to_string());
            }
        }
    }

    None
}

/// Install scripts from local paths or URLs
fn install_scripts(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    script_inputs: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
) -> Result<()> {
    println!("{}", "Scripts to install:".bold());

    let mut scripts_to_install: Vec<(String, String, ScriptType, String)> = Vec::new(); // (name, content, type, origin)

    for input in script_inputs {
        // Determine if local or remote
        let is_url = input.starts_with("http://") || input.starts_with("https://");

        // Get script content
        let content = if is_url {
            match download_script(input) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Failed to download {}: {}", "✗".red(), input, e);
                    continue;
                }
            }
        } else {
            let path = Path::new(input);
            match read_local_script(path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{} Failed to read {}: {}", "✗".red(), input, e);
                    continue;
                }
            }
        };

        // Detect script type
        let script_type = match detect_script_type(input, &content) {
            Some(t) => t,
            None => {
                eprintln!("{} Cannot detect script type for: {}", "✗".red(), input);
                continue;
            }
        };

        // Check platform compatibility
        if !script_type.is_supported_on_current_platform() {
            println!(
                "  {} {} ({}) - {}",
                "⚠".yellow(),
                input,
                script_type.display_name(),
                "not supported on this platform".yellow()
            );
            continue;
        }

        // Determine script name
        let name = if let Some(custom) = custom_name {
            custom.to_string()
        } else {
            match extract_script_name(input) {
                Some(n) => n,
                None => {
                    eprintln!("{} Cannot extract name from: {}", "✗".red(), input);
                    continue;
                }
            }
        };

        // Check if already installed
        if installed.is_installed(&name) {
            println!(
                "  {} {} ({}) - {}",
                "•".yellow(),
                name,
                script_type.display_name(),
                "already installed, will be replaced".yellow()
            );
        } else {
            println!(
                "  {} {} ({}) {}",
                "•".green(),
                name,
                script_type.display_name(),
                "(new)".green()
            );
        }

        scripts_to_install.push((name, content, script_type, input.clone()));
    }

    if scripts_to_install.is_empty() {
        println!("{}", "No scripts to install".yellow());
        return Ok(());
    }

    // Show security warning
    println!();
    println!(
        "{}",
        "⚠  Security Warning: Review scripts before running them!"
            .yellow()
            .bold()
    );

    // Confirm installation
    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(());
    }

    println!();

    let mut success_count = 0;
    let mut fail_count = 0;
    let mut successful_scripts: Vec<String> = Vec::new();
    let mut failed_scripts: Vec<String> = Vec::new();

    for (name, content, script_type, origin) in scripts_to_install {
        println!(
            "{} {} ({})...",
            "Installing".cyan(),
            name,
            script_type.display_name()
        );

        match install_single_script(paths, &name, &content, &script_type, &origin) {
            Ok(inst_pkg) => {
                record_installed(paths, installed, name.clone(), inst_pkg);
                println!("  {} Installed successfully", "✓".green());
                success_count += 1;
                successful_scripts.push(name);
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                fail_count += 1;
                failed_scripts.push(name);
            }
        }
    }

    println!();
    println!("{}", "Summary:".bold());
    if success_count > 0 {
        println!(
            "  {} {} script(s) installed: {}",
            "✓".green(),
            success_count,
            successful_scripts.join(" ")
        );
    }
    if fail_count > 0 {
        println!(
            "  {} {} script(s) failed: {}",
            "✗".red(),
            fail_count,
            failed_scripts.join(" ")
        );
    }

    Ok(())
}

/// Install a single script
fn install_single_script(
    paths: &WenPaths,
    name: &str,
    content: &str,
    script_type: &ScriptType,
    origin: &str,
) -> Result<InstalledPackage> {
    // Install script to app directory
    let files = install_script(paths, name, content, script_type)?;

    println!("  Command will be available as: {}", name);

    // Create shim
    println!("  Creating launcher...");
    create_script_shim(paths, name, script_type)?;

    // Create executables map
    let mut executables = HashMap::new();
    if let Some(script_file) = files.first() {
        executables.insert(script_file.clone(), name.to_string());
    } else {
        executables.insert(
            format!("{}.{}", name, script_type.extension()),
            name.to_string(),
        );
    }

    // Create installed package info
    let inst_pkg = InstalledPackage {
        meta_version: crate::core::manifest::CURRENT_META_VERSION,
        repo_name: name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: format!("{}-script", script_type.display_name().to_lowercase()),
        installed_at: Utc::now(),
        install_path: paths.app_dir(name).to_string_lossy().to_string(),
        executables,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from {}", script_type.display_name(), origin),
        command_names: vec![],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: None,
    };

    Ok(inst_pkg)
}

/// Install local binary or archive files
fn install_local_files(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    files: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
) -> Result<()> {
    println!("{}", "Local files to install:".bold());

    for file in &files {
        println!("  • {}", file);
    }

    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(());
    }

    println!();

    let mut success_count = 0;
    let mut fail_count = 0;
    let mut successful_files: Vec<String> = Vec::new();
    let mut failed_files: Vec<String> = Vec::new();

    for file in files {
        println!("{} {}...", "Installing".cyan(), file);
        let path = Path::new(file);

        match install_local_file(paths, path, custom_name, None) {
            Ok(inst_pkg) => {
                // Use first command name as package name
                let command_names = inst_pkg.get_command_names();
                let name = match command_names.first() {
                    Some(n) => n.to_string(),
                    None => {
                        println!(
                            "  {} No command names found in installed package",
                            "✗".red()
                        );
                        fail_count += 1;
                        failed_files.push(file.to_string());
                        continue;
                    }
                };
                let display_names = inst_pkg.get_command_names().join(", ");
                record_installed(paths, installed, name.clone(), inst_pkg);
                println!(
                    "  {} Installed successfully as {}",
                    "✓".green(),
                    display_names
                );
                success_count += 1;
                successful_files.push(name);
            }
            Err(e) => {
                println!("  {} Failed to install {}: {}", "✗".red(), file, e);
                fail_count += 1;
                failed_files.push(file.to_string());
            }
        }
        println!();
    }

    println!("{}", "Summary:".bold());
    if success_count > 0 {
        println!(
            "  {} {} file(s) installed: {}",
            "✓".green(),
            success_count,
            successful_files.join(" ")
        );
    }
    if fail_count > 0 {
        println!(
            "  {} {} file(s) failed: {}",
            "✗".red(),
            fail_count,
            failed_files.join(" ")
        );
    }

    Ok(())
}

/// Install binary or archive from direct URLs
fn install_from_urls(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    urls: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
) -> Result<()> {
    println!("{}", "URLs to install:".bold());

    for url in &urls {
        println!("  • {}", url);
    }

    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(());
    }

    println!();

    let mut success_count = 0;
    let mut fail_count = 0;
    let mut successful_urls: Vec<String> = Vec::new();
    let mut failed_urls: Vec<String> = Vec::new();

    // Create temp dir for downloads
    let temp_dir = paths.cache_dir().join("downloads");
    fs::create_dir_all(&temp_dir)?;

    for url in urls {
        println!("{} {}...", "Downloading".cyan(), url);

        let filename = match url.split('/').next_back() {
            Some(name) => name,
            None => {
                println!("  {} Invalid URL", "✗".red());
                fail_count += 1;
                failed_urls.push(url.to_string());
                continue;
            }
        };

        // Handle query parameters in URL
        let filename = filename.split('?').next().unwrap_or(filename);
        let download_path = temp_dir.join(filename);

        match downloader::download_file(url, &download_path) {
            Ok(_) => {
                println!("  {} Downloaded", "✓".green());

                if let Err(e) =
                    crate::core::checksum::verify_download(url, filename, &download_path)
                {
                    println!("  {} {}", "✗".red(), e);
                    fail_count += 1;
                    failed_urls.push(url.to_string());
                } else {
                    println!("{} {}...", "Installing".cyan(), filename);

                    match install_local_file(
                        paths,
                        &download_path,
                        custom_name,
                        Some(url.to_string()),
                    ) {
                        Ok(inst_pkg) => {
                            // Use first command name as package name
                            let command_names = inst_pkg.get_command_names();
                            let name = match command_names.first() {
                                Some(n) => n.to_string(),
                                None => {
                                    println!(
                                        "  {} No command names found in installed package",
                                        "✗".red()
                                    );
                                    fail_count += 1;
                                    failed_urls.push(filename.to_string());
                                    continue;
                                }
                            };
                            let display_names = inst_pkg.get_command_names().join(", ");
                            record_installed(paths, installed, name.clone(), inst_pkg);
                            println!(
                                "  {} Installed successfully as {}",
                                "✓".green(),
                                display_names
                            );
                            success_count += 1;
                            successful_urls.push(name);
                        }
                        Err(e) => {
                            println!("  {} Failed to install {}: {}", "✗".red(), filename, e);
                            fail_count += 1;
                            failed_urls.push(filename.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                println!("  {} Failed to download {}: {}", "✗".red(), url, e);
                fail_count += 1;
                failed_urls.push(url.to_string());
            }
        }

        // Clean up downloaded file
        if download_path.exists() {
            if let Err(e) = fs::remove_file(&download_path) {
                log::warn!(
                    "Failed to cleanup downloaded file: {}: {}",
                    download_path.display(),
                    e
                );
            }
        }
        println!();
    }

    println!("{}", "Summary:".bold());
    if success_count > 0 {
        println!(
            "  {} {} URL(s) installed: {}",
            "✓".green(),
            success_count,
            successful_urls.join(" ")
        );
    }
    if fail_count > 0 {
        println!(
            "  {} {} URL(s) failed: {}",
            "✗".red(),
            fail_count,
            failed_urls.join(" ")
        );
    }

    Ok(())
}

/// Print available variant names for a package's binaries
fn print_available_variants(binaries: &[crate::core::manifest::PlatformBinary], pkg_name: &str) {
    for binary in binaries {
        let variant =
            crate::core::manifest::extract_variant_from_asset(&binary.asset_name, pkg_name);
        if let Some(v) = variant {
            println!("    - {}", v);
        } else {
            println!("    - (default)");
        }
    }
}

/// Normalize an asset filename for template-based matching across versions.
///
/// Strips file extensions and version-like segments so that the same binary
/// across different releases produces the same template string.
///
/// # Examples
/// - `uv-x86_64-unknown-linux-gnu.tar.gz` → `uv-x86-64-unknown-linux-gnu`
/// - `gh_copilot_1.0.22_linux_amd64.tar.gz` → `gh-copilot-linux-amd64`
/// - `ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz` → `ripgrep-x86-64-unknown-linux-musl`
fn normalize_asset_for_matching(asset_name: &str) -> String {
    let name = asset_name
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".zip")
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".exe")
        .trim_end_matches(".7z");

    // Split on both - and _, filter out version segments, rejoin
    name.split(['-', '_'])
        .filter(|seg| {
            if seg.is_empty() {
                return false;
            }
            let s = seg.trim_start_matches('v');
            // A version segment starts with a digit AND contains a dot (e.g. 1.0.22, v0.11.6)
            !(s.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
                && s.contains('.'))
        })
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Select packages from a platform that has multiple binaries.
///
/// If only one binary: auto-select.
/// In update mode with multiple binaries (asset-name matching failed): pick first with warning.
/// In add mode with --yes: select all.
/// Otherwise: show MultiSelect dialog.
fn select_packages_for_platform(
    pkg_name: &str,
    binaries: &[crate::core::manifest::PlatformBinary],
    yes: bool,
    update_mode: bool,
) -> Result<Vec<usize>> {
    if binaries.len() == 1 {
        // Single package: auto-select
        return Ok(vec![0]);
    }

    if yes {
        if update_mode {
            // Asset-name matching already ran before this call. If we're here with multiple
            // binaries it means the match failed (package restructured its releases).
            // Best-effort: select the first binary rather than installing all variants.
            println!(
                "  {} Could not determine exact binary for {}, selecting: {}",
                "⚠".yellow(),
                pkg_name,
                binaries[0].asset_name
            );
            return Ok(vec![0]);
        }
        // Add mode with --yes: select all
        println!(
            "  {} Found {} packages for {}, selecting all (--yes)",
            "ℹ".cyan(),
            binaries.len(),
            pkg_name
        );
        return Ok((0..binaries.len()).collect());
    }

    // Multiple packages: show selection dialog
    use dialoguer::MultiSelect;

    println!(
        "\n  {} Found {} packages for {}:",
        "ℹ".cyan(),
        binaries.len(),
        pkg_name
    );

    let items: Vec<String> = binaries
        .iter()
        .map(|b| format!("{} ({:.2} MB)", b.asset_name, b.size as f64 / 1_048_576.0))
        .collect();

    let selections = MultiSelect::new()
        .with_prompt("Select packages to install (Space to select, Enter to confirm)")
        .items(&items)
        .interact()?;

    if selections.is_empty() {
        anyhow::bail!("No packages selected");
    }

    Ok(selections)
}

/// Install packages from cache or GitHub (existing logic)
#[allow(clippy::too_many_arguments)]
fn install_packages(
    config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    names: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
    custom_platform: Option<&str>,
    custom_version: Option<&str>,
    variant_filter: Option<&str>,
    no_suffix: bool,
    update_mode: bool,
) -> Result<()> {
    // Get current platform (used for informational messages).
    let current_platform = Platform::current();

    // Determine the effective platform override: the `-p/--platform` flag takes
    // precedence over the `preferred_platform` config setting. When neither is
    // set, auto-detection (`Platform::current`) is used.
    let platform_override =
        custom_platform.or_else(|| config.preferences().preferred_platform.as_deref());

    // Load cache once for both script lookup and package resolution
    let cache = config.get_or_rebuild_cache()?;

    // Resolve all inputs and collect packages/scripts to install
    let resolver = PackageResolver::new(config, &cache)?;
    let mut packages_to_install: Vec<(
        String,
        ResolvedPackage,
        crate::core::platform::PlatformMatch,
    )> = Vec::new();
    let mut scripts_to_install: Vec<(String, String, ScriptType, String)> = Vec::new(); // (name, url, type, origin)

    for original_name in &names {
        let input = PackageInput::parse(original_name);

        match resolver.resolve(&input) {
            Ok(resolved) => {
                for pkg_resolved in resolved {
                    // Use smart platform matching. When an override (flag or
                    // config) is set, resolve against it; otherwise auto-detect.
                    let matches = if let Some(override_str) = platform_override {
                        Platform::match_override(override_str, &pkg_resolved.package.platforms)
                    } else {
                        current_platform.find_best_match(&pkg_resolved.package.platforms)
                    };

                    if matches.is_empty() {
                        let target = platform_override
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| current_platform.to_string());
                        println!(
                            "{} {} does not support platform {}",
                            "Warning:".yellow(),
                            pkg_resolved.package.name,
                            target
                        );
                        println!(
                            "  Available platforms: {}",
                            pkg_resolved
                                .package
                                .platforms
                                .keys()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                        continue;
                    }

                    let best_match = &matches[0];

                    // Check if fallback requires confirmation
                    if let Some(fallback_type) = &best_match.fallback_type {
                        if fallback_type.requires_confirmation() && !yes {
                            println!(
                                "{} {} - no exact match for {}, but {} is available",
                                "⚠".yellow(),
                                pkg_resolved.package.name,
                                current_platform,
                                best_match.platform_id
                            );
                            println!("  This is a fallback: {}", fallback_type.description());

                            if !crate::utils::prompt::confirm_no_default("  Install anyway?")? {
                                println!("  Skipped");
                                continue;
                            }
                        } else if !yes {
                            // Fallback doesn't require confirmation, but inform user
                            println!(
                                "{} Using fallback: {} ({})",
                                "ℹ".cyan(),
                                best_match.platform_id,
                                fallback_type.description()
                            );
                        }
                    }

                    packages_to_install.push((
                        original_name.to_string(),
                        pkg_resolved,
                        best_match.clone(),
                    ));
                }
            }
            Err(_) => {
                // If not found as package, check if it's a script in cache
                if let Some(cached_script) = cache.find_script(original_name) {
                    let script = &cached_script.script;

                    // Get installable script for current platform (checks if interpreter exists)
                    if let Some((script_type, platform_info)) = script.get_installable_script() {
                        // Prepare script for installation
                        let source_name = match &cached_script.source {
                            PackageSource::Bucket { name } => format!("bucket:{}", name),
                            _ => "unknown".to_string(),
                        };

                        scripts_to_install.push((
                            script.name.clone(),
                            platform_info.url.clone(),
                            script_type,
                            source_name,
                        ));
                    } else {
                        println!(
                            "{} {} is not supported on current platform (available: {})",
                            "Warning:".yellow(),
                            script.name,
                            script.platforms_display()
                        );
                    }
                } else {
                    eprintln!("{} {}: Not found", "Error".red().bold(), original_name);
                }
            }
        }
    }

    if packages_to_install.is_empty() && scripts_to_install.is_empty() {
        println!("{}", "No packages or scripts to install".yellow());
        return Ok(());
    }

    // Create GitHub provider to fetch versions (for packages)
    let github = if !packages_to_install.is_empty() {
        Some(GitHubProvider::new()?)
    } else {
        None
    };

    // Show packages to install with versions and handle already-installed packages
    if !packages_to_install.is_empty() {
        println!("{}", "Packages to install:".bold());
    }

    let mut to_install: Vec<(
        String,
        ResolvedPackage,
        crate::core::platform::PlatformMatch,
        Option<String>,
    )> = Vec::new();
    let mut to_update: Vec<(
        String,
        ResolvedPackage,
        crate::core::platform::PlatformMatch,
        Option<String>,
    )> = Vec::new();

    for (original_name, mut resolved, _) in packages_to_install {
        let pkg_name = resolved.package.name.clone();
        let repo = &resolved.package.repo;

        let mut target_pkg = resolved.package.clone();

        // Fetch version (either custom, or latest from API, falling back to cache)
        // IMPORTANT: Always fetch from GitHub API first to ensure accurate version comparison
        // for update detection. Cached bucket version may be stale.
        let version = if let Some(custom_ver) = custom_version {
            // User specified a version
            let ver = custom_ver.trim_start_matches('v').to_string();
            if let Some(ref gh) = github {
                if let Ok(pkg) = gh.fetch_package_by_version(repo, custom_ver) {
                    target_pkg = pkg;
                } else if let Some(derived) =
                    derive_versioned_package(&resolved.package, custom_ver)
                {
                    target_pkg = derived;
                }
            } else if let Some(derived) = derive_versioned_package(&resolved.package, custom_ver) {
                target_pkg = derived;
            }
            ver
        } else if update_mode && matches!(resolved.source, PackageSource::Bucket { .. }) {
            // In update mode the cache was just refreshed with the latest version by the
            // update command. Trust it instead of making a redundant API call that may
            // flake and fall back to stale data.
            resolved
                .package
                .version
                .clone()
                .unwrap_or_else(|| "unknown".to_string())
        } else if let Some(ref gh) = github {
            // Fetch latest package info from API for accurate comparison and correct URLs
            if let Ok(pkg) = gh.fetch_package(repo) {
                target_pkg = pkg;
                target_pkg
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                resolved
                    .package
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string())
            }
        } else if let Some(ref v) = resolved.package.version {
            // No GitHub provider available - use cached version
            v.clone()
        } else {
            "unknown".to_string()
        };

        resolved.package = target_pkg;

        // Recompute platform match for the new target package platforms
        let matches = if let Some(override_str) = platform_override {
            Platform::match_override(override_str, &resolved.package.platforms)
        } else {
            current_platform.find_best_match(&resolved.package.platforms)
        };

        if matches.is_empty() {
            let target = platform_override
                .map(|s| s.to_string())
                .unwrap_or_else(|| current_platform.to_string());
            println!(
                "{} {} v{} does not support platform {}",
                "Warning:".yellow(),
                resolved.package.name,
                version,
                target
            );
            continue;
        }
        let platform_match = matches[0].clone();

        // Check if already installed
        // Determine which key to check based on input type and variant filter
        let variant_key; // Storage for temporary String if needed
        let check_name: &str = if original_name.contains("::") {
            original_name.as_str()
        } else if let Some(filter) = variant_filter {
            variant_key = crate::core::manifest::generate_installed_key(&pkg_name, Some(filter));
            &variant_key
        } else {
            &pkg_name
        };

        if installed.is_installed(check_name) {
            // Package already installed
            let inst_pkg = installed.get_package(check_name).unwrap();
            if inst_pkg.version == version {
                // Same version installed - ask if user wants to reinstall
                println!(
                    "  {} {} v{} {}",
                    "•".cyan(),
                    check_name,
                    version,
                    "(already installed, same version)".dimmed()
                );
                if !yes && crate::utils::prompt::confirm_no_default("  Reinstall?")? {
                    // User wants to reinstall
                    to_install.push((original_name.clone(), resolved, platform_match, None));
                }
                // If user says no or --yes flag is used, skip reinstallation
            } else {
                println!(
                    "  {} {} v{} {} → {}",
                    "•".yellow(),
                    check_name,
                    inst_pkg.version.dimmed(),
                    "upgrade to".yellow(),
                    version.green()
                );
                // Show download URLs for the matched platform
                if let Some(binaries) = resolved.package.platforms.get(&platform_match.platform_id)
                {
                    for binary in binaries {
                        println!("    {} {}", "↳".dimmed(), binary.url.dimmed());
                    }
                }
                to_update.push((
                    original_name.clone(),
                    resolved,
                    platform_match,
                    Some(check_name.to_string()),
                ));
            }
        } else {
            // New installation
            if update_mode {
                // Update mode: don't install new packages
                println!(
                    "  {} {} is not installed, skipping (use 'wenget add' to install new packages)",
                    "⚠".yellow(),
                    pkg_name
                );
                continue;
            }
            println!(
                "  {} {} v{} {}",
                "•".green(),
                pkg_name,
                version,
                "(new)".green()
            );
            // Show download URLs for the matched platform
            if let Some(binaries) = resolved.package.platforms.get(&platform_match.platform_id) {
                for binary in binaries {
                    println!("    {} {}", "↳".dimmed(), binary.url.dimmed());
                }
            }
            to_install.push((original_name.clone(), resolved, platform_match, None));
        }
    }

    // Show scripts to install
    let mut scripts_to_process: Vec<(String, String, ScriptType, String)> = Vec::new();

    if !scripts_to_install.is_empty() {
        println!();
        println!("{}", "Scripts to install:".bold());

        for (name, url, script_type, origin) in scripts_to_install {
            if installed.is_installed(&name) {
                println!(
                    "  {} {} ({}) {}",
                    "•".yellow(),
                    name,
                    script_type.display_name(),
                    "(already installed, will update)".dimmed()
                );
            } else {
                println!(
                    "  {} {} ({}) {}",
                    "•".green(),
                    name,
                    script_type.display_name(),
                    "(new)".green()
                );
            }
            scripts_to_process.push((name, url, script_type, origin));
        }
    }

    // Check if there's anything to do
    if to_install.is_empty() && to_update.is_empty() && scripts_to_process.is_empty() {
        println!();
        println!(
            "{}",
            "All packages and scripts are already up to date".green()
        );
        return Ok(());
    }

    // Confirm installation
    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(());
    }

    println!();

    // Install/update packages
    let mut success_count = 0;
    let mut fail_count = 0;
    let mut successful_packages: Vec<String> = Vec::new();
    let mut failed_packages: Vec<String> = Vec::new();

    // Combine new installs and updates
    let all_packages: Vec<_> = to_install.into_iter().chain(to_update).collect();

    // Collect packages to update in cache (packages fetched from GitHub API)
    let mut packages_to_cache: Vec<(crate::core::Package, PackageSource)> = Vec::new();

    for (original_input_name, resolved, platform_match, installed_check_name) in all_packages {
        let pkg_name = &resolved.package.name;
        let repo_url = &resolved.package.repo;

        // Extract variant from input name (e.g., "bun::baseline" -> Some("baseline"))
        // This takes precedence over the global variant_filter parameter
        let input_variant = if original_input_name.contains("::") {
            original_input_name
                .split("::")
                .nth(1)
                .map(|s| s.to_string())
        } else {
            None
        };

        // Determine effective variant filter: input-specific variant takes precedence,
        // then global --variant flag, then (in update mode) the previously installed variant
        let mut effective_variant_filter = input_variant
            .as_deref()
            .or(variant_filter)
            .map(|s| s.to_string());

        if update_mode && effective_variant_filter.is_none() {
            if let Some(ref check_name) = installed_check_name {
                if let Some(inst_pkg) = installed.get_package(check_name) {
                    if let Some(ref variant) = inst_pkg.variant {
                        effective_variant_filter = Some(variant.clone());
                        log::debug!(
                            "Update mode: auto-selecting variant '{}' for {}",
                            variant,
                            check_name
                        );
                    }
                    // If variant is None: template matching handles selection in the filter step
                }
            }
        }
        let effective_variant_filter = effective_variant_filter.as_deref();

        // Try to fetch package info from GitHub API (includes download links)
        // If API rate limit is hit, fallback to cached package info
        let (pkg_to_install, version, using_fallback) = if let Some(custom_ver) = custom_version {
            let normalized_custom = custom_ver.trim_start_matches('v');
            if resolved.package.version.as_deref() == Some(normalized_custom) {
                // Already resolved in planning phase
                (
                    resolved.package.clone(),
                    normalized_custom.to_string(),
                    false,
                )
            } else if let Some(ref gh) = github {
                // User specified a version - fetch that specific version
                match gh.fetch_package_by_version(repo_url, custom_ver) {
                    Ok(versioned_pkg) => {
                        // Successfully fetched specific version from GitHub API
                        let version = custom_ver.trim_start_matches('v').to_string();
                        (versioned_pkg, version, false)
                    }
                    Err(e) => {
                        // API unavailable / version lookup failed. For bucket packages,
                        // try to derive the download URL by rewriting the cached URLs with
                        // the requested version (best-effort, validated by the download).
                        match (
                            matches!(resolved.source, PackageSource::Bucket { .. }),
                            derive_versioned_package(&resolved.package, custom_ver),
                        ) {
                            (true, Some(derived)) => {
                                let version = custom_ver.trim_start_matches('v').to_string();
                                println!(
                                    "  {} GitHub API unavailable; trying derived download URL for v{}",
                                    "⚠".yellow(),
                                    version
                                );
                                (derived, version, true)
                            }
                            _ => {
                                // Not a bucket package or no usable cached version - abort.
                                println!("  {} {}", "✗".red(), e);
                                fail_count += 1;
                                continue;
                            }
                        }
                    }
                }
            } else {
                match derive_versioned_package(&resolved.package, custom_ver) {
                    Some(derived) => {
                        let version = custom_ver.trim_start_matches('v').to_string();
                        (derived, version, true)
                    }
                    None => {
                        println!(
                            "  {} No usable cached version to derive {}",
                            "✗".red(),
                            custom_ver
                        );
                        fail_count += 1;
                        continue;
                    }
                }
            }
        } else if update_mode && matches!(resolved.source, PackageSource::Bucket { .. }) {
            // Update mode: the cache holds the latest package info synced by the update
            // command, so use it directly and skip the redundant install-time API round.
            let version = resolved
                .package
                .version
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            (resolved.package.clone(), version, false)
        } else if resolved.package.version.is_some() {
            // Already resolved in planning phase (e.g. latest version)
            let version = resolved.package.version.clone().unwrap();
            (resolved.package.clone(), version, false)
        } else if let Some(ref gh) = github {
            // No version specified - fetch latest
            match gh.fetch_package(repo_url) {
                Ok(latest_pkg) => {
                    // Successfully fetched from GitHub API - use latest download links
                    // Version is now included in the package struct
                    let version = latest_pkg
                        .version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    (latest_pkg, version, false)
                }
                Err(e) => {
                    // Failed to fetch from GitHub API (likely rate limit) - use cached package info
                    log::warn!(
                        "Failed to fetch latest package info from GitHub API for {}: {}",
                        pkg_name,
                        e
                    );
                    println!(
                        "  {} Using cached download links (GitHub API unavailable)",
                        "⚠".yellow()
                    );

                    // Use version from cached package if available
                    let version = resolved
                        .package
                        .version
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());
                    (resolved.package.clone(), version, true)
                }
            }
        } else {
            // No GitHub provider available, use cached package info
            let version = resolved
                .package
                .version
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            (resolved.package.clone(), version, true)
        };

        // Get all binaries for this platform
        let binaries = match pkg_to_install.platforms.get(&platform_match.platform_id) {
            Some(bins) => bins,
            None => {
                println!("  {} Platform binary not found", "✗".red());
                fail_count += 1;
                failed_packages.push(pkg_name.to_string());
                continue;
            }
        };

        // Apply variant filter / asset-name matching to narrow the binary candidates.
        //
        // In update mode: try asset-name template matching FIRST (most reliable).
        //   - This handles stale variant strings stored by old code (e.g. "(default)")
        //   - And cases where extract_variant_from_asset returns false positives (e.g. "64")
        //   - If asset-name matching succeeds, use it regardless of effective_variant_filter.
        //   - If it fails, fall back to named variant filter.
        // In add mode: use named variant filter, or return all binaries.
        // Precompute, once per binary, the normalized asset name and the variant.
        // Both normalize_asset_for_matching and extract_variant_from_asset allocate
        // (lowercase, split, join) and were previously recomputed inside every
        // filter pass below.
        let binary_meta: Vec<(
            usize,
            &crate::core::manifest::PlatformBinary,
            String,
            Option<String>,
        )> = binaries
            .iter()
            .enumerate()
            .map(|(idx, binary)| {
                let normalized = normalize_asset_for_matching(&binary.asset_name);
                let variant =
                    crate::core::manifest::extract_variant_from_asset(&binary.asset_name, pkg_name);
                (idx, binary, normalized, variant)
            })
            .collect();

        let (filtered_binaries, _original_indices): (Vec<_>, Vec<_>) = if update_mode {
            // Compute asset-name template from the previously installed package.
            let stored_template = installed_check_name
                .as_ref()
                .and_then(|k| installed.get_package(k))
                .map(|p| normalize_asset_for_matching(&p.asset_name));

            if let Some(template) = stored_template {
                let matched: Vec<_> = binary_meta
                    .iter()
                    .filter(|(_, _, normalized, _)| normalized.as_str() == template)
                    .map(|(idx, binary, _, _)| ((*binary).clone(), *idx))
                    .collect();

                if !matched.is_empty() {
                    // Exact asset-name match found — use it directly.
                    matched.into_iter().unzip()
                } else if let Some(filter) = effective_variant_filter {
                    // Asset-name match failed (package renamed its assets?): fall back to
                    // named variant filter as a secondary attempt.
                    binary_meta
                        .iter()
                        .filter(|(_, _, _, variant)| variant.as_deref() == Some(filter))
                        .map(|(idx, binary, _, _)| ((*binary).clone(), *idx))
                        .unzip()
                } else {
                    // Neither match succeeded: return all binaries and let
                    // select_packages_for_platform pick the best one with a warning.
                    (binaries.clone(), (0..binaries.len()).collect())
                }
            } else {
                // No stored asset_name (package not in installed.json): fall back to
                // named variant filter or return all binaries.
                if let Some(filter) = effective_variant_filter {
                    binary_meta
                        .iter()
                        .filter(|(_, _, _, variant)| variant.as_deref() == Some(filter))
                        .map(|(idx, binary, _, _)| ((*binary).clone(), *idx))
                        .unzip()
                } else {
                    (binaries.clone(), (0..binaries.len()).collect())
                }
            }
        } else if let Some(filter) = effective_variant_filter {
            // Normal add mode with named variant filter.
            binary_meta
                .iter()
                .filter(|(_, _, _, variant)| variant.as_deref() == Some(filter))
                .map(|(idx, binary, _, _)| ((*binary).clone(), *idx))
                .unzip()
        } else {
            // Normal add mode, no filter: return all binaries.
            (binaries.clone(), (0..binaries.len()).collect())
        };

        // Check if any binaries remain after filtering
        if filtered_binaries.is_empty() {
            if let Some(filter) = effective_variant_filter {
                if update_mode {
                    if yes {
                        println!(
                            "  {} Variant '{}' no longer available for {}, skipping",
                            "⚠".yellow(),
                            filter,
                            pkg_name
                        );
                    } else {
                        println!(
                            "  {} Variant '{}' no longer available for {}. Available variants:",
                            "⚠".yellow(),
                            filter,
                            pkg_name
                        );
                        print_available_variants(binaries, pkg_name);
                        println!(
                            "  Skipping this variant. Use 'wenget add {}::VARIANT' to switch.",
                            pkg_name
                        );
                    }
                } else {
                    println!(
                        "  {} No binaries found for variant '{}'. Available variants:",
                        "✗".red(),
                        filter
                    );
                    print_available_variants(binaries, pkg_name);
                }
            }
            fail_count += 1;
            failed_packages.push(pkg_name.to_string());
            continue;
        }

        // Select which packages to install (single, all, or user selection)
        let selected_indices =
            match select_packages_for_platform(pkg_name, &filtered_binaries, yes, update_mode) {
                Ok(indices) => indices,
                Err(e) => {
                    println!("  {} {}", "✗".red(), e);
                    fail_count += 1;
                    failed_packages.push(pkg_name.to_string());
                    continue;
                }
            };

        // Install each selected binary
        let mut parent_key: Option<String> = None;

        for (i, &idx) in selected_indices.iter().enumerate() {
            let binary = &filtered_binaries[idx];

            // Extract variant name from asset_name
            // If platform originally has only one binary and no filters applied, treat as default (no variant)
            let variant = if binaries.len() == 1
                && effective_variant_filter.is_none()
                && custom_platform.is_none()
            {
                // Platform has only one binary originally, treat as default
                None
            } else {
                crate::core::manifest::extract_variant_from_asset(&binary.asset_name, pkg_name)
            };
            let installed_key =
                crate::core::manifest::generate_installed_key(pkg_name, variant.as_deref());

            // Determine parent package (first one is parent, rest are children)
            let parent_package = if i == 0 {
                parent_key = Some(installed_key.clone());
                None
            } else {
                parent_key.clone()
            };

            println!("{} {} v{}...", "Installing".cyan(), installed_key, version);
            if using_fallback {
                println!(
                    "  {} Falling back to bucket source download links",
                    "ℹ".cyan()
                );
            }
            if selected_indices.len() > 1 {
                println!("  {} From: {}", "ℹ".cyan(), binary.asset_name.dimmed());
            }

            match install_package(
                installed,
                paths,
                &pkg_to_install,
                &platform_match,
                binary,
                &version,
                &resolved.source,
                &installed_key,
                parent_package.as_deref(),
                custom_name,
                yes,
                no_suffix,
                update_mode,
            ) {
                Ok(inst_pkg) => {
                    record_installed(paths, installed, installed_key.clone(), inst_pkg);

                    // Collect package for cache update if fetched from GitHub API
                    // (only once, not for each binary)
                    if i == 0 && !using_fallback {
                        packages_to_cache.push((pkg_to_install.clone(), resolved.source.clone()));
                    }

                    println!("  {} Installed successfully", "✓".green());
                    success_count += 1;
                    successful_packages.push(installed_key.clone());
                }
                Err(e) => {
                    println!("  {} {}", "✗".red(), e);
                    fail_count += 1;
                    failed_packages.push(installed_key.clone());
                }
            }
            println!();
        }
    }

    // Update cache with latest package info from GitHub API
    if !packages_to_cache.is_empty() {
        match update_cache_with_packages(config, packages_to_cache) {
            Ok(count) => {
                log::info!("Updated cache with {} latest package(s)", count);
            }
            Err(e) => {
                log::warn!("Failed to update cache: {}", e);
                // Don't fail the entire operation if cache update fails
            }
        }
    }

    // Install scripts from bucket cache
    let mut script_success_count = 0;
    let mut script_fail_count = 0;
    let mut successful_scripts: Vec<String> = Vec::new();
    let mut failed_scripts: Vec<String> = Vec::new();

    for (name, url, script_type, origin) in scripts_to_process {
        println!(
            "{}",
            format!("Installing {} ({})...", name, script_type.display_name()).bold()
        );

        match install_script_from_bucket(
            config,
            paths,
            installed,
            &name,
            &url,
            script_type.clone(),
            &origin,
            custom_name,
        ) {
            Ok(_) => {
                println!("  {} Installed successfully", "✓".green());
                script_success_count += 1;
                successful_scripts.push(name);
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                script_fail_count += 1;
                failed_scripts.push(name);
            }
        }
        println!();
    }

    // Summary
    println!("{}", "Summary:".bold());
    if success_count > 0 {
        println!(
            "  {} {} package(s) installed: {}",
            "✓".green(),
            success_count,
            successful_packages.join(" ")
        );
    }
    if fail_count > 0 {
        println!(
            "  {} {} package(s) failed: {}",
            "✗".red(),
            fail_count,
            failed_packages.join(" ")
        );
    }
    if script_success_count > 0 {
        println!(
            "  {} {} script(s) installed: {}",
            "✓".green(),
            script_success_count,
            successful_scripts.join(" ")
        );
    }
    if script_fail_count > 0 {
        println!(
            "  {} {} script(s) failed: {}",
            "✗".red(),
            script_fail_count,
            failed_scripts.join(" ")
        );
    }

    Ok(())
}

/// Install a single package
///
/// `installed` is the in-memory snapshot of `installed.json` held by the caller
/// (`install_packages`). It is used for executable reuse in update mode and for
/// command-name conflict resolution. It must NOT be re-read from disk here: the
/// caller holds the authoritative in-memory copy and persists it after install.
#[allow(clippy::too_many_arguments)]
fn install_package(
    installed: &crate::core::InstalledSet,
    paths: &WenPaths,
    pkg: &crate::core::Package,
    platform_match: &crate::core::platform::PlatformMatch,
    binary: &crate::core::manifest::PlatformBinary,
    version: &str,
    source: &PackageSource,
    installed_key: &str,
    _parent_package: Option<&str>,
    custom_name: Option<&str>,
    yes: bool,
    no_suffix: bool,
    update_mode: bool,
) -> Result<InstalledPackage> {
    // Log if using fallback
    if let Some(fallback_type) = &platform_match.fallback_type {
        log::info!(
            "Using fallback platform {} ({})",
            platform_match.platform_id,
            fallback_type.description()
        );
    }

    // Download binary
    println!("  Downloading from {}...", binary.url);

    let download_dir = paths.downloads_dir();
    fs::create_dir_all(&download_dir)?;

    // Determine file extension from URL
    let filename = binary
        .url
        .split('/')
        .next_back()
        .context("Invalid download URL")?;

    let download_path = download_dir.join(filename);

    downloader::download_file(&binary.url, &download_path)?;

    if let Err(e) =
        crate::core::checksum::verify_download(&binary.url, &binary.asset_name, &download_path)
    {
        fs::remove_file(&download_path).ok();
        return Err(e);
    }

    // Sanitized directory names are lossy, so a different package may already
    // own the directory this key maps to.
    crate::core::InstalledStore::new(paths.clone()).ensure_dir_available(installed_key)?;

    // Stage the extraction beside the app directory and swap it in on success, so
    // a failed install leaves the previous install and its record untouched.
    let staged = crate::installer::StagedInstall::begin(paths, installed_key)?;
    let app_dir = staged.target().to_path_buf();

    println!("  Extracting to {}...", app_dir.display());

    let extracted_files = extract_archive(&download_path, staged.path())?;

    // Find executable candidates (pass the staging dir for Unix permission checks)
    let candidates = find_executable_candidates(&extracted_files, &pkg.name, Some(staged.path()));

    if candidates.is_empty() {
        anyhow::bail!(
            "Failed to find executable in archive. Extracted files:\n{}",
            extracted_files.join("\n")
        );
    }

    // Select executables
    let selected_executables = if candidates.len() == 1 {
        // Single candidate - auto-select
        let selected = &candidates[0];
        println!(
            "  Found executable: {} ({})",
            selected.path, selected.reason
        );
        vec![candidates[0].path.clone()]
    } else if update_mode {
        // Update mode: keep previously installed executables, ignore new ones,
        // prompt for replacement when old executables disappear
        let old_exes = installed
            .get_package(installed_key)
            .map(|p| p.executables.clone());

        if let Some(ref old) = old_exes {
            let old_paths: std::collections::HashSet<_> = old.keys().cloned().collect();

            // Separate: previously installed vs new candidates
            let mut kept: Vec<&crate::installer::extractor::ExecutableCandidate> = Vec::new();
            let mut new_candidates: Vec<&crate::installer::extractor::ExecutableCandidate> =
                Vec::new();

            for c in &candidates {
                if old_paths.contains(&c.path) {
                    kept.push(c);
                } else if c.score > 0 {
                    new_candidates.push(c);
                }
            }

            if kept.is_empty() && new_candidates.is_empty() {
                println!(
                    "  {} No matching executables found for update, skipping {}",
                    "⚠".yellow(),
                    installed_key
                );
                anyhow::bail!(
                    "No matching executables found for update of {}",
                    installed_key
                );
            }

            let mut selected: Vec<String> = kept.iter().map(|c| c.path.clone()).collect();

            // Detect disappeared executables: old paths not found in any candidate
            let disappeared: Vec<(&String, &String)> = old
                .iter()
                .filter(|(path, _)| !kept.iter().any(|c| &c.path == *path))
                .collect();

            if !disappeared.is_empty() {
                for (old_path, old_cmd) in &disappeared {
                    let old_filename = Path::new(old_path).file_name().and_then(|s| s.to_str());

                    // Try auto-match by filename in new candidates
                    let auto_match = old_filename.and_then(|old_fname| {
                        new_candidates.iter().find(|c| {
                            Path::new(&c.path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .map(|f| f == old_fname)
                                .unwrap_or(false)
                        })
                    });

                    if let Some(matched) = auto_match {
                        // Auto-matched by filename — select silently
                        if !selected.contains(&matched.path) {
                            println!(
                                "  {} Executable '{}' relocated to '{}' (auto-matched)",
                                "ℹ".cyan(),
                                old_path,
                                matched.path
                            );
                            selected.push(matched.path.clone());
                        }
                    } else if !new_candidates.is_empty() && !yes {
                        // No auto-match — prompt user to pick a replacement
                        println!(
                            "  {} Executable '{}' (command: {}) is no longer available in this release",
                            "⚠".yellow(),
                            old_path,
                            old_cmd
                        );

                        use dialoguer::Select;
                        let mut items: Vec<String> = new_candidates
                            .iter()
                            .filter(|c| !selected.contains(&c.path))
                            .map(|c| format!("{} ({})", c.path, c.reason))
                            .collect();
                        items.push("Skip (remove this command)".to_string());

                        let selection = Select::new()
                            .with_prompt(format!("    Select replacement for '{}'", old_cmd))
                            .items(&items)
                            .default(items.len() - 1)
                            .interact()?;

                        if selection < items.len() - 1 {
                            // User picked a replacement from new candidates
                            let available: Vec<_> = new_candidates
                                .iter()
                                .filter(|c| !selected.contains(&c.path))
                                .collect();
                            if selection < available.len() {
                                selected.push(available[selection].path.clone());
                            }
                        }
                        // else: user chose "Skip" — old command will be cleaned up
                    } else {
                        // --yes mode or no new candidates: warn and auto-cleanup
                        println!(
                            "  {} Executable '{}' (command: {}) no longer available, will be removed",
                            "⚠".yellow(),
                            old_path,
                            old_cmd
                        );
                    }
                }
            }

            // New executables not in old install are silently ignored during updates

            println!("  Found {} executables (update mode):", selected.len());
            for s in &selected {
                let reason = candidates
                    .iter()
                    .find(|c| c.path == *s)
                    .map(|c| c.reason.as_str())
                    .unwrap_or("matched");
                println!("    {} ({})", s, reason);
            }
            selected
        } else {
            // No old executables — fall through to normal auto-select
            let auto_select: Vec<_> = candidates.iter().filter(|c| c.score > 0).collect();
            println!("  Found {} executables:", auto_select.len());
            for c in &auto_select {
                println!("    {} ({})", c.path, c.reason);
            }
            auto_select.into_iter().map(|c| c.path.clone()).collect()
        }
    } else {
        // Multiple candidates - select all with valid scores (exec permission or name match)
        // On Unix, exec permission gives +35 score, name match gives +50
        // Files without any match get score 0 and should be filtered out
        let auto_select: Vec<_> = candidates
            .iter()
            .filter(|c| c.score > 0) // All valid candidates
            .collect();

        if auto_select.len() <= 3 || yes {
            // Auto-select if reasonable count (<=3) or --yes flag
            println!("  Found {} executables:", auto_select.len());
            for c in &auto_select {
                println!("    {} ({})", c.path, c.reason);
            }
            auto_select.into_iter().map(|c| c.path.clone()).collect()
        } else {
            // Too many candidates - show interactive selection
            use dialoguer::MultiSelect;

            println!("  Found {} possible executables:", candidates.len());

            let items: Vec<String> = candidates
                .iter()
                .map(|c| format!("{} (score: {}, {})", c.path, c.score, c.reason))
                .collect();

            let selections = MultiSelect::new()
                .with_prompt("Select executables to install (Space to select, Enter to confirm)")
                .items(&items)
                .interact()?;

            if selections.is_empty() {
                anyhow::bail!("No executables selected");
            }

            selections
                .into_iter()
                .map(|i| candidates[i].path.clone())
                .collect()
        }
    };

    // Every remaining step reads from the final location: swap now, so the
    // launchers point at the app directory rather than the staging path.
    let app_dir = staged.commit()?;

    // Install all selected executables
    let mut executables: HashMap<String, String> = HashMap::new();

    // Extract repo_name and variant from installed_key for resolve_command_name
    // installed_key format: "repo_name" or "repo_name::variant"
    let (_, variant_opt) = if let Some(pos) = installed_key.find("::") {
        (
            installed_key[..pos].to_string(),
            if no_suffix {
                None
            } else {
                Some(installed_key[pos + 2..].to_string())
            },
        )
    } else {
        (installed_key.to_string(), None)
    };

    // If this package is already installed, grab old executables for command name reuse
    let old_executables = installed
        .get_package(installed_key)
        .map(|p| p.executables.clone());

    // Precompute the set of command names already in use (excluding this package)
    // so per-executable conflict checks are O(1) instead of scanning all packages
    // for every candidate suffix in `resolve_command_name`.
    let mut taken_names = installed.command_name_set(Some(installed_key));

    for exe_relative in selected_executables {
        let exe_path = app_dir.join(&exe_relative);

        if !exe_path.exists() {
            anyhow::bail!("Executable not found: {}", exe_path.display());
        }

        // When updating, try to reuse old command names
        let reused_name = if update_mode {
            if let Some(ref old_exes) = old_executables {
                let filename = exe_path.file_name().and_then(|s| s.to_str()).unwrap_or("");

                // Try path match first, then filename match
                old_exes.get(&exe_relative).cloned().or_else(|| {
                    old_exes
                        .iter()
                        .find(|(old_path, _)| {
                            std::path::Path::new(old_path.as_str())
                                .file_name()
                                .and_then(|s| s.to_str())
                                == Some(filename)
                        })
                        .map(|(_, name)| name.clone())
                })
            } else {
                None
            }
        } else {
            None
        };

        let resolved_name = if let Some(reused) = reused_name {
            println!("  Reusing command name: {}", reused);
            reused
        } else {
            // Extract the actual command name from the executable path
            let (base_name, is_custom) = if let Some(custom) = custom_name {
                // Use custom name if provided (only for first executable)
                if executables.is_empty() {
                    (custom.to_string(), true)
                } else {
                    // For additional executables, use auto-detected name
                    let raw_name = exe_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .context("Failed to extract command name")?;
                    (normalize_command_name(raw_name), false)
                }
            } else {
                // Auto-detect and normalize command name
                let raw_name = exe_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .context("Failed to extract command name")?;

                // Apply smart normalization to remove platform suffixes
                (normalize_command_name(raw_name), false)
            };

            // Resolve command name with variant to avoid conflicts
            resolve_command_name(&base_name, variant_opt.as_deref(), &taken_names, is_custom)
        };

        println!("  Command will be available as: {}", resolved_name);

        // Create symlink/shim using the resolved name
        let bin_path = paths.bin_shim_path(&resolved_name);

        println!("  Creating launcher at {}...", bin_path.display());

        #[cfg(unix)]
        {
            create_symlink(&exe_path, &bin_path)?;
        }

        #[cfg(windows)]
        {
            create_shim(&exe_path, &bin_path, &resolved_name)?;
        }

        // Record the name as taken so subsequent executables in the same package
        // don't resolve to a colliding name.
        taken_names.insert(resolved_name.clone());
        executables.insert(exe_relative.clone(), resolved_name);
    }

    // Clean up symlinks/shims for old executables that no longer exist in the new version
    if let Some(ref old_exes) = old_executables {
        for old_cmd in old_exes.values() {
            if !executables.values().any(|n| n == old_cmd) {
                let old_bin = paths.bin_shim_path(old_cmd);
                if old_bin.exists() {
                    fs::remove_file(&old_bin).ok();
                    println!("  Removed obsolete command: {}", old_cmd);
                }
            }
        }
    }

    // Clean up download
    fs::remove_file(&download_path)?;

    // Extract repo_name and variant from installed_key
    // installed_key format: "repo_name" or "repo_name::variant"
    let (repo_name, variant) = if let Some(pos) = installed_key.find("::") {
        (
            installed_key[..pos].to_string(),
            Some(installed_key[pos + 2..].to_string()),
        )
    } else {
        (installed_key.to_string(), None)
    };

    // Create installed package info
    let inst_pkg = InstalledPackage {
        meta_version: crate::core::manifest::CURRENT_META_VERSION,
        repo_name,
        variant,
        version: version.to_string(),
        platform: platform_match.platform_id.clone(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        executables,
        source: source.clone(),
        description: pkg.description.clone(),
        command_names: vec![],
        command_name: None,
        asset_name: binary.asset_name.clone(),
        parent_package: None, // Deprecated field
        download_url: None,
    };

    Ok(inst_pkg)
}

/// Derive a package for a specific version by rewriting the cached download URLs.
///
/// GitHub release assets always live at `.../releases/download/{tag}/{asset_name}`,
/// so replacing the cached version substring with the requested version yields the URL
/// for that version. This covers both the version in the tag path and any version
/// embedded in the asset name, and preserves a `v` prefix because only the numeric
/// part is swapped (e.g. `v0.8.1/foo` -> `v2.0.0/foo`).
///
/// This is a best-effort fallback used only when the GitHub API is unavailable: it
/// fails (the download later 404s) if the project uses a tag scheme that doesn't
/// contain the version, or changed its asset naming between versions.
///
/// Returns `None` when the cached version can't be used as a substitution anchor.
fn derive_versioned_package(
    cached: &crate::core::Package,
    requested_version: &str,
) -> Option<crate::core::Package> {
    let old_ver = cached.version.as_deref()?.trim_start_matches('v');
    let new_ver = requested_version.trim_start_matches('v');

    if old_ver.is_empty() || old_ver == "unknown" || old_ver == "local" {
        return None;
    }

    let platforms = cached
        .platforms
        .iter()
        .map(|(platform_id, binaries)| {
            let rewritten = binaries
                .iter()
                .map(|b| crate::core::manifest::PlatformBinary {
                    url: b.url.replace(old_ver, new_ver),
                    size: 0,        // unknown for a derived URL
                    checksum: None, // cached checksum is for a different version
                    asset_name: b.asset_name.replace(old_ver, new_ver),
                })
                .collect();
            (platform_id.clone(), rewritten)
        })
        .collect();

    Some(crate::core::Package {
        name: cached.name.clone(),
        description: cached.description.clone(),
        repo: cached.repo.clone(),
        homepage: cached.homepage.clone(),
        license: cached.license.clone(),
        version: Some(new_ver.to_string()),
        platforms,
    })
}

/// Update manifest cache with latest package info from GitHub API
fn update_cache_with_packages(
    config: &Config,
    packages: Vec<(crate::core::Package, PackageSource)>,
) -> Result<usize> {
    // Load current cache
    let mut cache = config.get_or_rebuild_cache()?;

    // Save count before moving packages
    let count = packages.len();

    // Update cache with new package info
    for (package, source) in packages {
        log::debug!(
            "Updating cache with latest info for {} from GitHub API",
            package.name
        );
        cache.add_package(package, source);
    }

    // Save updated cache
    config.save_cache(&cache)?;

    Ok(count)
}

/// Persist one package's record, then update the caller's in-memory snapshot.
///
/// The snapshot is what command-name conflict resolution reads during this run;
/// the record on disk is what survives it.
fn record_installed(
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    key: String,
    pkg: InstalledPackage,
) {
    if let Err(e) = crate::core::InstalledStore::new(paths.clone()).save_package(&key, &pkg) {
        eprintln!(
            "{} Failed to save the package record for {}: {}",
            "✗".red(),
            key,
            e
        );
    }
    installed.upsert_package(key, pkg);
}

/// Install a script from bucket cache
#[allow(clippy::too_many_arguments)]
fn install_script_from_bucket(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    name: &str,
    url: &str,
    script_type: ScriptType,
    origin: &str,
    custom_name: Option<&str>,
) -> Result<()> {
    println!("  Downloading script from {}...", url);

    // Download script content
    let content = download_script(url)?;

    // Determine the final command name
    let command_name = custom_name.unwrap_or(name);

    println!("  Installing script as '{}'...", command_name);

    // Install script to app directory
    let files = install_script(paths, command_name, &content, &script_type)?;

    println!("  Command will be available as: {}", command_name);

    // Create shim
    println!("  Creating launcher...");
    create_script_shim(paths, command_name, &script_type)?;

    // Create executables map
    let mut executables = HashMap::new();
    if let Some(script_file) = files.first() {
        executables.insert(script_file.clone(), command_name.to_string());
    } else {
        executables.insert(
            format!("{}.{}", command_name, script_type.extension()),
            command_name.to_string(),
        );
    }

    // Create installed package info
    let inst_pkg = InstalledPackage {
        meta_version: crate::core::manifest::CURRENT_META_VERSION,
        repo_name: command_name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: std::env::consts::OS.to_string(),
        installed_at: Utc::now(),
        install_path: paths.app_dir(command_name).display().to_string(),
        executables,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from bucket", script_type.display_name()),
        command_names: vec![],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: Some(url.to_string()),
    };
    record_installed(paths, installed, name.to_string(), inst_pkg);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached_pkg(version: &str, url: &str, asset_name: &str) -> crate::core::Package {
        let mut platforms = HashMap::new();
        platforms.insert(
            "linux-armv7".to_string(),
            vec![crate::core::manifest::PlatformBinary {
                url: url.to_string(),
                size: 123,
                checksum: Some("abc".to_string()),
                asset_name: asset_name.to_string(),
            }],
        );
        crate::core::Package {
            name: "sshi".to_string(),
            description: "desc".to_string(),
            repo: "https://github.com/superyngo/sshi".to_string(),
            homepage: None,
            license: None,
            version: Some(version.to_string()),
            platforms,
        }
    }

    #[test]
    fn test_derive_versioned_package_version_in_tag_only() {
        // sshi-style: version only in the tag path, not the asset name.
        let cached = cached_pkg(
            "1.2.0",
            "https://github.com/superyngo/sshi/releases/download/v1.2.0/sshi-linux-armv7.tar.gz",
            "sshi-linux-armv7.tar.gz",
        );
        let derived = derive_versioned_package(&cached, "1.3.0").unwrap();
        let bin = &derived.platforms["linux-armv7"][0];
        assert_eq!(
            bin.url,
            "https://github.com/superyngo/sshi/releases/download/v1.3.0/sshi-linux-armv7.tar.gz"
        );
        assert_eq!(bin.asset_name, "sshi-linux-armv7.tar.gz");
        assert_eq!(derived.version.as_deref(), Some("1.3.0"));
        assert!(bin.checksum.is_none());
    }

    #[test]
    fn test_derive_versioned_package_version_in_asset_name() {
        // Nexus-style: version in both the tag and the asset name.
        let cached = cached_pkg(
            "0.8.1",
            "https://github.com/x/y/releases/download/v0.8.1/Setup_0.8.1.exe",
            "Setup_0.8.1.exe",
        );
        let derived = derive_versioned_package(&cached, "v2.0.0").unwrap();
        let bin = &derived.platforms["linux-armv7"][0];
        assert_eq!(
            bin.url,
            "https://github.com/x/y/releases/download/v2.0.0/Setup_2.0.0.exe"
        );
        assert_eq!(bin.asset_name, "Setup_2.0.0.exe");
    }

    #[test]
    fn test_derive_versioned_package_rejects_unusable_version() {
        let cached = cached_pkg(
            "local",
            "https://github.com/x/y/releases/download/v1/y.tar.gz",
            "y.tar.gz",
        );
        assert!(derive_versioned_package(&cached, "1.0.0").is_none());

        let mut no_version = cached_pkg(
            "1.0.0",
            "https://github.com/x/y/releases/download/v1.0.0/y.tar.gz",
            "y.tar.gz",
        );
        no_version.version = None;
        assert!(derive_versioned_package(&no_version, "1.0.0").is_none());
    }

    #[test]
    fn test_normalize_asset_for_matching() {
        // Same binary across versions should produce identical templates
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            "uv-x86-64-unknown-linux-gnu"
        );
        assert_eq!(
            normalize_asset_for_matching("gh_copilot_1.0.21_linux_amd64.tar.gz"),
            normalize_asset_for_matching("gh_copilot_1.0.22_linux_amd64.tar.gz")
        );
        assert_eq!(
            normalize_asset_for_matching("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"),
            normalize_asset_for_matching("ripgrep-14.1.2-x86_64-unknown-linux-musl.tar.gz")
        );
        // Same asset produces same template
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz")
        );
        // Different arch should NOT match
        assert_ne!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-unknown-linux-gnu.tar.gz")
        );
        // zip extension
        assert_eq!(
            normalize_asset_for_matching("bun-linux-x64.zip"),
            "bun-linux-x64"
        );
        // apple stays — template matching uses full normalized name for comparison
        assert_eq!(
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz")
        );
    }

    #[test]
    fn test_resolve_command_name_no_conflict() {
        let taken = std::collections::HashSet::new();
        // No variant, name free -> base name returned as-is.
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg");
    }

    #[test]
    fn test_resolve_command_name_numeric_suffix_on_conflict() {
        let mut taken = std::collections::HashSet::new();
        taken.insert("rg".to_string());

        // "rg" taken -> "rg-1"
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg-1");

        // "rg" and "rg-1" taken -> "rg-2"
        taken.insert("rg-1".to_string());
        assert_eq!(resolve_command_name("rg", None, &taken, false), "rg-2");
    }

    #[test]
    fn test_resolve_command_name_with_variant() {
        let taken = std::collections::HashSet::new();
        // Variant appends "-variant" when base doesn't already end with it.
        assert_eq!(
            resolve_command_name("bun", Some("baseline"), &taken, false),
            "bun-baseline"
        );
    }

    #[test]
    fn test_resolve_command_name_variant_already_suffixed() {
        let taken = std::collections::HashSet::new();
        // Base already ends with "-profile" variant -> kept as-is.
        assert_eq!(
            resolve_command_name("bun-profile", Some("profile"), &taken, false),
            "bun-profile"
        );
    }

    #[test]
    fn test_resolve_command_name_custom_takes_numeric_suffix() {
        let mut taken = std::collections::HashSet::new();
        taken.insert("mytool".to_string());
        // Custom name that conflicts falls through to numeric suffix.
        assert_eq!(
            resolve_command_name("mytool", None, &taken, true),
            "mytool-1"
        );
    }
}
