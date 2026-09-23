# Design: Update Scripts & Self-Check Integration
Status: Shipped (2026-03-18)

## Problem

1. **Scripts not updated**: Scripts (gists) installed from buckets have no version field, so `wenget update` skips them entirely in `find_upgradeable()`.
2. **No auto self-check**: Users must manually run `wenget update self` to check for wenget updates.
3. **`update self` is separate**: Should be integrated into the main update flow.
4. **Windows shell instability**: After self-updating on Windows, the shell environment can become unstable.

## Design

### Task 1: Script Update via URL Comparison

Add `download_url: Option<String>` to `InstalledPackage`. When scripts are installed from buckets, store the gist raw URL. During `find_upgradeable()`, for bucket-sourced scripts, compare the stored URL against the refreshed cache's URL. Different URL → re-install.

**Changes:**
- `src/core/manifest.rs`: Add `download_url: Option<String>` to `InstalledPackage`
- `src/commands/add.rs`: Set `download_url` in `install_script_from_bucket()`
- `src/commands/update.rs`: In `find_upgradeable()`, handle `PackageSource::Script` with bucket origin

### Task 2: Auto-Check Wenget Version

At the start of `update::run()`, fetch the latest wenget version. If newer, prompt the user. On acceptance, run self-update. On Windows, stop after self-update with a restart message. On Unix, continue to package updates.

**Changes:**
- `src/commands/update.rs`: Add `check_and_upgrade_self()` at start of `run()`

### Task 3: Remove `update self`

Remove the `names == ["self"]` branch from `update::run()`.

### Task 4: Windows Restart Message

After successful self-update on Windows, print a message asking the user to restart their shell and run `wenget update` again for packages. Exit early.

**Changes:**
- `src/commands/update.rs`: Platform-conditional return after self-update

## Data Flow

```
wenget update [names]
  ├─ Check wenget version → ask → upgrade → (Windows: exit / Unix: continue)
  ├─ Refresh cache
  ├─ find_upgradeable() with script URL comparison
  └─ Install via add::run()
```
