//! Bucket command implementation

use crate::bucket::Bucket;
use crate::cli::UpdateMode;
use crate::core::manifest::{Package, ScriptItem, ScriptPlatform, ScriptType};
use crate::core::Config;
use crate::providers::{GitHubProvider, GitHubRepo};
use crate::utils::HttpClient;
use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

/// Bucket subcommands
pub enum BucketCommand {
    Add {
        name: String,
        url: String,
    },
    Del {
        names: Vec<String>,
    },
    List,
    Refresh,
    Create {
        repos_src: Vec<String>,
        scripts_src: Vec<String>,
        direct: Vec<String>,
        output: Option<String>,
        token: Option<String>,
        update_mode: Option<UpdateMode>,
    },
}

/// Run bucket command
pub fn run(cmd: BucketCommand) -> Result<()> {
    match cmd {
        BucketCommand::Add { name, url } => run_add(name, url),
        BucketCommand::Del { names } => run_del(names),
        BucketCommand::List => run_list(),
        BucketCommand::Refresh => run_refresh(),
        BucketCommand::Create {
            repos_src,
            scripts_src,
            direct,
            output,
            token,
            update_mode,
        } => run_create(repos_src, scripts_src, direct, output, token, update_mode),
    }
}

/// Add a bucket
fn run_add(name: String, url: String) -> Result<()> {
    let config = Config::new()?;

    // Ensure wenget is initialized
    if !config.is_initialized() {
        config.init()?;
    }

    println!("{} bucket '{}'...\n", "Adding".cyan(), name);

    // Load bucket config
    let mut bucket_config = config.get_or_create_buckets()?;

    // Create bucket
    let bucket = Bucket {
        name: name.clone(),
        url: url.clone(),
        enabled: true,
        priority: 100,
    };

    // Try to add bucket
    if bucket_config.add_bucket(bucket) {
        // Save config
        config.save_buckets(&bucket_config)?;

        println!("{} Bucket '{}' added", "✓".green(), name);
        println!("  URL: {}", url);

        // Invalidate cache so it will be rebuilt on next access
        config.invalidate_cache()?;

        println!();
        println!("{}", "Cache will be rebuilt on next command.".cyan());
    } else {
        println!("{} Bucket '{}' already exists", "✗".red(), name);
        return Ok(());
    }

    Ok(())
}

/// Delete buckets
fn run_del(names: Vec<String>) -> Result<()> {
    let config = Config::new()?;

    // Load bucket config
    let mut bucket_config = config.get_or_create_buckets()?;

    if bucket_config.buckets.is_empty() {
        println!("{}", "No buckets configured".yellow());
        return Ok(());
    }

    if names.is_empty() {
        println!("{}", "No bucket names provided".yellow());
        println!("Usage: wenget bucket del <name>...");
        return Ok(());
    }

    println!("{} bucket(s)...\n", "Deleting".cyan());

    let mut deleted = 0;
    let mut not_found = 0;

    for name in names {
        print!("  {} {} ... ", "Deleting".cyan(), name);

        if bucket_config.remove_bucket(&name) {
            println!("{}", "Deleted".green());
            deleted += 1;
        } else {
            println!("{}", "Not found".yellow());
            not_found += 1;
        }
    }

    // Save config
    config.save_buckets(&bucket_config)?;

    // Invalidate cache
    if deleted > 0 {
        config.invalidate_cache()?;
    }

    // Summary
    println!();
    println!("{}", "Summary:".bold());
    if deleted > 0 {
        println!("  {} {} bucket(s) deleted", "✓".green(), deleted);
    }
    if not_found > 0 {
        println!("  {} {} bucket(s) not found", "•".yellow(), not_found);
    }

    println!();
    println!("Total buckets: {}", bucket_config.buckets.len());

    Ok(())
}

/// List buckets
fn run_list() -> Result<()> {
    let config = Config::new()?;

    // Load bucket config
    let bucket_config = config.get_or_create_buckets()?;

    if bucket_config.buckets.is_empty() {
        println!("{}", "No buckets configured".yellow());
        println!();
        println!("Add a bucket with: wenget bucket add <name> <url>");
        return Ok(());
    }

    // Print header
    println!("{}", "Configured buckets:".bold());
    println!();
    println!(
        "{:<20} {:<10} {}",
        "NAME".bold(),
        "STATUS".bold(),
        "URL".bold()
    );
    println!("{}", "─".repeat(80));

    // Print buckets
    for bucket in &bucket_config.buckets {
        let status = if bucket.enabled {
            "enabled".green()
        } else {
            "disabled".yellow()
        };

        println!(
            "{:<20} {:<18} {}",
            bucket.name.green(),
            status.to_string(),
            bucket.url
        );
    }

    println!();
    println!("Total: {} bucket(s)", bucket_config.buckets.len());

    Ok(())
}

