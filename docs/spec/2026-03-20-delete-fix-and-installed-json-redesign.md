# Delete Fix & installed.json Redesign
Status: Shipped (2026-03-20)

## Problem Statement

Two issues exist in the current Wenget codebase:

1. **Double deletion bug**: When running `wenget rm opencode::desktop.app` (explicitly specifying a variant with `::`), the package is attempted to be deleted twice — once successfully, once failing with "not found". This results in confusing output showing both success and failure for the same package.

2. **installed.json structure limitations**:
   - The `files` field stores every extracted file path but is never used functionally (delete removes the entire directory; info display will be removed).
   - The `command_names` field is a flat `Vec<String>` with no mapping back to which executable each command name corresponds to. During `wenget update`, the package is reinstalled from scratch via `add::run()`, causing command names to be re-detected — any user customizations via `wenget rename` are lost.

## Changes

### 1. Fix Double Deletion Bug

**File**: `src/commands/delete.rs`

**Root cause**: In the variant resolution loop (lines 73-144), when `is_specific_variant_request` is true, the code pushes to `final_to_delete` at line 101. Later, the confirmation loop (lines 162-208) processes `packages_to_delete` and pushes to `final_to_delete` again — both the single-variant auto-add path (line 167) and the `-y` flag path (line 206) duplicate the entry.

**Fix**: Remove `final_to_delete.push(name.clone())` from line 101. The specific-variant case should only add to `packages_to_delete` and `processed`, then let the unified confirmation flow handle `final_to_delete` population. This is a one-line removal.

**Before** (lines 98-103):
```rust
if is_specific_variant_request {
    packages_to_delete.push((name.clone(), vec![name.clone()]));
    final_to_delete.push(name.clone()); // BUG: causes double deletion
    processed.insert(name.clone());
    continue;
}
```

**After**:
```rust
if is_specific_variant_request {
    packages_to_delete.push((name.clone(), vec![name.clone()]));
    processed.insert(name.clone());
    continue;
}
```

### 2. Replace `files` and `command_names` with `executables` Map

**File**: `src/core/manifest.rs`

**Current structure** (relevant fields of `InstalledPackage`):
```rust
pub files: Vec<String>,           // All extracted file paths
pub command_names: Vec<String>,   // Flat list of command names
pub command_name: Option<String>, // Legacy single name (migration)
```

**New structure**:
```rust
/// Map of executable relative path (from install_path) to command name.
/// Example: {"bin/rg": "rg", "bin/rg-completions": "rg-completions"}
#[serde(default)]
pub executables: HashMap<String, String>,

/// DEPRECATED: Legacy flat command names list.
/// Kept for backward compatibility during migration.
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub command_names: Vec<String>,

/// DEPRECATED: Legacy single command name.
/// Kept for backward compatibility during migration.
#[serde(skip_serializing_if = "Option::is_none")]
pub command_name: Option<String>,
```

The `files` field is **removed entirely**. Serde ignores unknown fields by default (no `deny_unknown_fields` is present), so old JSON files with `files` present will deserialize without error. The `#[serde(default)]` on `executables` ensures old JSON files missing the `executables` key get an empty HashMap.

#### Helper Methods

Add to `InstalledPackage`:
```rust
/// Get all command names from the executables map
pub fn get_command_names(&self) -> Vec<&str> {
    self.executables.values().map(|s| s.as_str()).collect()
}

/// Get the executable path for a given command name
pub fn get_exe_path_for_command(&self, command_name: &str) -> Option<&str> {
    self.executables.iter()
        .find(|(_, name)| name.as_str() == command_name)
        .map(|(path, _)| path.as_str())
}
```

#### Migration Logic

In `InstalledManifest::migrate()`, add a new migration step:

```
For each package where executables is empty but command_names is not empty:
  1. Try to match each command_name to a file in the install_path directory
     by scanning the actual filesystem for executable files.
     - Match by exact filename first (command_name == filename without extension)
     - Also match by stripping common extensions (.sh, .ps1, .bat, .py, .exe)
       to handle scripts where command_name="foo" maps to file "foo.sh"
  2. For each match found: insert (relative_exe_path → command_name) into executables.
  3. For unmatched command_names: insert (command_name → command_name) as a
     placeholder mapping (the key is the command name itself since we don't
     know the real path).
  4. Clear command_names and command_name after migration.
```

If the install_path doesn't exist (package was deleted externally), use the command_name as both key and value as a fallback.

**Note on scripts**: Script packages (source = `Script { .. }`) typically have one command whose name differs from the file (e.g., command `foo` → file `foo.sh`). The extension-stripping match in step 1 handles this case.

### 3. Update All Consumers of `files` and `command_names`

Every place that reads `pkg.files` or `pkg.command_names` must be updated:

#### `src/core/manifest.rs` — `is_command_taken()`

