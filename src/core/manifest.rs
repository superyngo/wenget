//! Manifest data structures for wenget
//!
//! This module defines the core data structures for package metadata:
//! - `Package`: Individual package information
//! - `PlatformBinary`: Platform-specific binary information
//! - `SourceManifest`: The sources.json structure
//! - `InstalledSet`: The set of installed packages

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

/// Cached interpreter availability results
static INTERPRETER_CACHE: OnceLock<InterpreterCache> = OnceLock::new();

/// Cache for interpreter availability checks
struct InterpreterCache {
    pwsh_available: bool,
    bash_available: bool,
    python_available: bool,
}

impl InterpreterCache {
    fn detect() -> Self {
        Self {
            pwsh_available: std::process::Command::new("pwsh")
                .arg("--version")
                .output()
                .is_ok(),
            bash_available: std::process::Command::new("bash")
                .arg("--version")
                .output()
                .is_ok(),
            python_available: std::process::Command::new("python")
                .arg("--version")
                .output()
                .is_ok()
                || std::process::Command::new("python3")
                    .arg("--version")
                    .output()
                    .is_ok(),
        }
    }
}

fn get_interpreter_cache() -> &'static InterpreterCache {
    INTERPRETER_CACHE.get_or_init(InterpreterCache::detect)
}

/// Script type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ScriptType {
    /// PowerShell script (.ps1)
    PowerShell,
    /// Windows Batch script (.bat, .cmd)
    Batch,
    /// Bash/Shell script (.sh)
    Bash,
    /// Python script (.py)
    Python,
}

impl ScriptType {
    /// Get the file extension for this script type
    pub fn extension(&self) -> &str {
        match self {
            ScriptType::PowerShell => "ps1",
            ScriptType::Batch => "cmd",
            ScriptType::Bash => "sh",
            ScriptType::Python => "py",
        }
    }

    /// Get the display name for this script type
    pub fn display_name(&self) -> &str {
        match self {
            ScriptType::PowerShell => "PowerShell",
            ScriptType::Batch => "Batch",
            ScriptType::Bash => "Bash",
            ScriptType::Python => "Python",
        }
    }

    /// Check if this script type is supported on the current platform.
    ///
    /// This checks if the required interpreter is actually available on the system.
    /// Results are cached for performance.
    pub fn is_supported_on_current_platform(&self) -> bool {
        let cache = get_interpreter_cache();

        match self {
            ScriptType::PowerShell => {
                // PowerShell is available on Windows natively, and on Linux/macOS via pwsh
                if cfg!(target_os = "windows") {
                    true
                } else {
                    cache.pwsh_available
                }
            }
            ScriptType::Batch => {
                // Batch scripts only work on Windows
                cfg!(target_os = "windows")
            }
            ScriptType::Bash => {
                // Bash is available on Linux and macOS, and on Windows via WSL/Git Bash
                if cfg!(target_os = "windows") {
                    cache.bash_available
                } else {
                    true
                }
            }
            ScriptType::Python => cache.python_available,
        }
    }

    /// Check basic OS compatibility without executing commands (for listing)
    /// This is faster than is_supported_on_current_platform and doesn't require
    /// the interpreter to be installed
    pub fn is_os_compatible(&self) -> bool {
        match self {
            ScriptType::PowerShell => {
                // PowerShell scripts work on Windows natively
                // On Unix, they require pwsh but we don't check here
                cfg!(target_os = "windows")
            }
            ScriptType::Batch => {
                // Batch scripts only work on Windows
                cfg!(target_os = "windows")
            }
            ScriptType::Bash => {
                // Bash scripts work natively on Unix-like systems
                // On Windows they require WSL/Git Bash but we don't check here
                !cfg!(target_os = "windows")
            }
            ScriptType::Python => {
                // Python scripts can work on any platform if Python is installed
                // We don't check for Python installation here
                true
            }
        }
    }

    /// Get the platform-specific script type preference order.
    ///
    /// - Windows: PowerShell > Batch > Python > Bash
    /// - Unix: Bash > Python > PowerShell
    pub fn preference_order() -> &'static [ScriptType] {
        #[cfg(target_os = "windows")]
        {
            &[
                ScriptType::PowerShell,
                ScriptType::Batch,
                ScriptType::Python,
                ScriptType::Bash,
            ]
        }

        #[cfg(not(target_os = "windows"))]
        {
            &[ScriptType::Bash, ScriptType::Python, ScriptType::PowerShell]
        }
    }
}

