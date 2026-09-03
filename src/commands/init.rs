//! Initialize wenget

use crate::bucket::Bucket;
use crate::core::is_elevated;
use crate::core::Config;
use anyhow::{Context, Result};
use colored::Colorize;
use std::env;
use std::path::PathBuf;

/// Default wenget bucket name and URL
const WENGET_BUCKET_NAME: &str = "wenget";
const WENGET_BUCKET_URL: &str =
    "https://raw.githubusercontent.com/superyngo/wenget/refs/heads/main/bucket/manifest.json";

#[cfg(windows)]
use std::path::Path;

#[cfg(not(windows))]
use std::fs::{self, OpenOptions};
#[cfg(not(windows))]
use std::io::Write;

/// Planned changes to show user before confirmation
struct PlannedChanges {
    create_dirs: Vec<PathBuf>,
    create_files: Vec<PathBuf>,
    create_shim: Option<PathBuf>,
    add_to_path: Option<String>,
    add_bucket: bool,
}

impl PlannedChanges {
    fn is_empty(&self) -> bool {
        self.create_dirs.is_empty()
            && self.create_files.is_empty()
            && self.create_shim.is_none()
            && self.add_to_path.is_none()
            && !self.add_bucket
    }

    fn display(&self) {
        println!("{}", "wenget will make the following changes:".bold());
        println!();

        for dir in &self.create_dirs {
            println!("  • Create directory: {}", dir.display().to_string().cyan());
        }

        for file in &self.create_files {
            println!("  • Create file: {}", file.display().to_string().cyan());
        }

        if let Some(shim) = &self.create_shim {
            println!("  • Create shim: {}", shim.display().to_string().cyan());
        }

        if let Some(path) = &self.add_to_path {
            println!("  • Add to PATH: {}", path.cyan());
        }

        if self.add_bucket {
            println!(
                "  • Add bucket: {} ({})",
                WENGET_BUCKET_NAME.cyan(),
                WENGET_BUCKET_URL
            );
        }

        println!();
    }
}

/// Collect planned changes for a fresh initialization
fn collect_fresh_init_changes(config: &Config) -> PlannedChanges {
    let mut changes = PlannedChanges {
        create_dirs: Vec::new(),
        create_files: Vec::new(),
        create_shim: None,
        add_to_path: None,
        add_bucket: true,
    };

    // Directories to create
    let paths = config.paths();
    if !paths.root().exists() {
        changes.create_dirs.push(paths.root().to_path_buf());
    }
    if !paths.apps_dir().exists() {
        changes.create_dirs.push(paths.apps_dir().to_path_buf());
    }
    if !paths.bin_dir().exists() {
        changes.create_dirs.push(paths.bin_dir().to_path_buf());
    }
    if !paths.cache_dir().exists() {
        changes.create_dirs.push(paths.cache_dir().to_path_buf());
    }

    // Files to create (no global installed index: each package owns its record)
    if !paths.buckets_json().exists() {
        changes
            .create_files
            .push(paths.buckets_json().to_path_buf());
    }

    // Shim/symlink
    #[cfg(windows)]
    {
        let shim_path = paths.bin_dir().join("wenget.cmd");
        if !shim_path.exists() {
            changes.create_shim = Some(shim_path);
        }
    }
    #[cfg(unix)]
    {
        let symlink_path = paths.bin_dir().join("wenget");
        if !symlink_path.exists() && !symlink_path.is_symlink() {
            changes.create_shim = Some(symlink_path);
        }
    }

    // PATH modification
    if !is_in_path(paths.bin_dir()).unwrap_or(false) {
        #[cfg(windows)]
        {
            let bin_dir = if paths.is_system_install() {
                paths.internal_bin_dir()
            } else {
                paths.bin_dir().to_path_buf()
            };
            changes.add_to_path = Some(bin_dir.to_string_lossy().to_string());
        }
        #[cfg(not(windows))]
        {
            // For system installs, /usr/local/bin is typically in PATH
            if !paths.is_system_install() {
                changes.add_to_path = Some(paths.bin_dir().to_string_lossy().to_string());
            }
        }
    }

    changes
}

