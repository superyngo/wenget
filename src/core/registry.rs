//! Windows Registry operations for wenget
//!
//! This module provides utilities for modifying the Windows system PATH
//! when running with Administrator privileges.

#[allow(unused_imports)]
use anyhow::{Context, Result};
use std::path::Path;

/// Path modification operation type
#[cfg(windows)]
enum PathOperation {
    Add,
    Remove,
}

/// Core implementation for modifying system PATH
#[cfg(windows)]
fn modify_system_path_inner(path: &Path, operation: PathOperation) -> Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let path_str = path.to_string_lossy().to_string();
    let path_lower = path_str.to_lowercase();

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let env = hklm
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            KEY_READ | KEY_WRITE,
        )
        .context("Failed to open environment registry key. Are you running as Administrator?")?;

    let current_path: String = env
        .get_value("Path")
        .context("Failed to read current PATH")?;

    let path_exists = current_path
        .split(';')
        .any(|p| p.trim().to_lowercase() == path_lower);

    let new_path = match operation {
        PathOperation::Add => {
            if path_exists {
                return Ok(false); // Already in PATH
            }
            if current_path.ends_with(';') {
                format!("{}{}", current_path, path_str)
            } else {
                format!("{};{}", current_path, path_str)
            }
        }
        PathOperation::Remove => {
            if !path_exists {
                return Ok(false); // Not in PATH
            }
            current_path
                .split(';')
                .filter(|p| !p.trim().is_empty() && p.trim().to_lowercase() != path_lower)
                .collect::<Vec<_>>()
                .join(";")
        }
    };

    env.set_value("Path", &new_path)
        .context("Failed to update PATH in registry")?;

    // Notify the system of the change
    broadcast_environment_change();

    Ok(true)
}

/// Add a directory to the system PATH on Windows
///
/// This modifies the system-wide PATH environment variable in the registry.
/// Requires Administrator privileges.
///
/// # Arguments
/// * `path` - The directory path to add to PATH
///
/// # Errors
/// Returns an error if:
/// - Not running with Administrator privileges
/// - Registry access fails
#[cfg(windows)]
pub fn add_to_system_path(path: &Path) -> Result<bool> {
    modify_system_path_inner(path, PathOperation::Add)
}

/// Remove a directory from the system PATH on Windows
///
/// This modifies the system-wide PATH environment variable in the registry.
/// Requires Administrator privileges.
///
/// # Arguments
/// * `path` - The directory path to remove from PATH
///
/// # Errors
/// Returns an error if:
/// - Not running with Administrator privileges
/// - Registry access fails
#[cfg(windows)]
#[allow(dead_code)]
pub fn remove_from_system_path(path: &Path) -> Result<bool> {
    modify_system_path_inner(path, PathOperation::Remove)
}

/// Broadcast a WM_SETTINGCHANGE message to notify other processes of environment change
#[cfg(windows)]
fn broadcast_environment_change() {
    // We use a simple approach here - in a real implementation, you might want to use
    // SendMessageTimeout with HWND_BROADCAST and WM_SETTINGCHANGE
    // For now, we just log that the change was made
    log::debug!("Environment change made. You may need to restart your terminal.");
}

/// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
#[allow(dead_code)]
pub fn add_to_system_path(_path: &Path) -> Result<bool> {
    anyhow::bail!("System PATH modification is only supported on Windows")
}

/// Stub implementation for non-Windows platforms
#[cfg(not(windows))]
#[allow(dead_code)]
pub fn remove_from_system_path(_path: &Path) -> Result<bool> {
    anyhow::bail!("System PATH modification is only supported on Windows")
}

#[cfg(test)]
mod tests {
    // Tests for Windows registry operations would require Administrator privileges
    // and could modify system settings, so we only test the basic structure here.

    #[test]
    fn test_module_compiles() {
        // This test just verifies the module compiles correctly
    }
}