/// Refresh cache from buckets
fn run_refresh() -> Result<()> {
    let config = Config::new()?;

    println!("{} manifest cache...\n", "Refreshing".cyan());

    // Force rebuild cache
    let cache = config.rebuild_cache()?;

    println!();
    println!("{}", "Summary:".bold());

    // Show source statistics
    for (source_name, info) in &cache.sources {
        println!(
            "  {} {} - {} package(s)",
            "✓".green(),
            source_name,
            info.package_count
        );
    }

    println!();
    println!("Total packages in cache: {}", cache.packages.len());
    println!("{}", "Cache refreshed successfully!".green());

    Ok(())
}

/// Bucket manifest structure for output
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BucketManifest {
    packages: Vec<Package>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scripts: Vec<ScriptItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_updated: Option<String>,
}

// ============================================================================
// Manifest Generator - Non-interactive batch processing
// ============================================================================

/// Rate limit delay between GitHub API requests (milliseconds)
const RATE_LIMIT_DELAY_MS: u64 = 1000;

// Gist API response structures (GitHub-specific, not shared with providers)

#[derive(Debug, Deserialize)]
struct GistFile {
    filename: String,
    raw_url: String,
}

#[derive(Debug, Deserialize)]
struct GistResponse {
    html_url: String,
    description: Option<String>,
    files: HashMap<String, GistFile>,
}

/// Manifest generator that processes source files and URLs
struct ManifestGenerator {
    http: HttpClient,
    github: GitHubProvider,
    packages: HashMap<String, Package>,
    scripts: HashMap<String, ScriptItem>,
}

impl ManifestGenerator {
    #[allow(dead_code)]
    fn new() -> Result<Self> {
        Self::with_token(None)
    }

    fn with_token(token: Option<String>) -> Result<Self> {
        Ok(Self {
            http: HttpClient::with_token(token.clone())?,
            github: GitHubProvider::with_token(token)?,
            packages: HashMap::new(),
            scripts: HashMap::new(),
        })
    }

    /// Load URLs from a source file (one URL per line, # for comments)
    fn load_sources(&self, file_path: &str) -> Result<Vec<String>> {
        let content = fs::read_to_string(file_path)
            .with_context(|| format!("Failed to read source file: {}", file_path))?;

        let urls: Vec<String> = content
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.to_string())
            .collect();