/// Platform-specific binary information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlatformBinary {
    /// Download URL for the binary
    pub url: String,

    /// File size in bytes
    pub size: u64,

    /// Optional SHA256 checksum (for future use)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,

    /// Original asset filename (used for variant identification and display)
    pub asset_name: String,
}

/// Platform-specific script information (for multi-platform scripts)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScriptPlatform {
    /// Download URL for this platform's script
    pub url: String,

    /// Optional SHA256 checksum
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

/// Package metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    /// Package name (used as identifier)
    pub name: String,

    /// Short description
    pub description: String,

    /// Repository URL (e.g., https://github.com/user/repo)
    pub repo: String,

    /// Homepage URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// License (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,

    /// Package version (e.g., "14.1.0")
    /// Populated when fetching from GitHub API, optional for bucket manifests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Platform-specific binaries
    /// Key format: "{os}-{arch}" or "{os}-{arch}-{variant}"
    /// Examples: "windows-x86_64", "linux-x86_64-musl", "macos-aarch64"
    /// Each platform can have multiple package variants (e.g., baseline, desktop, etc.)
    pub platforms: HashMap<String, Vec<PlatformBinary>>,
}

/// Script item metadata (for bucket scripts)
///
/// Supports multi-platform scripts where the same script name
/// can have different implementations for different platforms.
///
/// # Example JSON format:
/// ```json
/// {
///   "name": "rclonemm",
///   "description": "Manage rclone mount through ssh config.",
///   "repo": "https://gist.github.com/superyngo/...",
///   "platforms": {
///     "bash": { "url": "https://.../rclonemm.sh" },
///     "powershell": { "url": "https://.../rclonemm.ps1" }
///   }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptItem {
    /// Script name (used as identifier)
    pub name: String,

    /// Short description
    pub description: String,

    /// Repository URL (for reference, e.g., Gist URL)
    pub repo: String,

    /// Platform-specific scripts (key: script type like "bash", "powershell")
    pub platforms: HashMap<ScriptType, ScriptPlatform>,

    /// Homepage URL (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// License (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

impl ScriptItem {
    /// Get the best compatible script for the current platform (for display/listing)
    ///
    /// Priority order:
    /// - Windows: PowerShell > Batch > Python > Bash
    /// - Unix: Bash > Python > PowerShell
    ///
    /// Note: This uses `is_os_compatible()` for basic OS-level filtering,
    /// which doesn't check if the actual interpreter is installed.
    /// For installation, use `get_installable_script()` instead.
    ///
    /// Returns the script type and its platform info if a compatible one is found.
    pub fn get_compatible_script(&self) -> Option<(ScriptType, &ScriptPlatform)> {
        for script_type in ScriptType::preference_order() {
            if script_type.is_os_compatible() {
                if let Some(platform) = self.platforms.get(script_type) {
                    return Some((script_type.clone(), platform));
                }
            }
        }
        None
    }

    /// Get the best installable script for the current platform (checks if interpreter exists)
    ///
    /// This is more thorough than `get_compatible_script()` as it actually checks
    /// if the required interpreter is installed on the system.
    ///
    /// Returns the script type and its platform info if an installable one is found.
    pub fn get_installable_script(&self) -> Option<(ScriptType, &ScriptPlatform)> {
        for script_type in ScriptType::preference_order() {
            if script_type.is_supported_on_current_platform() {
                if let Some(platform) = self.platforms.get(script_type) {
                    return Some((script_type.clone(), platform));
                }
            }
        }
        None
    }

    /// Get all available platforms for this script
    #[allow(dead_code)]
    pub fn available_platforms(&self) -> Vec<ScriptType> {
        self.platforms.keys().cloned().collect()
    }

    /// Check if this script has a compatible version for the current platform
    pub fn is_compatible_with_current_platform(&self) -> bool {
        self.get_compatible_script().is_some()
    }

    /// Get a specific platform's script info
    #[allow(dead_code)]
    pub fn get_platform(&self, script_type: &ScriptType) -> Option<&ScriptPlatform> {
        self.platforms.get(script_type)
    }

    /// Get a display string showing available platforms
    pub fn platforms_display(&self) -> String {
        let platforms: Vec<&str> = self.platforms.keys().map(|st| st.display_name()).collect();
        platforms.join(", ")
    }
}

/// Source manifest (sources.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceManifest {
    /// List of available packages
    pub packages: Vec<Package>,

    /// List of available scripts
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<ScriptItem>,
}