**Before**:
```rust
if package.command_names.contains(&command_name.to_string()) {
```

**After**:
```rust
if package.executables.values().any(|n| n == command_name) {
```

#### `src/commands/delete.rs` — `delete_package()`

**Before**:
```rust
for command_name in &pkg.command_names { ... }
```

**After**:
```rust
for command_name in pkg.executables.values() { ... }
```

#### `src/commands/add.rs` — `install_package()`

**Before**:
```rust
let mut command_names = Vec::new();
// ... for each exe ...
command_names.push(resolved_name);
// ...
InstalledPackage {
    files: extracted_files,
    command_names: resolved_command_names,
    ...
}
```

**After**:
```rust
let mut executables = HashMap::new();
// ... for each exe_relative and resolved_name ...
executables.insert(exe_relative.clone(), resolved_name);
// ...
InstalledPackage {
    executables,
    command_names: Vec::new(), // deprecated, kept empty
    ...
}
```

#### `src/commands/info.rs`

Remove the "Installed files" display section. Update command name display to use `pkg.executables.values()`.

#### `src/commands/list.rs`

Update to use `pkg.executables.values()` instead of `pkg.command_names`.

#### `src/commands/rename.rs`

Update to work with `executables` map — when renaming, find the entry by old command name value, update the value to the new name, and update the symlink/shim.

#### `src/installer/local.rs`

Update `InstalledPackage` construction to use `executables` instead of `files` + `command_names`.

### 4. Update Behavior During `wenget update`

The update command calls `add::run()` to reinstall. The add command's `install_package()` function needs to handle the case where a package is already installed.

**When `-y` flag is present** (auto-mode):

After extracting the new archive and running `find_executable_candidates()`:

1. Load the old `InstalledPackage` from the manifest.
2. Build a mapping from old executables:
   - `old_by_path`: `HashMap<&str, &str>` (path → command_name)
   - `old_by_filename`: `HashMap<&str, &str>` (filename → command_name)
3. For each new executable candidate:
   - If its relative path exists in `old_by_path` → reuse the old command name
   - Else if its filename exists in `old_by_filename` → reuse the old command name
   - Else → generate a new command name using the normal resolution flow
4. For old executables not matched by any new executable → remove their symlinks/shims (the executable was removed in the new version)

**When `-y` flag is NOT present** (interactive mode):

Maintain the current interactive flow — user selects executables and confirms command names. No automatic mapping applied.

**Script packages** (source = `Script { .. }`):

Script packages have a single command name and are reinstalled via the same `add::run()` path. Since scripts have exactly one executable, the mapping is trivial: the old command name is preserved by matching the single entry. No special handling needed beyond the binary logic above.

### 5. Serde Handling for Backward Compatibility

To cleanly handle the removal of `files`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackage {
    // ... active fields ...

    /// IGNORED during deserialization; not written during serialization.
    /// Previously stored all extracted file paths.
    #[serde(default, skip_serializing)]
    _files_compat: Vec<String>,
}
```

Alternatively, since serde by default ignores unknown fields, simply removing the `files` field from the struct is sufficient — old JSON files will deserialize without error, and new serializations won't include it. **We'll use this simpler approach** and add `#[serde(deny_unknown_fields)]` is NOT present (confirm this).

## Files to Modify

| File | Changes |
|------|---------|
| `src/core/manifest.rs` | Replace `files` + `command_names` with `executables` HashMap; add helper methods; update `is_command_taken()` to use `executables.values()`; update migration; update test `InstalledPackage` construction |
| `src/commands/delete.rs` | Fix double-deletion bug (remove line 101); update to use `executables.values()` |
| `src/commands/add.rs` | Build `executables` HashMap during install; add `-y` mapping logic for updates |
| `src/commands/update.rs` | Pass old package info to add command for mapping preservation |
| `src/commands/info.rs` | Remove files display; use `executables` |
| `src/commands/list.rs` | Use `executables.values()` |
| `src/commands/rename.rs` | Update to modify `executables` map values; update test `InstalledPackage` construction |
| `src/installer/local.rs` | Use `executables` in InstalledPackage construction |

## Testing

- **Delete bug**: Test that `wenget rm pkg::variant` with explicit variant only deletes once
- **Migration (binaries)**: Test that old installed.json with `files` + `command_names` migrates correctly to `executables`
- **Migration (scripts)**: Test that a script package with `command_names: ["foo"]` and install_path containing `foo.sh` migrates to `executables: {"foo.sh": "foo"}`
- **is_command_taken**: Test that `is_command_taken()` works correctly with the new `executables` map
- **Update with `-y`**: Test that command names are preserved after update
- **Serialization round-trip**: Test that new format serializes/deserializes correctly; old JSON with `files` key deserializes without error
- **Missing exe in new version**: Test that old symlinks are cleaned up when executable disappears
