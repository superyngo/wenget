//! Configuration file repair utilities for wenget
//!
//! This module handles intelligent repair/rebuild of corrupted config files:
//! - buckets.json: Reset to empty, user can re-add
//! - manifest-cache.json: Rebuild from buckets

use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

/// Severity level for repair warnings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairSeverity {
    /// Informational - no data loss (cache)
    Info,
    /// Warning - some data may be affected (buckets)
    Warning,
    /// Critical - data loss occurred (installed)
    Critical,
}

/// Type of repair action taken
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum RepairAction {
    /// File was missing, created new
    CreatedNew,
    /// Parse error, reset to empty (with backup path if backed up)
    ResetToEmpty { backup_path: Option<PathBuf> },
    /// Parse error, rebuilt from sources
    Rebuilt { source: String },
    /// File was deleted (will be rebuilt on next access)
    Deleted,
}

impl RepairAction {
    /// Get user-friendly description of the repair action
    pub fn description(&self) -> String {
        match self {
            RepairAction::CreatedNew => "Created new configuration file".to_string(),
            RepairAction::ResetToEmpty {
                backup_path: Some(p),
            } => {
                format!("Reset to empty (backup: {})", p.display())
            }
            RepairAction::ResetToEmpty { backup_path: None } => {
                "Reset to empty configuration".to_string()
            }
            RepairAction::Rebuilt { source } => {
                format!("Will rebuild from {}", source)
            }
            RepairAction::Deleted => "Deleted corrupted file".to_string(),
        }
    }
}

/// Detailed JSON parse error
#[derive(Debug)]
pub struct JsonParseError {
    pub path: PathBuf,
    pub error: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for JsonParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSON parse error in {} at line {}, column {}: {}",
            self.path.display(),
            self.line,
            self.column,
            self.error
        )
    }
}

impl std::error::Error for JsonParseError {}

/// Try to parse JSON, returning a detailed error on failure
pub fn try_parse_json<T: DeserializeOwned>(
    content: &str,
    path: &Path,
) -> Result<T, JsonParseError> {
    serde_json::from_str(content).map_err(|e| JsonParseError {
        path: path.to_path_buf(),
        error: e.to_string(),
        line: e.line(),
        column: e.column(),
    })
}

/// Create a backup of a file before repair
/// Returns the backup path if successful
pub fn create_backup(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        anyhow::bail!("Cannot backup non-existent file: {}", path.display());
    }

    // Generate backup filename with timestamp
    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let file_name = path
        .file_name()
        .context("Invalid file path")?
        .to_string_lossy();
    let backup_name = format!("{}.backup.{}", file_name, timestamp);

    let backup_path = path
        .parent()
        .context("Invalid file path")?
        .join(&backup_name);

    fs::copy(path, &backup_path)
        .with_context(|| format!("Failed to create backup at {}", backup_path.display()))?;

    // Clean up old backups (keep only last 3)
    cleanup_old_backups(path, 3)?;

    Ok(backup_path)
}

/// Clean up old backup files, keeping only the most recent `keep` count
pub fn cleanup_old_backups(original_path: &Path, keep: usize) -> Result<()> {
    let parent = original_path.parent().context("Invalid file path")?;
    let file_name = original_path
        .file_name()
        .context("Invalid file path")?
        .to_string_lossy();

    let pattern = format!("{}.backup.", file_name);

    let mut backups: Vec<_> = fs::read_dir(parent)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with(&pattern))
        .collect();

    // Sort by modification time (oldest first)
    backups.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));

    // Remove oldest backups if we have more than `keep`
    if backups.len() > keep {
        for entry in backups.iter().take(backups.len() - keep) {
            let _ = fs::remove_file(entry.path());
        }
    }

    Ok(())
}

