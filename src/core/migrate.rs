//! Legacy installed-package migration
//!
//! Upgrades package records written by older wenget versions (single
//! `command_name`, `parent_package`, `::` install paths, missing
//! `executables`). Kept apart from the data definitions in `manifest.rs`
//! because it touches the filesystem (renames and scans install dirs).

use std::collections::HashMap;
use std::path::Path;

use crate::core::manifest::InstalledSet;

impl InstalledSet {
    /// Migrate old format (single command_name) to new format (command_names vec)
    /// Also migrates the legacy `parent_package` field to repo_name/variant;
    /// `legacy_parents` maps key -> `parent_package`, read from the raw legacy JSON.
    /// Also migrates install paths with `::` to use `-` separator (for Windows compatibility)
    pub fn migrate(&mut self, legacy_parents: &HashMap<String, String>) {
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
                } else if let Some(parent) = legacy_parents.get(key).filter(|_| key.contains('-')) {
                    // Old format with parent_package
                    package.repo_name = parent.clone();

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

                            // `/`-separated like keys written by fresh installs
                            let rel_path = entry_path
                                .strip_prefix(install_path)
                                .unwrap_or(entry_path)
                                .to_string_lossy()
                                .replace('\\', "/");

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