impl SourceManifest {
    /// Create a new empty source manifest
    pub fn new() -> Self {
        Self {
            packages: Vec::new(),
            scripts: Vec::new(),
        }
    }

    /// Get packages that support a specific platform
    #[allow(dead_code)]
    pub fn packages_for_platform(&self, platform: &str) -> Vec<&Package> {
        self.packages
            .iter()
            .filter(|p| p.platforms.contains_key(platform))
            .collect()
    }

    /// Get scripts that are supported on the current platform
    #[allow(dead_code)]
    pub fn scripts_for_current_platform(&self) -> Vec<&ScriptItem> {
        self.scripts
            .iter()
            .filter(|s| s.is_compatible_with_current_platform())
            .collect()
    }
}

impl Default for SourceManifest {
    fn default() -> Self {
        Self::new()
    }
}

/// Package source tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackageSource {
    /// Package installed from a bucket
    Bucket { name: String },
    /// Package installed directly from a GitHub repository URL
    DirectRepo { url: String },
    /// Script installed from local path or URL
    Script {
        /// Original source (local path or URL)
        origin: String,
        /// Script type
        script_type: ScriptType,
    },
}

/// Highest package-record schema version this build understands.
///
/// A record above this version is skipped on load, never rewritten: a downgrade
/// must be read-only toward data it cannot interpret.
pub const CURRENT_META_VERSION: u32 = 1;

fn default_meta_version() -> u32 {
    1
}

/// Installed package information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    /// Schema version of this package record. Absent means version 1.
    #[serde(default = "default_meta_version")]
    pub meta_version: u32,

    /// Canonical repository name (e.g., "bun", "cli")
    /// This is the base name from the repository, without variant suffix
    #[serde(default)]
    pub repo_name: String,

    /// Variant identifier (e.g., "baseline-profile", "desktop")
    /// None indicates the default/main variant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,

    /// Installed version
    pub version: String,

    /// Platform identifier
    pub platform: String,

    /// Installation timestamp
    pub installed_at: DateTime<Utc>,

    /// Installation path
    pub install_path: String,

    /// Map of executable relative path (from install_path) to command name
    /// Example: {"bin/rg": "rg", "bin/rg-completions": "rg-completions"}
    #[serde(default)]
    pub executables: HashMap<String, String>,

    /// Package source (where it was installed from)
    pub source: PackageSource,

    /// Package description
    pub description: String,

    /// DEPRECATED: Legacy flat command names list.
    /// Kept for backward compatibility during migration from older versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_names: Vec<String>,

    /// Legacy single command name (for backward compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_name: Option<String>,

    /// Original asset filename (for variant identification)
    pub asset_name: String,

    /// DEPRECATED: Parent package name (if this is a variant)
    /// Kept for backward compatibility during migration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_package: Option<String>,

    /// Download URL used to install the package/script
    /// Used for scripts from buckets to detect updates via URL change
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
}

impl InstalledPackage {
    /// Get all command names from the executables map.
    /// Falls back to legacy command_names if executables is empty (pre-migration).
    pub fn get_command_names(&self) -> Vec<&str> {
        if !self.executables.is_empty() {
            self.executables.values().map(|s| s.as_str()).collect()
        } else {
            self.command_names.iter().map(|s| s.as_str()).collect()
        }
    }

    /// Get the executable path for a given command name
    pub fn get_exe_path_for_command(&self, command_name: &str) -> Option<&str> {
        self.executables
            .iter()
            .find(|(_, name)| name.as_str() == command_name)
            .map(|(path, _)| path.as_str())
    }
}

/// The set of installed packages, loaded from per-package records
///
/// This is an in-memory collection, not a file format: each package's state is
/// stored in its own `{app_dir}/.wenget/package.json` (see `src/core/store.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSet {
    /// Map of package name to installed package info
    pub packages: HashMap<String, InstalledPackage>,
}

impl InstalledSet {
    /// Create a new empty installed manifest
    pub fn new() -> Self {
        Self {
            packages: HashMap::new(),
        }
    }

