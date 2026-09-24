//! Script installation module for wenget
//!
//! This module handles:
//! - Script type detection (by extension and shebang)
//! - Platform compatibility checking
//! - Script installation and shim creation

use crate::core::manifest::{ScriptItem, ScriptPlatform, ScriptType};
use crate::core::WenPaths;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Cached PowerShell command detection result (Windows only)
#[cfg(windows)]
use std::sync::OnceLock;

#[cfg(windows)]
static POWERSHELL_CMD: OnceLock<&'static str> = OnceLock::new();

/// Get the best available PowerShell command.
///
/// On Windows, this checks if `pwsh` (PowerShell Core) is available and uses it
/// if so, otherwise falls back to `powershell` (Windows PowerShell).
///
/// The result is cached using OnceLock for efficiency.
#[cfg(windows)]
pub fn get_powershell_command() -> &'static str {
    POWERSHELL_CMD.get_or_init(|| {
        if std::process::Command::new("pwsh")
            .arg("--version")
            .output()
            .is_ok()
        {
            "pwsh"
        } else {
            "powershell"
        }
    })
}

/// Get the PowerShell command on Unix systems.
///
/// On Unix, PowerShell Core (pwsh) must be installed, so we always use "pwsh".
#[cfg(not(windows))]
#[cfg_attr(not(test), allow(dead_code))]
pub fn get_powershell_command() -> &'static str {
    "pwsh"
}

/// Cached interpreter availability results
static INTERPRETER_CACHE: std::sync::OnceLock<InterpreterCache> = std::sync::OnceLock::new();

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

