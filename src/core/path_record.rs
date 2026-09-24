//! Record of the PATH entries `wenget init` added
//!
//! `init` only adds a bin directory that is not already on PATH, and a user may
//! point `custom_bin_path` at a directory they put on PATH themselves. So PATH
//! ownership can't be inferred from the current bin directory: `init` records
//! each entry it actually wrote, and `del self` removes exactly those. With no
//! record, `del self` leaves PATH alone.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::WenPaths;

/// One PATH entry `init` wrote
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PathEntry {
    /// A `# wenget` + `export PATH="{dir}:$PATH"` block appended to a shell rc file
    ShellFile { file: PathBuf, dir: PathBuf },
    /// `dir` appended to the Windows user PATH (registry)
    UserRegistry { dir: PathBuf },
    /// `dir` appended to the Windows system PATH (registry)
    SystemRegistry { dir: PathBuf },
}

/// All PATH entries `init` added, stored at `{root}/path.json`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PathRecord {
    #[serde(default)]
    pub entries: Vec<PathEntry>,
}

impl PathRecord {
    /// Load the record; a missing or unreadable file means nothing was recorded
    pub fn load(paths: &WenPaths) -> Self {
        let path = paths.path_record_json();
        let Ok(content) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&content).unwrap_or_else(|e| {
            log::warn!("Ignoring unreadable {}: {}", path.display(), e);
            Self::default()
        })
    }

    /// Append `entry` (if new) and save
    pub fn add(paths: &WenPaths, entry: PathEntry) -> Result<()> {
        let mut record = Self::load(paths);
        if record.entries.contains(&entry) {
            return Ok(());
        }
        record.entries.push(entry);
        let path = paths.path_record_json();
        fs::write(&path, serde_json::to_string_pretty(&record)?)
            .with_context(|| format!("Failed to write {}", path.display()))
    }
}

/// The exact line `init` appends to a shell rc file for `dir`
pub fn shell_export_line(dir: &Path) -> String {
    format!("export PATH=\"{}:$PATH\"", dir.display())
}

/// Remove the block `init` wrote for `dir`: its exact export line and the
/// `# wenget` comment directly above it. Other lines mentioning `dir` stay.
pub fn strip_shell_block(content: &str, dir: &Path) -> String {
    let export = shell_export_line(dir);
    let mut out: Vec<&str> = Vec::new();
    for line in content.lines() {
        if line.trim() == export {
            if out.last().is_some_and(|l| l.trim() == "# wenget") {
                out.pop();
            }
            // `init` prefixes the block with a blank line
            if out.last().is_some_and(|l| l.trim().is_empty()) {
                out.pop();
            }
            continue;
        }
        out.push(line);
    }
    let mut result = out.join("\n");
    if content.ends_with('\n') && !result.is_empty() {
        result.push('\n');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn strip_removes_only_the_wenget_block() {
        let dir = Path::new("/home/u/bin");
        let content = "alias x=y\n# my own\nexport PATH=\"/home/u/bin:$HOME/go/bin:$PATH\"\n\n# wenget\nexport PATH=\"/home/u/bin:$PATH\"\nalias z=w\n";
        assert_eq!(
            strip_shell_block(content, dir),
            "alias x=y\n# my own\nexport PATH=\"/home/u/bin:$HOME/go/bin:$PATH\"\nalias z=w\n"
        );
        // Nothing of ours: unchanged
        let mine = "export PATH=\"/home/u/bin:$PATH:/x\"\n";
        assert_eq!(strip_shell_block(mine, dir), mine);
    }

    #[test]
    fn record_round_trips_and_dedups() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        assert!(PathRecord::load(&paths).entries.is_empty());
        let e = PathEntry::ShellFile {
            file: tmp.path().join(".zshrc"),
            dir: tmp.path().join("bin"),
        };
        PathRecord::add(&paths, e.clone()).unwrap();
        PathRecord::add(&paths, e.clone()).unwrap();
        assert_eq!(PathRecord::load(&paths).entries, vec![e]);
    }
}