    /// Check if a package is installed
    pub fn is_installed(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    /// Get installed package info
    pub fn get_package(&self, name: &str) -> Option<&InstalledPackage> {
        self.packages.get(name)
    }

    /// Add or update an installed package
    pub fn upsert_package(&mut self, name: String, package: InstalledPackage) {
        self.packages.insert(name, package);
    }

    /// Remove an installed package
    pub fn remove_package(&mut self, name: &str) -> Option<InstalledPackage> {
        self.packages.remove(name)
    }

    /// Get all installed package names
    #[allow(dead_code)]
    pub fn installed_names(&self) -> Vec<&str> {
        self.packages.keys().map(|s| s.as_str()).collect()
    }

    /// Group packages by their repo_name
    /// Returns a HashMap where keys are repo names and values are vectors of (key, package) tuples
    pub fn group_by_repo(&self) -> HashMap<String, Vec<(&String, &InstalledPackage)>> {
        let mut grouped: HashMap<String, Vec<(&String, &InstalledPackage)>> = HashMap::new();

        for (key, package) in &self.packages {
            let repo_name = if package.repo_name.is_empty() {
                // Fallback for packages without repo_name (old format)
                // Try to extract from key
                if let Some(pos) = key.find("::") {
                    &key[..pos]
                } else {
                    key.as_str()
                }
            } else {
                &package.repo_name
            };

            grouped
                .entry(repo_name.to_string())
                .or_default()
                .push((key, package));
        }

        grouped
    }

    /// Find all packages from a specific repository
    pub fn find_by_repo(&self, repo_name: &str) -> Vec<(&String, &InstalledPackage)> {
        self.packages
            .iter()
            .filter(|(_, pkg)| {
                if !pkg.repo_name.is_empty() {
                    pkg.repo_name == repo_name
                } else {
                    // Fallback for old format
                    let key_repo = if let Some(pos) = pkg.asset_name.find('-') {
                        &pkg.asset_name[..pos]
                    } else {
                        &pkg.asset_name
                    };
                    key_repo.eq_ignore_ascii_case(repo_name)
                }
            })
            .collect()
    }

    /// Check if a command name is already taken by another package
    #[allow(dead_code)] // Superseded by command_name_set for bulk probes; kept for single-check use and tests.
    pub fn is_command_taken(&self, command_name: &str, exclude_key: Option<&str>) -> bool {
        for (key, package) in &self.packages {
            if let Some(exclude) = exclude_key {
                if key == exclude {
                    continue;
                }
            }
            if package.executables.values().any(|n| n == command_name)
                || package.command_names.contains(&command_name.to_string())
            {
                return true;
            }
        }
        false
    }

    /// Build the set of all command names currently in use, optionally excluding
    /// one package key.
    ///
    /// This is the bulk form of `is_command_taken`: build the set once and probe
    /// it O(1) per candidate, instead of scanning every package's executables for
    /// each candidate. Used by command-name resolution loops that probe many
    /// suffixes (e.g. the 1..=99 numeric fallback in `resolve_command_name`).
    pub fn command_name_set(&self, exclude_key: Option<&str>) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for (key, package) in &self.packages {
            if let Some(exclude) = exclude_key {
                if key == exclude {
                    continue;
                }
            }
            for name in package.executables.values() {
                set.insert(name.clone());
            }
            for name in &package.command_names {
                set.insert(name.clone());
            }
        }
        set
    }

