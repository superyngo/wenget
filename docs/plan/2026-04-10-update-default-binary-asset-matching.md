# Update Default Binary: Asset-Name Template Matching — Implementation Plan
Status: Shipped (2026-04-10)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken `DEFAULT_VARIANT_SENTINEL` approach with asset-name template matching for selecting the correct binary when updating packages installed without a named variant.

**Architecture:** Single-file change in `src/commands/add.rs`. Remove the sentinel constant and its 4 usage sites. Add `normalize_asset_for_matching()` helper. In the binary filter block, add a new `else if update_mode` branch that compares normalized asset names instead of variant names.

**Tech Stack:** Rust, anyhow, colored

---

### Task 1: Add `normalize_asset_for_matching` and its unit test

**Files:**
- Modify: `src/commands/add.rs` — add helper near `print_available_variants` (~line 694)

- [ ] **Step 1: Write the unit test**

Add to the `#[cfg(test)]` module at the bottom of `src/commands/add.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_asset_for_matching() {
        // Version stripping
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            "uv-x86-64-unknown-linux-gnu"
        );
        assert_eq!(
            normalize_asset_for_matching("gh_copilot_1.0.21_linux_amd64.tar.gz"),
            normalize_asset_for_matching("gh_copilot_1.0.22_linux_amd64.tar.gz")
        );
        assert_eq!(
            normalize_asset_for_matching("ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz"),
            normalize_asset_for_matching("ripgrep-14.1.2-x86_64-unknown-linux-musl.tar.gz")
        );
        // Same arch matches, different arch doesn't
        assert_eq!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz")
        );
        assert_ne!(
            normalize_asset_for_matching("uv-x86_64-unknown-linux-gnu.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-unknown-linux-gnu.tar.gz")
        );
        // zip extension
        assert_eq!(
            normalize_asset_for_matching("bun-linux-x64.zip"),
            "bun-linux-x64"
        );
        // apple stays (not in platform patterns — that's fine, it's part of the template)
        assert_eq!(
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz"),
            normalize_asset_for_matching("uv-aarch64-apple-darwin.tar.gz")
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_normalize_asset_for_matching -- --nocapture
```

Expected: FAIL with "cannot find function `normalize_asset_for_matching`"

- [ ] **Step 3: Add `normalize_asset_for_matching` function**

Insert after the `print_available_variants` function (~line 708 after the existing helper):

```rust
/// Normalize an asset filename for template-based matching across versions.
///
/// Strips file extensions and version-like segments so that the same binary
/// across different releases produces the same template string.
///
/// # Examples
/// - `uv-x86_64-unknown-linux-gnu.tar.gz` → `uv-x86-64-unknown-linux-gnu`
/// - `gh_copilot_1.0.22_linux_amd64.tar.gz` → `gh-copilot-linux-amd64`
/// - `ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz` → `ripgrep-x86-64-unknown-linux-musl`
fn normalize_asset_for_matching(asset_name: &str) -> String {
    let name = asset_name
        .trim_end_matches(".tar.gz")
        .trim_end_matches(".zip")
        .trim_end_matches(".tar.xz")
        .trim_end_matches(".tgz")
        .trim_end_matches(".exe")
        .trim_end_matches(".7z");

    // Split on both - and _, filter out version segments, rejoin
    name.split(|c| c == '-' || c == '_')
        .filter(|seg| {
            if seg.is_empty() {
                return false;
            }
            let s = seg.trim_start_matches('v');
            // A version segment starts with a digit AND contains a dot (e.g. 1.0.22, v0.11.6)
            !(s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && s.contains('.'))
        })
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_normalize_asset_for_matching -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/commands/add.rs
git commit -m "feat: add normalize_asset_for_matching helper with tests"
```

---

### Task 2: Remove the sentinel, add template matching in binary filter

**Files:**
- Modify: `src/commands/add.rs` — 4 changes described below

- [ ] **Step 1: Remove `DEFAULT_VARIANT_SENTINEL` constant (line 30)**

Delete this line:
```rust
const DEFAULT_VARIANT_SENTINEL: &str = "__default__";
```

- [ ] **Step 2: Remove the sentinel `else` branch in the `effective_variant_filter` setup**

