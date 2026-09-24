//! Add (Install) command implementation

use crate::core::cache::ManifestCache;
use crate::core::manifest::{PackageSource, ScriptType};
use crate::core::{Config, InstalledPackage, Platform, WenPaths};
use crate::downloader;
use crate::installer::package::{
    filter_binaries, target_package, InstallRequest, InstallUi, PackageInstaller, TargetStatus,
};
use crate::installer::{
    create_script_shim, detect_script_type, download_script, extract_script_name,
    input_detector::{detect_input_type, InputType},
    install_script,
    local::install_local_file,
    read_local_script,
};
use crate::package_resolver::{PackageInput, PackageResolver, ResolvedPackage};
use crate::providers::GitHubProvider;
use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Options for `add::run`, shared by the `add` command and `update`
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Skip confirmation prompts
    pub yes: bool,
    /// Custom command name for the installed launcher
    pub script_name: Option<String>,
    /// Platform override (e.g. `linux-x64`)
    pub platform: Option<String>,
    /// Install this version instead of the latest
    pub version: Option<String>,
    /// Only consider binaries of this variant
    pub variant_filter: Option<String>,
    /// Don't append a variant suffix to command names
    pub no_suffix: bool,
    /// Set by `update`: reinstall already-installed packages, keeping their variant
    pub update_mode: bool,
    /// `--skip-checksum`: don't verify downloads against published checksums
    pub skip_checksum: bool,
}

/// Successes and failures of one batch (packages, scripts, files or URLs)
struct BatchReport {
    noun: &'static str,
    succeeded: Vec<String>,
    failed: Vec<String>,
}

impl BatchReport {
    fn new(noun: &'static str) -> Self {
        Self {
            noun,
            succeeded: Vec::new(),
            failed: Vec::new(),
        }
    }

    fn ok(&mut self, name: String) {
        self.succeeded.push(name);
    }

    fn fail(&mut self, name: String) {
        self.failed.push(name);
    }

    fn failures(&self) -> usize {
        self.failed.len()
    }

    /// Print the installed/failed summary lines (nothing for an empty side)
    fn print(&self) {
        if !self.succeeded.is_empty() {
            println!(
                "  {} {} {}(s) installed: {}",
                "✓".green(),
                self.succeeded.len(),
                self.noun,
                self.succeeded.join(" ")
            );
        }
        if !self.failed.is_empty() {
            println!(
                "  {} {} {}(s) failed: {}",
                "✗".red(),
                self.failed.len(),
                self.noun,
                self.failed.join(" ")
            );
        }
    }
}

/// Install packages (smart detection: package names from cache or GitHub URLs)
pub fn run(names: Vec<String>, opts: InstallOptions) -> Result<()> {
    let config = Config::new()?;

    // Ensure initialized
    if !config.is_initialized() {
        config.init()?;
    }

    let mut installed = config.load_installed()?;
    run_with(&config, &mut installed, None, names, opts)
}

/// Install with state the caller already loaded
///
/// `update` passes its `InstalledSet` and freshly synced `ManifestCache` so neither is
/// re-read from disk; `cache: None` loads (or rebuilds) the cache as needed.
pub fn run_with(
    config: &Config,
    installed: &mut crate::core::InstalledSet,
    cache: Option<ManifestCache>,
    names: Vec<String>,
    opts: InstallOptions,
) -> Result<()> {
    let yes = opts.yes;
    let script_name = opts.script_name.clone();
    let paths = config.paths().clone();

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

    // Each installer returns its failure count; run them all before reporting failure
    let mut failures = 0;

    // Handle script installations
    if !script_inputs.is_empty() {
        failures += install_scripts(
            config,
            &paths,
            installed,
            script_inputs,
            yes,
            script_name.as_deref(),
        )?;
    }

    // Handle local file installations
    if !local_inputs.is_empty() {
        failures += install_local_files(
            config,
            &paths,
            installed,
            local_inputs,
            yes,
            script_name.as_deref(),
        )?;
    }

    // Handle direct URL installations
    if !url_inputs.is_empty() {
        failures += install_from_urls(
            config,
            &paths,
            installed,
            url_inputs,
            yes,
            script_name.as_deref(),
            opts.skip_checksum,
        )?;
    }

    // Handle package installations (existing logic)
    if !package_inputs.is_empty() {
        failures += install_packages(
            config,
            &paths,
            installed,
            package_inputs,
            &opts,
            &crate::utils::prompt::TerminalUi,
            cache,
        )?;
    }

    if failures > 0 {
        anyhow::bail!("{} install(s) failed", failures);
    }

    Ok(())
}