/// Collect planned changes for an already-initialized state
fn collect_existing_init_changes(config: &Config) -> Result<PlannedChanges> {
    let mut changes = PlannedChanges {
        create_dirs: Vec::new(),
        create_files: Vec::new(),
        create_shim: None,
        add_to_path: None,
        add_bucket: false,
    };

    let paths = config.paths();

    // Check shim/symlink
    #[cfg(windows)]
    {
        let shim_path = paths.bin_dir().join("wenget.cmd");
        if !shim_path.exists() {
            changes.create_shim = Some(shim_path);
        }
    }
    #[cfg(unix)]
    {
        let symlink_path = paths.bin_dir().join("wenget");
        if !symlink_path.exists() && !symlink_path.is_symlink() {
            changes.create_shim = Some(symlink_path);
        }
    }

    // Check PATH
    if !is_in_path(paths.bin_dir())? {
        #[cfg(windows)]
        {
            let bin_dir = if paths.is_system_install() {
                paths.internal_bin_dir()
            } else {
                paths.bin_dir().to_path_buf()
            };
            changes.add_to_path = Some(bin_dir.to_string_lossy().to_string());
        }
        #[cfg(not(windows))]
        {
            if !paths.is_system_install() {
                changes.add_to_path = Some(paths.bin_dir().to_string_lossy().to_string());
            }
        }
    }

    // Check bucket
    if !has_wenget_bucket(config)? {
        changes.add_bucket = true;
    }

    Ok(changes)
}

/// Prompt user to confirm changes
fn prompt_confirm_changes(changes: &PlannedChanges) -> Result<bool> {
    changes.display();

    crate::utils::confirm("Proceed?")
}

/// Initialize wenget (create directories and manifests)
pub fn run(yes: bool) -> Result<()> {
    // Show installation mode
    if is_elevated() {
        println!(
            "{}",
            "Initializing wenget (system-level installation)...".cyan()
        );
        #[cfg(unix)]
        {
            println!("  Apps: /opt/wenget/apps");
            println!("  Bin:  /usr/local/bin (symlinks)");
        }
        #[cfg(windows)]
        {
            let config = Config::new()?;
            println!("  Root: {}", config.paths().root().display());
            println!(
                "  Bin:  {} (added to system PATH)",
                config.paths().bin_dir().display()
            );
        }
    } else {
        println!("{}", "Initializing wenget...".cyan());
        let config = Config::new()?;
        println!("  Apps: {}", config.paths().apps_dir().display());
        println!(
            "  Bin:  {} (symlinks/shims)",
            config.paths().bin_dir().display()
        );
    }
    println!();

    let config = Config::new()?;

    if config.is_initialized() {
        println!("{}", "✓ wenget is already initialized".green());
        println!("  Root: {}", config.paths().root().display());
        println!();

        // Collect changes needed for existing installation
        let changes = collect_existing_init_changes(&config)?;

        if changes.is_empty() {
            // Everything is already set up
            println!("{}", "✓ wenget shim is in bin directory".green());
            if is_in_path(config.paths().bin_dir())? {
                println!("{}", "✓ wenget bin directory is in PATH".green());
            }
            if has_wenget_bucket(&config)? {
                println!("{}", "✓ wenget bucket is configured".green());
            }
            return Ok(());
        }

        // Show what needs to be done and confirm
        if !yes {
            if !prompt_confirm_changes(&changes)? {
                println!("{}", "Initialization cancelled.".yellow());
                return Ok(());
            }
        } else {
            changes.display();
        }

        // Apply changes
        if changes.create_shim.is_some() {
            setup_wenget_executable(&config)?;
        }

        if changes.add_to_path.is_some() {
            setup_path(&config)?;
        }

        if changes.add_bucket {
            add_wenget_bucket(&config)?;
        }

        return Ok(());
    }

    // Fresh initialization - collect all changes
    let changes = collect_fresh_init_changes(&config);

    // Show what will be done and confirm
    if !yes {
        if !prompt_confirm_changes(&changes)? {
            println!("{}", "Initialization cancelled.".yellow());
            return Ok(());
        }
    } else {
        changes.display();
    }

    // Perform initialization
    config.init()?;

    println!("{}", "✓ wenget initialized successfully!".green());
    println!();
    println!("Created directories:");
    println!("  Root:      {}", config.paths().root().display());
    println!("  Apps:      {}", config.paths().apps_dir().display());
    println!("  Bin:       {}", config.paths().bin_dir().display());
    println!("  Cache:     {}", config.paths().cache_dir().display());
    println!();
    println!("Created manifests:");
    println!("  Buckets:   {}", config.paths().buckets_json().display());
    println!();

    // Setup wenget executable itself
    setup_wenget_executable(&config)?;

    // Set up PATH
    setup_path(&config)?;

    // Add wenget bucket (already confirmed above)
    if changes.add_bucket {
        add_wenget_bucket(&config)?;
    }

    println!();
    println!("{}", "Next steps:".bold());
    println!("  1. List available:       wenget bucket list");
    println!("  2. Search packages:      wenget search <keyword>");
    println!("  3. Install packages:     wenget add <package-name>");

    Ok(())
}