Find (~line 1120 after Task 1's additions):
```rust
                    } else {
                        // Base variant (no variant suffix): filter to default binary only
                        effective_variant_filter = Some(DEFAULT_VARIANT_SENTINEL.to_string());
                        log::debug!(
                            "Update mode: auto-selecting default (base) variant for {}",
                            check_name
                        );
                    }
```

Replace with nothing — delete these lines entirely. The `if let Some(ref variant) = inst_pkg.variant` block should now have no `else`.

After the change, the block looks like:
```rust
        if update_mode && effective_variant_filter.is_none() {
            if let Some(ref check_name) = installed_check_name {
                if let Some(inst_pkg) = installed.get_package(check_name) {
                    if let Some(ref variant) = inst_pkg.variant {
                        effective_variant_filter = Some(variant.clone());
                        log::debug!(
                            "Update mode: auto-selecting variant '{}' for {}",
                            variant,
                            check_name
                        );
                    }
                    // If variant is None: template matching handles selection in the filter step
                }
            }
        }
```

- [ ] **Step 3: Replace the sentinel filter branch with template matching**

Find (~line 1215) the binary filter block:
```rust
        let (filtered_binaries, _original_indices): (Vec<_>, Vec<_>) =
            if let Some(filter) = effective_variant_filter {
                if filter == DEFAULT_VARIANT_SENTINEL {
                    // Select only binaries with NO variant (the default/base binary)
                    binaries
                        .iter()
                        .enumerate()
                        .filter(|(_, binary)| {
                            let variant = crate::core::manifest::extract_variant_from_asset(
                                &binary.asset_name,
                                pkg_name,
                            );
                            variant.is_none()
                        })
                        .map(|(idx, binary)| (binary.clone(), idx))
                        .unzip()
                } else {
                    binaries
                        .iter()
                        .enumerate()
                        .filter(|(_, binary)| {
                            let variant = crate::core::manifest::extract_variant_from_asset(
                                &binary.asset_name,
                                pkg_name,
                            );
                            variant.as_deref() == Some(filter)
                        })
                        .map(|(idx, binary)| (binary.clone(), idx))
                        .unzip()
                }
            } else {
                (binaries.clone(), (0..binaries.len()).collect())
            };
```

Replace with:
```rust
        let (filtered_binaries, _original_indices): (Vec<_>, Vec<_>) =
            if let Some(filter) = effective_variant_filter {
                // Named variant: filter by variant name
                binaries
                    .iter()
                    .enumerate()
                    .filter(|(_, binary)| {
                        let variant = crate::core::manifest::extract_variant_from_asset(
                            &binary.asset_name,
                            pkg_name,
                        );
                        variant.as_deref() == Some(filter)
                    })
                    .map(|(idx, binary)| (binary.clone(), idx))
                    .unzip()
            } else if update_mode {
                // Update mode with no named variant: match by asset_name template
                let stored_template = installed_check_name
                    .as_ref()
                    .and_then(|k| installed.get_package(k))
                    .map(|p| normalize_asset_for_matching(&p.asset_name));

                if let Some(template) = stored_template {
                    let matched: Vec<_> = binaries
                        .iter()
                        .enumerate()
                        .filter(|(_, binary)| {
                            normalize_asset_for_matching(&binary.asset_name) == template
                        })
                        .map(|(idx, binary)| (binary.clone(), idx))
                        .collect();

                    if !matched.is_empty() {
                        matched.into_iter().unzip()
                    } else {
                        // Template didn't match (package renamed assets?): pick first binary
                        log::warn!(
                            "Asset template '{}' not matched in new release for {}, using first binary",
                            template,
                            pkg_name
                        );
                        vec![(binaries[0].clone(), 0usize)].into_iter().unzip()
                    }
                } else {
                    // No stored asset_name: pick first binary
                    vec![(binaries[0].clone(), 0usize)].into_iter().unzip()
                }
            } else {
                (binaries.clone(), (0..binaries.len()).collect())
            };
```

- [ ] **Step 4: Simplify `filtered_binaries.is_empty()` error handling**

The sentinel's `display_filter` logic is no longer needed. The empty check now only triggers for the named variant case. Find (~line 1255):

```rust
        if filtered_binaries.is_empty() {
            if let Some(filter) = effective_variant_filter {
                let display_filter = if filter == DEFAULT_VARIANT_SENTINEL {
                    "(default)"
                } else {
                    filter
                };

                if update_mode {
                    if yes {
                        println!(
                            "  {} Variant '{}' no longer available for {}, skipping",
                            "⚠".yellow(),
                            display_filter,
                            pkg_name
                        );
                    } else {
                        println!(
                            "  {} Variant '{}' no longer available for {}. Available variants:",
                            "⚠".yellow(),
                            display_filter,
                            pkg_name
                        );
                        print_available_variants(binaries, pkg_name);
                        println!(
                            "  Skipping this variant. Use 'wenget add {}::VARIANT' to switch.",
                            pkg_name
                        );
                    }
                } else {
                    println!(
                        "  {} No binaries found for variant '{}'. Available variants:",
                        "✗".red(),
                        display_filter
                    );
                    print_available_variants(binaries, pkg_name);
                }
            }
            fail_count += 1;
            failed_packages.push(pkg_name.to_string());
            continue;
        }
```

Replace with (sentinel display_filter logic removed, update_mode branch simplified):
```rust
        if filtered_binaries.is_empty() {
            if let Some(filter) = effective_variant_filter {
                if update_mode {
                    if yes {
                        println!(
                            "  {} Variant '{}' no longer available for {}, skipping",
                            "⚠".yellow(),
                            filter,
                            pkg_name
                        );
                    } else {
                        println!(
                            "  {} Variant '{}' no longer available for {}. Available variants:",
                            "⚠".yellow(),
                            filter,
                            pkg_name
                        );
                        print_available_variants(binaries, pkg_name);
                        println!(
                            "  Skipping this variant. Use 'wenget add {}::VARIANT' to switch.",
                            pkg_name
                        );
                    }
                } else {
                    println!(
                        "  {} No binaries found for variant '{}'. Available variants:",
                        "✗".red(),
                        filter
                    );
                    print_available_variants(binaries, pkg_name);
                }
            }
            fail_count += 1;
            failed_packages.push(pkg_name.to_string());
            continue;
        }
```

- [ ] **Step 5: Run `cargo check` to verify it compiles**

```bash
cargo check
```

Expected: `Finished` with no errors

- [ ] **Step 6: Run `cargo clippy` and `cargo fmt --check`**

```bash
cargo clippy && cargo fmt --check
```

Expected: no warnings, no formatting diff. If `cargo fmt --check` fails, run `cargo fmt` and re-check.

- [ ] **Step 7: Run all tests**

```bash
cargo test
```

Expected: all 96 tests pass (95 previous + 1 new from Task 1), 0 failed, 3 ignored

- [ ] **Step 8: Commit**

```bash
git add src/commands/add.rs
git commit -m "fix: replace DEFAULT_VARIANT_SENTINEL with asset-name template matching

The sentinel approach was broken for packages like uv and copilot-cli
where extract_variant_from_asset returns non-None values (e.g. '64', 'apple',
'gh-copilot') for what are actually default binaries due to imperfect
platform pattern stripping.

Replace with normalize_asset_for_matching() which strips version numbers
and compares normalized asset names directly, bypassing extract_variant_from_asset
entirely for the no-named-variant update case.

Fixes: wenget update uv, wenget update copilot-cli failing with
'Variant (default) no longer available'"
```

---

## Self-Review

**Spec coverage:**
- ✅ Remove `DEFAULT_VARIANT_SENTINEL` → Task 2 Step 1
- ✅ Remove sentinel `else` branch → Task 2 Step 2
- ✅ Add `normalize_asset_for_matching()` → Task 1 Steps 3
- ✅ Template matching in binary filter → Task 2 Step 3
- ✅ Simplify error handling → Task 2 Step 4
- ✅ Fallback to `binaries[0]` when no template match → Task 2 Step 3
- ✅ Named variant path unchanged → both filter blocks in Step 3 preserve named variant behavior
- ✅ Tests → Task 1

**Placeholder scan:** No TBD, no TODO, all code is complete.

**Type consistency:**
- `normalize_asset_for_matching` returns `String` in definition (Task 1) and is called as `String` in comparison (Task 2) ✅
- `(Vec<_>, Vec<_>)` unzip type matches existing code pattern ✅
- `installed.get_package(k)` returns `Option<&InstalledPackage>` and `.asset_name` is `String` field ✅
