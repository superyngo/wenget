//! Path management for wenget
//!
//! This module provides utilities for managing all wenget-related paths:
//!
//! ## User-level installation (~/.wenget/):
//! - Root directory: ~/.wenget/
//! - Installed manifest: ~/.wenget/installed.json
//! - Buckets config: ~/.wenget/buckets.json
//! - Manifest cache: ~/.wenget/manifest-cache.json
//! - Apps directory: ~/.wenget/apps/
//! - Bin directory: ~/.local/bin/
//! - Cache directory: ~/.wenget/cache/
//!
//! ## System-level installation (when running as root/Administrator):
//! - Linux: /opt/wenget/ with symlinks in /usr/local/bin
//! - Windows: %ProgramW6432%\wenget\ with bin in PATH

use crate::core::privilege::is_elevated;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Sanitize a path component by replacing invalid filesystem characters
///
/// Converts `::` (used in variant keys) to `-` for filesystem compatibility, so
/// internal keys like "bun::baseline" become safe paths like "bun-baseline".
///
/// Also neutralizes path traversal: separators become `-`, `..` is collapsed, and
/// an all-dots component is replaced. Names reach here from remote manifests and
/// the GitHub API, and the resulting path is what `delete` removes, so it must
/// never be able to point outside its intended parent directory.
///
/// # Examples
/// ```
/// assert_eq!(sanitize_path_component("bun::baseline"), "bun-baseline");
/// assert_eq!(sanitize_path_component("ripgrep"), "ripgrep");
/// assert_eq!(sanitize_path_component("../../etc/passwd"), ".-.-etc-passwd");
/// ```
pub fn sanitize_path_component(name: &str) -> String {
    let mut out = name.replace("::", "-");

    // Package and command names arrive from remote bucket manifests and the
    // GitHub API, and are joined onto the apps/bin directories. Neutralize
    // anything that could escape that parent, since the resulting path is also
    // what `wenget delete` passes to `remove_dir_all`/`remove_file`.
    out = out.replace(['/', '\\'], "-");
    while out.contains("..") {
        out = out.replace("..", ".");
    }

    // A component of only dots resolves to the parent directory itself.
    if out.is_empty() || out.chars().all(|c| c == '.') {
        return "_".to_string();
    }

    out
}

/// wenget paths manager
#[derive(Debug, Clone)]
pub struct WenPaths {
    /// Root directory
    root: PathBuf,
    /// Whether this is a system-level installation
    is_system_install: bool,
    /// Custom bin directory (overrides default)
    custom_bin_dir: Option<PathBuf>,
}

impl WenPaths {
    /// Create a new WenPaths instance
    ///
    /// Automatically detects if running with elevated privileges and uses
    /// the appropriate paths:
    /// - User: ~/.wenget/
    /// - System (Linux): /opt/wenget/
    /// - System (Windows): %ProgramW6432%\wenget\
    ///
    /// # Errors
    /// Returns an error if the home directory cannot be determined (for user installs)
    pub fn new() -> Result<Self> {
        Self::new_with_custom_bin(None)
    }

    /// Create a new WenPaths instance with optional custom bin directory
    ///
    /// `WENGET_ROOT`, when set and non-empty, overrides both the root and the bin
    /// directory. This exists so migration, quarantine, the collision guard, and
    /// orphaned-shim detection can be exercised on the shipped binary without
    /// pointing it at the user's real `~/.wenget/`.
    pub fn new_with_custom_bin(custom_bin_dir: Option<PathBuf>) -> Result<Self> {
        if let Some(root) = std::env::var_os("WENGET_ROOT") {
            if !root.is_empty() {
                let mut paths = Self::with_root(PathBuf::from(root));
                if let Some(bin) = custom_bin_dir {
                    paths.custom_bin_dir = Some(bin);
                }
                return Ok(paths);
            }
        }

        let is_system = is_elevated();

        let root = if is_system {
            Self::system_root_path()
        } else {
            Self::user_root_path()?
        };

        Ok(Self {
            root,
            is_system_install: is_system,
            custom_bin_dir,
        })
    }