/// Create wenget shim with absolute path (Windows)
#[cfg(windows)]
fn create_wenget_shim(target: &Path, shim: &Path) -> Result<()> {
    use std::fs;

    log::debug!("Creating wenget shim: {}", shim.display());

    // Use absolute path in shim to avoid relative path issues
    let shim_content = format!("@echo off\r\n\"{}\" %*\r\n", target.display());

    // Create parent directory
    if let Some(parent) = shim.parent() {
        fs::create_dir_all(parent)?;
    }

    // Remove existing shim if it exists
    if shim.exists() {
        fs::remove_file(shim)
            .with_context(|| format!("Failed to remove existing shim: {}", shim.display()))?;
    }

    // Write shim file
    fs::write(shim, shim_content)
        .with_context(|| format!("Failed to create shim: {}", shim.display()))?;

    Ok(())
}

/// Create wenget symlink (Unix)
#[cfg(unix)]
fn create_wenget_symlink(target: &PathBuf, link: &PathBuf) -> Result<()> {
    use std::os::unix::fs::symlink;

    log::debug!(
        "Creating wenget symlink: {} -> {}",
        link.display(),
        target.display()
    );

    // Remove existing symlink if it exists
    if link.exists() || link.is_symlink() {
        std::fs::remove_file(link)
            .with_context(|| format!("Failed to remove existing symlink: {}", link.display()))?;
    }

    // Create parent directory
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }

    symlink(target, link)
        .with_context(|| format!("Failed to create symlink: {}", link.display()))?;

    Ok(())
}

/// Setup wenget executable itself in bin directory
fn setup_wenget_executable(config: &Config) -> Result<()> {
    let current_exe = env::current_exe().context("Failed to get current executable path")?;
    let bin_dir = config.paths().bin_dir();

    #[cfg(windows)]
    {
        let shim_path = bin_dir.join("wenget.cmd");

        match create_wenget_shim(&current_exe, &shim_path) {
            Ok(_) => {
                println!("{}", "✓ Created wenget shim in bin directory".green());
            }
            Err(e) => {
                println!("{} Failed to create wenget shim: {}", "⚠".yellow(), e);
                println!("  You can manually create a shim to wenget.exe later");
            }
        }
    }

    #[cfg(unix)]
    {
        let symlink_path = bin_dir.join("wenget");

        match create_wenget_symlink(&current_exe, &symlink_path) {
            Ok(_) => {
                println!("{}", "✓ Created wenget symlink in bin directory".green());
            }
            Err(e) => {
                println!("{} Failed to create wenget symlink: {}", "⚠".yellow(), e);
                println!("  You can manually link wenget to the bin directory later");
            }
        }
    }

    println!();
    Ok(())
}

