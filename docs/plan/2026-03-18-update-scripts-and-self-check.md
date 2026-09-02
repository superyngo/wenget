# Update Scripts & Self-Check Integration Plan
Status: Shipped (2026-03-18)

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `wenget update` handle scripts via URL comparison, auto-check wenget version before package updates, remove `update self`, and handle Windows shell stability after self-update.

**Architecture:** Add `download_url` field to `InstalledPackage` for script URL tracking. Restructure `update::run()` to first check/upgrade wenget, then find upgradeable packages+scripts. On Windows, exit after self-update with restart message.

**Tech Stack:** Rust, serde, clap, anyhow, colored

---

## Chunk 1: Add download_url field and update script installation

### Task 1: Add `download_url` to InstalledPackage

**Files:**
- Modify: `src/core/manifest.rs:395-442` (InstalledPackage struct)

- [ ] **Step 1: Add `download_url` field to `InstalledPackage`**

In `src/core/manifest.rs`, add after the `parent_package` field (line 441):

```rust
    /// Download URL used to install the package/script
    /// Used for scripts from buckets to detect updates via URL change
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
```

- [ ] **Step 2: Update the existing test to include the new field**

In `src/core/manifest.rs`, in the `test_installed_manifest` test (line 821), add the field to the `InstalledPackage` literal:

```rust
            parent_package: None,
            download_url: None,
```

- [ ] **Step 3: Run tests to verify backward compatibility**

Run: `cargo test core::manifest::tests -- --nocapture`
Expected: PASS — existing installed.json without `download_url` will deserialize with `None` thanks to `#[serde(default)]`.

- [ ] **Step 4: Commit**

