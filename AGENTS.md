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
cargo clippy --all-targets -- -D warnings  # Run linter (as CI does)

# Run development build
cargo run -- <command> [args]
cargo run -- add ripgrep
cargo run -- list --all
```

## Project Structure

See [`CONTEXT.md`](CONTEXT.md) for the documentation index. [`docs/reference/`](docs/reference/README.md) holds the glossary, the asset-filtering rules, and [`ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md) (config, cache, package records, launchers); read each module's `//!` docs for structure.
`docs/memo/` is the human's free space: read or tidy it only when asked, never write session output there, and never treat it as instruction or as evidence of behavior. Agent scratch goes in `docs/tmp/<agent>-scratch/` (gitignored).

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
- Use `anyhow` as the codebase standard; the crate has no typed error enums (`thiserror` is not a dependency)

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
- **Functions/Methods**: `snake_case` (`fetch_latest_release`, `install_packages`)
- **Function length**: `src/main.rs` enables `clippy::too_many_lines` (100 lines) and CI fails on it; split long functions into phase helpers
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

Use conditional compilation for platform differences, behind one cross-platform entry point where
possible. Launchers go through `installer::create_launcher` (symlink on Unix, `.cmd` shim on Windows):

```rust
crate::installer::create_launcher(&target, &bin_path, command)?;

// inside create_launcher:
#[cfg(unix)]
create_symlink(target, bin_path)
#[cfg(windows)]
create_shim(target, bin_path, command)
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

Behavior facts (config/paths, 24-hour manifest cache, per-package records, auto-repair, launchers,
`del self`) live in [`docs/reference/ARCHITECTURE.md`](docs/reference/ARCHITECTURE.md). Keep them there, not here.

## Gotchas

- **Sandbox manual runs**: Always pass `env WENGET_ROOT=/tmp/wg-...` when testing the real binary locally so operations do not touch the developer's real `~/.wenget`.
- **GitHub rate limits**: Unauthenticated requests are limited to 60/hr. Set `GITHUB_TOKEN` in the environment to raise the limit to 5000/hr during multi-package testing.

## Release Workflow

When releasing a new version, follow these steps:

### 1. Code Quality Checks

Stop the release unless all of these pass: `cargo fmt --check` shows no differences, clippy
reports no warnings, and `cargo test` has no failures. `release.yml` builds artifacts but runs
no tests, so this is the only gate before a tag ships.

```bash
cargo fmt
cargo fmt --check
cargo clippy --all-targets -- -D warnings  # same invocation as CI
cargo test
```

### 2. Commit All Updates

Commit every outstanding change before starting, so the release commits contain only the
version bump:

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

Stop the release if `git status` still shows modified, staged, or untracked files.

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
- Update `README.md` version badge:
  - Change `[![Version](https://img.shields.io/badge/version-X.X.X-blue.svg)]` to new version
  - Update usage examples if new features added
  - Update feature descriptions if behavior changed
- Update `CHANGELOG.md`:
  - During development, entries accumulate under `## [Unreleased]` → `### YYYY-MM-DD`.
  - At release, move **every** entry under `## [Unreleased]` (all of its `### YYYY-MM-DD` day
    headings) into one new section directly below it:
    ```markdown
    ## [X.Y.Z] - YYYY-MM-DD
    ```
    (Ensure an empty `## [Unreleased]` heading remains at the top for future development).
  - Add the version comparison link at the bottom of `CHANGELOG.md` (newest first, below the
    `[Unreleased]` link) and point `[Unreleased]` at `compare/vX.Y.Z...HEAD`:
    ```markdown
    [X.Y.Z]: https://github.com/superyngo/wenget/compare/vPREV...vX.Y.Z
    ```
- **Changelog archiving**: follow [`docs/reference/changelog/README.md`](docs/reference/changelog/README.md) (keep `[Unreleased]` plus the current major series; archive the previous series on the first release of a new major).

### 5b. Commit the Version Bump
The tag must point at the commit that contains the new version:

```bash
git add Cargo.toml Cargo.lock README.md CHANGELOG.md
git commit -m "chore: release vX.Y.Z"
git status  # Must be clean before tagging
```

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

### 8. Sweep `docs/tmp/`

Run after the tag is pushed. `docs/tmp/` accumulates scratch, so it is swept at every release
rather than "when it grows stale". Triage before removing, and report before moving anything.

1. **Inventory**: list each top-level entry of `docs/tmp/` and `docs/tmp/*-scratch/` with size,
   last-modified date, and tracked/untracked (`git ls-files`). Build output (`target/`,
   `node_modules/`, `.venv/`, `dist/`, `build/`) is listed separately.
2. **Classify** each entry into exactly one class:
   - **Promote**: someone will want to cite or re-run it, or it holds an open follow-up. Write
     the `docs/audit/` record, `docs/plan/BACKLOG.md` row, or `docs/reference/` section first;
     a forming idea the human wants kept goes to `docs/memo/`. The scratch copy then becomes Stale.
   - **Regenerable**: build output. Trash the directory, keep its sources.
   - **Stale**: not modified for 30 days **and** not referenced from a living document or open
     working record. Tracked files are archived; untracked files are trashed.
   - **Live**: everything else. Leave it.
3. **Report and stop**: show the table (entry, size, date, class, proposed action) and wait for
   approval. Ask every sweep; an earlier approval does not carry over.
4. **Execute** only what was approved. Never use `rm`, and never archive `archive/` or build output.

```bash
# Stale, tracked: archive in one commit
mkdir -p docs/tmp/archive
tar -czf docs/tmp/archive/$(date +%Y-%m).tar.gz -C docs/tmp <files and dirs to archive>
git rm -r docs/tmp/<those same files and dirs>
git add docs/tmp/archive
git commit -m "docs: archive tmp/ scratch to tmp/archive/$(date +%Y-%m).tar.gz"

# Stale untracked, and Regenerable: OS trash (macOS `trash`; Linux `gio trash`)
trash <paths>
```

Check that `git check-ignore docs/tmp/<agent>-scratch` succeeds; if not, add
`docs/tmp/*-scratch/` to `.gitignore`.