/// Set up PATH for wenget bin directory
fn setup_path(config: &Config) -> Result<()> {
    let bin_dir = config.paths().bin_dir();

    println!("{}", "Setting up PATH...".cyan());

    #[cfg(windows)]
    {
        // For system installs, use the internal bin dir (not /usr/local/bin equivalent)
        let actual_bin_dir = if config.paths().is_system_install() {
            config.paths().internal_bin_dir()
        } else {
            bin_dir.clone()
        };
        setup_path_windows(
            &actual_bin_dir.to_string_lossy(),
            config.paths().is_system_install(),
        )?;
    }

    #[cfg(not(windows))]
    {
        // For system installs on Linux, /usr/local/bin is typically already in PATH
        if config.paths().is_system_install() {
            println!(
                "{}",
                "✓ System PATH (/usr/local/bin) is typically pre-configured".green()
            );
            println!("  Symlinks will be created in /usr/local/bin");
        } else {
            setup_path_unix(&bin_dir.to_string_lossy())?;
        }
    }

    Ok(())
}

/// Set up PATH on Windows (modify user or system environment variable)
#[cfg(windows)]
fn setup_path_windows(bin_dir: &str, is_system_install: bool) -> Result<()> {
    use crate::core::registry::add_to_system_path;
    use std::path::Path;
    use std::process::Command;

    if is_system_install {
        // For system installs, use registry to modify system PATH
        match add_to_system_path(Path::new(bin_dir)) {
            Ok(true) => {
                println!("{}", "✓ Added wenget bin directory to system PATH".green());
                println!();
                println!("{}", "IMPORTANT:".yellow().bold());
                println!("  Please restart your terminal or command prompt");
                println!("  for the PATH changes to take effect.");
            }
            Ok(false) => {
                println!(
                    "{}",
                    "✓ wenget bin directory is already in system PATH".green()
                );
            }
            Err(e) => {
                println!("{} Failed to update system PATH: {}", "⚠".yellow(), e);
                println!();
                println!("Please manually add the following to your system PATH:");
                println!("  {}", bin_dir.cyan());
            }
        }
        return Ok(());
    }

    // For user installs, use PowerShell to add to user PATH
    let ps_script = format!(
        r#"
        $oldPath = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($oldPath -notlike '*{}*') {{
            $newPath = $oldPath + ';{}'
            [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
            Write-Output 'Added'
        }} else {{
            Write-Output 'Already exists'
        }}
        "#,
        bin_dir, bin_dir
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
        .context("Failed to execute PowerShell command")?;

    let result = String::from_utf8_lossy(&output.stdout);

    if result.contains("Added") {
        println!("{}", "✓ Added wenget bin directory to user PATH".green());
        println!();
        println!("{}", "IMPORTANT:".yellow().bold());
        println!("  Please restart your terminal or command prompt");
        println!("  for the PATH changes to take effect.");
    } else if result.contains("Already exists") {
        println!("{}", "✓ wenget bin directory is already in PATH".green());
    } else if !output.status.success() {
        println!("{}", "⚠ Failed to automatically update PATH".yellow());
        println!();
        println!("Please manually add the following to your PATH:");
        println!("  {}", bin_dir.cyan());
    }

    Ok(())
}

/// Set up PATH on Unix-like systems (add to shell config)
#[cfg(not(windows))]
fn setup_path_unix(bin_dir: &str) -> Result<()> {
    let home = dirs::home_dir().context("Failed to determine home directory")?;

    // Determine which shell configs to update
    let shell_configs = detect_shell_configs(&home);

    if shell_configs.is_empty() {
        println!("{}", "⚠ No shell configuration files found".yellow());
        println!();
        println!("Please manually add the following to your shell configuration:");
        println!("  export PATH=\"{}:$PATH\"", bin_dir.cyan());
        return Ok(());
    }

    let export_line = format!("\n# wenget\nexport PATH=\"{}:$PATH\"\n", bin_dir);

    let mut updated_files = Vec::new();
    let mut skipped_files = Vec::new();

    for config_path in shell_configs {
        match update_shell_config(&config_path, &export_line, bin_dir) {
            Ok(true) => updated_files.push(config_path),
            Ok(false) => skipped_files.push(config_path),
            Err(e) => {
                println!(
                    "  {} Failed to update {}: {}",
                    "⚠".yellow(),
                    config_path.display(),
                    e
                );
            }
        }
    }

    if !updated_files.is_empty() {
        println!("{}", "✓ Updated shell configuration files:".green());
        for path in &updated_files {
            println!("  • {}", path.display());
        }
        println!();
        println!("{}", "IMPORTANT:".yellow().bold());
        println!("  Run the following command to apply changes:");
        println!(
            "  source ~/{}",
            updated_files[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .cyan()
        );
        println!();
        println!("  Or restart your terminal");
    }

    if !skipped_files.is_empty() {
        println!("{}", "✓ wenget is already configured in:".green());
        for path in &skipped_files {
            println!("  • {}", path.display());
        }
    }

    Ok(())
}

/// Detect available shell configuration files
#[cfg(not(windows))]
fn detect_shell_configs(home: &std::path::Path) -> Vec<PathBuf> {
    let mut configs = Vec::new();

    // Check for common shell configs
    let candidates = vec![".bashrc", ".bash_profile", ".zshrc", ".profile"];

    for candidate in candidates {
        let path = home.join(candidate);
        if path.exists() {
            configs.push(path);
        }
    }

    // If no configs found, try to create .profile
    if configs.is_empty() {
        let profile = home.join(".profile");
        configs.push(profile);
    }

    configs
}

/// Update a shell configuration file
#[cfg(not(windows))]
fn update_shell_config(config_path: &PathBuf, export_line: &str, bin_dir: &str) -> Result<bool> {
    // Check if already configured
    if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;

        if content.contains(bin_dir) {
            return Ok(false); // Already configured
        }
    }

    // Append to file
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(config_path)
        .with_context(|| format!("Failed to open {}", config_path.display()))?;

    file.write_all(export_line.as_bytes())
        .with_context(|| format!("Failed to write to {}", config_path.display()))?;

    Ok(true)
}

/// Check if a directory is in PATH
fn is_in_path(dir: PathBuf) -> Result<bool> {
    let path_var = env::var("PATH").unwrap_or_default();
    let dir_str = dir.to_string_lossy();

    Ok(path_var
        .split(if cfg!(windows) { ';' } else { ':' })
        .any(|p| p == dir_str.as_ref()))
}

/// Check if wenget bucket is already configured
fn has_wenget_bucket(config: &Config) -> Result<bool> {
    match config.get_or_create_buckets() {
        Ok(bucket_config) => {
            // Check if any bucket has the wenget URL
            Ok(bucket_config
                .buckets
                .iter()
                .any(|b| b.url == WENGET_BUCKET_URL))
        }
        Err(_) => Ok(false),
    }
}

/// Add wenget bucket
fn add_wenget_bucket(config: &Config) -> Result<()> {
    println!();
    println!("{} wenget bucket...", "Adding".cyan());

    // Load bucket config
    let mut bucket_config = config.get_or_create_buckets()?;

    // Create bucket
    let bucket = Bucket {
        name: WENGET_BUCKET_NAME.to_string(),
        url: WENGET_BUCKET_URL.to_string(),
        enabled: true,
        priority: 100,
    };

    // Try to add bucket
    if bucket_config.add_bucket(bucket) {
        // Save config
        config.save_buckets(&bucket_config)?;

        println!("{} Bucket '{}' added", "✓".green(), WENGET_BUCKET_NAME);
        println!("  URL: {}", WENGET_BUCKET_URL);

        // Build cache immediately
        match config.rebuild_cache() {
            Ok(cache) => {
                println!();
                println!(
                    "{} {} package(s) available from wenget bucket",
                    "✓".green(),
                    cache.packages.len()
                );
            }
            Err(e) => {
                println!();
                println!("{} Failed to build cache: {}", "⚠".yellow(), e);
                println!("  You can rebuild it later with: wenget bucket refresh");
            }
        }
    } else {
        println!(
            "{} Bucket '{}' already exists",
            "✗".yellow(),
            WENGET_BUCKET_NAME
        );
    }

    Ok(())
}
