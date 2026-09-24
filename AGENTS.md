# AGENTS.md

Guidelines for AI coding agents working on this Rust codebase. See [`CONTEXT.md`](CONTEXT.md)
for the documentation index (reference, ADRs, specs, plans).

## Build & Test Commands

```bash
# Build
cargo build              # Development build
cargo build --release    # Release build (optimized for size)

# Check without building
cargo check

# Run all tests
cargo test

# Run single test by name
cargo test test_config_creation

# Run tests in a specific module
cargo test bucket::tests
cargo test core::repair::tests

# Run tests with stdout/stderr visible
cargo test -- --nocapture

# Run specific test with output visible
cargo test test_add_bucket -- --nocapture

# Code quality
cargo fmt                # Format code
cargo fmt --check        # Check formatting without changing
cargo clippy             # Run linter

# Run development build
cargo run -- <command> [args]
cargo run -- add ripgrep
cargo run -- list --all
```

## Project Structure

See [`CONTEXT.md`](CONTEXT.md) and [`docs/reference/`](docs/reference/) for architecture, module structure, and data flow.
Note that `src/core/bucket.rs` (bucket configuration and models) and `src/commands/bucket.rs` (CLI bucket subcommands) are distinct modules.

## Code Style Guidelines

### Imports

Order imports in groups separated by blank lines:
1. Standard library (`std::`)
2. External crates (alphabetical)
3. Internal crate modules (`crate::`, `super::`)

```rust
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::core::Config;
use crate::providers::GitHubProvider;
```

### Error Handling

- Use `anyhow::Result<T>` for functions that can fail
- Use `anyhow::bail!()` for early error returns
- Add context with `.context()` or `.with_context()`
- Use `anyhow` as the codebase standard; use `thiserror` only if a typed error is genuinely needed

```rust
pub fn load_config(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read: {}", path.display()))?;
    
    serde_json::from_str(&content)
        .context("Failed to parse JSON")
}
```

### Naming Conventions

- **Types/Structs/Enums**: `PascalCase` (`InstalledPackage`, `ScriptType`)
- **Functions/Methods**: `snake_case` (`fetch_latest_release`, `install_package`)
- **Constants**: `SCREAMING_SNAKE_CASE` (`INTERPRETER_CACHE`)
- **Modules**: `snake_case` (`package_resolver.rs`)
- **Boolean methods**: Use `is_`/`has_` prefix (`is_installed`, `is_valid`)

### Documentation

- Add `//!` module-level docs at the top of each file
- Use `///` for public function/struct documentation
- Document non-obvious behavior and edge cases

```rust
//! Bucket management for wenget
//!
//! Buckets are remote manifest sources that can be added to wenget.

/// A bucket configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bucket {
    /// Bucket name (unique identifier)
    pub name: String,
}
```

### Struct Patterns

- Derive common traits: `Debug, Clone, Serialize, Deserialize`
- Use `#[serde(skip_serializing_if = "Option::is_none")]` for optional fields
- Implement `Default` via `new()` method pattern
- Use `#[allow(dead_code)]` for intentionally unused helper methods

### Testing

- Place unit tests in the same file using `#[cfg(test)]` module
- Use `tempfile::TempDir` for file operation tests
- Test both success and error paths

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_bucket_config_new() {
        let config = BucketConfig::new();
        assert_eq!(config.buckets.len(), 0);
    }
}
```

### CLI Arguments (clap)

- Use derive macros with `#[command]` and `#[arg]` attributes
- Add `visible_alias` for common shortcuts
- Use `short` and `long` for flags

```rust
#[derive(Parser)]
#[command(name = "wenget")]
pub struct Cli {
    #[arg(short = 'y', long)]
    pub yes: bool,
}
```

### Platform-Specific Code

Use conditional compilation for platform differences:

```rust
#[cfg(unix)]
use crate::installer::create_symlink;

#[cfg(windows)]
use crate::installer::create_shim;
```

### Logging

- Use `log` macros (`log::info!`, `log::warn!`, `log::error!`)
- Use `colored` crate for user-facing terminal output
- Print user messages to stdout, errors to stderr

```rust
log::info!("Fetching bucket from {}", url);
println!("{} {}", "Installing".cyan(), name);
eprintln!("{} {}", "Error:".red().bold(), e);
```

## Key Implementation Notes

- `Config` coordinates path management, preferences, and bucket/manifest caching
- `WenPaths` manages all directory paths (user vs system level), with `WENGET_ROOT` override for testing
- Platform detection uses fuzzy matching for binary names
- Cache has 24-hour TTL; invalidate after bucket changes
- Write one package record per install/rename (`InstalledStore::save_package`); deleting an app
  directory removes its record with it. There is no global installed index
- JSON config files have auto-repair on parse errors with backup (`repair.rs`), and corrupt package records are quarantined per package (`store.rs`)

## Gotchas

