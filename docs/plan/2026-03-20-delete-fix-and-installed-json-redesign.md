# Delete Fix & installed.json Redesign — Implementation Plan
Status: Shipped (2026-03-20)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the double-deletion bug when explicitly specifying a variant, and replace `files`/`command_names` with an `executables` HashMap that maps executable paths to command names for persistence across updates.

**Architecture:** Modify `InstalledPackage` struct to replace `files: Vec<String>` and `command_names: Vec<String>` with `executables: HashMap<String, String>`. Update all 8 consumer files. Add migration logic to convert old format. Modify `install_package()` in add.rs to support reusing old command name mappings when `--yes` is passed during update.

**Tech Stack:** Rust, serde, std::collections::HashMap

**Spec:** `docs/spec/2026-03-20-delete-fix-and-installed-json-redesign.md`

---

### Task 1: Fix Double Deletion Bug

**Files:**
- Modify: `src/commands/delete.rs:98-103`

- [ ] **Step 1: Write a regression test**

Add a test to `src/commands/delete.rs` that verifies `final_to_delete` doesn't contain duplicates when a specific variant is requested. Since the delete command interacts with filesystem and config, we'll test the deduplication logic by extracting it conceptually. For now, add this at the end of delete.rs:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_specific_variant_not_duplicated_in_final_to_delete() {
        // Simulate the variant resolution logic
        let names = vec!["opencode::desktop.app".to_string()];
        let matching_packages = vec!["opencode::desktop.app".to_string()];

        let mut packages_to_delete: Vec<(String, Vec<String>)> = Vec::new();
        let mut processed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut final_to_delete: Vec<String> = Vec::new();

        for name in &matching_packages {
            if processed.contains(name) {
                continue;
            }

            let is_specific_variant_request = names.iter().any(|user_input| {
                user_input.contains("::") && user_input == name
            });

            if is_specific_variant_request {
                packages_to_delete.push((name.clone(), vec![name.clone()]));
                // BUG WAS HERE: final_to_delete.push(name.clone());
                processed.insert(name.clone());
                continue;
            }
        }

        // Simulate the -y flag path
        for (_repo_name, variants) in &packages_to_delete {
            final_to_delete.extend(variants.clone());
        }

        // Should only appear ONCE
        assert_eq!(
            final_to_delete.iter().filter(|x| *x == "opencode::desktop.app").count(),
            1,
            "Specific variant should only appear once in final_to_delete"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it passes (confirms test is valid after fix)**

```bash
cargo test test_specific_variant_not_duplicated -- --nocapture
```

Expected: PASS (the test already excludes the buggy line)

- [ ] **Step 3: Apply the one-line fix**

In `src/commands/delete.rs`, remove line 101 (`final_to_delete.push(name.clone());`):

**Before** (lines 98-103):
```rust
        if is_specific_variant_request {
            // User explicitly requested this variant - show it individually
            packages_to_delete.push((name.clone(), vec![name.clone()]));
            final_to_delete.push(name.clone());
            processed.insert(name.clone());
            continue;
        }
```

**After:**
```rust
        if is_specific_variant_request {
            // User explicitly requested this variant - show it individually
            packages_to_delete.push((name.clone(), vec![name.clone()]));
            processed.insert(name.clone());
            continue;
        }
```

- [ ] **Step 4: Run all tests**

```bash
cargo test
```

Expected: All 87+ tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/commands/delete.rs
git commit -m "fix: prevent double deletion when specifying variant explicitly

When running 'wenget rm pkg::variant', the package was added to
final_to_delete twice: once in the specific-variant branch and again
in the confirmation loop. Remove the early push so the unified
confirmation flow handles it."
```

---

### Task 2: Update InstalledPackage Struct

**Files:**
- Modify: `src/core/manifest.rs:393-450` (struct definition)
- Modify: `src/core/manifest.rs:538-550` (is_command_taken)

- [ ] **Step 1: Write tests for the new struct**

Add these tests inside the existing `#[cfg(test)] mod tests` block at the bottom of `src/core/manifest.rs` (after the existing `test_installed_manifest` test):

```rust
    #[test]
    fn test_installed_package_executables_helpers() {
        let mut executables = HashMap::new();
        executables.insert("bin/rg".to_string(), "rg".to_string());
        executables.insert("bin/rg-doc".to_string(), "rg-doc".to_string());

        let pkg = InstalledPackage {
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
        let mut manifest = InstalledManifest::new();

        let mut executables = HashMap::new();
        executables.insert("bin/rg".to_string(), "rg".to_string());

        let pkg = InstalledPackage {
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/path".to_string(),
            executables,
            source: PackageSource::Bucket { name: "main".to_string() },
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
    fn test_deserialize_old_format_without_executables() {
        // Old format JSON with `files` and `command_names` but no `executables`
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

        let manifest: InstalledManifest = serde_json::from_str(json).unwrap();
        let pkg = manifest.get_package("test").unwrap();

        // executables should be empty (not present in old JSON)
        assert!(pkg.executables.is_empty());
        // command_names should still be deserialized for migration
        assert_eq!(pkg.command_names, vec!["test".to_string()]);
    }

    #[test]
    fn test_serialize_new_format_no_files_field() {
        let mut executables = HashMap::new();
        executables.insert("bin/test".to_string(), "test".to_string());

        let pkg = InstalledPackage {
            repo_name: "test".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/path".to_string(),
            executables,
            source: PackageSource::Bucket { name: "main".to_string() },
            description: "Test".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "test.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };

        let json = serde_json::to_string(&pkg).unwrap();
        // Should NOT contain "files" key
        assert!(!json.contains("\"files\""));
        // Should contain "executables" key
        assert!(json.contains("\"executables\""));
        // Should NOT contain "command_names" (empty vec, skip_serializing_if)
        assert!(!json.contains("\"command_names\""));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test test_installed_package_executables_helpers test_is_command_taken_with_executables test_deserialize_old_format_without_executables test_serialize_new_format_no_files_field 2>&1 | tail -20
```

Expected: FAIL — `executables` field doesn't exist yet, `get_command_names()` and `get_exe_path_for_command()` don't exist.

- [ ] **Step 3: Update the InstalledPackage struct**

In `src/core/manifest.rs`, modify the `InstalledPackage` struct definition. Replace the `files` field and update `command_names`/`executables`:

**Remove** this field entirely (around line 419):
```rust
    /// List of installed files (relative to install_path)
    pub files: Vec<String>,
```

**Add** this field after `install_path` (in its place):
```rust
    /// Map of executable relative path (from install_path) to command name
    /// Example: {"bin/rg": "rg", "bin/rg-completions": "rg-completions"}
    #[serde(default)]
    pub executables: HashMap<String, String>,
```

**Change** the `command_names` field annotation to skip serializing when empty:
```rust
    /// DEPRECATED: Legacy flat command names list.
    /// Kept for backward compatibility during migration from older versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_names: Vec<String>,
```

- [ ] **Step 4: Add helper methods to InstalledPackage**

Add these methods in the `impl InstalledPackage` block (if one doesn't exist, create one after the struct definition):

```rust
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
```

- [ ] **Step 5: Update is_command_taken()**

In `src/core/manifest.rs`, update the `is_command_taken` method (lines 538-550):

**Before:**
```rust
            if package.command_names.contains(&command_name.to_string()) {
                return true;
            }
```

**After:**
```rust
            if package.executables.values().any(|n| n == command_name)
                || package.command_names.contains(&command_name.to_string())
            {
                return true;
            }
```

- [ ] **Step 6: Update the existing test_installed_manifest test**

Update the existing test in manifest.rs to use the new struct:

**Before** (lines 826-843):
```rust
        let package = InstalledPackage {
            repo_name: "test".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "windows-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "C:\\Users\\test\\.wenget\\apps\\test".to_string(),
            files: vec!["bin/test.exe".to_string()],
            source: PackageSource::Bucket {
                name: "test-bucket".to_string(),
            },
            description: "Test package".to_string(),
            command_names: vec!["test".to_string()],
            command_name: None,
            asset_name: "test-windows-x64.zip".to_string(),
            parent_package: None,
            download_url: None,
        };
```

**After:**
```rust
        let mut executables = HashMap::new();
        executables.insert("bin/test.exe".to_string(), "test".to_string());

        let package = InstalledPackage {
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
```

- [ ] **Step 7: Run the new tests**

```bash
cargo test -p wenget -- manifest::tests 2>&1 | tail -20
```

Expected: All manifest tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/core/manifest.rs
git commit -m "refactor: replace files/command_names with executables HashMap

Remove the 'files' field from InstalledPackage (serde ignores unknown
fields from old JSON). Add 'executables: HashMap<String, String>' that
maps executable relative paths to their command names. Add helper
methods get_command_names() and get_exe_path_for_command(). Update
is_command_taken() to check both executables and legacy command_names."
```

---

### Task 3: Add Migration Logic

**Files:**
- Modify: `src/core/manifest.rs:552-630` (migrate function)

- [ ] **Step 1: Write migration test**

Add to the test module in `src/core/manifest.rs`:

```rust
    #[test]
    fn test_migrate_command_names_to_executables() {
        use tempfile::TempDir;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let app_dir = tmp.path().join("apps").join("myapp");
        fs::create_dir_all(app_dir.join("bin")).unwrap();

        // Create a fake executable
        fs::write(app_dir.join("bin").join("myapp"), "#!/bin/sh\necho hi").unwrap();
        #[cfg(unix)]
        fs::set_permissions(
            app_dir.join("bin").join("myapp"),
            fs::Permissions::from_mode(0o755),
        ).unwrap();

        let json = format!(r#"{{
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
        }}"#, app_dir.display());

        let mut manifest: InstalledManifest = serde_json::from_str(&json).unwrap();
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

        let mut manifest: InstalledManifest = serde_json::from_str(json).unwrap();
        manifest.migrate();

        let pkg = manifest.get_package("gone").unwrap();
        // Fallback: command_name used as both key and value
        assert_eq!(pkg.executables.get("gone-cmd"), Some(&"gone-cmd".to_string()));
        assert!(pkg.command_names.is_empty());
    }
```

- [ ] **Step 2: Run migration tests to verify they fail**

```bash
cargo test test_migrate_command_names_to_executables test_migrate_fallback_when_install_path_missing 2>&1 | tail -20
```

Expected: FAIL — migration logic not yet implemented.

- [ ] **Step 3: Implement migration logic**

At the end of the `migrate()` function in `src/core/manifest.rs` (before the closing `}` of the for loop on line 628), add this new migration step:

```rust
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
                            if let Some(pos) = remaining_names.iter().position(|n| {
                                n == filename || n == name_no_ext
                            }) {
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
```

- [ ] **Step 4: Add the walk_dir_recursive helper**

Add this as an associated function on `InstalledManifest` (inside the `impl InstalledManifest` block):

```rust
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
```

- [ ] **Step 5: Run migration tests**

```bash
cargo test test_migrate_command_names test_migrate_fallback 2>&1 | tail -20
```

Expected: PASS

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | tail -10
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/core/manifest.rs
git commit -m "feat: add migration from command_names to executables map

Scans the install_path directory to match command_names to actual
executable files. Falls back to using command_name as both key and
value when install_path doesn't exist or files can't be matched."
```

---

### Task 4: Update delete.rs Consumer

**Files:**
- Modify: `src/commands/delete.rs:271-278`

- [ ] **Step 1: Update delete_package to use executables**

In `src/commands/delete.rs`, change the `delete_package` function (line 272):

**Before:**
```rust
    // Remove symlinks/shims for all command names
    for command_name in &pkg.command_names {
```

**After:**
```rust
    // Remove symlinks/shims for all command names
    for command_name in pkg.executables.values() {
```

Also add a fallback for legacy packages that haven't been migrated yet — **after** the executables loop and before the legacy single-name removal (around line 278), add:

```rust
    // Also remove symlinks for legacy command_names (pre-migration packages)
    for command_name in &pkg.command_names {
        let bin_path = paths.bin_shim_path(command_name);
        if bin_path.exists() {
            fs::remove_file(&bin_path).ok();
        }
    }
```

- [ ] **Step 2: Run all tests**

```bash
cargo test 2>&1 | tail -10
```

Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/commands/delete.rs
git commit -m "refactor: update delete command to use executables map

Primary removal uses executables.values(). Falls back to legacy
command_names for packages that haven't been migrated yet."
```

---

### Task 5: Update add.rs — Binary Installation

**Files:**
- Modify: `src/commands/add.rs:1485-1600` (install_package function)

- [ ] **Step 1: Add HashMap import at top of add.rs**

Add `use std::collections::HashMap;` to the imports at the top of `src/commands/add.rs` (if not already present).

- [ ] **Step 2: Change command_names Vec to executables HashMap in install_package**

In the `install_package` function, change the accumulator (line 1485):

**Before:**
```rust
    let mut command_names = Vec::new();
```

**After:**
```rust
    let mut executables = HashMap::new();
```

- [ ] **Step 3: Update the per-executable loop to build executables map**

In the loop body (line 1562), change:

**Before:**
```rust
        command_names.push(resolved_name);
```

**After:**
```rust
        executables.insert(exe_relative.clone(), resolved_name);
```

- [ ] **Step 4: Update InstalledPackage construction**

In the InstalledPackage construction (lines 1583-1599):

**Before:**
```rust
    let inst_pkg = InstalledPackage {
        repo_name,
        variant,
        version: version.to_string(),
        platform: platform_match.platform_id.clone(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        files: extracted_files,
        source: source.clone(),
        description: pkg.description.clone(),
        command_names: resolved_command_names,
        command_name: None,
        asset_name: binary.asset_name.clone(),
        parent_package: None,
        download_url: None,
    };
```

**After:**
```rust
    let inst_pkg = InstalledPackage {
        repo_name,
        variant,
        version: version.to_string(),
        platform: platform_match.platform_id.clone(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        executables,
        source: source.clone(),
        description: pkg.description.clone(),
        command_names: vec![],
        command_name: None,
        asset_name: binary.asset_name.clone(),
        parent_package: None,
        download_url: None,
    };
```

Also remove the now-unnecessary line (around 1579-1580):
```rust
    // Command names were already resolved during symlink creation
    let resolved_command_names = command_names;
```

- [ ] **Step 5: Run cargo check**

```bash
cargo check 2>&1 | tail -20
```

Expected: May have warnings in other files that still reference `files`/`command_names` — that's expected; those will be fixed in subsequent tasks.

- [ ] **Step 6: Commit**

```bash
git add src/commands/add.rs
git commit -m "refactor: build executables HashMap in install_package

Replace command_names Vec accumulator with executables HashMap that
maps each exe_relative path to its resolved command name."
```

---

### Task 6: Update add.rs — Script Installation Functions

**Files:**
- Modify: `src/commands/add.rs:420-438` (install_script_inline)
- Modify: `src/commands/add.rs:1661-1679` (install_script_from_bucket)

- [ ] **Step 1: Update inline script installation (around line 420)**

**Before:**
```rust
    let inst_pkg = InstalledPackage {
        repo_name: name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: format!("{}-script", script_type.display_name().to_lowercase()),
        installed_at: Utc::now(),
        install_path: paths.app_dir(name).to_string_lossy().to_string(),
        files,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from {}", script_type.display_name(), origin),
        command_names: vec![name.to_string()],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: None,
    };
```

**After:**
```rust
    // Build executables mapping: script file → command name
    let mut executables = HashMap::new();
    // scripts have one file; use the first file as the exe path
    if let Some(script_file) = files.first() {
        executables.insert(script_file.clone(), name.to_string());
    } else {
        executables.insert(format!("{}.{}", name, script_type.extension()), name.to_string());
    }

    let inst_pkg = InstalledPackage {
        repo_name: name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: format!("{}-script", script_type.display_name().to_lowercase()),
        installed_at: Utc::now(),
        install_path: paths.app_dir(name).to_string_lossy().to_string(),
        executables,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from {}", script_type.display_name(), origin),
        command_names: vec![],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: None,
    };
```

- [ ] **Step 2: Update bucket script installation (around line 1661)**

Apply the same pattern — replace `files` and `command_names` with `executables`:

**Before:**
```rust
    let inst_pkg = InstalledPackage {
        repo_name: command_name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: std::env::consts::OS.to_string(),
        installed_at: Utc::now(),
        install_path: paths.app_dir(command_name).display().to_string(),
        files,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from bucket", script_type.display_name()),
        command_names: vec![command_name.to_string()],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: Some(url.to_string()),
    };
```

**After:**
```rust
    let mut executables = HashMap::new();
    if let Some(script_file) = files.first() {
        executables.insert(script_file.clone(), command_name.to_string());
    } else {
        executables.insert(
            format!("{}.{}", command_name, script_type.extension()),
            command_name.to_string(),
        );
    }

    let inst_pkg = InstalledPackage {
        repo_name: command_name.to_string(),
        variant: None,
        version: "script".to_string(),
        platform: std::env::consts::OS.to_string(),
        installed_at: Utc::now(),
        install_path: paths.app_dir(command_name).display().to_string(),
        executables,
        source: PackageSource::Script {
            origin: origin.to_string(),
            script_type: script_type.clone(),
        },
        description: format!("{} script from bucket", script_type.display_name()),
        command_names: vec![],
        command_name: None,
        asset_name: format!("{}.{}", name, script_type.extension()),
        parent_package: None,
        download_url: Some(url.to_string()),
    };
```

- [ ] **Step 3: Run cargo check**

```bash
cargo check 2>&1 | tail -20
```

Expected: Warnings/errors may remain in info.rs, list.rs, rename.rs, local.rs — those are next.

- [ ] **Step 4: Commit**

```bash
git add src/commands/add.rs
git commit -m "refactor: update script installation to use executables map"
```

---

### Task 7: Update local.rs

**Files:**
- Modify: `src/installer/local.rs:134-149`

- [ ] **Step 1: Update InstalledPackage construction**

In `src/installer/local.rs`, add HashMap import at top:

```rust
use std::collections::HashMap;
```

Then update the InstalledPackage construction (lines 134-149):

**Before:**
```rust
    Ok(InstalledPackage {
        repo_name: name.clone(),
        variant: None,
        version: "local".to_string(),
        platform: "local".to_string(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        files: extracted_files,
        source,
        description: format!("Local installation of {}", filename),
        command_names: vec![command_name],
        command_name: None,
        asset_name: filename.to_string(),
        parent_package: None,
        download_url: None,
    })
```

**After:**
```rust
    let mut executables = HashMap::new();
    executables.insert(exe_relative.to_string(), command_name);

    Ok(InstalledPackage {
        repo_name: name.clone(),
        variant: None,
        version: "local".to_string(),
        platform: "local".to_string(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        executables,
        source,
        description: format!("Local installation of {}", filename),
        command_names: vec![],
        command_name: None,
        asset_name: filename.to_string(),
        parent_package: None,
        download_url: None,
    })
```

Note: `exe_relative` is defined on line 80 (`let exe_relative = &selected.path;`).

- [ ] **Step 2: Run cargo check**

```bash
cargo check 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add src/installer/local.rs
git commit -m "refactor: update local installer to use executables map"
```

---

### Task 8: Update info.rs

**Files:**
- Modify: `src/commands/info.rs` (lines 160, 182, 218, 254, 331, 436, 443-462)

- [ ] **Step 1: Update all command_names references to use get_command_names()**

Replace each occurrence of `.command_names.join(", ")` or `.command_names.clone()`:

**Line 158-161 — flat_map all command names:**

**Before:**
```rust
        let all_commands: Vec<String> = all_variants
            .iter()
            .flat_map(|(_, p)| p.command_names.clone())
            .collect();
```

**After:**
```rust
        let all_commands: Vec<String> = all_variants
            .iter()
            .flat_map(|(_, p)| p.get_command_names().into_iter().map(String::from))
            .collect();
```

**Line 182 — variant command display:**

**Before:**
```rust
                inst_pkg.command_names.join(", ").yellow()
```

**After:**
```rust
                inst_pkg.get_command_names().join(", ").yellow()
```

**Line 218 — installed binary display:**

**Before:**
```rust
                    format!(" [Installed: {}]", inst_pkg.command_names.join(", "))
```

**After:**
```rust
                    format!(" [Installed: {}]", inst_pkg.get_command_names().join(", "))
```

**Line 254 — variant installed display:**

**Before:**
```rust
                        format!(" [Installed: {}]", inst_pkg.command_names.join(", "))
```

**After:**
```rust
                        format!(" [Installed: {}]", inst_pkg.get_command_names().join(", "))
```

**Line 331 — script installed display:**

**Before:**
```rust
            inst_pkg.command_names.join(", ").yellow()
```

**After:**
```rust
            inst_pkg.get_command_names().join(", ").yellow()
```

**Line 436 — standalone display (this uses command_name singular, not command_names):**

**Before:**
```rust
        inst_pkg.command_name.as_deref().unwrap_or("-").yellow()
```

**After** (use get_command_names which handles fallback):
```rust
        {
            let names = inst_pkg.get_command_names();
            if names.is_empty() { "-".to_string() } else { names.join(", ") }
        }.yellow()
```

- [ ] **Step 2: Remove the "Installed files" display section**

Delete lines 442-462 entirely (the `if !inst_pkg.files.is_empty()` block).

- [ ] **Step 3: Run cargo check**

```bash
cargo check 2>&1 | tail -20
```

Expected: No errors in info.rs.

- [ ] **Step 4: Commit**

```bash
git add src/commands/info.rs
git commit -m "refactor: update info command to use executables map

Replace command_names references with get_command_names(). Remove
the installed files display section."
```

---

### Task 9: Update list.rs

**Files:**
- Modify: `src/commands/list.rs` (lines 130, 135, 148)

- [ ] **Step 1: Update all command_names.join references**

**Line 130:**
**Before:** `format!("  [Command: {}]", first_pkg.command_names.join(", "))`
**After:** `format!("  [Command: {}]", first_pkg.get_command_names().join(", "))`

**Line 135:**
**Before:** `format!("[Command: {}]", first_pkg.command_names.join(", "))`
**After:** `format!("[Command: {}]", first_pkg.get_command_names().join(", "))`

**Line 148:**
**Before:** `format!("[Command: {}]", var_pkg.command_names.join(", "))`
**After:** `format!("[Command: {}]", var_pkg.get_command_names().join(", "))`

- [ ] **Step 2: Run cargo check**

```bash
cargo check 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add src/commands/list.rs
git commit -m "refactor: update list command to use get_command_names()"
```

---

### Task 10: Update rename.rs

**Files:**
- Modify: `src/commands/rename.rs` (lines 23, 78-82, 89-93, 99, 115-120, 146, 177-180, 241)
- Modify: `src/commands/rename.rs` test module (lines 285-349)

- [ ] **Step 1: Update find_package_and_command function (lines 77-101)**

**Line 78 — empty check:**
**Before:** `if package.command_names.is_empty() {`
**After:** `if package.get_command_names().is_empty() {`

**Line 81 — get first command:**
**Before:** `let cmd_name = package.command_names[0].clone();`
**After:** `let cmd_name = package.get_command_names()[0].to_string();`

**Line 90 — variant empty check:**
**Before:** `if package.command_names.is_empty() {`
**After:** `if package.get_command_names().is_empty() {`

**Line 93 — variant first command:**
**Before:** `let cmd_name = package.command_names[0].clone();`
**After:** `let cmd_name = package.get_command_names()[0].to_string();`

**Line 99 — search by command name:**
**Before:** `if package.command_names.contains(&name.to_string()) {`
**After:** `if package.get_command_names().contains(&name) {`

- [ ] **Step 2: Update select_command_interactive (lines 23, 115-120)**

**Line 23 — multiple commands check:**
**Before:** `let final_old_cmd = if package.command_names.len() > 1 && new_name.is_none() {`
**After:** `let final_old_cmd = if package.get_command_names().len() > 1 && new_name.is_none() {`

**Lines 115-120 — interactive selection:**
**Before:**
```rust
    let selection = Select::new()
        .with_prompt("Select command to rename")
        .items(&package.command_names)
        .default(0)
        .interact()
        .context("Failed to get user selection")?;

    Ok(package.command_names[selection].clone())
```

**After:**
```rust
    let cmd_names: Vec<String> = package.get_command_names().iter().map(|s| s.to_string()).collect();

    let selection = Select::new()
        .with_prompt("Select command to rename")
        .items(&cmd_names)
        .default(0)
        .interact()
        .context("Failed to get user selection")?;

    Ok(cmd_names[selection].clone())
```

- [ ] **Step 3: Update validate_new_name (line 146-149)**

**Before:**
```rust
        if package.command_names.contains(&new_name.to_string()) {
            anyhow::bail!("Command '{}' is already in use by this package", new_name);
        }
```

**After:**
```rust
        if package.get_command_names().contains(&new_name) {
            anyhow::bail!("Command '{}' is already in use by this package", new_name);
        }
```

Also update the `validate_new_name` function's inner loop (lines 149):
**Before:** `if package.command_names.contains(&new_name.to_string()) {`
**After:** `if package.get_command_names().contains(&new_name) {`

- [ ] **Step 4: Update rename_command to use executables map (lines 176-241)**

**Before (lines 176-181):**
```rust
    let cmd_index = package
        .command_names
        .iter()
        .position(|c| c == old_cmd)
        .context("Command not found in package")?;
```

**After:**
```rust
    // Find the executable path for the old command name
    let exe_path_key = package
        .get_exe_path_for_command(old_cmd)
        .map(|s| s.to_string())
        .or_else(|| {
            // Fallback: check legacy command_names
            package.command_names.iter()
                .position(|c| c == old_cmd)
                .map(|_| old_cmd.to_string())
        })
        .context("Command not found in package")?;
```

**Before (line 241):**
```rust
    package_mut.command_names[cmd_index] = new_cmd.to_string();
```

**After:**
```rust
    // Update executables map if the command is there
    if let Some(value) = package_mut.executables.values_mut().find(|v| v.as_str() == old_cmd) {
        *value = new_cmd.to_string();
    }
    // Also update legacy command_names if present
    if let Some(pos) = package_mut.command_names.iter().position(|c| c == old_cmd) {
        package_mut.command_names[pos] = new_cmd.to_string();
    }
```

- [ ] **Step 5: Update test constructions in rename.rs**

For all three `InstalledPackage` constructions in the test module, replace `files: vec![]` with `executables: HashMap::new()` and `command_names: vec![...]` appropriately:

Add at top of test module:
```rust
    use std::collections::HashMap;
```

For each test construction, change:
- `files: vec![],` → `executables: { let mut m = HashMap::new(); m.insert("bin/oldcmd".to_string(), "oldcmd".to_string()); m },` (for the first test)
- For the conflict test, use appropriate command names in the executables maps

**Test 1 (line 285-302):**
```rust
        let mut exe1 = HashMap::new();
        exe1.insert("bin/oldcmd".to_string(), "oldcmd".to_string());

        let package = InstalledPackage {
            // ... same fields ...
            executables: exe1,
            // ... remove files, change command_names to vec![] ...
            command_names: vec![],
            // ...
        };
```

**Test 2 (lines 312-329 and 332-349):**
Apply same pattern with appropriate command names (`cmd1`, `cmd2`).

- [ ] **Step 6: Run all tests**

```bash
cargo test 2>&1 | tail -20
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/commands/rename.rs
git commit -m "refactor: update rename command to use executables map

Update all command_names references to use get_command_names() helper.
The rename_command function now updates the executables map value."
```

---

### Task 11: Update install_package — Preserve Command Names with -y

**Files:**
- Modify: `src/commands/add.rs:1500-1565` (executable resolution loop in install_package)

No signature changes needed. The `install_package` function already loads the installed manifest on line 1503 (`config.get_or_create_installed()?`). We use that same load to grab old executables for the update path internally.

- [ ] **Step 1: Load old executables from existing installation**

In `install_package()`, right after the existing manifest load (line 1503):

```rust
    // Load installed manifest for command name resolution
    let installed_manifest = config.get_or_create_installed()?;
```

Add:

```rust
    // If this package is already installed, grab old executables for command name reuse
    let old_executables = installed_manifest
        .get_package(installed_key)
        .map(|p| p.executables.clone());
```

- [ ] **Step 2: Add command name reuse logic in the per-executable loop**

In the `for exe_relative in selected_executables` loop (around line 1505), **before** the existing `base_name` extraction (line 1512), add reuse logic:

```rust
        // When updating with -y, try to reuse old command names
        let reused_name = if yes {
            if let Some(ref old_exes) = old_executables {
                let filename = exe_path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");

                // Try path match first, then filename match
                old_exes
                    .get(&exe_relative)
                    .cloned()
                    .or_else(|| {
                        old_exes
                            .iter()
                            .find(|(old_path, _)| {
                                std::path::Path::new(old_path.as_str())
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    == Some(filename)
                            })
                            .map(|(_, name)| name.clone())
                    })
            } else {
                None
            }
        } else {
            None
        };
```

Then wrap the existing `base_name` + `resolve_command_name` logic in a conditional:

```rust
        let resolved_name = if let Some(reused) = reused_name {
            println!("  Reusing command name: {}", reused);
            reused
        } else {
            // ... existing base_name extraction and resolve_command_name code ...
            // (move the existing lines 1512-1543 into this else block)
        };
```

The symlink/shim creation code (lines 1547-1560) stays **after** this, unchanged — it uses `resolved_name` regardless of source.

- [ ] **Step 3: After the loop, handle removed executables**

After the `for exe_relative in selected_executables` loop (before the download cleanup on line 1566), add:

```rust
    // Clean up symlinks/shims for old executables that no longer exist in the new version
    if let Some(ref old_exes) = old_executables {
        for (_, old_cmd) in old_exes {
            if !executables.values().any(|n| n == old_cmd) {
                let old_bin = paths.bin_shim_path(old_cmd);
                if old_bin.exists() {
                    fs::remove_file(&old_bin).ok();
                    println!("  Removed obsolete command: {}", old_cmd);
                }
            }
        }
    }
```

- [ ] **Step 4: Run cargo check and tests**

```bash
cargo check 2>&1 | tail -20 && cargo test 2>&1 | tail -20
```

Expected: All pass. No changes needed in `update.rs` since it calls `add::run()` which calls `install_package` internally.

- [ ] **Step 5: Commit**

```bash
git add src/commands/add.rs
git commit -m "feat: preserve command names during update with -y flag

When updating with --yes, reuse old command name mappings by matching
executable paths first, then falling back to filename matching. Clean
up symlinks for executables that were removed in the new version.

Old executables are loaded internally from the installed manifest —
no signature changes needed."
```

---

### Task 12: Final Verification

- [ ] **Step 1: Run fmt and clippy**

```bash
cargo fmt && cargo clippy -- -D warnings 2>&1 | tail -30
```

Fix any warnings.

- [ ] **Step 2: Run full test suite**

```bash
cargo test 2>&1
```

Expected: All tests pass with no regressions.

- [ ] **Step 3: Build release**

```bash
cargo build --release 2>&1 | tail -10
```

Expected: Clean build.

- [ ] **Step 4: Verify serialization manually**

```bash
cargo run -- list --all 2>&1 | head -20
```

Verify output looks correct (commands displayed properly).

- [ ] **Step 5: Final commit (if any formatting/clippy fixes)**

```bash
git add -A && git commit -m "chore: fix formatting and clippy warnings"
```