        Ok(urls)
    }

    /// Parse Gist URL to extract gist ID
    fn parse_gist_url(&self, url: &str) -> Option<String> {
        // https://gist.github.com/username/gist_id
        let url = url.trim_end_matches('/');
        if url.contains("gist.github.com") || url.contains("gist.githubusercontent.com") {
            let parts: Vec<&str> = url.split('/').collect();
            // Find the gist ID (last part that looks like a hex string)
            for part in parts.iter().rev() {
                if part.len() >= 20 && part.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Some(part.to_string());
                }
            }
        }
        None
    }

    /// Check if URL is a raw script URL
    fn is_raw_script_url(&self, url: &str) -> bool {
        url.contains("raw.githubusercontent.com") || url.starts_with("https://raw.")
    }

    /// Check if URL is a GitHub repo URL
    fn is_github_repo_url(&self, url: &str) -> bool {
        url.contains("github.com/") && !url.contains("gist.") && !url.contains("raw.")
    }

    /// Check if URL is a Gist URL
    fn is_gist_url(&self, url: &str) -> bool {
        url.contains("gist.github.com") || url.contains("gist.githubusercontent.com")
    }

    /// Detect script type from filename extension
    fn detect_script_type(&self, filename: &str) -> Option<ScriptType> {
        let lower = filename.to_lowercase();
        if lower.ends_with(".ps1") {
            Some(ScriptType::PowerShell)
        } else if lower.ends_with(".sh") {
            Some(ScriptType::Bash)
        } else if lower.ends_with(".bat") || lower.ends_with(".cmd") {
            Some(ScriptType::Batch)
        } else if lower.ends_with(".py") {
            Some(ScriptType::Python)
        } else {
            None
        }
    }

    /// Detect script type from shebang line
    fn detect_script_type_from_content(&self, content: &str) -> Option<ScriptType> {
        let first_line = content.lines().next().unwrap_or("");
        if first_line.starts_with("#!") {
            let lower = first_line.to_lowercase();
            if lower.contains("bash") || lower.contains("/sh") {
                return Some(ScriptType::Bash);
            } else if lower.contains("python") {
                return Some(ScriptType::Python);
            } else if lower.contains("pwsh") || lower.contains("powershell") {
                return Some(ScriptType::PowerShell);
            }
        }
        None
    }

    /// Extract script name from filename (remove extension)
    fn extract_script_name(&self, filename: &str) -> String {
        let name = filename;
        for ext in &[".ps1", ".sh", ".bat", ".cmd", ".py"] {
            if let Some(stripped) = name.strip_suffix(ext) {
                return stripped.to_string();
            }
        }
        name.to_string()
    }

    /// Fetch package info from GitHub repo URL using shared GitHubProvider logic
    fn fetch_package(&mut self, url: &str) -> Result<()> {
        let (owner, repo) = GitHubProvider::parse_github_url(url)
            .ok_or_else(|| anyhow::anyhow!("Invalid GitHub URL: {}", url))?;

        print!("  {} {}/{}...", "Fetching".cyan(), owner, repo);

        // Fetch repo info using GitHubProvider
        let repo_info: GitHubRepo = match self.github.fetch_repo_info(&owner, &repo) {
            Ok(info) => info,
            Err(e) => {
                println!(" {} failed to fetch repo info", "✗".red());
                log::warn!("Failed to fetch repo info for {}/{}: {}", owner, repo, e);
                return Ok(());
            }
        };

        // Fetch latest release using GitHubProvider
        let release = match self.github.fetch_latest_release(&owner, &repo) {
            Ok(r) => r,
            Err(e) => {
                println!(" {} no releases", "⚠".yellow());
                log::warn!("No releases for {}/{}: {}", owner, repo, e);
                return Ok(());
            }
        };

        // Use shared platform extraction logic from GitHubProvider
        let platforms = GitHubProvider::extract_platform_binaries(&release.assets);

        if platforms.is_empty() {
            println!(" {} no binaries", "⚠".yellow());
            return Ok(());
        }

        let package = Package {
            name: repo_info.name.clone(),
            description: repo_info.description.clone().unwrap_or_default(),
            repo: repo_info.html_url.clone(),
            homepage: repo_info.homepage.clone().filter(|h| !h.is_empty()),
            license: repo_info.license.as_ref().and_then(|l| l.spdx_id.clone()),
            version: Some(release.tag_name.trim_start_matches('v').to_string()),
            platforms,
        };

        println!(" {} {} platform(s)", "✓".green(), package.platforms.len());

        // Merge with existing package if same name
        self.merge_package(package);

        Ok(())
    }

    /// Fetch scripts from Gist URL
    fn fetch_gist(&mut self, url: &str) -> Result<()> {
        let gist_id = self
            .parse_gist_url(url)
            .ok_or_else(|| anyhow::anyhow!("Invalid Gist URL: {}", url))?;

        print!("  {} gist/{}...", "Fetching".cyan(), &gist_id[..8]);

        // Create HTTP client without token for public gists
        // GitHub Actions GITHUB_TOKEN doesn't have gist read permission
        let gist_http = HttpClient::new()?;
        let gist_url = format!("https://api.github.com/gists/{}", gist_id);
        let gist: GistResponse = gist_http
            .get_json(&gist_url)
            .with_context(|| format!("Failed to fetch gist {}", gist_id))?;

        let mut script_count = 0;

        for file_info in gist.files.values() {
            if let Some(script_type) = self.detect_script_type(&file_info.filename) {
                let name = self.extract_script_name(&file_info.filename);

                let script = ScriptItem {
                    name: name.clone(),
                    description: gist.description.clone().unwrap_or_default(),
                    repo: gist.html_url.clone(),
                    platforms: {
                        let mut p = HashMap::new();
                        p.insert(
                            script_type,
                            ScriptPlatform {
                                url: file_info.raw_url.clone(),
                                checksum: None,
                            },
                        );
                        p
                    },
                    homepage: None,
                    license: None,
                };

                self.merge_script(script);
                script_count += 1;
            }
        }

        if script_count > 0 {
            println!(" {} {} script(s)", "✓".green(), script_count);
        } else {
            println!(" {} no scripts", "⚠".yellow());
        }

        Ok(())
    }

    /// Fetch script from raw URL
    fn fetch_raw_script(&mut self, url: &str) -> Result<()> {
        let filename = url.rsplit('/').next().unwrap_or("script");
        print!("  {} {}...", "Fetching".cyan(), filename);

        // Detect script type from filename first
        let script_type = if let Some(st) = self.detect_script_type(filename) {
            st
        } else {
            // Try to fetch content and detect from shebang
            match self.http.get_text(url) {
                Ok(content) => {
                    if let Some(st) = self.detect_script_type_from_content(&content) {
                        st
                    } else {
                        println!(" {} unknown type", "⚠".yellow());
                        return Ok(());
                    }
                }
                Err(e) => {
                    println!(" {} {}", "✗".red(), e);
                    return Ok(());
                }
            }
        };

        let name = self.extract_script_name(filename);

        // Try to extract repo URL from raw URL
        let repo = if let Some(caps) = url
            .strip_prefix("https://raw.githubusercontent.com/")
            .and_then(|s| s.split('/').take(2).collect::<Vec<_>>().into_iter().next())
        {
            format!("https://github.com/{}", caps)
        } else {
            url.to_string()
        };

        let script = ScriptItem {
            name: name.clone(),
            description: format!("{} script", script_type.display_name()),
            repo,
            platforms: {
                let mut p = HashMap::new();
                p.insert(
                    script_type.clone(),
                    ScriptPlatform {
                        url: url.to_string(),
                        checksum: None,
                    },
                );
                p
            },
            homepage: None,
            license: None,
        };

        println!(" {} {}", "✓".green(), script_type.display_name());
        self.merge_script(script);

        Ok(())
    }

    /// Process a local file (binary or script)
    fn process_local_file(&mut self, path: &str) -> Result<()> {
        let file_path = Path::new(path);
        if !file_path.exists() {
            println!("  {} {} (file not found)", "⚠".yellow(), path);
            return Ok(());
        }

        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        print!("  {} {}...", "Processing".cyan(), filename);

        // Check if it's a script
        if let Some(script_type) = self.detect_script_type(filename) {
            let name = self.extract_script_name(filename);
            let abs_path = file_path
                .canonicalize()
                .unwrap_or_else(|_| file_path.to_path_buf());

            let script = ScriptItem {
                name: name.clone(),
                description: format!("Local {} script", script_type.display_name()),
                repo: format!("file://{}", abs_path.display()),
                platforms: {
                    let mut p = HashMap::new();
                    p.insert(
                        script_type.clone(),
                        ScriptPlatform {
                            url: format!("file://{}", abs_path.display()),
                            checksum: None,
                        },
                    );
                    p
                },
                homepage: None,
                license: None,
            };

            println!(" {} {} script", "✓".green(), script_type.display_name());
            self.merge_script(script);
        } else {
            // Treat as binary - need platform info
            println!(
                " {} local binaries require platform info (use -r with source file)",
                "⚠".yellow()
            );
        }

        Ok(())
    }

    /// Process a URL or path from --direct
    fn process_direct(&mut self, input: &str) -> Result<()> {
        if self.is_gist_url(input) {
            self.fetch_gist(input)?;
        } else if self.is_raw_script_url(input) {
            self.fetch_raw_script(input)?;
        } else if self.is_github_repo_url(input) {
            self.fetch_package(input)?;
        } else if Path::new(input).exists() {
            self.process_local_file(input)?;
        } else {
            println!("  {} {} (unknown format)", "⚠".yellow(), input);
        }
        Ok(())
    }

    /// Merge a package into the collection (combine platforms for same name)
    fn merge_package(&mut self, package: Package) {
        if let Some(existing) = self.packages.get_mut(&package.name) {
            // Merge platforms
            for (platform, binary) in package.platforms {
                existing.platforms.insert(platform, binary);
            }
            // Update description if empty
            if existing.description.is_empty() && !package.description.is_empty() {
                existing.description = package.description;
            }
        } else {
            self.packages.insert(package.name.clone(), package);
        }
    }

    /// Merge a script into the collection (combine platforms for same name)
    fn merge_script(&mut self, script: ScriptItem) {
        if let Some(existing) = self.scripts.get_mut(&script.name) {
            // Merge platforms
            for (script_type, platform) in script.platforms {
                existing.platforms.insert(script_type, platform);
            }
            // Update description if empty
            if existing.description.is_empty() && !script.description.is_empty() {
                existing.description = script.description;
            }
        } else {
            self.scripts.insert(script.name.clone(), script);
        }
    }

    /// Generate the manifest
    fn generate(
        &mut self,
        repos_src: Vec<String>,
        scripts_src: Vec<String>,
        direct: Vec<String>,
    ) -> Result<BucketManifest> {
        // Process repos source files
        if !repos_src.is_empty() {
            println!("\n{}", "Processing repository sources...".bold());
            for src_file in &repos_src {
                println!("  {} {}", "Loading".cyan(), src_file);
                let urls = self.load_sources(src_file)?;
                println!("    Found {} repositories", urls.len());

                for url in urls {
                    if let Err(e) = self.fetch_package(&url) {
                        println!("    {} {}: {}", "✗".red(), url, e);
                    }
                    thread::sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS));
                }
            }
        }

        // Process scripts source files
        if !scripts_src.is_empty() {
            println!("\n{}", "Processing script sources...".bold());
            for src_file in &scripts_src {
                println!("  {} {}", "Loading".cyan(), src_file);
                let urls = self.load_sources(src_file)?;
                println!("    Found {} script URLs", urls.len());

                for url in urls {
                    let result = if self.is_gist_url(&url) {
                        self.fetch_gist(&url)
                    } else if self.is_raw_script_url(&url) {
                        self.fetch_raw_script(&url)
                    } else {
                        println!("    {} {} (unsupported format)", "⚠".yellow(), url);
                        Ok(())
                    };

                    if let Err(e) = result {
                        println!("    {} {}: {}", "✗".red(), url, e);
                    }
                    thread::sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS));
                }
            }
        }

        // Process direct inputs
        if !direct.is_empty() {
            println!("\n{}", "Processing direct inputs...".bold());
            for input in &direct {
                if let Err(e) = self.process_direct(input) {
                    println!("    {} {}: {}", "✗".red(), input, e);
                }
                thread::sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS));
            }
        }

        // Build manifest
        let manifest = BucketManifest {
            packages: self.packages.values().cloned().collect(),
            scripts: self.scripts.values().cloned().collect(),
            last_updated: Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        };

        Ok(manifest)
    }
}

