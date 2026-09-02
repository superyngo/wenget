# Update Command Behavior Redesign
Status: Shipped (2026-04-10)

## Problem Statement

Two issues with the `update` command:

1. **Bug:** `update -y` still installs all variants like `install -y`, despite commit `618d358` intending to fix this. Root cause: the base variant (`variant: None`) has no value to filter on, so `effective_variant_filter` stays `None` and all binaries are selected.

2. **Behavior Change:** The default `update` (without `-y`) should follow installed.json content for variant and executable selection, only prompting for genuine edge cases.

## Approach

**Approach A: Minimal Fix + update_mode propagation.** Surgically fix the variant filter bug, decouple `update_mode` from `yes`, and add installed-only validation. Changes concentrated in `add.rs` and `update.rs`.

## Detailed Design

### 1. Fix Base-Variant Bug

**File:** `src/commands/add.rs`, around line 1096

**Current code:**
```rust
if update_mode && yes && effective_variant_filter.is_none() {
    if let Some(ref check_name) = installed_check_name {
        if let Some(inst_pkg) = installed.get_package(check_name) {
            if let Some(ref variant) = inst_pkg.variant {
                effective_variant_filter = Some(variant.clone());
            }
            // BUG: base variant (variant: None) falls through with no filter!
        }
    }
}
```

**Fix:** Add an `else` branch for `variant: None` that sets a sentinel value `"__default__"`:

```rust
if update_mode && effective_variant_filter.is_none() {
    if let Some(ref check_name) = installed_check_name {
        if let Some(inst_pkg) = installed.get_package(check_name) {
            if let Some(ref variant) = inst_pkg.variant {
                effective_variant_filter = Some(variant.clone());
            } else {
                // Base variant (no variant suffix): filter to default binary only
                effective_variant_filter = Some("__default__".to_string());
            }
        }
    }
}
```

**File:** `src/commands/add.rs`, around line 1186 (binary filter logic)

Update the filter to handle the sentinel:

```rust
let (filtered_binaries, _original_indices): (Vec<_>, Vec<_>) =
    if let Some(filter) = effective_variant_filter {
        if filter == "__default__" {
            // Select only binaries with NO variant (the default/base binary)
            binaries.iter().enumerate()
                .filter(|(_, binary)| {
                    let variant = extract_variant_from_asset(&binary.asset_name, pkg_name);
                    variant.is_none()
                })
                .map(|(idx, binary)| (binary.clone(), idx))
                .unzip()
        } else {
            // Existing variant filter logic
            binaries.iter().enumerate()
                .filter(|(_, binary)| {
                    let variant = extract_variant_from_asset(&binary.asset_name, pkg_name);
                    variant.as_deref() == Some(filter)
                })
                .map(|(idx, binary)| (binary.clone(), idx))
                .unzip()
        }
    } else {
        (binaries.clone(), (0..binaries.len()).collect())
    };
```

### 2. Decouple update_mode from yes Flag

**File:** `src/commands/add.rs`

Remove `&& yes` from all `update_mode` guards:

| Location | Current | New |
|----------|---------|-----|
| ~Line 1096 (variant auto-select) | `update_mode && yes` | `update_mode` |
| ~Line 1482 (executable auto-select) | `update_mode && yes` | `update_mode` |
| ~Line 1602 (command name reuse) | `yes && old_executables` | `update_mode && old_executables` (check update_mode, not yes) |

**`yes` flag's role after change:**
- In `update.rs`: Skip "Proceed with upgrades?" confirmation
- In `add.rs`: Auto-confirm edge case prompts (local overwrite, etc.)
- Does NOT affect variant/executable auto-selection (that's `update_mode`)

### 3. Edge Case Prompts (update_mode && !yes)

When `update_mode` is true and `yes` is false, prompt for these edge cases:

1. **Locally installed package overwrite:**
   - Already implemented in `find_upgradeable()` (line 213-226)
   - No change needed

2. **New executables found not in old install:**
   - In the executable selection block (~line 1482), when matching finds candidates NOT in old executables:
   - Prompt: "New executable `X` found in update. Install it?"
   - If `yes`: include automatically

3. **Previously installed variant no longer available:**
   - After binary filtering, if `filtered_binaries` is empty and `update_mode` is true:
   - Prompt: "Variant `X` no longer available for `pkg`. Skip?"
   - If `yes`: skip automatically

### 4. Prevent Update from Adding New Packages

**File:** `src/commands/update.rs`, in the expansion loop (~line 80)

Add validation before expansion:

```rust
let mut expanded = Vec::new();
for name in &to_upgrade {
    let base = name.split("::").next().unwrap_or(name);
    
    if !installed.is_installed(name) && installed.find_by_repo(base).is_empty() {
        eprintln!(
            "{} '{}' is not installed, skipping (use 'wenget add' to install new packages)",
            "Warning:".yellow(),
            name
        );
        continue;
    }
    
    // ... existing expansion logic
}
```

**File:** `src/commands/add.rs`, in the package categorization (~line 950)

When `update_mode` is true and a package is NOT found in installed.json, skip instead of treating as new install:

```rust
} else {
    // New installation
    if update_mode {
        println!("  {} {} is not installed, skipping", "⚠".yellow(), pkg_name);
        continue;
    }
    // ... existing new install logic
}
```

## Behavioral Summary

| Scenario | `update` | `update -y` | `add` | `add -y` |
|----------|----------|-------------|-------|----------|
| Variant selection | Auto from installed.json | Auto from installed.json | Interactive | All variants |
| Executable selection | Auto from installed.json | Auto from installed.json | Interactive | All with score>0 |
| Command name | Preserved from installed.json | Preserved from installed.json | New resolution | New resolution |
| New package (not installed) | Rejected with warning | Rejected with warning | Installed | Installed |
| Local pkg overwrite | Prompt | Auto-yes | N/A | N/A |
| New executable in update | Prompt | Auto-include | N/A | N/A |
| Variant unavailable | Prompt | Auto-skip | N/A | N/A |
| "Proceed?" confirmation | Prompt | Skipped | N/A | N/A |

## Files to Modify

1. `src/commands/add.rs` — variant filter fix, update_mode decoupling, edge case prompts, new-package guard
2. `src/commands/update.rs` — installed-only validation
3. `src/main.rs` — no changes expected
4. `src/core/manifest.rs` — no changes expected

## Testing

- Test update with base variant only (no variant suffix)
- Test update with named variant (e.g., "bun::baseline")
- Test update with mix of base + named variants
- Test `update new-pkg` rejects uninstalled packages
- Test edge case prompts appear when not using -y
- Test -y auto-confirms edge cases