```bash
git add src/core/manifest.rs
git commit -m "feat: add download_url field to InstalledPackage for script update tracking

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

### Task 2: Populate `download_url` when installing bucket scripts

**Files:**
- Modify: `src/commands/add.rs:1658-1676` (install_script_from_bucket function)

- [ ] **Step 1: Populate `download_url` in `install_script_from_bucket`**

In `src/commands/add.rs`, the function signature at line 1629 becomes:

```rust
fn install_script_from_bucket(
    config: &Config,
    paths: &WenPaths,
    installed: &mut crate::core::InstalledManifest,
    name: &str,
    url: &str,
    script_type: ScriptType,
    origin: &str,
    custom_name: Option<&str>,
) -> Result<()> {
```

The `url` parameter is already passed in. Just populate the field in the `InstalledPackage` literal at line 1659. Change:

```rust
        parent_package: None,
```

to:

```rust
        parent_package: None,
        download_url: Some(url.to_string()),
```

- [ ] **Step 2: Also update `install_single_script` for non-bucket scripts**

In `src/commands/add.rs`, in the `install_single_script` function (line 402), add to the `InstalledPackage` literal:

```rust
        parent_package: None,
        download_url: None,
```

- [ ] **Step 3: Search for ALL places that create InstalledPackage across the entire codebase**

Run: `grep -rn "InstalledPackage {" src/`

For every `InstalledPackage { ... }` literal found (in `add.rs`, `installer/local.rs`, `commands/rename.rs`, test code, etc.), ensure `download_url` is set:
- For bucket script installs: `download_url: Some(url.to_string())`
- For all other installs (packages, local files, renames, tests): `download_url: None`

- [ ] **Step 4: Build and test**

Run: `cargo build 2>&1 | head -50`
Expected: No compilation errors. If there are errors about missing `download_url` field in other files, fix them by adding `download_url: None`.

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/commands/add.rs
git commit -m "feat: populate download_url for bucket script installations

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

## Chunk 2: Script update detection in find_upgradeable

### Task 3: Add script update detection to `find_upgradeable()`

**Files:**
- Modify: `src/commands/update.rs:76-180` (find_upgradeable function)

- [ ] **Step 1: Replace the Script skip branch with URL comparison logic**

In `src/commands/update.rs`, replace lines 116-123:

```rust
            PackageSource::Script { .. } => {
                // Scripts don't support updates
                log::debug!(
                    "Skipping script '{}' - scripts don't support updates",
                    repo_name
                );
                continue;
            }
```

with:

```rust
            PackageSource::Script { origin, .. } => {
                // Check if this is a bucket-sourced script
                if !origin.starts_with("bucket:") {
                    log::debug!(
                        "Skipping non-bucket script '{}' - no update source",
                        repo_name
                    );
                    continue;
                }

                // Look up the script in the refreshed cache
                let cached_script = cache.find_script(&repo_name);
                if cached_script.is_none() {
                    log::debug!(
                        "Script '{}' not found in cache, skipping update",
                        repo_name
                    );
                    continue;
                }

                let cached_script = cached_script.unwrap();

                // Get the installable script URL for current platform
                if let Some((_script_type, platform_info)) =
                    cached_script.script.get_installable_script()
                {
                    let cache_url = &platform_info.url;

                    // Compare with installed download_url
                    let needs_update = match &inst_pkg.download_url {
                        Some(installed_url) => installed_url != cache_url,
                        None => true, // No stored URL = always update
                    };

                    if needs_update {
                        upgradeable.push((
                            repo_name.clone(),
                            inst_pkg.download_url.clone().unwrap_or_default(),
                            cache_url.clone(),
                        ));
                    }
                }

                continue;
            }
```

- [ ] **Step 2: Build to verify compilation**

Run: `cargo build 2>&1 | head -30`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/commands/update.rs
git commit -m "feat: detect script updates via URL comparison in find_upgradeable

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

### Task 4: Handle script re-installation in the update flow

**Files:**
- Modify: `src/commands/update.rs:54-72` (variant expansion and add::run call)

- [ ] **Step 1: Ensure scripts are passed through to `add::run`**

Scripts don't have `::` variants and won't match `installed.find_by_repo()`. We need to ensure scripts pass through the expansion loop. In `src/commands/update.rs`, modify the expansion loop (lines 54-69):

```rust
    // Expand: include all variants when upgrading a repo
    let mut expanded = Vec::new();
    for name in &to_upgrade {
        // Check if this is a repo name or a specific variant
        if name.contains("::") {
            expanded.push(name.clone());
            continue;
        }

        // Find all variants of this repo and add them to be upgraded
        let variants = installed.find_by_repo(name);
        if variants.is_empty() {
            // Could be a script or standalone package - pass through as-is
            expanded.push(name.clone());
        } else {
            for (key, _pkg) in variants {
                expanded.push(key.clone());
            }
        }
    }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build 2>&1 | head -30`
Expected: No errors.

- [ ] **Step 3: Commit**

```bash
git add src/commands/update.rs
git commit -m "fix: pass through scripts in update variant expansion

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

## Chunk 3: Auto-check wenget version and remove update self

### Task 5: Add `check_and_upgrade_self()` function

**Files:**
- Modify: `src/commands/update.rs` (add new function, modify `run()`)

- [ ] **Step 1: Create `check_and_upgrade_self()` function**

Add a new function in `src/commands/update.rs` before `upgrade_self()`:

```rust
/// Check for wenget updates and prompt user
/// Returns true if wenget was updated on Windows (caller should exit)
fn check_and_upgrade_self(yes: bool) -> Result<bool> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Try to check latest version - don't fail the whole update if this fails
    let provider = match GitHubProvider::new() {
        Ok(p) => p,
        Err(e) => {
            log::debug!("Failed to create GitHub provider for self-check: {}", e);
            return Ok(false);
        }
    };

    let latest_version = match provider.fetch_latest_version("https://github.com/superyngo/wenget")
    {
        Ok(v) => v,
        Err(e) => {
            log::debug!("Failed to check wenget updates: {}", e);
            return Ok(false);
        }
    };

    if current_version == latest_version {
        return Ok(false);
    }

    println!(
        "{} {} -> {}",
        "New wenget version available:".yellow().bold(),
        current_version.yellow(),
        latest_version.green()
    );

    let should_update = if yes {
        true
    } else {
        crate::utils::confirm("Update wenget first?")?
    };

    if !should_update {
        println!();
        return Ok(false);
    }

    // Perform self-update, passing provider and known version to avoid redundant API calls
    upgrade_self_with_provider(provider, &latest_version)?;

    // On Windows, recommend restarting shell
    #[cfg(windows)]
    {
        println!();
        println!(
            "{}",
            "⚠  Please restart your shell, then run 'wenget update' again to update packages."
                .yellow()
                .bold()
        );
        return Ok(true); // Signal caller to exit
    }

    #[cfg(not(windows))]
    {
        println!();
        return Ok(false); // Continue with package updates on Unix
    }
}
```

Also refactor `upgrade_self()` into `upgrade_self_with_provider(provider, latest_version)` that accepts the already-created provider and known version, skipping the redundant `fetch_latest_version()` call. The existing `upgrade_self()` body from line 191 onward (after the version check) becomes the body of `upgrade_self_with_provider()`, starting from `println!("Downloading...")`. Remove the standalone `upgrade_self()` function entirely.

- [ ] **Step 2: Modify `run()` to call `check_and_upgrade_self()` and remove `update self`**

Replace the beginning of `run()` (lines 12-17):

```rust
/// Upgrade installed packages
pub fn run(names: Vec<String>, yes: bool) -> Result<()> {
    // Handle "wenget update self"
    if names.len() == 1 && names[0] == "self" {
        return upgrade_self();
    }
```

with:

```rust
/// Upgrade installed packages
pub fn run(names: Vec<String>, yes: bool) -> Result<()> {
    // Check for wenget updates first
    if check_and_upgrade_self(yes)? {
        // On Windows, exit after self-update to avoid shell instability
        return Ok(());
    }
```

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | head -30`
Expected: No compilation errors. There may be a dead_code warning on `upgrade_self` since it's no longer called directly from outside — that's fine, it's called by `check_and_upgrade_self`.

- [ ] **Step 4: Commit**

```bash
git add src/commands/update.rs
git commit -m "feat: auto-check wenget version before package updates, remove 'update self'

- Check for new wenget version at start of 'wenget update'
- Prompt user to update if new version available
- On Windows: exit after self-update with restart message
- On Unix: continue with package updates after self-update
- Remove 'wenget update self' subcommand

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```

## Chunk 4: Final verification and cleanup

### Task 6: Full build and test verification

**Files:**
- All modified files

- [ ] **Step 1: Run `cargo fmt`**

Run: `cargo fmt`

- [ ] **Step 2: Run `cargo clippy`**

Run: `cargo clippy -- -D warnings 2>&1 | tail -30`
Expected: No warnings. Fix any issues found.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass. If any InstalledPackage literal in tests is missing `download_url`, add `download_url: None`.

- [ ] **Step 4: Verify the CLI help text no longer mentions "self"**

Run: `cargo run -- update --help 2>&1`
Expected: Help text shows `wenget update [names]` — "self" is no longer a special value.

- [ ] **Step 5: Final commit if any formatting/clippy changes**

```bash
git add -A
git commit -m "chore: fmt and clippy fixes

Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>"
```