/// Create a bucket manifest from source files or direct URLs
fn run_create(
    repos_src: Vec<String>,
    scripts_src: Vec<String>,
    direct: Vec<String>,
    output_path: Option<String>,
    token: Option<String>,
    update_mode: Option<UpdateMode>,
) -> Result<()> {
    // Validate inputs
    if repos_src.is_empty() && scripts_src.is_empty() && direct.is_empty() {
        println!("{}", "No input sources provided.".yellow());
        println!();
        println!("{}", "Usage:".bold());
        println!("  wenget bucket create -r sources_repos.txt -s sources_scripts.txt");
        println!("  wenget bucket create -d https://github.com/user/repo,https://gist.github.com/user/id");
        println!("  wenget bucket create -r repos.txt -d https://gist.github.com/user/id");
        println!();
        println!("{}", "Options:".bold());
        println!("  -r, --repos-src    Source file(s) with GitHub repo URLs");
        println!("  -s, --scripts-src  Source file(s) with Gist/script URLs");
        println!("  -d, --direct       Direct URLs or local paths (comma-separated)");
        println!("  -o, --output       Output file (default: manifest.json)");
        println!("  -t, --token        GitHub token for higher API rate limit");
        return Ok(());
    }

    // Try to get token from environment variable if not provided
    let auth_token = token.or_else(|| std::env::var("GITHUB_TOKEN").ok());

    if let Some(ref _token) = auth_token {
        println!(
            "{}",
            "ℹ Using GitHub authentication (rate limit: 5000/hour)".cyan()
        );
    } else {
        println!(
            "{}",
            "ℹ Using unauthenticated requests (rate limit: 60/hour)".yellow()
        );
        println!(
            "{}",
            "  Tip: Use --token or set GITHUB_TOKEN env var for higher rate limit".dimmed()
        );
    }

    println!(
        "{}",
        "╔════════════════════════════════════════════════════════════╗"
            .bold()
            .cyan()
    );
    println!(
        "{}",
        "║           wenget Bucket Manifest Generator                 ║"
            .bold()
            .cyan()
    );
    println!(
        "{}",
        "╚════════════════════════════════════════════════════════════╝"
            .bold()
            .cyan()
    );

    let mut generator = ManifestGenerator::with_token(auth_token)?;
    let new_manifest = generator.generate(repos_src, scripts_src, direct)?;

    // Determine output path
    let output_file = output_path.unwrap_or_else(|| "manifest.json".to_string());
    let output_path = Path::new(&output_file);

    // Check if file exists and determine update mode
    let final_manifest = if output_path.exists() {
        let mode = match update_mode {
            Some(m) => m,
            None => {
                // Interactive prompt
                use dialoguer::Select;
                let choice = Select::new()
                    .with_prompt("Output file exists. How should it be updated?")
                    .items(&[
                        "Overwrite (replace entire file)",
                        "Incremental (merge with existing)",
                    ])
                    .default(0)
                    .interact()?;
                if choice == 0 {
                    UpdateMode::Overwrite
                } else {
                    UpdateMode::Incremental
                }
            }
        };

        match mode {
            UpdateMode::Overwrite => {
                println!("{}", "  Mode: Overwrite".yellow());
                new_manifest
            }
            UpdateMode::Incremental => {
                println!("{}", "  Mode: Incremental merge".cyan());
                // Load existing manifest
                let existing_content = fs::read_to_string(output_path)?;
                let existing: BucketManifest = serde_json::from_str(&existing_content)
                    .context("Failed to parse existing manifest")?;

                // Merge manifests
                merge_manifests(existing, new_manifest)
            }
        }
    } else {
        new_manifest
    };

    // Serialize and write
    let json =
        serde_json::to_string_pretty(&final_manifest).context("Failed to serialize manifest")?;
    fs::write(&output_file, &json)
        .with_context(|| format!("Failed to write to {}", output_file))?;

    // Summary
    println!();
    println!("{}", "═".repeat(60).green());
    println!("{}", "Manifest generated successfully!".green().bold());
    println!("{}", "═".repeat(60).green());
    println!();
    println!("  {} {}", "Output file:".bold(), output_file.cyan());
    println!(
        "  {} {} package(s), {} script(s)",
        "Contents:".bold(),
        final_manifest.packages.len(),
        final_manifest.scripts.len()
    );

    // Platform statistics
    if !final_manifest.packages.is_empty() {
        let mut platform_stats: HashMap<String, usize> = HashMap::new();
        for pkg in &final_manifest.packages {
            for platform in pkg.platforms.keys() {
                *platform_stats.entry(platform.clone()).or_insert(0) += 1;
            }
        }
        println!();
        println!("{}", "Platform coverage:".bold());
        let mut sorted: Vec<_> = platform_stats.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());
        for (platform, count) in sorted {
            println!("    {}: {} packages", platform, count);
        }
    }

    // Script type statistics
    if !final_manifest.scripts.is_empty() {
        let mut type_stats: HashMap<String, usize> = HashMap::new();
        for script in &final_manifest.scripts {
            for script_type in script.platforms.keys() {
                *type_stats
                    .entry(script_type.display_name().to_string())
                    .or_insert(0) += 1;
            }
        }
        println!();
        println!("{}", "Script types:".bold());
        for (script_type, count) in &type_stats {
            println!("    {}: {} scripts", script_type, count);
        }
    }

    println!();
    println!("{}", "Next steps:".bold());
    println!("  1. Upload the manifest to a GitHub repository");
    println!("  2. Get the raw URL of the manifest file");
    println!("  3. Add it as a bucket: wenget bucket add <name> <url>");
    println!();

    Ok(())
}

/// Merge two manifests - keeps existing entries not in new manifest, updates/adds new entries
fn merge_manifests(mut existing: BucketManifest, new: BucketManifest) -> BucketManifest {
    use std::collections::HashSet;

    // Create hash sets for efficient lookups
    let new_package_names: HashSet<_> = new.packages.iter().map(|p| &p.name).collect();
    let new_script_names: HashSet<_> = new.scripts.iter().map(|s| &s.name).collect();

    // Keep existing packages not in new manifest
    existing
        .packages
        .retain(|p| !new_package_names.contains(&p.name));
    existing
        .scripts
        .retain(|s| !new_script_names.contains(&s.name));

    // Add all new packages and scripts
    existing.packages.extend(new.packages);
    existing.scripts.extend(new.scripts);

    // Update timestamp
    existing.last_updated = Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());

    existing
}