    /// Create a WenPaths instance explicitly for user-level installation
    ///
    /// This bypasses the privilege detection and always uses ~/.wenget/
    #[allow(dead_code)]
    pub fn new_user() -> Result<Self> {
        Ok(Self {
            root: Self::user_root_path()?,
            is_system_install: false,
            custom_bin_dir: None,
        })
    }

    /// Create a WenPaths instance rooted at an arbitrary directory
    ///
    /// Used by tests and by the `WENGET_ROOT` override, so both operate on a
    /// self-contained layout instead of the real `~/.wenget/`. `bin_dir` also
    /// resolves under `root`, keeping the whole layout self-contained.
    pub fn with_root(root: PathBuf) -> Self {
        Self {
            custom_bin_dir: Some(root.join("bin")),
            root,
            is_system_install: false,
        }
    }

    /// Create a WenPaths instance explicitly for system-level installation
    ///
    /// This bypasses the privilege detection and always uses system paths
    #[allow(dead_code)]
    pub fn new_system() -> Self {
        Self {
            root: Self::system_root_path(),
            is_system_install: true,
            custom_bin_dir: None,
        }
    }

    /// Get the user-level root path (~/.wenget/)
    fn user_root_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to determine home directory")?;
        Ok(home.join(".wenget"))
    }

    /// Get the system-level root path
    fn system_root_path() -> PathBuf {
        #[cfg(unix)]
        {
            PathBuf::from("/opt/wenget")
        }

        #[cfg(windows)]
        {
            let program_files =
                std::env::var("ProgramW6432").unwrap_or_else(|_| "C:\\Program Files".to_string());
            PathBuf::from(program_files).join("wenget")
        }

        #[cfg(not(any(unix, windows)))]
        {
            // Fallback for other platforms
            PathBuf::from("/opt/wenget")
        }
    }

    /// Check if this is a system-level installation
    pub fn is_system_install(&self) -> bool {
        self.is_system_install
    }

    /// Get the root directory
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the legacy installed.json path
    ///
    /// Retained only so `InstalledStore` can migrate and retire the file. wenget
    /// never writes it.
    pub fn installed_json(&self) -> PathBuf {
        self.root.join("installed.json")
    }

    /// Get the buckets config path
    pub fn buckets_json(&self) -> PathBuf {
        self.root.join("buckets.json")
    }

    /// Get the manifest cache path
    pub fn manifest_cache_json(&self) -> PathBuf {
        self.root.join("manifest-cache.json")
    }

    /// Get the apps directory
    pub fn apps_dir(&self) -> PathBuf {
        self.root.join("apps")
    }

    /// Get a specific app's directory
    pub fn app_dir(&self, name: &str) -> PathBuf {
        self.apps_dir().join(sanitize_path_component(name))
    }

    /// Get a specific app's bin directory
    #[allow(dead_code)]
    pub fn app_bin_dir(&self, name: &str) -> PathBuf {
        self.app_dir(name).join("bin")
    }

    /// Get the `.wenget` directory holding a package's record
    // First callers land with InstalledStore.
    #[allow(dead_code)]
    pub fn record_dir(&self, key: &str) -> PathBuf {
        self.app_dir(key).join(".wenget")
    }

    /// Get a package's record path: `{app_dir}/.wenget/package.json`
    #[allow(dead_code)]
    pub fn package_record_path(&self, key: &str) -> PathBuf {
        self.record_dir(key).join("package.json")
    }

    /// Get the staging directory used by install/update swaps
    ///
    /// Lives under `apps/` so the swap is a same-filesystem `rename`. The leading
    /// dot keeps it out of the app-directory scan.
    // First caller lands with stage-and-swap installs.
    #[allow(dead_code)]
    pub fn staging_dir(&self) -> PathBuf {
        self.apps_dir().join(".staging")
    }

    /// Get the bin directory
    ///
    /// Returns custom bin directory if set, otherwise:
    /// - For system installs on Linux: /usr/local/bin for symlinks
    /// - For user installs: ~/.local/bin
    pub fn bin_dir(&self) -> PathBuf {
        if let Some(ref custom) = self.custom_bin_dir {
            return custom.clone();
        }

        if self.is_system_install {
            #[cfg(unix)]
            {
                PathBuf::from("/usr/local/bin")
            }

            #[cfg(not(unix))]
            {
                self.root.join("bin")
            }
        } else {
            // User-level installation: use ~/.local/bin
            let home = dirs::home_dir().expect("Failed to determine home directory");
            home.join(".local").join("bin")
        }
    }

    /// Get the internal bin directory (always {root}/bin)
    ///
    /// This is used for Windows system installs where we need to add
    /// {root}/bin to PATH rather than using symlinks
    pub fn internal_bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Get the cache directory
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Get the downloads directory
    pub fn downloads_dir(&self) -> PathBuf {
        self.cache_dir().join("downloads")
    }

    /// Get the config file path (config.toml)
    pub fn config_toml(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Initialize all required directories
    ///
    /// Creates the following directories if they don't exist:
    /// - {root}/
    /// - {root}/apps/
    /// - ~/.local/bin/ (or /usr/local/bin for system installs on Linux)
    /// - {root}/cache/downloads/
    pub fn init_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root).context("Failed to create wenget root directory")?;

        std::fs::create_dir_all(self.apps_dir()).context("Failed to create apps directory")?;

        // Create bin directory based on installation level
        if self.is_system_install {
            // For system installs on Linux, /usr/local/bin should already exist
            // For Windows system installs, create {root}/bin
            #[cfg(not(unix))]
            {
                std::fs::create_dir_all(self.bin_dir())
                    .context("Failed to create bin directory")?;
            }
            // Also need internal bin dir
            std::fs::create_dir_all(self.internal_bin_dir())
                .context("Failed to create internal bin directory")?;
        } else {
            // For user installs, create ~/.local/bin
            std::fs::create_dir_all(self.bin_dir()).context("Failed to create bin directory")?;
        }

        std::fs::create_dir_all(self.downloads_dir())
            .context("Failed to create downloads directory")?;

        Ok(())
    }

    /// Check if wenget is initialized (root directory exists)
    pub fn is_initialized(&self) -> bool {
        self.root.exists()
    }

    /// Get the symlink/shim path for an app in the bin directory
    ///
    /// On Unix: {bin_dir}/{name}
    /// On Windows: {bin_dir}/{name}.cmd
    pub fn bin_shim_path(&self, name: &str) -> PathBuf {
        #[cfg(windows)]
        {
            self.bin_dir().join(format!("{}.cmd", name))
        }

        #[cfg(not(windows))]
        {
            self.bin_dir().join(name)
        }
    }

    /// Get the platform-specific executable name
    ///
    /// On Windows: {name}.exe
    /// On Unix: {name}
    #[allow(dead_code)]
    pub fn executable_name(name: &str) -> String {
        #[cfg(windows)]
        {
            format!("{}.exe", name)
        }

        #[cfg(not(windows))]
        {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_path_component_neutralizes_traversal() {
        // Normal names and variant keys are untouched / mapped as before.
        assert_eq!(sanitize_path_component("ripgrep"), "ripgrep");
        assert_eq!(sanitize_path_component("bun::baseline"), "bun-baseline");

        // A hostile manifest name must not be able to escape its parent dir.
        for hostile in [
            "../../etc/passwd",
            "..\\..\\Windows\\System32",
            "/etc/passwd",
            "foo/../../bar",
            "..",
            "...",
            "",
        ] {
            let safe = sanitize_path_component(hostile);
            let path = std::path::Path::new(&safe);
            assert!(
                !safe.contains("..")
                    && !safe.contains('/')
                    && !safe.contains('\\')
                    && !path.is_absolute()
                    && path.components().count() == 1,
                "{hostile:?} sanitized to {safe:?}, which is still not a single safe component"
            );
        }
    }

    #[test]
    fn test_paths_creation() {
        let paths = WenPaths::new().unwrap();
        // The root should end with .wenget (user) or wenget (system)
        let root_str = paths.root().to_string_lossy();
        assert!(
            root_str.ends_with(".wenget") || root_str.ends_with("wenget"),
            "Root should end with .wenget or wenget"
        );
        assert!(paths.installed_json().ends_with("installed.json"));
        assert!(paths.buckets_json().ends_with("buckets.json"));
        assert!(paths.manifest_cache_json().ends_with("manifest-cache.json"));
    }

    #[test]
    fn test_user_paths() {
        let paths = WenPaths::new_user().unwrap();
        assert!(!paths.is_system_install());
        assert!(paths.root().ends_with(".wenget"));
    }

    #[test]
    fn test_system_paths() {
        let paths = WenPaths::new_system();
        assert!(paths.is_system_install());

        #[cfg(unix)]
        {
            assert_eq!(paths.root(), Path::new("/opt/wenget"));
            assert_eq!(paths.bin_dir(), PathBuf::from("/usr/local/bin"));
        }

        #[cfg(windows)]
        {
            // On Windows, verify the path contains "wenget"
            assert!(paths.root().to_string_lossy().contains("wenget"));
        }
    }

    #[test]
    fn test_app_paths() {
        let paths = WenPaths::new_user().unwrap();
        let app_dir = paths.app_dir("test");
        assert!(app_dir.ends_with("apps/test") || app_dir.ends_with("apps\\test"));

        let bin_dir = paths.app_bin_dir("test");
        assert!(bin_dir.ends_with("apps/test/bin") || bin_dir.ends_with("apps\\test\\bin"));
    }

    #[test]
    fn test_executable_name() {
        #[cfg(windows)]
        {
            assert_eq!(WenPaths::executable_name("test"), "test.exe");
        }

        #[cfg(not(windows))]
        {
            assert_eq!(WenPaths::executable_name("test"), "test");
        }
    }

    #[test]
    fn test_bin_shim_path() {
        let paths = WenPaths::new_user().unwrap();
        let shim = paths.bin_shim_path("test");

        #[cfg(windows)]
        {
            assert!(shim.ends_with(".local\\bin\\test.cmd"));
        }

        #[cfg(not(windows))]
        {
            assert!(shim.ends_with(".local/bin/test"));
        }
    }

    #[test]
    fn test_internal_bin_dir() {
        let paths = WenPaths::new_system();
        // internal_bin_dir should always be {root}/bin
        let internal = paths.internal_bin_dir();
        assert!(
            internal.ends_with("bin"),
            "Internal bin dir should end with 'bin'"
        );
    }

    #[test]
    fn test_wenget_root_env_override() {
        // Serialized against other env-mutating tests by running in one test fn.
        let tmp = tempfile::TempDir::new().unwrap();
        let previous = std::env::var_os("WENGET_ROOT");

        std::env::set_var("WENGET_ROOT", tmp.path());
        let paths = WenPaths::new_with_custom_bin(None).unwrap();
        assert_eq!(paths.root(), tmp.path());
        assert_eq!(paths.bin_dir(), tmp.path().join("bin"));

        std::env::remove_var("WENGET_ROOT");
        let default_paths = WenPaths::new_with_custom_bin(None).unwrap();
        assert_ne!(default_paths.root(), tmp.path());

        if let Some(value) = previous {
            std::env::set_var("WENGET_ROOT", value);
        }
    }

    #[test]
    fn test_record_path_helpers() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());

        assert_eq!(
            paths.package_record_path("bun::baseline"),
            tmp.path()
                .join("apps")
                .join("bun-baseline")
                .join(".wenget")
                .join("package.json")
        );
        assert_eq!(
            paths.record_dir("bun::baseline"),
            tmp.path().join("apps").join("bun-baseline").join(".wenget")
        );
        assert_eq!(
            paths.staging_dir(),
            tmp.path().join("apps").join(".staging")
        );
    }
}