/// Print a repair warning to the user
pub fn print_repair_warning(
    file_name: &str,
    action: &RepairAction,
    severity: RepairSeverity,
    details: Option<&str>,
) {
    let (icon, header) = match severity {
        RepairSeverity::Info => ("i".cyan(), "Configuration repaired:".cyan()),
        RepairSeverity::Warning => ("!".yellow(), "Configuration warning:".yellow()),
        RepairSeverity::Critical => ("!".red().bold(), "Configuration error:".red().bold()),
    };

    eprintln!();
    eprintln!("{} {}", icon, header);
    eprintln!("  File: {}", file_name);
    eprintln!("  Action: {}", action.description());

    if let Some(detail) = details {
        eprintln!("  Details: {}", detail);
    }

    if severity == RepairSeverity::Critical {
        eprintln!();
        eprintln!(
            "{}",
            "  The original file was corrupted. A backup has been created.".yellow()
        );
        if let RepairAction::ResetToEmpty {
            backup_path: Some(p),
        } = action
        {
            eprintln!("  You may manually recover data from: {}", p.display());
        }
    }

    eprintln!();
}

/// Check file status for repair command
#[derive(Debug, Clone)]
pub enum FileStatus {
    /// File is OK and parseable
    Ok,
    /// File is missing
    Missing,
    /// File exists but is corrupted/unparseable
    Corrupted(String),
}

impl std::fmt::Display for FileStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileStatus::Ok => write!(f, "{}", "OK".green()),
            FileStatus::Missing => write!(f, "{}", "Missing".yellow()),
            FileStatus::Corrupted(err) => write!(f, "{} ({})", "Corrupted".red(), err),
        }
    }
}

/// Check if a JSON file is valid
pub fn check_json_file<T: DeserializeOwned>(path: &Path) -> FileStatus {
    if !path.exists() {
        return FileStatus::Missing;
    }

    match fs::read_to_string(path) {
        Ok(content) => match try_parse_json::<T>(&content, path) {
            Ok(_) => FileStatus::Ok,
            Err(e) => FileStatus::Corrupted(format!("line {}, col {}", e.line, e.column)),
        },
        Err(e) => FileStatus::Corrupted(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_backup() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.json");
        fs::write(&file_path, "{}").unwrap();

        let backup = create_backup(&file_path).unwrap();
        assert!(backup.exists());
        assert!(backup.to_string_lossy().contains(".backup."));
    }

    #[test]
    fn test_cleanup_old_backups() {
        let temp = TempDir::new().unwrap();
        let file_path = temp.path().join("test.json");
        fs::write(&file_path, "{}").unwrap();

        // Create multiple backups
        for i in 0..5 {
            let backup = temp.path().join(format!("test.json.backup.{:02}", i));
            fs::write(&backup, "{}").unwrap();
        }

        cleanup_old_backups(&file_path, 3).unwrap();

        // Count remaining backups
        let count = fs::read_dir(temp.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains(".backup.")
            })
            .count();

        assert_eq!(count, 3);
    }

    #[test]
    fn test_try_parse_json_valid() {
        #[derive(serde::Deserialize)]
        struct Test {
            value: i32,
        }

        let content = r#"{"value": 42}"#;
        let result: Result<Test, _> = try_parse_json(content, Path::new("test.json"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().value, 42);
    }

    #[test]
    fn test_try_parse_json_invalid() {
        #[derive(serde::Deserialize)]
        #[allow(dead_code)]
        struct Test {
            value: i32,
        }

        let content = "not valid json {{{";
        let result: Result<Test, _> = try_parse_json(content, Path::new("test.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_check_json_file_missing() {
        #[derive(serde::Deserialize)]
        struct Test {}

        let status = check_json_file::<Test>(Path::new("/nonexistent/file.json"));
        assert!(matches!(status, FileStatus::Missing));
    }

    #[test]
    fn test_repair_action_description() {
        let action = RepairAction::ResetToEmpty {
            backup_path: Some(PathBuf::from("/tmp/backup.json")),
        };
        assert!(action.description().contains("backup"));

        let action = RepairAction::Rebuilt {
            source: "buckets".to_string(),
        };
        assert!(action.description().contains("buckets"));
    }
}
