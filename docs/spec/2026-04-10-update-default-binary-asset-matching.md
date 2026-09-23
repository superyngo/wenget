# Update Default Binary: Asset-Name Template Matching
Status: Shipped (2026-04-10)

## Problem

The `DEFAULT_VARIANT_SENTINEL` (`"__default__"`) approach introduced to fix the base-variant update bug is broken for packages whose asset filenames produce non-`None` results from `extract_variant_from_asset`, even though they have no meaningful variant:

| Asset | Repo | `extract_variant_from_asset` | Why wrong |
|-------|------|------------------------------|-----------|
| `uv-x86_64-unknown-linux-gnu.tar.gz` | `uv` | `Some("64")` | `x86_64` → `x86-64` (underscore normalized) → `x86` removed → `64` fragment remains |
| `uv-aarch64-apple-darwin.tar.gz` | `uv` | `Some("apple")` | `apple` missing from platform patterns list |
| `gh_copilot_1.0.22_linux_amd64.tar.gz` | `copilot-cli` | `Some("gh-copilot")` | asset prefix `gh_copilot` doesn't start with repo name `copilot-cli` |

The sentinel filters for binaries where `extract_variant_from_asset() == None`, but these binaries never return `None`, so the filter produces 0 results → "Variant '(default)' no longer available" error.

**Root cause:** `extract_variant_from_asset` was designed to detect *meaningful* variant names (baseline/minimal/profile), not to reliably detect "absence of variant". These are fundamentally different questions.

## Approach

Replace the sentinel approach with **asset-name template matching**: when a package was installed with no variant (`inst_pkg.variant == None`) and update_mode is active, find the matching binary in the new release by comparing normalized asset names (version stripped) rather than by variant name.

## Design

### `normalize_asset_for_matching(asset_name: &str) -> String`

New private function in `src/commands/add.rs`. Produces a version-independent template for comparison:

1. Strip file extensions (`.tar.gz`, `.zip`, `.tar.xz`, `.tgz`, `.exe`, `.7z`)
2. Split on `-` and `_`
3. Filter out version-like segments: any segment that starts with a digit (after optional `v` prefix) AND contains a `.` (e.g., `1.0.22`, `v0.11.6`)
4. Rejoin with `-`
5. Lowercase

Examples:
- `uv-x86_64-unknown-linux-gnu.tar.gz` → `uv-x86-64-unknown-linux-gnu`
- `gh_copilot_1.0.22_linux_amd64.tar.gz` → `gh-copilot-linux-amd64`
- `ripgrep-14.1.1-x86_64-unknown-linux-musl.tar.gz` → `ripgrep-x86-64-unknown-linux-musl`

### Binary filter logic in `install_packages`

Replace `DEFAULT_VARIANT_SENTINEL` branches in the binary filter block with:

```
if effective_variant_filter == Some(name):
    filter by variant name using extract_variant_from_asset  ← unchanged

else if update_mode AND stored_asset_name available:
    template = normalize_asset_for_matching(stored_asset_name)
    matched = binaries where normalize_asset_for_matching(b.asset_name) == template
    if matched non-empty: use matched
    else: warn + use binaries[0] as fallback

else:
    all binaries  ← unchanged (normal add behavior)
```

### Effective variant filter setup

Remove the `else` branch that set the sentinel:

```rust
// Before (BROKEN):
} else {
    effective_variant_filter = Some(DEFAULT_VARIANT_SENTINEL.to_string());
}

// After (CORRECT):
// No else branch — leave effective_variant_filter as None.
// Template matching handles this in the filter step.
```

### Error handling for empty filtered binaries

The `filtered_binaries.is_empty()` check only triggers now when:
- Named variant filter (`inst_pkg.variant = Some("x")`) finds no match — keep existing warning
- Template matching found 0 results AND `binaries` is empty (impossible with a valid platform match)

In practice, the template matching always falls back to `binaries[0]`, so the empty check is now only reachable through the named variant path.

## Files Changed

- `src/commands/add.rs` only:
  - Remove `DEFAULT_VARIANT_SENTINEL` constant (line 30)
  - Remove sentinel `else` branch in effective_variant_filter setup (~line 1131)
  - Add `normalize_asset_for_matching()` helper near `print_available_variants` (~line 694)
  - Replace `DEFAULT_VARIANT_SENTINEL` filter branch with template matching (~lines 1220-1245)
  - Simplify `filtered_binaries.is_empty()` handling (remove sentinel display_filter logic)

## Verification

Template matching correctly handles:

| Stored asset | New release asset | Match? |
|---|---|---|
| `uv-x86_64-unknown-linux-gnu.tar.gz` | `uv-x86_64-unknown-linux-gnu.tar.gz` (new version) | ✅ |
| `uv-aarch64-apple-darwin.tar.gz` | `uv-aarch64-apple-darwin.tar.gz` | ✅ |
| `gh_copilot_1.0.21_linux_amd64.tar.gz` | `gh_copilot_1.0.22_linux_amd64.tar.gz` | ✅ |
| `uv-x86_64-unknown-linux-gnu.tar.gz` | `uv-aarch64-unknown-linux-gnu.tar.gz` (wrong arch) | ❌ (correct, no match) |
| `bun-linux-x64-baseline.zip` | (goes through named variant path, not template) | N/A |