/// Load script content from a remote URL or a local file path
fn fetch_script_content(input: &str) -> Option<String> {
    let is_url = input.starts_with("http://") || input.starts_with("https://");
    if is_url {
        match download_script(input) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("{} Failed to download {}: {}", "✗".red(), input, e);
                None
            }
        }
    } else {
        let path = Path::new(input);
        match read_local_script(path) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("{} Failed to read {}: {}", "✗".red(), input, e);
                None
            }
        }
    }
}

/// Validate and resolve a single script input to its installable metadata
fn resolve_script_candidate(
    installed: &crate::core::InstalledSet,
    input: &str,
    custom_name: Option<&str>,
) -> Option<(String, String, ScriptType, String)> {
    let content = fetch_script_content(input)?;

    let script_type = match detect_script_type(input, &content) {
        Some(t) => t,
        None => {
            eprintln!("{} Cannot detect script type for: {}", "✗".red(), input);
            return None;
        }
    };

    if !crate::installer::script::is_interpreter_available(&script_type) {
        println!(
            "  {} {} ({}) - {}",
            "⚠".yellow(),
            input,
            script_type.display_name(),
            "not supported on this platform".yellow()
        );
        return None;
    }

    let name = if let Some(custom) = custom_name {
        custom.to_string()
    } else {
        match extract_script_name(input) {
            Some(n) => n,
            None => {
                eprintln!("{} Cannot extract name from: {}", "✗".red(), input);
                return None;
            }
        }
    };

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

    Some((name, content, script_type, input.to_string()))
}

/// Show security warning and prompt for user confirmation if not --yes
fn confirm_script_installation(yes: bool) -> Result<bool> {
    println!();
    println!(
        "{}",
        "⚠  Security Warning: Review scripts before running them!"
            .yellow()
            .bold()
    );

    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(false);
    }
    Ok(true)
}

/// Install each resolved script and record it in the installed set
fn execute_script_installs(
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    scripts: Vec<(String, String, ScriptType, String)>,
) -> BatchReport {
    let mut report = BatchReport::new("script");

    for (name, content, script_type, origin) in scripts {
        println!(
            "{} {} ({})...",
            "Installing".cyan(),
            name,
            script_type.display_name()
        );

        match install_single_script(paths, &name, &content, &script_type, &origin) {
            Ok(inst_pkg) => {
                if let Err(e) = record_installed(paths, installed, name.clone(), inst_pkg) {
                    println!("  {} {:#}", "✗".red(), e);
                    report.fail(name);
                    continue;
                }
                println!("  {} Installed successfully", "✓".green());
                report.ok(name);
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                report.fail(name);
            }
        }
    }

    report
}

/// Install scripts from local paths or URLs
fn install_scripts(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    script_inputs: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
) -> Result<usize> {
    println!("{}", "Scripts to install:".bold());

    let mut scripts_to_install = Vec::new();
    let mut resolve_failures = 0;

    for input in script_inputs {
        match resolve_script_candidate(installed, input, custom_name) {
            Some(item) => scripts_to_install.push(item),
            None => resolve_failures += 1,
        }
    }

    if scripts_to_install.is_empty() {
        println!("{}", "No scripts to install".yellow());
        return Ok(resolve_failures);
    }

    if !confirm_script_installation(yes)? {
        return Ok(resolve_failures);
    }

    println!();
    let report = execute_script_installs(paths, installed, scripts_to_install);

    println!();
    println!("{}", "Summary:".bold());
    report.print();

    Ok(resolve_failures + report.failures())
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
        schema_version: crate::core::manifest::CURRENT_SCHEMA_VERSION,
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
) -> Result<usize> {
    println!("{}", "Local files to install:".bold());

    for file in &files {
        println!("  • {}", file);
    }

    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(0);
    }

    println!();

    let mut report = BatchReport::new("file");

    for file in files {
        println!("{} {}...", "Installing".cyan(), file);
        let path = Path::new(file);

        match install_local_file(paths, path, custom_name, None) {
            Ok(inst_pkg) => {
                // Key the record by the app dir the installer created
                let name = inst_pkg.repo_name.clone();
                let display_names = inst_pkg.get_command_names().join(", ");
                if let Err(e) = record_installed(paths, installed, name.clone(), inst_pkg) {
                    println!("  {} {:#}", "✗".red(), e);
                    report.fail(file.to_string());
                    continue;
                }
                println!(
                    "  {} Installed successfully as {}",
                    "✓".green(),
                    display_names
                );
                report.ok(name);
            }
            Err(e) => {
                println!("  {} Failed to install {}: {}", "✗".red(), file, e);
                report.fail(file.to_string());
            }
        }
        println!();
    }

    println!("{}", "Summary:".bold());
    report.print();

    Ok(report.failures())
}