    /// Migrate old format (single command_name) to new format (command_names vec)
    /// Also migrates parent_package to repo_name/variant
    /// Also migrates install paths with `::` to use `-` separator (for Windows compatibility)
    pub fn migrate(&mut self) {
        use std::path::Path;

        for (key, package) in self.packages.iter_mut() {
            // Migrate command_name to command_names
            if package.command_names.is_empty() {
                if let Some(ref name) = package.command_name {
                    package.command_names = vec![name.clone()];
                }
            }

            // Migrate parent_package to repo_name/variant
            if package.repo_name.is_empty() {
                // Parse repo_name and variant from key
                if let Some(pos) = key.find("::") {
                    // New format key: "repo::variant"
                    package.repo_name = key[..pos].to_string();
                    package.variant = Some(key[pos + 2..].to_string());
                } else if key.contains('-') && package.parent_package.is_some() {
                    // Old format with parent_package
                    package.repo_name = package.parent_package.clone().unwrap_or_else(|| {
                        // Try to extract from key by removing variant suffix
                        if let Some(pos) = key.rfind('-') {
                            key[..pos].to_string()
                        } else {
                            key.clone()
                        }
                    });

                    // Extract variant from key
                    if let Some(pos) = key.rfind('-') {
                        let potential_variant = &key[pos + 1..];
                        // Only set variant if it's not empty and looks like a variant
                        if !potential_variant.is_empty()
                            && !potential_variant.chars().next().unwrap().is_numeric()
                        {
                            package.variant = Some(potential_variant.to_string());
                        }
                    }
                } else {
                    // No variant, just use key as repo_name
                    package.repo_name = key.clone();
                    package.variant = None;
                }
            }

            // Migrate install_path: replace `::` with `-` for filesystem compatibility
            if package.install_path.contains("::") {
                let old_path = Path::new(&package.install_path);

                // Try to rename the actual directory if it exists
                if old_path.exists() {
                    let new_path_str = package.install_path.replace("::", "-");
                    let new_path = Path::new(&new_path_str);

                    if let Err(e) = std::fs::rename(old_path, new_path) {
                        log::warn!(
                            "Failed to rename directory from {} to {}: {}",
                            old_path.display(),
                            new_path.display(),
                            e
                        );
                    } else {
                        log::info!(
                            "Migrated package directory: {} -> {}",
                            old_path.display(),
                            new_path.display()
                        );
                    }
                }

                // Update install_path in metadata
                package.install_path = package.install_path.replace("::", "-");
            }

            // Migrate command_names to executables map
            if package.executables.is_empty() && !package.command_names.is_empty() {
                let install_path = Path::new(&package.install_path);

                if install_path.exists() {
                    // Scan filesystem to match command_names to actual executable files
                    let mut remaining_names: Vec<String> = package.command_names.clone();

                    // Walk directory to find executables
                    if let Ok(entries) = Self::walk_dir_recursive(install_path) {
                        for entry_path in &entries {
                            if remaining_names.is_empty() {
                                break;
                            }

                            let rel_path = entry_path
                                .strip_prefix(install_path)
                                .unwrap_or(entry_path)
                                .to_string_lossy()
                                .to_string();

                            let filename = entry_path
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("");

                            // Strip known extensions for matching
                            let name_no_ext = filename
                                .trim_end_matches(".exe")
                                .trim_end_matches(".sh")
                                .trim_end_matches(".ps1")
                                .trim_end_matches(".bat")
                                .trim_end_matches(".cmd")
                                .trim_end_matches(".py");

                            // Try to match against remaining command names
                            if let Some(pos) = remaining_names
                                .iter()
                                .position(|n| n == filename || n == name_no_ext)
                            {
                                let cmd_name = remaining_names.remove(pos);
                                package.executables.insert(rel_path, cmd_name);
                            }
                        }
                    }

                    // Fallback for unmatched names
                    for name in remaining_names {
                        package.executables.insert(name.clone(), name);
                    }
                } else {
                    // Install path doesn't exist — use command_name as both key and value
                    for name in &package.command_names {
                        package.executables.insert(name.clone(), name.clone());
                    }
                }

                // Clear legacy fields
                package.command_names.clear();
                package.command_name = None;
            }
        }
    }

    /// Recursively walk a directory and return all file paths
    fn walk_dir_recursive(dir: &Path) -> Result<Vec<std::path::PathBuf>, std::io::Error> {
        let mut files = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    files.extend(Self::walk_dir_recursive(&path)?);
                } else {
                    files.push(path);
                }
            }
        }
        Ok(files)
    }
}

impl Default for InstalledSet {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract variant identifier from asset filename
///
/// Removes repo name prefix, platform suffixes, and file extensions
/// to identify the variant name (e.g., "baseline", "desktop")
///
/// # Examples
/// ```
/// use wenget::core::manifest::extract_variant_from_asset;
///
/// assert_eq!(extract_variant_from_asset("opencode-windows-x64.zip", "opencode"), None);
/// assert_eq!(extract_variant_from_asset("opencode-windows-x64-baseline.zip", "opencode"), Some("baseline".to_string()));
/// assert_eq!(extract_variant_from_asset("opencode-desktop-windows-x64.exe", "opencode"), Some("desktop".to_string()));
/// ```
pub fn extract_variant_from_asset(asset_name: &str, repo_name: &str) -> Option<String> {
    // Remove file extensions
    let name = asset_name
        .trim_end_matches(".zip")
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".exe")
        .trim_end_matches(".7z")
        .trim_end_matches(".tgz");

    // Remove repo name prefix (case-insensitive)
    let repo_lower = repo_name.to_lowercase();
    let name_lower = name.to_lowercase();

    let without_repo = if name_lower.starts_with(&repo_lower) {
        &name[repo_lower.len()..]
    } else {
        name
    };

    // Remove leading hyphens and underscores
    let without_repo = without_repo.trim_start_matches('-').trim_start_matches('_');

    // Normalize separators: replace all underscores with hyphens for consistent processing
    let normalized = without_repo.replace('_', "-");