- **Sandbox manual runs**: Always pass `env WENGET_ROOT=/tmp/wg-...` when testing the real binary locally so operations do not touch the developer's real `~/.wenget`.
- **GitHub rate limits**: Unauthenticated requests are limited to 60/hr. Set `GITHUB_TOKEN` in the environment to raise the limit to 5000/hr during multi-package testing.
- **Platform launchers**: User installs place launchers in `~/.local/bin/` (or custom bin dir): Unix uses symlinks (`installer::symlink`), Windows uses `.cmd` shims (`installer::shim`). Shims must quote paths to handle spaces.
- **Per-package records**: Each installed package owns its record at `{app_dir}/.wenget/package.json`. There is no global installed index; removing an app directory removes its tracking record.
- **Cache freshness**: Manifest cache has a 24-hour TTL. Rebuild or invalidate cache (`wenget bucket refresh` or `Config::invalidate_cache`) after bucket changes.
- **Self-deletion**: `wenget del self` uninstalls wenget itself, with platform-specific handling for running executables.

## Release Workflow

When releasing a new version, follow these steps:

### 1. Code Quality Checks (MANDATORY)
**MUST complete before proceeding with release:**

```bash
# Format code (must pass without changes)
cargo fmt
cargo fmt --check  # Verify no formatting issues remain

# Lint code (must resolve all clippy warnings)
cargo clippy -- -D warnings  # Fail on any warnings
# Fix all clippy warnings before proceeding
```

**DO NOT proceed with release if:**
- `cargo fmt --check` shows formatting differences
- `cargo clippy` reports any warnings or errors

### 2. Commit All Updates (MANDATORY)
**MUST ensure all changes are committed before proceeding:**

```bash
# Check for uncommitted changes
git status

# If there are uncommitted changes:
# 1. Review all changes carefully
git diff

# 2. Stage and commit all changes
git add .
git commit -m "descriptive message"

# 3. Verify working directory is clean
git status  # Should show "nothing to commit, working tree clean"
```

**DO NOT proceed with release if:**
- `git status` shows any uncommitted changes (modified, staged, or untracked files)
- Working directory is not clean

**Important Notes:**
- All code changes must be committed before starting the release process
- Organize commit messages clearly describing the updates
- Verify code quality checks pass (fmt and clippy) for all committed code
- The release process will create additional commits for version updates

### 3. Determine Version Number
- Review changes to suggest appropriate version bump:
  - **PATCH** (x.x.+1): Bug fixes, minor improvements
  - **MINOR** (x.+1.0): New features, backward compatible
  - **MAJOR** (+1.0.0): Breaking changes
- Ask user to confirm the new version number

### 4. Collect Version Changes
- Review accumulated entries under `## [Unreleased]` in `CHANGELOG.md`
- Categorize changes clearly into user-facing descriptions

### 5. Update Documentation and Changelog
- Update `Cargo.toml` version field
- Update `README.md` version badge (MANDATORY):
  - Change `[![Version](https://img.shields.io/badge/version-X.X.X-blue.svg)]` to new version
  - Update usage examples if new features added
  - Update feature descriptions if behavior changed
- Update `CHANGELOG.md`:
  - During development, entries accumulate under `## [Unreleased]` → `### YYYY-MM-DD`.
  - At release, rename the day's heading under `## [Unreleased]` to:
    ```markdown
    ## [X.Y.Z] - YYYY-MM-DD
    ```
    (Ensure an empty `## [Unreleased]` heading remains at the top for future development).
  - Add the version comparison link at the bottom of `CHANGELOG.md`:
    ```markdown
    [X.Y.Z]: https://github.com/superyngo/wenget/compare/vPREV...vX.Y.Z
    ```
- **Changelog Archiving Rule**: Root `CHANGELOG.md` keeps `[Unreleased]` plus the current major series only (e.g. 3.x). When cutting the first release of a new major series (e.g. `v4.0.0`), move the entire preceding major series verbatim into `docs/reference/changelog/` (e.g. `docs/reference/changelog/v3.x.md`) and update the index in `docs/reference/changelog/README.md`. Never archive the series the next tag belongs to.

### 6. Create Tag and Release
```bash
# Create annotated tag with release notes
git tag -a vX.X.X -m "Release vX.X.X

- Feature 1
- Fix 1
"
```

**Note on release notes**: `.github/workflows/release.yml` extracts release notes directly from the tag annotation message (`git tag -l --format='%(contents)' "$TAG_NAME"`) and does **not** read `CHANGELOG.md`. Always provide descriptive notes in the annotated tag.

### 7. Push and Publish
```bash
# Push commits and tags
git push origin main
git push origin vX.X.X

# GitHub Actions will automatically build and create release
```

The `.github/workflows/release.yml` workflow will:
- Trigger on version tags matching `v*.*.*`
- Build binaries for all platforms
- Create GitHub Release with artifacts and tag annotation notes