/// Install binary or archive from direct URLs
fn install_from_urls(
    _config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    urls: Vec<&String>,
    yes: bool,
    custom_name: Option<&str>,
    skip_checksum: bool,
) -> Result<usize> {
    println!("{}", "URLs to install:".bold());

    for url in &urls {
        println!("  • {}", url);
    }

    if !yes && !crate::utils::confirm("\nProceed with installation?")? {
        println!("Installation cancelled");
        return Ok(0);
    }

    println!();

    let mut report = BatchReport::new("URL");

    // Create temp dir for downloads
    let temp_dir = paths.cache_dir().join("downloads");
    fs::create_dir_all(&temp_dir)?;

    for url in urls {
        println!("{} {}...", "Downloading".cyan(), url);

        let filename = match url.split('/').next_back() {
            Some(name) => name,
            None => {
                println!("  {} Invalid URL", "✗".red());
                report.fail(url.to_string());
                continue;
            }
        };

        // Handle query parameters in URL
        let filename = filename.split('?').next().unwrap_or(filename);
        let download_path = temp_dir.join(filename);

        match downloader::download_file(url, &download_path) {
            Ok(_) => {
                println!("  {} Downloaded", "✓".green());

                if let Err(e) = crate::core::checksum::verify_download(
                    url,
                    filename,
                    &download_path,
                    None,
                    skip_checksum,
                ) {
                    println!("  {} {}", "✗".red(), e);
                    report.fail(url.to_string());
                } else {
                    println!("{} {}...", "Installing".cyan(), filename);

                    match install_local_file(
                        paths,
                        &download_path,
                        custom_name,
                        Some(url.to_string()),
                    ) {
                        Ok(inst_pkg) => {
                            // Key the record by the app dir the installer created
                            let name = inst_pkg.repo_name.clone();
                            let display_names = inst_pkg.get_command_names().join(", ");
                            if let Err(e) =
                                record_installed(paths, installed, name.clone(), inst_pkg)
                            {
                                println!("  {} {:#}", "✗".red(), e);
                                report.fail(filename.to_string());
                            } else {
                                println!(
                                    "  {} Installed successfully as {}",
                                    "✓".green(),
                                    display_names
                                );
                                report.ok(name);
                            }
                        }
                        Err(e) => {
                            println!("  {} Failed to install {}: {}", "✗".red(), filename, e);
                            report.fail(filename.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                println!("  {} Failed to download {}: {}", "✗".red(), url, e);
                report.fail(url.to_string());
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
    report.print();

    Ok(report.failures())
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
    ui: &dyn InstallUi,
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

    let selections = ui.multi_select(
        "Select packages to install (Space to select, Enter to confirm)",
        &items,
    )?;

    if selections.is_empty() {
        anyhow::bail!("No packages selected");
    }

    Ok(selections)
}

/// One package the plan decided to install or upgrade
struct PlanItem {
    /// The name as typed (may carry `::variant`)
    input: String,
    resolved: ResolvedPackage,
    platform_match: crate::core::platform::PlatformMatch,
    /// The installed key being upgraded, if any
    installed_key: Option<String>,
    version: String,
    status: TargetStatus,
}

/// A bucket script found while resolving inputs: (name, url, type, origin)
type ScriptJob = (String, String, ScriptType, String);

/// Settings shared by every phase of one `install_packages` run
struct Session<'a> {
    opts: &'a InstallOptions,
    ui: &'a dyn InstallUi,
    current_platform: Platform,
    /// `-p/--platform` flag, else the `preferred_platform` setting; `None` = auto-detect
    platform_override: Option<&'a str>,
}

impl Session<'_> {
    /// Platforms of `pkg` usable here, best first
    fn platform_matches(
        &self,
        pkg: &crate::core::Package,
    ) -> Vec<crate::core::platform::PlatformMatch> {
        match self.platform_override {
            Some(o) => Platform::match_override(o, &pkg.platforms),
            None => self.current_platform.find_best_match(&pkg.platforms),
        }
    }

    /// The platform name reported when nothing matches
    fn target_platform(&self) -> String {
        self.platform_override
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.current_platform.to_string())
    }
}

/// Install packages from cache or GitHub.
///
/// Runs in phases: resolve inputs, plan (fetch target releases, classify new vs upgrade),
/// confirm, install each planned package, then install bucket scripts.
fn install_packages(
    config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    names: Vec<&String>,
    opts: &InstallOptions,
    ui: &dyn InstallUi,
    cache: Option<ManifestCache>,
) -> Result<usize> {
    let session = Session {
        opts,
        ui,
        current_platform: Platform::current(),
        platform_override: opts
            .platform
            .as_deref()
            .or_else(|| config.preferences().preferred_platform.as_deref()),
    };

    // Load cache once for both script lookup and package resolution
    let mut cache = match cache {
        Some(cache) => cache,
        None => config.get_or_rebuild_cache()?,
    };

    let (packages, scripts, mut failures) = resolve_inputs(&session, config, &cache, &names)?;
    if packages.is_empty() && scripts.is_empty() {
        println!("{}", "No packages or scripts to install".yellow());
        return Ok(failures);
    }

    let (to_install, to_update, plan_failures) = plan_packages(&session, installed, packages)?;
    failures += plan_failures;
    print_script_plan(installed, &scripts);

    if to_install.is_empty() && to_update.is_empty() && scripts.is_empty() {
        println!();
        println!(
            "{}",
            "All packages and scripts are already up to date".green()
        );
        return Ok(failures);
    }

    if !opts.yes && !ui.confirm("\nProceed with installation?", true)? {
        println!("Installation cancelled");
        return Ok(failures);
    }

    println!();

    let installer = PackageInstaller {
        paths,
        ui,
        command_name: opts.script_name.as_deref(),
        yes: opts.yes,
        no_suffix: opts.no_suffix,
        update_mode: opts.update_mode,
        skip_checksum: opts.skip_checksum,
    };
    let mut report = BatchReport::new("package");
    // Packages fetched from the GitHub API, to refresh in the cache
    let mut packages_to_cache: Vec<(crate::core::Package, PackageSource)> = Vec::new();
    for item in to_install.into_iter().chain(to_update) {
        packages_to_cache.extend(install_plan_item(
            &session,
            &installer,
            paths,
            installed,
            item,
            &mut report,
        ));
    }

    if !packages_to_cache.is_empty() {
        match update_cache_with_packages(config, &mut cache, packages_to_cache) {
            Ok(count) => {
                log::info!("Updated cache with {} latest package(s)", count);
            }
            Err(e) => {
                // Don't fail the entire operation if cache update fails
                log::warn!("Failed to update cache: {}", e);
            }
        }
    }

    let script_report = install_bucket_scripts(
        config,
        paths,
        installed,
        scripts,
        opts.script_name.as_deref(),
    );

    println!("{}", "Summary:".bold());
    report.print();
    script_report.print();

    Ok(failures + report.failures() + script_report.failures())
}

/// Phase 1: resolve every input to packages (with a platform match) or bucket scripts.
///
/// Returns the packages, the scripts, and how many inputs failed to resolve.
#[allow(clippy::type_complexity)]
fn resolve_inputs(
    s: &Session,
    config: &Config,
    cache: &ManifestCache,
    names: &[&String],
) -> Result<(Vec<(String, ResolvedPackage)>, Vec<ScriptJob>, usize)> {
    let resolver = PackageResolver::new(config, cache)?;
    let mut packages = Vec::new();
    let mut scripts: Vec<ScriptJob> = Vec::new();
    let mut failures = 0;

    for original_name in names {
        let input = PackageInput::parse(original_name);

        match resolver.resolve(&input) {
            Ok(resolved) => {
                for pkg_resolved in resolved {
                    let matches = s.platform_matches(&pkg_resolved.package);

                    if matches.is_empty() {
                        println!(
                            "{} {} does not support platform {}",
                            "Warning:".yellow(),
                            pkg_resolved.package.name,
                            s.target_platform()
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
                        failures += 1;
                        continue;
                    }

                    let best_match = &matches[0];

                    // Check if fallback requires confirmation
                    if let Some(fallback_type) = &best_match.fallback_type {
                        if fallback_type.requires_confirmation() && !s.opts.yes {
                            println!(
                                "{} {} - no exact match for {}, but {} is available",
                                "⚠".yellow(),
                                pkg_resolved.package.name,
                                s.current_platform,
                                best_match.platform_id
                            );
                            println!("  This is a fallback: {}", fallback_type.description());

                            if !s.ui.confirm("  Install anyway?", false)? {
                                println!("  Skipped");
                                continue;
                            }
                        } else if !s.opts.yes {
                            // Fallback doesn't require confirmation, but inform user
                            println!(
                                "{} Using fallback: {} ({})",
                                "ℹ".cyan(),
                                best_match.platform_id,
                                fallback_type.description()
                            );
                        }
                    }

                    packages.push((original_name.to_string(), pkg_resolved));
                }
            }
            Err(e) if matches!(input, PackageInput::DirectUrl(_)) => {
                // A URL never falls back to cache lookups: report the real cause
                eprintln!("{} {}: {:#}", "Error".red().bold(), original_name, e);
                failures += 1;
            }
            Err(_) => match resolve_script(cache, original_name) {
                Some(job) => scripts.push(job),
                None => failures += 1,
            },
        }
    }

    Ok((packages, scripts, failures))
}

/// Look `name` up as a bucket script, printing why when it cannot be installed here
fn resolve_script(cache: &ManifestCache, name: &str) -> Option<ScriptJob> {
    let Some(cached_script) = cache.find_script(name) else {
        let base = name.split("::").next().unwrap_or(name);
        let suggestions = crate::core::fuzzy::suggest(
            base,
            cache
                .packages
                .values()
                .map(|c| c.package.name.as_str())
                .chain(cache.scripts.values().map(|c| c.script.name.as_str())),
            3,
        );
        if suggestions.is_empty() || crate::core::fuzzy::is_glob(base) {
            eprintln!("{} {}: Not found", "Error".red().bold(), name);
        } else {
            eprintln!(
                "{} {}: Not found. Did you mean: {}?",
                "Error".red().bold(),
                name,
                suggestions.join(", ")
            );
        }
        return None;
    };
    let script = &cached_script.script;

    // Installable only when this platform has an interpreter for one of its types
    let Some((script_type, platform_info)) = crate::installer::script::installable_script(script)
    else {
        println!(
            "{} {} is not supported on current platform (available: {})",
            "Warning:".yellow(),
            script.name,
            script.platforms_display()
        );
        return None;
    };
    let source_name = match &cached_script.source {
        PackageSource::Bucket { name } => format!("bucket:{}", name),
        _ => "unknown".to_string(),
    };
    Some((
        script.name.clone(),
        platform_info.url.clone(),
        script_type,
        source_name,
    ))
}

/// Candidate package data prepared for plan classification
struct PlanCandidate {
    original_name: String,
    resolved: ResolvedPackage,
    platform_match: crate::core::platform::PlatformMatch,
    version: String,
    status: TargetStatus,
}

/// Fetch target release for a resolved package and verify platform compatibility
fn resolve_target_and_platform(
    s: &Session,
    github: &GitHubProvider,
    original_name: String,
    mut resolved: ResolvedPackage,
) -> Option<PlanCandidate> {
    let repo = &resolved.package.repo;

    // Fetch the target release: always ask GitHub first (the cached bucket version
    // may be stale), except where the cache was just refreshed.
    let target = target_package(
        &resolved.package,
        &resolved.source,
        s.opts.version.as_deref(),
        s.opts.update_mode,
        |v| github.fetch_package(repo, v, Some((&resolved.package).into())),
    );
    let version = target.version;
    let status = target.status;
    resolved.package = target.package;

    // Recompute platform match for the new target package platforms
    let matches = s.platform_matches(&resolved.package);
    if matches.is_empty() {
        println!(
            "{} {} v{} does not support platform {}",
            "Warning:".yellow(),
            resolved.package.name,
            version,
            s.target_platform()
        );
        return None;
    }
    let platform_match = matches[0].clone();

    Some(PlanCandidate {
        original_name,
        resolved,
        platform_match,
        version,
        status,
    })
}

/// Classify a resolved package candidate into new install, reinstall, upgrade, or skipped
fn classify_package_plan(
    s: &Session,
    installed: &crate::core::InstalledSet,
    candidate: PlanCandidate,
    to_install: &mut Vec<PlanItem>,
    to_update: &mut Vec<PlanItem>,
) -> Result<()> {
    let PlanCandidate {
        original_name,
        resolved,
        platform_match,
        version,
        status,
    } = candidate;

    let pkg_name = resolved.package.name.clone();
    let check_name = if original_name.contains("::") {
        original_name.clone()
    } else if let Some(filter) = s.opts.variant_filter.as_deref() {
        crate::core::manifest::generate_installed_key(&pkg_name, Some(filter))
    } else {
        pkg_name.clone()
    };

    let print_urls = || {
        if let Some(binaries) = resolved.package.platforms.get(&platform_match.platform_id) {
            for binary in binaries {
                println!("    {} {}", "↳".dimmed(), binary.url.dimmed());
            }
        }
    };

    if let Some(inst_pkg) = installed.get_package(&check_name) {
        if inst_pkg.version == version {
            println!(
                "  {} {} v{} {}",
                "•".cyan(),
                check_name,
                version,
                "(already installed, same version)".dimmed()
            );
            // With --yes or a "no", skip reinstallation
            if !s.opts.yes && s.ui.confirm("  Reinstall?", false)? {
                to_install.push(PlanItem {
                    input: original_name,
                    resolved,
                    platform_match,
                    installed_key: None,
                    version,
                    status,
                });
            }
        } else {
            println!(
                "  {} {} v{} {} → {}",
                "•".yellow(),
                check_name,
                inst_pkg.version.dimmed(),
                "upgrade to".yellow(),
                version.green()
            );
            print_urls();
            to_update.push(PlanItem {
                input: original_name,
                resolved,
                platform_match,
                installed_key: Some(check_name),
                version,
                status,
            });
        }
    } else if s.opts.update_mode {
        // Update mode: don't install new packages
        println!(
            "  {} {} is not installed, skipping (use 'wenget add' to install new packages)",
            "⚠".yellow(),
            pkg_name
        );
    } else {
        println!(
            "  {} {} v{} {}",
            "•".green(),
            pkg_name,
            version,
            "(new)".green()
        );
        print_urls();
        to_install.push(PlanItem {
            input: original_name,
            resolved,
            platform_match,
            installed_key: None,
            version,
            status,
        });
    }

    Ok(())
}

/// Phase 2: fetch each package's target release and classify it as a new install or
/// an upgrade, printing the plan.
///
/// Returns `(to_install, to_update, failures)`.
fn plan_packages(
    s: &Session,
    installed: &crate::core::InstalledSet,
    packages: Vec<(String, ResolvedPackage)>,
) -> Result<(Vec<PlanItem>, Vec<PlanItem>, usize)> {
    let mut to_install: Vec<PlanItem> = Vec::new();
    let mut to_update: Vec<PlanItem> = Vec::new();
    let mut failures = 0;
    if packages.is_empty() {
        return Ok((to_install, to_update, failures));
    }

    let github = GitHubProvider::new()?;
    println!("{}", "Packages to install:".bold());

    for (original_name, resolved) in packages {
        match resolve_target_and_platform(s, &github, original_name, resolved) {
            Some(candidate) => {
                classify_package_plan(s, installed, candidate, &mut to_install, &mut to_update)?;
            }
            None => {
                failures += 1;
            }
        }
    }

    Ok((to_install, to_update, failures))
}

/// Print the bucket scripts about to be installed
fn print_script_plan(installed: &crate::core::InstalledSet, scripts: &[ScriptJob]) {
    if scripts.is_empty() {
        return;
    }
    println!();
    println!("{}", "Scripts to install:".bold());
    for (name, _, script_type, _) in scripts {
        if installed.is_installed(name) {
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
    }
}

/// The variant to install: `name::variant` input, else `--variant`, else (updating)
/// the installed package's variant
fn effective_variant(
    s: &Session,
    installed: &crate::core::InstalledSet,
    item: &PlanItem,
) -> Option<String> {
    let from_input = item.input.split("::").nth(1);
    if let Some(v) = from_input.or(s.opts.variant_filter.as_deref()) {
        return Some(v.to_string());
    }
    if !s.opts.update_mode {
        return None;
    }
    // A `None` variant is left to asset-template matching in the filter step
    let key = item.installed_key.as_ref()?;
    let variant = installed.get_package(key)?.variant.clone()?;
    log::debug!(
        "Update mode: auto-selecting variant '{}' for {}",
        variant,
        key
    );
    Some(variant)
}

/// Selected binaries and fallback status for installing a plan item
struct PreparedBinaries<'a> {
    using_fallback: bool,
    binaries: &'a [crate::core::manifest::PlatformBinary],
    filtered: Vec<crate::core::manifest::PlatformBinary>,
    selected_indices: Vec<usize>,
}

/// Validate release status, locate platform binaries, and prompt for user selection
fn prepare_plan_binaries<'a>(
    s: &Session,
    installed: &crate::core::InstalledSet,
    item: &'a PlanItem,
    effective_variant_filter: Option<&str>,
    report: &mut BatchReport,
) -> Option<PreparedBinaries<'a>> {
    let yes = s.opts.yes;
    let update_mode = s.opts.update_mode;
    let pkg_name = &item.resolved.package.name;

    // The release was chosen during planning; report how it was obtained
    let pkg_to_install = &item.resolved.package;
    let using_fallback = match &item.status {
        TargetStatus::Failed(e) => {
            println!("  {} {}", "✗".red(), e);
            report.fail(pkg_name.to_string());
            return None;
        }
        TargetStatus::Cached => {
            println!(
                "  {} Using cached download links (GitHub API unavailable)",
                "⚠".yellow()
            );
            true
        }
        TargetStatus::Fresh => false,
    };

    let Some(binaries) = pkg_to_install
        .platforms
        .get(&item.platform_match.platform_id)
    else {
        println!("  {} Platform binary not found", "✗".red());
        report.fail(pkg_name.to_string());
        return None;
    };

    // In update mode, match the previously installed asset first
    let stored_asset = if update_mode {
        item.installed_key
            .as_ref()
            .and_then(|k| installed.get_package(k))
            .map(|p| p.asset_name.clone())
    } else {
        None
    };
    let filtered_binaries = filter_binaries(
        binaries,
        pkg_name,
        stored_asset.as_deref(),
        effective_variant_filter,
    );

    if filtered_binaries.is_empty() {
        if let Some(filter) = effective_variant_filter {
            print_missing_variant(binaries, pkg_name, filter, update_mode, yes);
        }
        report.fail(pkg_name.to_string());
        return None;
    }

    // Select which packages to install (single, all, or user selection)
    let selected_indices =
        match select_packages_for_platform(pkg_name, &filtered_binaries, yes, update_mode, s.ui) {
            Ok(indices) => indices,
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                report.fail(pkg_name.to_string());
                return None;
            }
        };

    Some(PreparedBinaries {
        using_fallback,
        binaries,
        filtered: filtered_binaries,
        selected_indices,
    })
}

/// Phase 4: install the selected binaries of one planned package.
///
/// Returns the package to refresh in the cache when it came from the GitHub API.
fn install_plan_item(
    s: &Session,
    installer: &PackageInstaller,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    item: PlanItem,
    report: &mut BatchReport,
) -> Option<(crate::core::Package, PackageSource)> {
    let variant = effective_variant(s, installed, &item);
    let effective_variant_filter = variant.as_deref();

    let prep = prepare_plan_binaries(s, installed, &item, effective_variant_filter, report)?;

    let PlanItem {
        resolved,
        platform_match,
        version,
        ..
    } = &item;
    let pkg_name = &resolved.package.name;
    let pkg_to_install = &resolved.package;

    let mut to_cache = None;
    for (i, &idx) in prep.selected_indices.iter().enumerate() {
        let binary = &prep.filtered[idx];

        // A platform with a single binary and no filters installs as the default (no variant)
        let variant = if prep.binaries.len() == 1
            && effective_variant_filter.is_none()
            && s.opts.platform.is_none()
        {
            None
        } else {
            crate::core::manifest::extract_variant_from_asset(&binary.asset_name, pkg_name)
        };
        let installed_key =
            crate::core::manifest::generate_installed_key(pkg_name, variant.as_deref());

        println!("{} {} v{}...", "Installing".cyan(), installed_key, version);
        if prep.using_fallback {
            println!(
                "  {} Falling back to bucket source download links",
                "ℹ".cyan()
            );
        }
        if prep.selected_indices.len() > 1 {
            println!("  {} From: {}", "ℹ".cyan(), binary.asset_name.dimmed());
        }

        let request = InstallRequest {
            package: pkg_to_install,
            platform_match,
            binary,
            version,
            source: &resolved.source,
            installed_key: &installed_key,
        };
        match installer.install(installed, &request) {
            Ok(inst_pkg) => {
                if let Err(e) = record_installed(paths, installed, installed_key.clone(), inst_pkg)
                {
                    println!("  {} {:#}", "✗".red(), e);
                    report.fail(installed_key.clone());
                    println!();
                    continue;
                }

                // Cache the GitHub API result once, not per binary
                if i == 0 && !prep.using_fallback {
                    to_cache = Some((pkg_to_install.clone(), resolved.source.clone()));
                }

                println!("  {} Installed successfully", "✓".green());
                report.ok(installed_key.clone());
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                report.fail(installed_key.clone());
            }
        }
        println!();
    }
    to_cache
}

/// Explain that no binary of `pkg_name` matches `filter`
fn print_missing_variant(
    binaries: &[crate::core::manifest::PlatformBinary],
    pkg_name: &str,
    filter: &str,
    update_mode: bool,
    yes: bool,
) {
    if !update_mode {
        println!(
            "  {} No binaries found for variant '{}'. Available variants:",
            "✗".red(),
            filter
        );
        print_available_variants(binaries, pkg_name);
    } else if yes {
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
}

/// Phase 5: install the planned bucket scripts
fn install_bucket_scripts(
    config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledSet,
    scripts: Vec<ScriptJob>,
    custom_name: Option<&str>,
) -> BatchReport {
    let mut report = BatchReport::new("script");
    for (name, url, script_type, origin) in scripts {
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
                report.ok(name);
            }
            Err(e) => {
                println!("  {} {}", "✗".red(), e);
                report.fail(name);
            }
        }
        println!();
    }
    report
}

/// Update manifest cache with latest package info from GitHub API
fn update_cache_with_packages(
    config: &Config,
    cache: &mut ManifestCache,
    packages: Vec<(crate::core::Package, PackageSource)>,
) -> Result<usize> {
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
    config.save_cache(cache)?;

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
) -> Result<()> {
    let saved = crate::core::InstalledStore::new(paths.clone())
        .save_package(&key, &pkg)
        .with_context(|| format!("Failed to save the package record for {}", key));
    // The files are on disk either way, so the snapshot keeps the name taken.
    installed.upsert_package(key, pkg);
    saved
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

    // Keep a name set by `wenget rename` when the script is reinstalled/updated
    let launcher_name = match custom_name {
        Some(_) => None,
        None => installed
            .get_package(name)
            .filter(|p| matches!(p.source, PackageSource::Script { .. }))
            .and_then(|p| p.executables.values().next().cloned())
            .filter(|cmd| cmd != name),
    };
    let launcher_name = launcher_name.as_deref().unwrap_or(command_name);

    println!("  Installing script as '{}'...", launcher_name);

    // Install script to app directory
    let files = install_script(paths, command_name, &content, &script_type)?;
    let script_file = files
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{}.{}", command_name, script_type.extension()));

    println!("  Command will be available as: {}", launcher_name);

    // Create shim
    println!("  Creating launcher...");
    crate::installer::script::create_script_launcher(
        paths,
        launcher_name,
        &paths.app_dir(command_name).join(&script_file),
        &script_type,
    )?;

    // Create executables map
    let mut executables = HashMap::new();
    executables.insert(script_file, launcher_name.to_string());

    // Create installed package info
    let inst_pkg = InstalledPackage {
        schema_version: crate::core::manifest::CURRENT_SCHEMA_VERSION,
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
        download_url: Some(url.to_string()),
    };
    record_installed(paths, installed, name.to_string(), inst_pkg)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_installed_reports_save_failure() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        // A directory where the record file belongs makes the save fail.
        fs::create_dir_all(paths.record_dir("hello").join("package.json")).unwrap();
        let mut installed = crate::core::InstalledSet::default();
        let pkg = InstalledPackage {
            schema_version: crate::core::manifest::CURRENT_SCHEMA_VERSION,
            repo_name: "hello".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: String::new(),
            executables: HashMap::new(),
            source: crate::core::manifest::PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: String::new(),
            download_url: None,
        };

        let result = record_installed(&paths, &mut installed, "hello".to_string(), pkg);

        assert!(result.is_err());
        // The files are on disk, so the name stays taken for this run.
        assert!(installed.get_package("hello").is_some());
    }
}