    // Remove version numbers (improved pattern matching)
    // Split by '-' and filter out version-like segments (e.g., "1.0.0", "v1.0.0", "2.86.0")
    let segments: Vec<&str> = normalized.split('-').collect();
    let filtered_segments: Vec<&str> = segments
        .into_iter()
        .filter(|seg| {
            // Helper to check if a segment is a version number
            let is_version = |s: &str| -> bool {
                let s = s.trim_start_matches('v');
                // Must start with a digit and contain at least one dot
                s.chars().next().is_some_and(|c| c.is_ascii_digit())
                    && s.contains('.')
                    && s.chars().all(|c| c.is_ascii_digit() || c == '.')
            };

            !is_version(seg)
        })
        .collect();

    let without_version = filtered_segments.join("-");

    // Remove "unknown" keyword (common in Rust target triples)
    let without_unknown = without_version.replace("unknown", "");

    // Platform patterns to remove (ordered by specificity)
    let platform_patterns = [
        // OS-arch-variant combinations
        "windows-x86_64-msvc",
        "windows-x86_64-gnu",
        "linux-x86_64-musl",
        "linux-x86_64-gnu",
        // OS-arch combinations
        "windows-x86_64",
        "windows-amd64",
        "windows-x64",
        "windows-i686",
        "windows-x86",
        "windows-arm64",
        "windows-aarch64",
        "linux-x86_64",
        "linux-amd64",
        "linux-x64",
        "linux-i686",
        "linux-x86",
        "linux-arm64",
        "linux-aarch64",
        "linux-armv7",
        "darwin-x86_64",
        "darwin-amd64",
        "darwin-x64",
        "darwin-arm64",
        "darwin-aarch64",
        "macos-x86_64",
        "macos-amd64",
        "macos-x64",
        "macos-arm64",
        "macos-aarch64",
        "freebsd-x86_64",
        "freebsd-amd64",
        "freebsd-x64",
        // Generic arch patterns
        "x86_64",
        "amd64",
        "x64",
        "i686",
        "x86",
        "arm64",
        "aarch64",
        "armv7",
        // OS-only patterns
        "windows",
        "linux",
        "darwin",
        "macos",
        "freebsd",
        // Other common patterns
        "win32",
        "win64",
        "win",
        "musl",
        "gnu",
        "msvc",
        "pc", // Common in Rust target triples (e.g., x86_64-pc-windows-msvc)
    ];

    let mut result = without_unknown;

    // Remove platform patterns
    for pattern in &platform_patterns {
        // Try both with and without hyphens
        result = result.replace(&format!("-{}", pattern), "");
        result = result.replace(&format!("_{}", pattern), "");
        result = result.replace(pattern, "");
    }

    // Clean up multiple hyphens/underscores
    while result.contains("--") {
        result = result.replace("--", "-");
    }
    while result.contains("__") {
        result = result.replace("__", "_");
    }

    // Trim leading/trailing hyphens and underscores
    let result = result.trim_matches('-').trim_matches('_').to_string();

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Generate installed.json key from repo name and variant
///
/// When a package has no variant, the key is just the repo_name.
/// When a package has a variant, the key is "{repo_name}::{variant}"
/// This ensures all packages from the same repo can be easily identified.
///
/// # Examples
/// ```
/// use wenget::core::manifest::generate_installed_key;
///
/// assert_eq!(generate_installed_key("bun", None), "bun");
/// assert_eq!(generate_installed_key("bun", Some("baseline-profile")), "bun::baseline-profile");
/// assert_eq!(generate_installed_key("cli", Some("v2")), "cli::v2");
/// ```
pub fn generate_installed_key(repo_name: &str, variant: Option<&str>) -> String {
    match variant {
        Some(v) if !v.is_empty() => format!("{}::{}", repo_name, v),
        _ => repo_name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_manifest_new() {
        let manifest = SourceManifest::new();
        assert_eq!(manifest.packages.len(), 0);
    }

    #[test]
    fn test_installed_manifest() {
        let mut manifest = InstalledSet::new();

        let mut executables = HashMap::new();
        executables.insert("bin/test.exe".to_string(), "test".to_string());

        let package = InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "test".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "windows-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "C:\\Users\\test\\.wenget\\apps\\test".to_string(),
            executables,
            source: PackageSource::Bucket {
                name: "test-bucket".to_string(),
            },
            description: "Test package".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "test-windows-x64.zip".to_string(),
            parent_package: None,
            download_url: None,
        };

        manifest.upsert_package("test".to_string(), package);
        assert!(manifest.is_installed("test"));
        assert_eq!(manifest.get_package("test").unwrap().version, "1.0.0");

        manifest.remove_package("test");
        assert!(!manifest.is_installed("test"));
    }

    #[test]
    fn test_installed_package_executables_helpers() {
        let mut executables = HashMap::new();
        executables.insert("bin/rg".to_string(), "rg".to_string());
        executables.insert("bin/rg-doc".to_string(), "rg-doc".to_string());

        let pkg = InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/home/test/.wenget/apps/ripgrep".to_string(),
            executables,
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "Search tool".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "ripgrep-linux-x64.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };

        let names = pkg.get_command_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"rg"));
        assert!(names.contains(&"rg-doc"));

