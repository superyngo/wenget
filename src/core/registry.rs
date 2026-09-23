//! Windows Registry operations for wenget
//!
//! This module provides utilities for modifying the Windows system PATH
//! when running with Administrator privileges.

#[allow(unused_imports)]
use anyhow::{Context, Result};
use std::path::Path;

/// Path modification operation type
#[cfg_attr(not(windows), allow(dead_code))]
enum PathOperation {
    Add,
    Remove,
}

/// Compute the new PATH string, or `None` when no change is needed
#[cfg_attr(not(windows), allow(dead_code))]
fn compute_new_path(
    current_path: &str,
    path_str: &str,
    operation: PathOperation,
) -> Option<String> {
    let path_lower = path_str.to_lowercase();
    let path_exists = current_path
        .split(';')
        .any(|p| p.trim().to_lowercase() == path_lower);

    match operation {
        PathOperation::Add => {
            if path_exists {
                return None; // Already in PATH
            }
            if current_path.is_empty() || current_path.ends_with(';') {
                Some(format!("{}{}", current_path, path_str))
            } else {
                Some(format!("{};{}", current_path, path_str))
            }
        }
        PathOperation::Remove => {
            if !path_exists {
                return None; // Not in PATH
            }
            Some(
                current_path
                    .split(';')
                    .filter(|p| !p.trim().is_empty() && p.trim().to_lowercase() != path_lower)
                    .collect::<Vec<_>>()
                    .join(";"),
            )
        }
    }
}

/// Modify a PATH-style value under `key`, preserving its registry type
///
/// The system `Path` is normally `REG_EXPAND_SZ`; writing it back as `REG_SZ`
/// would stop entries such as `%SystemRoot%\system32` from expanding.
#[cfg(windows)]
fn modify_path_value(
    key: &winreg::RegKey,
    name: &str,
    path: &Path,
    operation: PathOperation,
) -> Result<bool> {
    use winreg::enums::*;
    use winreg::RegValue;

    let vtype = key
        .get_raw_value(name)
        .map(|v| v.vtype)
        .context("Failed to read current PATH")?;
    let vtype = match vtype {
        REG_SZ | REG_EXPAND_SZ => vtype,
        _ => REG_EXPAND_SZ,
    };
    let current_path: String = key.get_value(name).context("Failed to read current PATH")?;

    let path_str = path.to_string_lossy();
    let Some(new_path) = compute_new_path(&current_path, &path_str, operation) else {
        return Ok(false);
    };

    let bytes: Vec<u8> = new_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|u| u.to_le_bytes())
        .collect();
    key.set_raw_value(name, &RegValue { bytes, vtype })
        .context("Failed to update PATH in registry")?;

    Ok(true)
}

/// Core implementation for modifying system PATH
#[cfg(windows)]
fn modify_system_path_inner(path: &Path, operation: PathOperation) -> Result<bool> {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let env = hklm
        .open_subkey_with_flags(
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            KEY_READ | KEY_WRITE,
        )
        .context("Failed to open environment registry key. Are you running as Administrator?")?;

    let changed = modify_path_value(&env, "Path", path, operation)?;
    if changed {
        // Notify the system of the change
        broadcast_environment_change();
    }
    Ok(changed)
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
    use super::*;

    #[test]
    fn test_compute_new_path() {
        assert_eq!(
            compute_new_path("C:\\a;C:\\b", "C:\\w", PathOperation::Add).as_deref(),
            Some("C:\\a;C:\\b;C:\\w")
        );
        assert_eq!(
            compute_new_path("C:\\a;C:\\W", "c:\\w", PathOperation::Add),
            None
        );
        assert_eq!(
            compute_new_path("C:\\a;C:\\w;", "C:\\W", PathOperation::Remove).as_deref(),
            Some("C:\\a")
        );
        assert_eq!(
            compute_new_path("C:\\a", "C:\\w", PathOperation::Remove),
            None
        );
    }

    /// Writes under a scratch HKCU key (no admin needed) and checks the type survives
    #[cfg(windows)]
    #[test]
    fn test_modify_path_value_preserves_expand_sz() {
        use winreg::enums::*;
        use winreg::{RegKey, RegValue};

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let sub = format!("Software\\wenget-test-{}", std::process::id());
        let (key, _) = hkcu.create_subkey(&sub).unwrap();
        let orig = "%SystemRoot%\\system32";
        let bytes: Vec<u8> = orig
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(|u| u.to_le_bytes())
            .collect();
        key.set_raw_value(
            "Path",
            &RegValue {
                bytes,
                vtype: REG_EXPAND_SZ,
            },
        )
        .unwrap();

        let changed = modify_path_value(
            &key,
            "Path",
            Path::new("C:\\wenget\\bin"),
            PathOperation::Add,
        );
        let raw = key.get_raw_value("Path").unwrap();
        let value: String = key.get_value("Path").unwrap();
        hkcu.delete_subkey_all(&sub).unwrap();

        assert!(changed.unwrap());
        assert_eq!(raw.vtype, REG_EXPAND_SZ);
        assert_eq!(value, "%SystemRoot%\\system32;C:\\wenget\\bin");
    }
}