/// Check if this script type is supported on the current platform.
///
/// This checks if the required interpreter is actually available on the system.
/// Results are cached for performance.
pub fn is_interpreter_available(script_type: &ScriptType) -> bool {
    let cache = get_interpreter_cache();

    match script_type {
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

/// Get the best installable script for the current platform (checks if interpreter exists)
///
/// This is more thorough than `ScriptItem::get_compatible_script()` as it actually checks
/// if the required interpreter is installed on the system.
///
/// Returns the script type and its platform info if an installable one is found.
pub fn installable_script(script: &ScriptItem) -> Option<(ScriptType, &ScriptPlatform)> {
    for script_type in ScriptType::preference_order() {
        if is_interpreter_available(script_type) {
            if let Some(platform) = script.platforms.get(script_type) {
                return Some((script_type.clone(), platform));
            }
        }
    }
    None
}

/// Detect script type from file extension
pub fn detect_script_type_from_extension(filename: &str) -> Option<ScriptType> {
    let filename_lower = filename.to_lowercase();

    if filename_lower.ends_with(".ps1") {
        Some(ScriptType::PowerShell)
    } else if filename_lower.ends_with(".bat") || filename_lower.ends_with(".cmd") {
        Some(ScriptType::Batch)
    } else if filename_lower.ends_with(".sh") {
        Some(ScriptType::Bash)
    } else if filename_lower.ends_with(".py") {
        Some(ScriptType::Python)
    } else {
        None
    }
}

/// Detect script type from shebang line
pub fn detect_script_type_from_shebang(content: &str) -> Option<ScriptType> {
    let first_line = content.lines().next()?;
    let first_line = first_line.trim();

    if !first_line.starts_with("#!") {
        return None;
    }

    let shebang = first_line.to_lowercase();

    if shebang.contains("bash") || shebang.contains("/sh") {
        Some(ScriptType::Bash)
    } else if shebang.contains("python") {
        Some(ScriptType::Python)
    } else if shebang.contains("pwsh") || shebang.contains("powershell") {
        Some(ScriptType::PowerShell)
    } else {
        None
    }
}

/// Detect script type from filename and content
pub fn detect_script_type(filename: &str, content: &str) -> Option<ScriptType> {
    // First try extension
    if let Some(script_type) = detect_script_type_from_extension(filename) {
        return Some(script_type);
    }

    // Then try shebang
    detect_script_type_from_shebang(content)
}

/// Check if the input looks like a script (local file or URL)
pub fn is_script_input(input: &str) -> bool {
    // Check if it's a local file with script extension
    let script_extensions = [".ps1", ".bat", ".cmd", ".sh", ".py"];
    let input_lower = input.to_lowercase();

    if script_extensions
        .iter()
        .any(|ext| input_lower.ends_with(ext))
    {
        return true;
    }

    // Check if it's a raw content URL (GitHub raw, pastebin, etc.)
    if input.starts_with("http://") || input.starts_with("https://") {
        // Common raw content hosts
        let raw_hosts = [
            "raw.githubusercontent.com",
            "gist.githubusercontent.com",
            "pastebin.com/raw",
            "paste.rs",
        ];

        if raw_hosts.iter().any(|host| input.contains(host)) {
            return true;
        }

        // Check URL path for script extensions
        if script_extensions
            .iter()
            .any(|ext| input_lower.ends_with(ext))
        {
            return true;
        }
    }

    false
}

/// Extract script name from file path or URL
pub fn extract_script_name(input: &str) -> Option<String> {
    // Get the filename from path or URL
    let filename = if input.starts_with("http://") || input.starts_with("https://") {
        // Parse URL to get filename
        input.split('/').next_back()?
    } else {
        // Local file path
        Path::new(input).file_name()?.to_str()?
    };

    // Remove query string if present (for URLs)
    let filename = filename.split('?').next()?;

    // Remove extension to get name
    let name = Path::new(filename).file_stem()?.to_str()?;

    // Sanitize name (remove special characters)
    let sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

/// Download script content from URL
pub fn download_script(url: &str) -> Result<String> {
    use crate::utils::HttpClient;

    crate::downloader::warn_if_plaintext(url);
    let client = HttpClient::new()?;
    let content = client
        .get_text(url)
        .with_context(|| format!("Failed to download script from {}", url))?;

    Ok(content)
}

/// Read script content from local file
pub fn read_local_script(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .with_context(|| format!("Failed to read script from {}", path.display()))
}

/// Install a script to the apps directory
pub fn install_script(
    paths: &WenPaths,
    name: &str,
    content: &str,
    script_type: &ScriptType,
) -> Result<Vec<String>> {
    crate::core::InstalledStore::new(paths.clone()).ensure_dir_available(name)?;

    // Stage, then swap: an interrupted write cannot leave a half-written script
    // in place of a working one.
    let staged = crate::installer::StagedInstall::begin(paths, name)?;

    // Determine script filename
    let script_filename = format!("{}.{}", name, script_type.extension());
    let staged_script = staged.path().join(&script_filename);

    // Write script content
    fs::write(&staged_script, content)
        .with_context(|| format!("Failed to write script: {}", staged_script.display()))?;

    let app_dir = staged.commit()?;
    #[cfg(not(unix))]
    let _ = app_dir;

    // Make script executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script_path = app_dir.join(&script_filename);
        let mut perms = fs::metadata(&script_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms)?;
    }

    Ok(vec![script_filename])
}

/// Create a shim for a script
pub fn create_script_shim(paths: &WenPaths, name: &str, script_type: &ScriptType) -> Result<()> {
    let app_dir = paths.app_dir(name);
    let script_filename = format!("{}.{}", name, script_type.extension());
    create_script_launcher(paths, name, &app_dir.join(&script_filename), script_type)
}

/// Create a launcher named `cmd_name` for the script at `script_path`
pub fn create_script_launcher(
    paths: &WenPaths,
    cmd_name: &str,
    script_path: &Path,
    script_type: &ScriptType,
) -> Result<()> {
    #[cfg(windows)]
    {
        create_script_shim_windows(paths, cmd_name, script_path, script_type)?;
    }

    #[cfg(unix)]
    {
        create_script_shim_unix(paths, cmd_name, script_path, script_type)?;
    }

    Ok(())
}

/// Escape text placed inside a double-quoted string in a `.cmd` file
///
/// Inside quotes cmd treats `& | < > ^` literally, so only `%` (variable
/// expansion) must be doubled; a `"` cannot occur in a Windows path.
#[cfg(any(windows, test))]
pub(crate) fn escape_cmd_quoted(text: &str) -> String {
    text.replace('%', "%%")
}

/// Quote `text` as one POSIX shell word
#[cfg(any(unix, test))]
pub(crate) fn sh_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod quoting_tests {
    use super::*;

    #[test]
    fn test_escape_cmd_quoted() {
        assert_eq!(
            escape_cmd_quoted(r"..\apps\a&b^c\x.exe"),
            r"..\apps\a&b^c\x.exe"
        );
        assert_eq!(
            escape_cmd_quoted(r"..\apps\100%\x.exe"),
            r"..\apps\100%%\x.exe"
        );
    }

    #[test]
    fn test_sh_single_quote() {
        assert_eq!(sh_single_quote("/a b/$x`y`\\z"), "'/a b/$x`y`\\z'");
        assert_eq!(sh_single_quote("/it's"), r"'/it'\''s'");
    }
}

/// Create script shim on Windows
#[cfg(windows)]
fn create_script_shim_windows(
    paths: &WenPaths,
    name: &str,
    script_path: &Path,
    script_type: &ScriptType,
) -> Result<()> {
    let shim_path = paths.bin_dir().join(format!("{}.cmd", name));

    // Calculate relative path from shim to script
    let relative_path = pathdiff::diff_paths(script_path, paths.bin_dir())
        .context("Failed to calculate relative path")?;
    let relative_path_str = relative_path.display().to_string().replace('/', "\\");

    // Escape for use inside the quoted "%~dp0..." argument
    let escaped_path = escape_cmd_quoted(&relative_path_str);

    let shim_content = match script_type {
        ScriptType::PowerShell => {
            // Note: -ExecutionPolicy Bypass is standard practice for package managers (like Scoop)
            // to ensure scripts can run regardless of system policy settings
            let ps_cmd = get_powershell_command();
            format!(
                "@echo off\r\n{} -NoProfile -ExecutionPolicy Bypass -File \"%~dp0{}\" %*\r\n",
                ps_cmd, escaped_path
            )
        }
        ScriptType::Batch => {
            format!("@echo off\r\ncall \"%~dp0{}\" %*\r\n", escaped_path)
        }
        ScriptType::Bash => {
            format!("@echo off\r\nbash \"%~dp0{}\" %*\r\n", escaped_path)
        }
        ScriptType::Python => {
            format!("@echo off\r\npython \"%~dp0{}\" %*\r\n", escaped_path)
        }
    };

    // Ensure bin directory exists
    fs::create_dir_all(paths.bin_dir())?;

    fs::write(&shim_path, shim_content)
        .with_context(|| format!("Failed to create shim: {}", shim_path.display()))?;

    Ok(())
}

/// Create script shim on Unix
#[cfg(unix)]
fn create_script_shim_unix(
    paths: &WenPaths,
    name: &str,
    script_path: &Path,
    script_type: &ScriptType,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let shim_path = paths.bin_dir().join(name);

    // For bash scripts, we can create a symlink directly
    // For other types, we create a wrapper script
    match script_type {
        ScriptType::Bash => {
            // Remove existing shim if any
            if shim_path.exists() {
                fs::remove_file(&shim_path)?;
            }

            // Create symlink
            std::os::unix::fs::symlink(script_path, &shim_path)
                .with_context(|| format!("Failed to create symlink: {}", shim_path.display()))?;
        }
        _ => {
            // Create wrapper script
            let wrapper_content = match script_type {
                ScriptType::PowerShell => {
                    format!(
                        "#!/bin/sh\nexec pwsh -NoProfile -File {} \"$@\"\n",
                        sh_single_quote(&script_path.display().to_string())
                    )
                }
                ScriptType::Python => {
                    format!(
                        "#!/bin/sh\nexec python3 {} \"$@\"\n",
                        sh_single_quote(&script_path.display().to_string())
                    )
                }
                ScriptType::Batch => {
                    // Batch scripts don't work on Unix, but we provide a placeholder
                    "#!/bin/sh\necho 'Batch scripts are not supported on this platform'\nexit 1\n"
                        .to_string()
                }
                // Note: Bash is handled in the outer match arm (line 336) with a symlink,
                // so this branch is unreachable. We need this arm to satisfy exhaustiveness.
                ScriptType::Bash => unreachable!("Bash scripts are handled above via symlink"),
            };

            fs::write(&shim_path, wrapper_content)
                .with_context(|| format!("Failed to create wrapper: {}", shim_path.display()))?;

            // Make executable
            let mut perms = fs::metadata(&shim_path)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&shim_path, perms)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_script_type_from_extension() {
        assert_eq!(
            detect_script_type_from_extension("script.ps1"),
            Some(ScriptType::PowerShell)
        );
        assert_eq!(
            detect_script_type_from_extension("script.bat"),
            Some(ScriptType::Batch)
        );
        assert_eq!(
            detect_script_type_from_extension("script.cmd"),
            Some(ScriptType::Batch)
        );
        assert_eq!(
            detect_script_type_from_extension("script.sh"),
            Some(ScriptType::Bash)
        );
        assert_eq!(
            detect_script_type_from_extension("script.py"),
            Some(ScriptType::Python)
        );
        assert_eq!(detect_script_type_from_extension("script.txt"), None);
    }

    #[test]
    fn test_detect_script_type_from_shebang() {
        assert_eq!(
            detect_script_type_from_shebang("#!/bin/bash\necho hello"),
            Some(ScriptType::Bash)
        );
        assert_eq!(
            detect_script_type_from_shebang("#!/usr/bin/env python3\nprint('hello')"),
            Some(ScriptType::Python)
        );
        assert_eq!(
            detect_script_type_from_shebang("#!/usr/bin/env pwsh\nWrite-Host 'hello'"),
            Some(ScriptType::PowerShell)
        );
        assert_eq!(detect_script_type_from_shebang("echo hello"), None);
    }

    #[test]
    fn test_is_script_input() {
        assert!(is_script_input("script.ps1"));
        assert!(is_script_input("./script.sh"));
        assert!(is_script_input("C:\\scripts\\tool.bat"));
        assert!(is_script_input(
            "https://raw.githubusercontent.com/user/repo/main/script.sh"
        ));
        assert!(!is_script_input("https://github.com/user/repo"));
        assert!(!is_script_input("ripgrep"));
    }

    #[test]
    fn test_extract_script_name() {
        assert_eq!(
            extract_script_name("script.ps1"),
            Some("script".to_string())
        );
        assert_eq!(
            extract_script_name("./my-tool.sh"),
            Some("my-tool".to_string())
        );
        assert_eq!(
            extract_script_name("https://example.com/path/to/script.py"),
            Some("script".to_string())
        );
        assert_eq!(
            extract_script_name("https://example.com/script.sh?token=abc"),
            Some("script".to_string())
        );
    }

    #[test]
    fn test_get_powershell_command() {
        // Test that the function returns a valid PowerShell command
        let ps_cmd = get_powershell_command();

        // Should be either "pwsh" or "powershell"
        assert!(
            ps_cmd == "pwsh" || ps_cmd == "powershell",
            "Expected 'pwsh' or 'powershell', got '{}'",
            ps_cmd
        );

        // Test that calling it again returns the same cached result
        let ps_cmd_again = get_powershell_command();
        assert_eq!(ps_cmd, ps_cmd_again, "PowerShell command should be cached");
    }
}