        assert_eq!(pkg.get_exe_path_for_command("rg"), Some("bin/rg"));
        assert_eq!(pkg.get_exe_path_for_command("rg-doc"), Some("bin/rg-doc"));
        assert_eq!(pkg.get_exe_path_for_command("nonexistent"), None);
    }

    #[test]
    fn test_is_command_taken_with_executables() {
        let mut manifest = InstalledSet::new();

        let mut executables = HashMap::new();
        executables.insert("bin/rg".to_string(), "rg".to_string());

        let pkg = InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/path".to_string(),
            executables,
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: String::new(),
            command_names: vec![],
            command_name: None,
            asset_name: "rg.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };

        manifest.upsert_package("ripgrep".to_string(), pkg);

        assert!(manifest.is_command_taken("rg", None));
        assert!(!manifest.is_command_taken("rg", Some("ripgrep")));
        assert!(!manifest.is_command_taken("nonexistent", None));
    }

    #[test]
    fn test_command_name_set() {
        use std::collections::HashSet;

        let mut manifest = InstalledSet::new();

        // Package A: executables map only
        let mut a_exes = HashMap::new();
        a_exes.insert("bin/rg".to_string(), "rg".to_string());
        a_exes.insert("bin/rg-doc".to_string(), "rg-doc".to_string());
        manifest.upsert_package(
            "ripgrep".to_string(),
            InstalledPackage {
                meta_version: CURRENT_META_VERSION,
                repo_name: "ripgrep".to_string(),
                variant: None,
                version: "14.0.0".to_string(),
                platform: "linux-x86_64".to_string(),
                installed_at: Utc::now(),
                install_path: "/path".to_string(),
                executables: a_exes,
                source: PackageSource::Bucket {
                    name: "main".to_string(),
                },
                description: String::new(),
                command_names: vec![],
                command_name: None,
                asset_name: "rg.tar.gz".to_string(),
                parent_package: None,
                download_url: None,
            },
        );

        // Package B: legacy command_names only
        manifest.upsert_package(
            "fzf".to_string(),
            InstalledPackage {
                meta_version: CURRENT_META_VERSION,
                repo_name: "fzf".to_string(),
                variant: None,
                version: "0.44.0".to_string(),
                platform: "linux-x86_64".to_string(),
                installed_at: Utc::now(),
                install_path: "/path".to_string(),
                executables: HashMap::new(),
                source: PackageSource::Bucket {
                    name: "main".to_string(),
                },
                description: String::new(),
                command_names: vec!["fzf".to_string()],
                command_name: None,
                asset_name: "fzf.tar.gz".to_string(),
                parent_package: None,
                download_url: None,
            },
        );

        let set: HashSet<String> = manifest.command_name_set(None);
        assert!(set.contains("rg"));
        assert!(set.contains("rg-doc"));
        assert!(set.contains("fzf"));
        assert!(!set.contains("missing"));

        // Excluding ripgrep should drop its commands but keep fzf's.
        let set_excluded = manifest.command_name_set(Some("ripgrep"));
        assert!(!set_excluded.contains("rg"));
        assert!(!set_excluded.contains("rg-doc"));
        assert!(set_excluded.contains("fzf"));
    }

    #[test]
    fn test_deserialize_old_format_without_executables() {
        let json = r#"{
            "packages": {
                "test": {
                    "repo_name": "test",
                    "version": "1.0.0",
                    "platform": "linux-x86_64",
                    "installed_at": "2025-01-01T00:00:00Z",
                    "install_path": "/path/to/test",
                    "files": ["bin/test", "README.md"],
                    "source": { "type": "bucket", "name": "main" },
                    "description": "Test",
                    "command_names": ["test"],
                    "asset_name": "test.tar.gz"
                }
            }
        }"#;

        let manifest: InstalledSet = serde_json::from_str(json).unwrap();
        let pkg = manifest.get_package("test").unwrap();

        assert!(pkg.executables.is_empty());
        assert_eq!(pkg.command_names, vec!["test".to_string()]);
    }

    #[test]
    fn test_serialize_new_format_no_files_field() {
        let mut executables = HashMap::new();
        executables.insert("bin/test".to_string(), "test".to_string());

        let pkg = InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "test".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/path".to_string(),
            executables,
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "Test".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "test.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };

        let json = serde_json::to_string(&pkg).unwrap();
        assert!(!json.contains("\"files\""));
        assert!(json.contains("\"executables\""));
        assert!(!json.contains("\"command_names\""));
    }

    #[test]
    fn test_migrate_command_names_to_executables() {
        use std::fs;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let app_dir = tmp.path().join("apps").join("myapp");
        fs::create_dir_all(app_dir.join("bin")).unwrap();

        // Create a fake executable
        fs::write(app_dir.join("bin").join("myapp"), "#!/bin/sh\necho hi").unwrap();
        #[cfg(unix)]
        fs::set_permissions(
            app_dir.join("bin").join("myapp"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();

        let json = format!(
            r#"{{
            "packages": {{
                "myapp": {{
                    "repo_name": "myapp",
                    "version": "1.0.0",
                    "platform": "linux-x86_64",
                    "installed_at": "2025-01-01T00:00:00Z",
                    "install_path": "{}",
                    "files": ["bin/myapp", "README.md"],
                    "source": {{ "type": "bucket", "name": "main" }},
                    "description": "Test",
                    "command_names": ["myapp"],
                    "asset_name": "myapp.tar.gz"
                }}
            }}
        }}"#,
            app_dir.display()
        );

        let mut manifest: InstalledSet = serde_json::from_str(&json).unwrap();
        manifest.migrate();

        let pkg = manifest.get_package("myapp").unwrap();
        // executables should now have the mapping
        assert!(!pkg.executables.is_empty());
        assert_eq!(pkg.executables.get("bin/myapp"), Some(&"myapp".to_string()));
        // command_names should be cleared after migration
        assert!(pkg.command_names.is_empty());
    }

    #[test]
    fn test_migrate_fallback_when_install_path_missing() {
        let json = r#"{
            "packages": {
                "gone": {
                    "repo_name": "gone",
                    "version": "1.0.0",
                    "platform": "linux-x86_64",
                    "installed_at": "2025-01-01T00:00:00Z",
                    "install_path": "/nonexistent/path/that/does/not/exist",
                    "files": [],
                    "source": { "type": "bucket", "name": "main" },
                    "description": "Test",
                    "command_names": ["gone-cmd"],
                    "asset_name": "gone.tar.gz"
                }
            }
        }"#;

        let mut manifest: InstalledSet = serde_json::from_str(json).unwrap();
        manifest.migrate();

        let pkg = manifest.get_package("gone").unwrap();
        // Fallback: command_name used as both key and value
        assert_eq!(
            pkg.executables.get("gone-cmd"),
            Some(&"gone-cmd".to_string())
        );
        assert!(pkg.command_names.is_empty());
    }

    /// A minimal, valid installed package for tests.
    fn sample_package() -> InstalledPackage {
        InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/tmp/apps/ripgrep".to_string(),
            executables: HashMap::from([("bin/rg".to_string(), "rg".to_string())]),
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "search tool".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "ripgrep-linux.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        }
    }

    #[test]
    fn test_meta_version_defaults_to_one_when_absent() {
        let json = r#"{
            "repo_name": "ripgrep",
            "version": "14.0.0",
            "platform": "linux-x86_64",
            "installed_at": "2026-01-01T00:00:00Z",
            "install_path": "/tmp/apps/ripgrep",
            "executables": {"bin/rg": "rg"},
            "source": {"type": "bucket", "name": "main"},
            "description": "search tool",
            "asset_name": "ripgrep-linux.tar.gz"
        }"#;

        let pkg: InstalledPackage = serde_json::from_str(json).unwrap();
        assert_eq!(pkg.meta_version, 1);
        assert_eq!(pkg.meta_version, CURRENT_META_VERSION);
    }

    #[test]
    fn test_meta_version_round_trips() {
        let json = serde_json::to_string(&sample_package()).unwrap();
        assert!(json.contains("\"meta_version\":1"));
        let back: InstalledPackage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.meta_version, 1);
    }
}
