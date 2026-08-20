# Changelog

All notable changes to wenget will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

## [3.8.7] - 2026-08-20

### Added

- feat(update): On Windows, `wenget update` now detects when wenget itself was installed via `winget` (executable path under `...\Microsoft\WinGet\Packages\...`) and skips the self-update replace step, instead printing `Run 'winget upgrade WenanLin.wenget' to update.` so winget's own version tracking doesn't get desynced by wenget silently swapping its binary.

### Fixed

- ci(winget): `publish-winget.yml` now syncs the `superyngo/winget-pkgs` fork with upstream (`gh api merge-upstream`) before invoking `winget-releaser`. A stale fork made `winget-releaser` (komac) fail branch creation with a misleading `does not have the correct permissions to execute CreateRef` error on the v3.8.6 publish run; syncing the fork and re-dispatching resolved it (see https://github.com/vedantmgoyal9/winget-releaser/issues/319).

## [3.8.6] - 2026-08-12

### Added

- ci: `.github/workflows/publish-gate.yml` + `.github/workflows/publish-winget.yml` — decoupled, manually-approved CI publish to winget (`WenanLin.wenget`) via `vedantmgoyal9/winget-releaser@v2`, replacing the previously deleted/never-working `winget.yml.disabled` step and the dead commented-out dispatch block in `release.yml`. Gated behind a `publish-gate-winget` GitHub Environment (required reviewer) so a rejected/misconfigured winget submission can't block or fail the tagged build. Requires PR #410386 (`New package: WenanLin.wenget`) to merge in `microsoft/winget-pkgs` first — `winget-releaser` only updates existing packages, never creates new ones.
- feat(core): SHA-256 checksum verification for downloaded release assets. Before extracting a freshly downloaded binary (in `add`, `add <url>`, and self-`update`), wenget probes up to three conventional checksum filenames next to the asset on GitHub Releases — `<asset>.sha256`, `checksums.txt`, `SHA256SUMS` — each with a 3s timeout. If one lists a hash for the asset, the download is verified against it: a mismatch deletes the downloaded file and aborts the install with no override (not even `--yes`). If nothing is published, or the probe fails at the network layer, the install proceeds unverified with a low-key status line distinguishing the two cases (`ℹ No checksum published` vs `⚠ Checksum lookup failed (network error)`). New `src/core/checksum.rs` module (`sha2` dependency added); `PlatformBinary.checksum` and manifest caching are untouched — lookups happen live, only for the exact binary chosen for install.

### Fixed

- fix(rename): Windows `rename` command's `read_shim_target` required each shim line to start with `@`, but generated shims (`installer::shim::create_shim`, `installer::init`) always split `@echo off` and the quoted target path across two separate lines — so the check never matched and every Windows rename of a real shim failed with `Failed to parse shim target from: ...`. The parser now skips the `@echo off` line and reads the quoted path from the following line regardless of leading `@`.
- fix(rename): `wenget rename <repo>` silently picked a single arbitrary variant package when a repo (e.g. `confy`) was installed as multiple separately-tracked variant packages, each contributing one command — the interactive "select a command" prompt never appeared, and direct-mode `wenget rename <repo> <new>` could rename the wrong command without warning. `find_command_candidates` now aggregates commands across every variant package sharing the repo name: a single match resolves directly, multiple matches trigger the interactive selector (unless a new name was already given, in which case it errors and lists the ambiguous candidates instead of guessing).
- fix: removed a `clippy::needless_return` in `check_and_upgrade_self`'s Windows branch, unblocking the `cargo clippy -- -D warnings` release gate.

## [3.8.5] - 2026-07-31

### Fixed

- fix(platform): `contains_keyword` now requires a word boundary when matching OS keywords in a release asset filename, instead of a plain substring check. Previously any short generic keyword (e.g. "mac" for macOS) could match inside an unrelated token — most visibly `komac-*-pc-windows-msvc.exe` / `komac-*-unknown-linux-gnu.tar.gz`, whose name contains "mac" inside "ko`mac`", causing every Windows/Linux Komac release asset to be misclassified as macOS. Genuine `-apple-darwin` assets are unaffected. Added regression tests covering both the unit-level keyword match and the end-to-end platform grouping for Komac's real v2.16.0 asset names.

### Added

- 2026-07-23: docs: add `docs/RESOURCE_FILTERING_RULES.md` — single source of truth cataloguing all package-analysis filtering rules (asset→platform bucketing gates and scoring, platform selection/fallback ordering, executable-candidate gates and scoring, variant extraction, command-name normalization, glob matching), each cited to its implementing `file:function`.

### Changed

- 2026-07-30: chore: renamed GitHub repo and project branding from `Wenget`/`WenPM` to lowercase `wenget` across README, CLAUDE.md, AGENTS.md, `.serena/project.yml`, Rust source doc comments, CLI output strings, and the HTTP `User-Agent` header (previously stale `WenPM/{version}`). Also fixed a pre-existing `wenpm` typo in `bucket del`/`bucket list` usage strings. GitHub repo moved from `superyngo/Wenget` to `superyngo/wenget` (case-insensitive redirects apply; existing binaries' hardcoded URLs keep working — verified `raw.githubusercontent.com` resolves case-insensitively). Historical CHANGELOG version entries are left unchanged.

## [3.8.4] - 2026-07-08

### Changed

- **Algorithmic complexity and allocation reductions on install/update hot paths**:
  - Removed per-binary re-read and re-parse of `installed.json` from the install loop; the caller's in-memory manifest snapshot is now reused, eliminating one disk read + JSON parse + migration walk per selected binary.
  - Added a name→entry index (`ManifestCache::packages_by_name`) and used it in `update.rs`, converting repeated O(installed × cache) linear scans into O(1) lookups during update-check filtering and bucket sync.
  - Command-name conflict resolution now builds a `HashSet` of taken names once per package instead of scanning every installed package's executables for each of up to 99 candidate suffixes.
  - `BinarySelector::extract_platforms` parses each release asset once instead of 11× (once per test platform), and drops a redundant unsupported-architecture rescan.
  - Variant filtering in the install path precomputes per-binary normalized names and variants once instead of recomputing them inside each filter pass.

## [3.8.3] - 2026-07-08

### Changed

- **Performance optimizations across I/O, HTTP, and concurrency hot paths**:
  - Batch `installed.json` writes: saves manifest once after all installs instead of per-package, eliminating O(n) full JSON serializations.
  - Shared HTTP client via `OnceLock`: reuses TCP/TLS connections across downloads instead of creating a new client per call.
  - Parallel bucket fetches in `rebuild_cache`: buckets fetched concurrently via `std::thread::scope`, reducing cache rebuild latency from sequential sum to max of fetch times.
  - Compact JSON serialization for `manifest-cache.json` (~30% smaller file).
  - Increased download buffer from 8 KB to 64 KB for fewer syscalls.
  - Removed unnecessary `.clone()` calls in update command.

## [3.8.2] - 2026-06-26

### Fixed

- **Version comparison for single package updates**: `wenget update <package>` now compares the target version with the installed version before updating. If the package is already up to date, it skips the installation rather than blindly upgrading/downgrading.
- **Correct download URL display when installing specific version**: `wenget add <package> -v <version>` now correctly displays the download URL for the target version during the planning phase, rather than showing the stale cached version's URL.

## [3.8.1] - 2026-06-22

### Fixed

- **`update self` honors `preferred_platform` for libc/compiler choice**: Self-update now applies `preferred_platform` (e.g. to pull a musl build on glibc) — but only when the override targets the same OS+arch as the host. A cross-OS/arch override is ignored with a notice, since replacing the running binary with one for another platform would break wenget.

## [3.8.0] - 2026-06-22

### Fixed

- **`install`/`update` now respect `preferred_platform` config**: The `preferred_platform` setting in `config.toml` was previously parsed but never applied, so installs and updates always used the auto-detected platform. It is now honored by both `wenget add` and `wenget update`. The setting accepts internal identifiers (e.g. `linux-aarch64-musl`) as well as Rust-style target triples (e.g. `aarch64-unknown-linux-musl`); when a libc/compiler variant such as `musl` is requested, that variant is preferred when available and otherwise falls back to a compatible build.

### Added

- **`wenget update -p/--platform <target>`**: `update` now accepts a platform override, matching `wenget add`. An explicit `-p` flag takes precedence over the `preferred_platform` config setting.

## [3.7.0] - 2026-06-12

### Changed

- **Cleaner progress bar during `wenget update`**: The update check phase now shows a single unified progress bar (`⠁ [====>] 24/24 checking for updates...`) that stays on screen after completion, instead of printing per-repo fetch info. Internal fetch messages moved to debug level (visible with `-v`).

## [3.6.0] - 2026-06-10

### Changed

- **Parallel update checks with a progress bar**: `wenget update` previously queried the GitHub API for each installed package one repo at a time, printing a line per repo. Update-info fetches now run concurrently (capped at 8 in-flight requests to stay within the unauthenticated rate limit) and the per-repo output is replaced by a single progress bar. All HTTP work happens on worker threads; cache mutations and any interactive prompts are still applied sequentially on the main thread, so behaviour (version comparison, cache fallback when the API is unavailable) is unchanged. Applies to both `wenget update` (all packages) and named updates such as `wenget update bun`.

## [3.5.0] - 2026-06-07

### Added

- **Derived download URL fallback for `--version` when the GitHub API is unavailable**: When installing a specific version of a bucket package with `-V/--version`, if the GitHub API call to fetch that release fails (e.g. rate limit), Wenget now derives the download URL by rewriting the cached package's URLs with the requested version (GitHub release assets always live at `.../releases/download/{tag}/{asset_name}`). This is a best-effort fallback validated by the download itself — it works for the common case (semver tags, asset names without or with the same version string) and fails cleanly with a 404 when a project uses an unusual tag scheme or changed its asset naming. Direct-repo packages are unaffected.

### Fixed

- **Update installing stale cached version when the GitHub API flakes mid-run**: `wenget update` previously made three independent GitHub API rounds per package (detection, install preview, install). If the API succeeded during detection but failed during installation, the install step silently fell back to the older bucket-cache version and reinstalled it. Now the version + download links detected via the API are synced into the cache up front, and in update mode the install step reads that freshly-synced cache instead of making redundant API calls — reducing API usage from 3 calls per bucket package to 1 and guaranteeing the detected version is the one installed. Direct-repo packages are unaffected (they always resolve live from the API).

## [3.4.1] - 2026-06-03

### Fixed

- **Stray `./` in symlink/shim targets from tar archives**: Tar archives that store entries with a leading `./` (e.g. `./agd`) no longer leak that prefix into recorded paths and launcher targets. Extraction now strips `CurDir` components, so links point to `apps/agd/agd` instead of `apps/agd/./agd`

## [3.4.0] - 2026-04-14

### Added

- **Magic bytes detection for executables**: `find_executable_candidates` now reads file headers (ELF, PE, Mach-O) and shebang lines to score candidates more accurately, reducing false positives from non-executable files
- **Disappeared executable handling during update**: When updating a package, Wenget now detects executables that were removed from the new release, auto-matches relocated files by filename, and prompts the user to pick a replacement when no auto-match is found

### Changed

- **Update mode no longer prompts for brand-new executables**: During updates, only previously installed executables are preserved; new executables are silently ignored to avoid unexpected additions

## [3.3.3] - 2026-04-10

### Added

- **`normalize_asset_for_matching` helper**: New utility function with tests for normalizing asset names when matching against installed variants

### Fixed

- **Variant selection during update**: Replaced `DEFAULT_VARIANT_SENTINEL` logic with asset-name template matching — update command now correctly identifies the originally installed asset using normalized name comparison instead of a sentinel constant

## [3.3.2] - 2026-04-10

### Fixed

- **`update` command now follows `installed.json`**: Variant selection and executable candidate selection are automatically matched against installed records instead of selecting all candidates

### Docs

- Added design spec for update command behavior redesign

## [3.3.1] - 2026-03-31

### Changed

- **`update -y` now respects previous installation choices**: When updating with `--yes`, binary variant selection and executable candidate selection are automatically matched against `installed.json` instead of selecting all. Command names continue to reuse old names as before.
- **`update -y` auto-overwrites local packages**: The local package overwrite prompt is now skipped with `--yes`, automatically upgrading locally installed packages when a cached version is available.

## [3.3.0] - 2026-03-30

### Fixed

- **Update fallback downgrade prevention**: When GitHub API is unavailable and `update` falls back to cache, versions are now compared to prevent downgrading to older cached versions

## [3.1.0] - 2026-03-18

### Added

- **Automatic version checking**: Wenget now automatically checks for updates on startup and notifies when a new version is available
- **Script update detection**: Added detection for script file updates, enabling automatic reinstallation when script sources change
- **Download URL tracking**: Added `download_url` field to `InstalledPackage` for tracking script source URLs

### Changed

- **Removed `update self` command**: Self-update is now automatically detected and prompted instead of requiring manual command
- **Upgrade success messages**: Updated to use `latest_version` for clearer messaging
- **Added design documentation**: New design docs included for architectural reference

## [3.2.0] - 2026-03-20

### Added

- **Command name preservation during update**: Update command with `-y` flag now preserves custom command names instead of reverting to defaults
- **Migration to executables HashMap**: Added automatic migration from `command_names` to `executables` HashMap in `installed.json`

### Changed

- **Executables HashMap migration**: Complete refactoring of command name storage system
  - Renamed command to use `executables` HashMap in all files
  - Update list command to use `get_command_names()` helper
  - Update add command to use `executables` map
  - Replace `files/command_names` with `executables` HashMap structure

### Fixed

- **Double deletion prevention**: Fixed bug where packages could be deleted twice when specifying variant explicitly
- **Clippy warning**: Fixed clippy warning in update cleanup loop

### Docs

- **Delete fix design documentation**: Added implementation plan and design spec for delete command fix and `installed.json` redesign

## [Unreleased]

## [3.0.4] - 2026-03-09

### Added

- **Enhanced Linux distribution detection**: Added detection for common Linux distribution names (ubuntu, debian, fedora, centos, alpine, opensuse, suse, gentoo, manjaro, archlinux) to better identify Linux binaries even without the "linux" keyword.

- **Arch Linux naming convention support**: Added special handling for Arch Linux distribution naming patterns (e.g., `_arch-`, `-arch-` in filenames like `youtube-tui-default_arch-x86_64`).

- **Bare binary detection**: Added detection for binaries without file extensions (e.g., `mytool-x86_64`) that use architecture keywords to indicate Linux platform.

## [3.0.3] - 2026-02-25

### Fixed

- **Platform detection for tar archives without OS keywords**: Fixed detection of Linux binaries from tar archives (`.tar.gz`, `.tar.xz`, `.tar.bz2`) that don't include OS keywords in their filenames (e.g., `nnn-static-5.2.x86_64.tar.gz`). These archives are now correctly identified as Linux when architecture keywords (x86_64, aarch64, etc.) are present.

### Added

- Added `nnn` repository to bucket sources
- Added `agm` repository to bucket sources

## [3.0.2] - 2026-02-14

### Fixed

- **`.tbz`/`.tgz` archive extraction**: Fixed `.tbz` and `.tgz` archives being misidentified as standalone executables instead of being extracted. The `is_standalone_executable()` check now recognizes these shorthand extensions.

### Added

- **`--no-suffix` flag for `add` command**: New `--no-suffix` flag prevents appending variant suffix to command names when installing variant packages. Falls back to numeric suffix if the base name conflicts.

- **Adaptive description truncation in `ls`**: Description column now adapts to terminal width instead of using fixed truncation limits. Also fixes potential panic on multi-byte UTF-8 characters by using char-based truncation.

## [3.0.1] - 2026-02-04

### Fixed

- **Rename command symlink preservation**: Fixed `wenget rename` to correctly preserve the original symlink/shim target by reading it before removal, instead of guessing which binary to link to. This ensures renamed commands continue pointing to the correct executable (e.g., `ffprobe` stays as `ffprobe`, not incorrectly changed to `ffmpeg`).

- **Tar archive symlink extraction**: Fixed extraction failure for archives containing symbolic links (e.g., FFmpeg shared builds). Previously, `fs::metadata()` would follow symlinks during permission setting, failing when the symlink target hadn't been extracted yet.

- **Executable detection false positives**: Fixed executables being incorrectly skipped when the archive path contained "test", "debug", "bench", or "example" (e.g., files under `*-latest-*/bin/` were skipped because "latest" contains "test"). The check now only examines the filename, not the full path.

### Changed

- **Increased bin/ directory confidence score**: Files in `bin/` directories now receive +40 points (up from +30) in executable detection scoring, making them more likely to be selected as the primary executable.

## [3.0.0] - 2026-02-02

### Fixed

- **Rename command variant matching**: Fixed `wenget rename` to properly match packages by repo_name, enabling renaming of variants using just the repo name (e.g., `wenget rn bun` now works for `bun::baseline-profile`)

### Changed

- **⚠️ BREAKING: User-level bin directory relocated for XDG compliance**
  - Unix/Linux user installs: `~/.wenget/bin/` → `~/.local/bin/`
  - Windows user installs: `%USERPROFILE%\.wenget\bin\` → `%USERPROFILE%\.local\bin\`
  - System-level installations (root/Administrator) remain unchanged
  - Aligns with XDG Base Directory specification on Unix-like systems
  - Improves cross-platform directory structure consistency
  
- **Migration Required for Existing Users**
  - No automatic migration - fresh installation recommended
  - Uninstall old version, remove old PATH entry, reinstall with new scripts
  - Packages must be reinstalled after migration

### Updated

- Installation scripts (`install.sh`, `install.ps1`) updated for new paths
- Documentation updated with migration instructions
- Unit tests updated to reflect new directory structure

## [2.3.1] - 2026-02-01

### Fixed

- **Windows Compilation Error**
  - Fixed missing `name` parameter in `create_shim` call within rename command
  - Resolves Windows build failures in GitHub Actions release workflow

## [2.3.0] - 2026-01-30

### Added

- **Windows Variant Path Compatibility**
  - Variant installation paths now use `-` separator instead of `::` (e.g., `apps/bun-baseline/` instead of `apps/bun::baseline/`)
  - Maintains `::` format for internal `installed.json` keys
  - Automatic migration of existing installations with `::` in paths on first load
  - Fixes Windows filesystem compatibility issues

- **User Configuration System (config.toml)**
  - New `wenget config` command to edit preferences in default editor ($EDITOR / nano / notepad)
  - Support for persistent user preferences in `~/.wenget/config.toml`
  - **Platform preference override**: Force specific platform builds (e.g., musl on glibc systems)
  - **Custom bin directory**: Override default bin location for custom PATH setups
  - Auto-generated config file with helpful comments and examples
  - Validation on save to catch configuration errors

- **Command Renaming**
  - New `wenget rename` command to rename installed commands without reinstalling
  - Direct mode: `wenget rename <old> <new>` - specify both names
  - Interactive mode: `wenget rename <package>` - select from multiple commands if applicable
  - Conflict detection prevents duplicate command names
  - Updates symlinks/shims and `installed.json` atomically
  - Preserves `repo_name` for proper update tracking

### Changed

- `WenPaths::app_dir()` now sanitizes path components by converting `::` to `-`
- `Config::new()` now loads user preferences from `config.toml` and applies custom bin directory if set
- Platform detection can now be overridden via `config.toml` preferences

### Fixed

- Windows: Package variants can now be installed without filesystem path errors
- Path sanitization prevents invalid characters in directory names across all platforms

## [2.2.3] - 2026-01-26

### Fixed

- **Variant Identification & Matching**
  - Improved version number filtering in `extract_variant_from_asset` to handle both `-` and `_` separators
  - Fixed incorrect variant identification when version numbers use underscores (e.g., `gh_2.86.0_linux_amd64.tar.gz`)
  - Single binary packages now correctly identified as default (no variant) when no filters are applied
  - Update command now correctly filters to only update already-installed variants, avoiding MultiSelect dialog
  - **Fixed delete command not finding packages when only variants are installed** - rm command now correctly matches repo name even when only variant packages exist (e.g., `wenget rm bun` now works when only `bun::baseline` is installed)

- **Add Command User Experience**
  - Add command now prompts to reinstall when same version is already installed (instead of silently skipping)
  - Update command respects variant-specific inputs (e.g., `wenget update bun::baseline` only updates that variant)

## [2.2.2] - 2026-01-24

### Fixed

- **Update Command Optimization**
  - Update command now refreshes bucket cache before checking versions
  - Added fallback to cache version when GitHub API fails, preventing silent skips
  - Improves reliability when API rate limits are exceeded or network issues occur

- **Delete Command Display Bug**
  - Fixed rm command not displaying variant packages when deleting by repo name
  - Previously, variant packages were skipped during display due to incorrect logic
  - Now correctly shows all matching packages before deletion confirmation

## [2.2.1] - 2026-01-23

### Fixed

- **Update Command Version Comparison** - Fix packages not being updated due to stale cache version
  - Version comparison in `add` command now fetches latest version from GitHub API first
  - Previously used cached bucket version for comparison, causing false "already up to date" results
  - Root cause: `find_upgradeable` correctly detected new versions, but `add` command compared against stale cache

## [2.2.0] - 2026-01-23

### Added

- **Variant Handling Enhancements**
  - Added support for `repo::variant` format in package resolution
  - Improved `wenget add` to correctly check for existing variant installations
  - Enhanced `wenget del` to match against repository names, enabling bulk deletion of all variants for a repository

### Changed

- **Command Name Resolution**
  - Renamed `-n/--name` parameter to `-c/--command` in `add` command for better clarity (Breaking change)
  - Custom command names (via `-c/--command`) now skip variant suffixing and use the exact user-specified name (with conflict resolution if needed)

### Fixed

- Fixed unused variable warning in `src/commands/add.rs`
- Fixed `--variant` + `-c/--command` behavior to ensure custom names are respected without unnecessary suffixes

## [2.1.0] - 2026-01-23

### Added

- **Variant Filtering** - Added `--variant` parameter to `add` and `del` commands
  - `wenget add bun --variant baseline` - Install only the baseline variant
  - `wenget del bun --variant profile` - Delete only the profile variant
  - Shows available variants when specified variant is not found
  - Helps manage packages with multiple release binaries more precisely
  - Note: Only long form `--variant` is available (no short flag to avoid conflict with `--version`)

- **7z Archive Support** - Added support for `.7z` archive extraction
  - Added `sevenz-rust` crate dependency
  - New `extract_7z()` function in `src/installer/extractor.rs`
  - Automatically sets executable permissions on Unix for extracted binaries

- **tar.bz2 Archive Support** - Added support for `.tar.bz2` and `.tbz` archive extraction
  - Added `bzip2` crate dependency
  - New `extract_tar_bz2()` function in `src/installer/extractor.rs`

### Changed

- **Variant Command Naming** - Fixed command name conflict resolution for variants

### Fixed

- **Info Platform Filtering** - Fixed `info` command to only show `[Installed]` markers for packages installed on the current platform
  - Previously showed `[Installed]` for all platforms when a package was installed
  - Now correctly filters by comparing `inst_pkg.platform` with displayed platform

- **Variant Symlink Creation** - Fixed symlink/shim naming for package variants
  - Previously created symlinks using base name before variant resolution
  - Now resolves variant-suffixed names before creating symlinks
  - **Smart variant suffix handling**: Avoids duplicate suffixes when binary name already contains variant info
    - Example: `bun::profile` with binary `bun-profile` → creates `bun-profile` (not `bun-profile-profile`)
    - Example: `bun::baseline-profile` with binary `bun-profile` → creates `bun-baseline-profile`
    - Example: `bun::baseline` with binary `bun` → creates `bun-baseline`
  - Variants now **always** use `{base}-{variant}` format (e.g., `bun-baseline`)
  - Default (non-variant) packages use plain `{base}` name (e.g., `bun`)
  - Previous behavior: default could be pushed to `bun-1` when variants existed
  - New behavior: ensures predictable and stable command naming

- **Info Command Enhancement** - Improved variant display in `wenget info`
  - Shows all variant command names in "Command name(s)" field
  - Displays detailed variant list with version and command mappings
  - Supported platforms section now shows variant labels (e.g., `[baseline]`, `[profile]`)
  - Indicates installation status for each variant: `[Installed: bun-baseline]`

- **Variant Handling Refactor** - Major refactoring of package variant handling
  - Added `repo_name` and `variant` fields to `InstalledPackage` struct
  - Changed installed.json key format from `{repo}-{variant}` to `{repo}::{variant}` for clarity
  - Deprecated `parent_package` field (kept for backward compatibility)
  - New `group_by_repo()` and `find_by_repo()` helper methods on `InstalledManifest`
  - Command name conflict resolution: automatically appends variant suffix when command names clash
  - Update command now uses `repo_name` for cache lookup (fixes update failures for variant packages)
  - Delete command now supports `wenget del repo::variant` syntax for deleting specific variants
  - List command shows grouped variants under their repository name with tree structure
  - Automatic migration from old format on first load

- **Enhanced Installation Summary** - Installation summaries now display package names
  - Summary now shows: `✓ 2 package(s) installed: ripgrep fd`
  - Applies to all installation methods: packages, scripts, local files, and URLs
  - Failed packages are also listed by name for easier troubleshooting

## [2.0.2] - 2026-01-19

### Fixed

- **ARM Platform Detection** - Fix `arm-unknown-linux-musleabihf` binaries being misclassified
  - Added "arm" keyword to Armv7 architecture detection in `src/core/platform.rs`
  - Previously, files like `uv-arm-unknown-linux-musleabihf.tar.gz` would incorrectly appear under x86_64, i686, and aarch64 platforms
  - Now correctly classified under armv7 platforms only
  - Note: Existing bucket manifests need to be regenerated to reflect this fix

- **Package Version "unknown"** - Fix version showing "unknown" after installation
  - Added `version` field to `Package` struct in `src/core/manifest.rs`
  - GitHub API responses now populate version directly in the Package object
  - `add.rs` now uses package.version instead of making extra API calls to fetch version
  - Reduces GitHub API usage and eliminates version lookup failures during rate limiting
  - Bucket manifests can now optionally include version information

- **Update Manifest Workflow** - Fix detection of new manifest.json when file was deleted
  - Changed `git diff` to `git add` + `git diff --staged` to detect untracked new files

## [2.0.1] - 2026-01-18

### Fixed

- **Batch Installation Error Handling** - Single package failure no longer interrupts entire operation
  - Modified `add` command to continue processing remaining packages when one fails
  - Changed `?` operators to explicit error handling with `fail_count` tracking
  - Applies to: script installations, local file installations, URL installations, and package installations
  - Failed packages are now reported in summary instead of aborting the entire batch
  - Improves `update` command behavior: failed package updates no longer block other updates

- **Release Workflow** - Fix "Update bucket binary" step failing due to .gitignore
  - Changed `git add` to `git add -f` to force-add ignored bucket/wenget binary

## [2.0.0] - 2026-01-16

### Added

- **Windows ARM64 Build Support** - Added aarch64-pc-windows-msvc target to CI/CD pipeline
  - New build artifact: `wenget-windows-aarch64.exe`
  - Supports Windows ARM64 devices (Snapdragon X Elite/Plus laptops, Windows Dev Kit 2023)
  - Uses conservative optimization settings to avoid antivirus false positives (same as other Windows builds)
  - Platform detection already supports ARM64 via existing `aarch64`/`arm64` keywords
  - Includes fallback to x86_64 emulation when ARM64 binary unavailable
  - Total build targets increased from 12 to 13 platforms

- **Version Selection** - Add `-v/--ver` flag to install specific package versions
  - Usage: `wenget add ripgrep -v 14.0.0` or `wenget add ripgrep --ver v14.0.0`
  - Supports both `v1.0.0` and `1.0.0` formats (automatically handles 'v' prefix)
  - Shows clear error message if specified version doesn't exist
  - Note: Use `--verbose` (no short form) for verbose logging to avoid flag conflicts

### Changed

- **Multiple Executable Detection** - Auto-install all executables with valid permissions
  - Packages like `uv` (containing both `uv` and `uvx`) now install all executables automatically
  - Changed from selecting only top-scoring candidate (score >= 80) to all candidates with score > 0
  - Auto-selects up to 3 executables; shows interactive menu for more than 3 (unless `--yes` flag)
  - Captures executables with execution permission even if name doesn't match package name

- **Architecture Filtering** - Improved unsupported architecture detection
  - Expanded UNSUPPORTED_ARCHS list to include all PowerPC variants (ppc, powerpc, powerpc64, powerpc64le)
  - Added RISC-V variants (riscv, riscv32, riscv64)
  - Added MIPS variants (mips, mips64, mipsel, mips64el)
  - Added exotic architectures (alpha, sh4, hppa, ia64, loong64, loongarch64, s390)
  - Prevents misclassification of unsupported binaries (e.g., `uv-powerpc64-unknown-linux-gnu.tar.gz` no longer incorrectly categorized under `linux-x86_64-gnu`)
  - Added pattern detection for unknown architecture-like keywords
  - Windows ARM64 support confirmed in test platforms

### Fixed

- Resolved clippy warning for too many function arguments

### Technical

- Modified executable selection logic in `src/commands/add.rs`
- Added `fetch_release_by_tag()` method to `GitHubProvider` for version-specific fetches
- Added `fetch_package_by_version()` method to fetch packages for specific versions
- Updated install flow to use custom version when specified
- Enhanced `ParsedAsset::contains_unknown_arch_pattern()` to detect unrecognized architecture patterns
- Updated architecture matching logic to check for unsupported patterns before falling back to OS defaults
- Code formatting improvements and clippy compliance

## [1.3.3] - 2026-01-16

### Changed

- **Platform Detection** - Improved architecture keyword matching
  - Added `386` keyword for I686 architecture detection
  - Added `armv6` keyword for Armv7 architecture detection
  - Better compatibility with Go-style binary naming conventions

## [1.3.2] - 2026-01-15

### Added

- **Interactive Self-Deletion Menu** - Granular control over what gets removed
  - New interactive menu when running `wenget del self` (without `-y` flag)
  - Users can choose which components to remove:
    - Apps & data (~/.wenget/)
    - PATH configuration
    - Wenget binary
  - Multiple selections supported via checkboxes
  - `-y` flag still removes everything for non-interactive use

### Fixed

- **Package Name Matching** - Fixed update command for packages with platform suffixes
  - `wenget update` now correctly finds packages even when installed with variant suffixes (e.g., "uv-pc")
  - Fallback matching strips trailing platform-related suffixes to find base package name
  - Handles cases where old installations have platform identifiers in the package name

- **Self-Update Platform Detection** - Better binary matching for self-updates
  - `wenget update self` now uses same smart platform matching as `add` command
  - Includes libc detection (musl vs glibc), compiler variants, and fallback support
  - Shows informative messages when using compatible fallback binaries
  - More reliable updates across different Linux distributions

### Technical

- Added `show_removal_menu()` function using `dialoguer::MultiSelect`
- Added `RemovalOptions` struct for tracking user's deletion preferences
- Enhanced `extract_variant_from_asset()` to include "pc" suffix (common in Rust target triples)
- Refactored self-update to use `Platform::find_best_match()` API

## [1.3.1] - 2026-01-14

### Fixed

- **CI/CD Workflow Improvements**
  - Added rebase before push in update-manifest workflow to prevent conflicts
  - Fixed bucket binary push logic in release workflow
  - Improved manifest update automation reliability

## [1.3.0] - 2026-01-14

### Added

- **Integrated Bucket Repository** - Merged official bucket repository into main repo
  - New `bucket/` directory containing package manifests
  - Includes README with bucket usage instructions
  - Centralized manifest management in the main repository
  - Simplifies bucket maintenance and distribution

### Changed

- Bucket manifest structure now co-located with main codebase
- Official bucket available directly from the main repository

## [1.2.0] - 2026-01-14

### Added

- **Multi-Package Variant Support** - Install multiple binary variants from a single package
  - Each platform can now have multiple binaries (e.g., baseline, desktop, musl, gnu variants)
  - Interactive MultiSelect dialog to choose which variants to install
  - `--yes` flag installs all variants automatically
  - New `parent_package` tracking for variant relationships
  - New `asset_name` field to track original asset filenames

- **Variant-Aware Commands** - All commands now understand package variants
  - `list`: Tree structure display showing parent packages and their variants (├─, └─)
  - `delete`: Select which variants to remove with MultiSelect dialog
  - `update`: Automatically includes all variants when updating a parent package
  - `info`: Shows variant information and displays multiple packages per platform

- **AGENTS.md** - AI coding agent guidelines for this repository
  - Build/test commands, code style guidelines, and release workflow

### Changed

- **Manifest Structure** - `platforms` now stores `Vec<PlatformBinary>` instead of single `PlatformBinary`
  - Bucket manifests capture ALL available variants for each platform
  - Better support for projects with multiple build configurations

- **Info Command UI** - Beautiful box-style header with improved formatting

- **Input Detection** - GitHub repo URLs now correctly treated as package names
  - Distinguishes between `github.com/owner/repo` (package) and `/releases/download/` (direct URL)

- **Download URL Display** - Shows all download URLs when multiple packages available

### Technical

- Added `extract_variant_from_asset()` function for variant name extraction
- Added `generate_installed_key()` function for consistent installed.json keys
- `PlatformBinary` now includes `asset_name` field
- `InstalledPackage` now includes `asset_name` and `parent_package` fields
- `BinarySelector::extract_platforms()` returns `HashMap<String, Vec<BinaryAsset>>`
- Fixed clippy warnings: `is_some_and()` and `or_default()` usage

## [1.1.1] - 2026-01-12

### Fixed

- **Token Propagation in Bucket Create** - Fixed GitHub token not being passed to GitHubProvider
  - `ManifestGenerator::with_token()` now correctly passes token to `GitHubProvider::with_token()`
  - Previously token was only passed to `HttpClient`, causing API rate limiting in CI/CD
  - This fixes Gist and repo fetching failures when using `bucket create` with authentication

### Documentation

- Updated README with comprehensive `bucket create` command documentation
  - Added usage examples for token authentication
  - Documented update mode options (overwrite/incremental)
  - Added all available command flags and options

## [1.1.0] - 2026-01-12

### Added

- **GitHub Token Support for Bucket Creation** - Higher API rate limits for manifest generation
  - New `-t, --token` option for `bucket create` command
  - Automatically reads `GITHUB_TOKEN` environment variable if no token provided
  - Authenticated requests get 5,000 requests/hour vs 60/hour unauthenticated
  - Token now properly passed to all GitHub API calls (repos, releases, gists)

- **Update Mode for Manifest Generation** - Control how existing manifests are updated
  - New `-u, --update-mode` option with two modes:
    - `overwrite`: Replace entire manifest file (default behavior)
    - `incremental`: Merge with existing manifest, keeping entries not in current run
  - Enables CI/CD pipelines to run non-interactively with `--update-mode overwrite`

- **Uncompressed Binary Support** - Install binaries without archive wrappers
  - Detects platform-specific binaries without file extensions (e.g., `m3u8-linux-amd64`)
  - Recognizes binaries with platform keywords (linux, darwin, windows, x86_64, amd64, etc.)
  - Properly handles repos like [llychao/m3u8-downloader](https://github.com/llychao/m3u8-downloader)

- **Enhanced Info Command** - Shows info for manually installed packages
  - Now displays details for packages installed via direct URL or local script
  - Falls back to `installed.json` when package not found in cache
  - Shows source type, origin URL, command name, and installed files

- **Download URL Display** - Shows resolved download URLs during installation
  - Displays the actual binary URL before confirmation
  - Helps users verify what will be downloaded

### Fixed

- **Token Propagation** - `GitHubProvider` now accepts token for authenticated API calls
  - Previously `bucket create` passed token to `HttpClient` but not to `GitHubProvider`
  - This caused API rate limiting issues in CI/CD environments

- **Bucket Fetch Timeout** - Reduced timeout from 30s to 10s for faster failure detection
  - Improves responsiveness when bucket URLs are unreachable

### Technical

- Added `GitHubProvider::with_token()` constructor for authenticated API access
- Added `FileExtension::UncompressedBinary` variant for extensionless binaries
- Added `is_likely_binary_without_extension()` helper for binary detection
- New `display_installed_only_info()` function in info command
- All 72+ unit tests passing

## [1.0.0] - 2026-01-05

### 🎉 Stable Release

This marks the first stable release of Wenget! After extensive development and testing,
Wenget is now production-ready for managing GitHub binaries across platforms.

### Added

- **User Confirmation Utilities** - New `utils/prompt` module
  - `confirm()` function for [Y/n] prompts
  - `confirm_no_default()` function for [y/N] prompts
  - Reduces code duplication across command modules

- **Interpreter Caching** - Performance optimization for script support
  - Cached interpreter availability detection using `OnceLock`
  - PowerShell, Bash, and Python availability checked once per session
  - Significantly faster script compatibility checks

- **Batch Script Path Escaping** - Security improvement
  - New `escape_batch_path()` function for special character handling
  - Properly escapes `&`, `|`, `<`, `>`, `^`, `%`, `!` in paths
  - Prevents potential command injection in Windows batch shims

### Changed

- **Removed Default Trait Panic Risk**
  - Removed `impl Default for Config` to avoid panic on home directory detection failure
  - Removed `impl Default for WenPaths` for the same reason
  - Applications should use explicit `Config::new()` and `WenPaths::new()` calls

- **Script Preference Order** - Extracted to shared function
  - New `ScriptType::preference_order()` returns platform-specific script preference
  - Windows: PowerShell > Batch > Python > Bash
  - Unix: Bash > Python > PowerShell
  - Eliminates code duplication between `get_compatible_script()` and `get_installable_script()`

- **Registry Operations Refactored** (Windows)
  - Extracted shared `modify_system_path_inner()` function
  - Reduced code duplication between `add_to_system_path()` and `remove_from_system_path()`

### Fixed

- **Improved Error Logging**
  - Backup failures now logged with `log::warn!` instead of silent ignore
  - File cleanup failures (temp files, old executables) now logged
  - Better debugging experience for troubleshooting

- **Clippy Warnings Resolved**
  - Collapsed nested `if` statements for better readability
  - Removed unused imports across command modules
  - All clippy warnings addressed

### Technical

- **Code Quality Improvements**
  - Reduced ~120 lines of duplicated code
  - Added ~80 lines of new utility functions
  - All 68 unit tests passing
  - No clippy warnings in codebase

### Documentation

- Added ExecutionPolicy Bypass explanation in script shim generation
- Improved comments for `unreachable!()` macro usage
- Updated inline documentation for new utility functions

## [0.9.1] - 2026-01-05

### Fixed

- Resolved clippy warnings for better code quality

## [0.9.0] - 2026-01-03

### Added

- **Multi-Platform Variant Support** - Bucket manifests now include ALL platform variants
  - `bucket create` now collects all available platform variants (musl, gnu, msvc, etc.) instead of selecting only the highest-scored one
  - Manifests can now contain both `linux-x86_64-musl` and `linux-x86_64-gnu` simultaneously
  - Enables users to choose their preferred variant during installation

- **Smart Platform Fallback System** - Intelligent platform matching with cross-compatibility
  - Automatically suggests compatible fallback platforms when exact match isn't available
  - **Architecture Fallback**:
    - 64-bit systems can install 32-bit binaries (with user confirmation)
    - macOS ARM (Apple Silicon) can install x86_64 binaries via Rosetta 2 (with user confirmation)
    - Windows ARM can install x86_64 binaries via emulation (with user confirmation)
  - **Compiler/libc Fallback**:
    - Linux systems can use musl binaries on glibc systems (automatic, statically linked)
    - Linux systems can use glibc binaries on musl systems (with user confirmation)
    - Windows systems can use different compiler variants (automatic)
  - Clear user prompts explaining the fallback type and compatibility implications
  - Preserves existing scoring-based platform preference system

- **New Platform Matching API**
  - Added `FallbackType` enum to categorize different fallback scenarios
  - Added `PlatformMatch` struct to provide detailed matching information
  - Added `Platform::find_best_match()` for intelligent platform selection
  - Added `BinarySelector::select_all_for_platform()` to retrieve all matching variants
  - Added `ScriptItem::get_installable_script()` for proper interpreter verification during installation

### Changed

- **Enhanced `extract_platforms()`** - Now returns all platform variants instead of only the best match
- **Improved Installation Flow** - Uses new `find_best_match()` API for smarter platform selection
- **Better User Experience** - Informative messages when using fallback platforms

### Fixed

- **Script Platform Compatibility** - Fixed incorrect script platform detection on Windows
  - `list --all` and `info` commands no longer show Bash scripts as compatible on Windows unless Bash is actually installed
  - Separated `is_os_compatible()` (for display) from `is_supported_on_current_platform()` (for installation verification)
  - Installation now properly checks if script interpreter exists before allowing installation

### Technical

- Added comprehensive test coverage for multi-platform and fallback scenarios
- Added `#[allow(dead_code)]` annotations for future-facing APIs
- Fixed all clippy warnings and code formatting issues
- Updated internal platform matching logic to support multiple variants per OS/architecture combination

## [0.8.0] - 2026-01-03

### Added

- **System-Level Installation** - Install scripts now auto-detect elevated privileges
  - Linux/macOS: Running as root installs to `/opt/wenget/app` with symlinks in `/usr/local/bin`
  - Windows: Running as Administrator installs to `%ProgramW6432%\wenget` with system PATH
  - User-level installation remains the default behavior

### Changed

- Refactored bucket management system
- Improved core paths module architecture
- Updated documentation with system-level installation guide

## [0.7.2] - 2025-12-30

### Fixed

- Windows compatibility improvements
- Minor bug fixes and code cleanup

## [0.7.1] - 2025-12-30

### Fixed

- **Linux Self-Update** - Resolved "Text file busy" error on Alpine Linux and other Unix systems
  - Implemented robust atomic rename strategy for updating the running executable
  - Added fallback mechanism to copy if rename fails (cross-filesystem)
  - Improved permission handling and error recovery during updates
- **Code Maintenance**
  - Fixed various clippy warnings and unused imports
  - Improved code hygiene in installer and command modules

## [0.7.0] - 2025-12-30

### Added

- **Platform Selection** - Explicit platform selection for installations
  - Added `-p`/`--platform` flag to `add` command
  - Allows installing binaries for specific platforms (e.g., `linux-x64`)
  - Supports both package and manual URL installations

- **Universal Installation Support** - Complete "Install anything" capability
  - **Local Binaries**: Install local `.exe` or binary files directly (`wenget add ./mytool.exe`)
  - **Local Archives**: Install from local `.zip`/`.tar.gz` (`wenget add ./tools.zip`)
  - **Direct URLs**: Install binaries/archives from any URL (`wenget add https://example.com/tool.zip`)
  - All installations generate shims and integrate seamlessly

- **UX Enhancements**
  - **Command Aliases**: Added convenient short aliases
    - `i` for `info`
    - `rm`, `uninstall` for `del`
  - **Source Visibility**: `wenget list --all` now shows the `SOURCE` column
    - Identify packages from Buckets, Direct URLs, or Scripts instantly

## [0.6.3] - 2025-12-08

### Fixed

- 修復 Linux 平台 update self 功能
- Removed unsupported architectures: s390x, ppc64, ppc64le, riscv64, mips
- Code formatting and clippy linting improvements

### Backward Compatible

- Platform string format unchanged: {os}-{arch} or {os}-{arch}-{compiler}
- Existing manifests continue to work
- New compiler-specific keys are additive

## [0.6.2] - 2025-12-08

### Fixed

- Minor bug fixes and improvements

## [0.6.1] - 2025-12-08

### Fixed

- Code quality improvements
  - Fixed clippy warnings for dead code in tests
  - Fixed pointer argument linting (PathBuf → Path)
  - Added allow attributes where appropriate
- Enhanced code formatting compliance with cargo fmt

## [0.6.0] - 2025-12-07

### Added

- **Advanced platform detection system** - Refactored binary matching logic for better compatibility
  - New 4-component parsing: file extension + OS + architecture + compiler/libc
  - `Compiler` enum supporting GNU, musl, and MSVC variants
  - Context-aware `x86` keyword resolution (macOS → x86_64, others → i686)
  - FreeBSD support with explicit architecture requirement
  - Compiler priority system: Linux prefers musl > gnu, Windows prefers msvc > gnu

### Improved

- **Default architecture handling** - Intelligent fallback for ambiguous binaries
  - Windows/Linux default to x86_64 when architecture not specified
  - macOS defaults to aarch64 (Rosetta 2 can run x86_64 binaries)
  - FreeBSD requires explicit architecture (no default)
  - Explicit architecture matches scored higher than defaults

### Changed

- **Platform detection scoring** - New 4-component scoring algorithm

  - OS match: +100 (mandatory)
  - Explicit arch match: +50
  - Default arch match: +25
  - Compiler priority: +10/20/30 based on OS preference
  - File format: +2 to +5

- Complete refactor of `src/core/platform.rs` with `ParsedAsset` struct
- Added `FileExtension` enum for archive format detection
- Added 17 comprehensive test cases for platform detection

## [0.5.3] - 2025-12-03

### Added

- **Fallback platform detection** - Intelligent handling of release files with ambiguous names
  - Added fallback OS keywords: "win", "mac", "osx", "msvc" for broader matching
  - Automatic architecture assumption when explicit info is missing:
    - Windows/Linux without arch → assumes x86_64 (most common)
    - macOS without arch → assumes aarch64 (Apple Silicon standard)
  - Fallback matches scored lower (125 points) than exact matches (150 points)
  - Warning messages displayed when using fallback assumptions
  - Enables detection of packages like `gitui-win.tar.gz` and `app-mac.zip`

### Fixed

- **Platform detection for ambiguous filenames** - Files like `gitui-win.tar.gz` are now correctly detected
  - Previously required explicit architecture in filename (e.g., `win64`, `x86_64`)
  - Now supports generic OS-only filenames with intelligent fallback
  - Maintains preference for explicitly-named binaries over fallback matches

### Changed

- **.msi file handling** - Removed support for .msi installer packages
  - .msi files now properly excluded from binary selection
  - Focuses on portable archive formats (tar.gz, zip, exe)
  - Avoids conflicts with Windows installer packages that need special handling

### Technical

- Enhanced `BinarySelector::score_asset()` with 2-tier detection logic
- Added `test_fallback_detection_gitui()` test case for validation
- Scoring system: Exact match (OS+Arch=150) > Fallback (OS=100, Fallback Arch=25) > No match

## [0.5.2] - 2025-12-03

### Improved

- **Script installation UX** - Now displays "Command will be available as:" message during script installation
  - Consistent with package installation behavior
  - Shows the command name that will be used to invoke the script
  - Applied to both direct script installations and bucket script installations

### Changed

- **Script filtering in list --all** - Improved platform compatibility filtering
  - Added `is_os_compatible()` method for basic OS compatibility checking
  - Scripts now filtered by native OS support without executing interpreter checks
  - Significantly faster performance (no command execution during listing)
  - Consistent with package filtering behavior (platform-based, not runtime-based)
  - Windows shows PowerShell/Batch/Python scripts only
  - Unix-like systems show Bash/Python scripts only

### Technical

- Script filtering now uses compile-time platform checks instead of runtime interpreter checks
- More efficient `list --all` command with no external command execution

## [0.5.1] - 2025-12-03

### Fixed

- **Script display in list command** - `list --all` now correctly shows scripts from buckets
  - Added TYPE column to distinguish between binaries and scripts
  - Scripts filtered by platform compatibility (PowerShell, Bash, Python, Batch)
  - Fixed issue where scripts were being filtered out due to missing platform field

### Changed

- **List output format** - Added TYPE column showing "binary" for packages and script type for scripts
  - Binary packages shown in cyan
  - Script types shown in magenta (powershell, bash, python, batch)
- **Summary statistics** - Now shows "X package(s), Y script(s) available" format

## [0.5.0] - 2025-12-02

### Added

- **Bucket Script Support** - Install and manage scripts directly from buckets

  - Support for PowerShell (.ps1), Bash (.sh), Batch (.bat/.cmd), and Python (.py) scripts
  - Automatic script type detection and platform compatibility checking
  - Scripts displayed separately in search results with type badges

- **Script Installation** - Multiple installation methods

  - Install from local files: `wenget add ./script.ps1`
  - Install from URLs: `wenget add https://example.com/script.sh`
  - Install from buckets: `wenget add script-name`

- **Smart Command Naming** - Automatic executable name normalization
  - Removes platform suffixes (e.g., `ripgrep-x86_64` → `ripgrep`)
  - Removes architecture indicators (e.g., `tool_amd64` → `tool`)
  - Cleans up file extensions intelligently
  - Custom naming support: `--name custom-command`

### Enhanced

- **Search Command** - Now searches both packages and scripts

  - Separate sections for "Binary Packages" and "Scripts"
  - Shows script type and description for each result

- **Info Command** - Extended to support scripts

  - Displays script-specific metadata (type, URL, platform support)
  - Shows installation status for both packages and scripts

- **List Command** - Enhanced display format

  - Shows command name alongside package name
  - Improved column alignment and truncation
  - Better visual distinction between installed and available items

- **Add Command** - Unified installation interface
  - Detects input type automatically (package name, URL, or script)
  - Mixed installations supported: `wenget add package1 ./script.sh url`
  - Security warnings for script installations
  - Separate success/failure counts for packages and scripts

### Improved

- **Cache System** - Script awareness

  - Scripts cached alongside packages for fast searches
  - Script-specific cache invalidation
  - Platform compatibility filtering

- **Error Handling** - Better script installation feedback
  - Clear messages for unsupported script types
  - Platform compatibility warnings
  - Detailed installation failure reasons

### Technical

- **Architecture** - New script management infrastructure
  - `ScriptItem` type for bucket scripts
  - `ScriptType` enum with platform detection
  - Script shim/launcher creation system
  - Unified package source tracking (Bucket/DirectRepo/Script)

## [0.4.0] - 2025-12-01

### Added

- **Self-update capability** - `wenget update self` command to upgrade Wenget itself
  - Automatic version detection from GitHub releases
  - Platform-specific binary selection
  - Smart executable replacement for Windows and Unix systems
  - Automatic cleanup of old versions

### Improved

- **Windows**: Special handling for locked executables with background cleanup script
- **Unix/Linux/macOS**: Direct executable replacement with permission management
- **Error handling**: Comprehensive error messages and validation

### Documentation

- Updated README with self-update instructions
- Added usage examples for the new command

## [0.3.0] - 2025-11-25

### Changed

- **Remove `source` command** - Eliminated sources.json and all source management
- **Smart `add` command** - Auto-detects package names vs GitHub URLs
- **New `info` command** - Query package details (supports names and URLs)
- **Enhanced `list` command** - Now shows SOURCE column and descriptions
- **Package descriptions** - Stored in installed.json for faster access
- **Integrated resolver** - Name-based operations work for URL-installed packages
- **Improved UX** - Better alignment and formatting in list output

### Breaking Changes

- `source` command removed entirely
- installed.json format changed (added description field)
- Old installed.json files need migration (reinstall packages)

## [0.2.0] - 2025-01-21

### Added

- Installation scripts for Windows and Unix
- Improved init bucket checking

### Fixed

- Self-deletion when executable is inside .wenget
- Shim absolute path issues

## [0.1.0] - 2025-01-21

### Added

- Initial release
- Basic package management
- Bucket system
- Cross-platform support (Windows, macOS, Linux)
- Platform detection and binary selection
- GitHub integration
- Package cache system

[2.3.1]: https://github.com/superyngo/wenget/compare/v2.3.0...v2.3.1
[2.3.0]: https://github.com/superyngo/wenget/compare/v2.2.3...v2.3.0
[2.2.3]: https://github.com/superyngo/wenget/compare/v2.2.2...v2.2.3
[2.2.2]: https://github.com/superyngo/wenget/compare/v2.2.1...v2.2.2
[2.2.1]: https://github.com/superyngo/wenget/compare/v2.2.0...v2.2.1
[2.2.0]: https://github.com/superyngo/wenget/compare/v2.1.1...v2.2.0
[2.1.1]: https://github.com/superyngo/wenget/compare/v2.1.0...v2.1.1
[2.1.0]: https://github.com/superyngo/wenget/compare/v2.0.2...v2.1.0
[2.0.2]: https://github.com/superyngo/wenget/compare/v2.0.1...v2.0.2
[2.0.1]: https://github.com/superyngo/wenget/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/superyngo/wenget/compare/v1.3.3...v2.0.0
[1.3.3]: https://github.com/superyngo/wenget/compare/v1.3.2...v1.3.3
[1.3.2]: https://github.com/superyngo/wenget/compare/v1.3.1...v1.3.2
[1.3.1]: https://github.com/superyngo/wenget/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/superyngo/wenget/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/superyngo/wenget/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/superyngo/wenget/compare/v1.1.0...v1.1.1
[2.3.1]: https://github.com/superyngo/wenget/compare/v2.3.0...v2.3.1
[2.3.0]: https://github.com/superyngo/wenget/compare/v2.2.0...v2.3.0
[2.2.0]: https://github.com/superyngo/wenget/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/superyngo/wenget/compare/v2.0.0...v2.1.0
[2.0.0]: https://github.com/superyngo/wenget/compare/v1.1.0...v2.0.0
[1.1.0]: https://github.com/superyngo/wenget/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/superyngo/wenget/compare/v0.9.1...v1.0.0
[0.9.1]: https://github.com/superyngo/wenget/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/superyngo/wenget/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/superyngo/wenget/compare/v0.7.2...v0.8.0
[0.7.2]: https://github.com/superyngo/wenget/compare/v0.7.1...v0.7.2
[0.7.1]: https://github.com/superyngo/wenget/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/superyngo/wenget/compare/v0.6.3...v0.7.0
[0.6.3]: https://github.com/superyngo/wenget/compare/v0.6.2...v0.6.3
[0.6.2]: https://github.com/superyngo/wenget/compare/v0.6.1...v0.6.2
[0.6.1]: https://github.com/superyngo/wenget/compare/v0.6.0...v0.6.1
[0.6.0]: https://github.com/superyngo/wenget/compare/v0.5.3...v0.6.0
[0.5.3]: https://github.com/superyngo/wenget/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/superyngo/wenget/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/superyngo/wenget/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/superyngo/wenget/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/superyngo/wenget/compare/v0.3.1...v0.4.0
[3.0.4]: https://github.com/superyngo/wenget/compare/v3.0.3...v3.0.4
[3.0.3]: https://github.com/superyngo/wenget/compare/v3.0.2...v3.0.3
[3.0.2]: https://github.com/superyngo/wenget/compare/v3.0.1...v3.0.2
[3.0.0]: https://github.com/superyngo/wenget/compare/v2.3.1...v3.0.0
[0.2.0]: https://github.com/superyngo/wenget/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/superyngo/wenget/releases/tag/v0.1.0
[3.1.0]: https://github.com/superyngo/wenget/compare/v3.0.4...v3.1.0
[3.2.0]: https://github.com/superyngo/wenget/compare/v3.1.0...v3.2.0
[3.3.0]: https://github.com/superyngo/wenget/compare/v3.2.0...v3.3.0
[3.3.3]: https://github.com/superyngo/wenget/compare/v3.3.2...v3.3.3
[3.3.2]: https://github.com/superyngo/wenget/compare/v3.3.1...v3.3.2
[3.3.1]: https://github.com/superyngo/wenget/compare/v3.3.0...v3.3.1
[3.4.0]: https://github.com/superyngo/wenget/compare/v3.3.3...v3.4.0
[3.4.1]: https://github.com/superyngo/wenget/compare/v3.4.0...v3.4.1
[3.5.0]: https://github.com/superyngo/wenget/compare/v3.4.1...v3.5.0
[3.6.0]: https://github.com/superyngo/wenget/compare/v3.5.0...v3.6.0
[3.7.0]: https://github.com/superyngo/wenget/compare/v3.6.0...v3.7.0
[3.8.0]: https://github.com/superyngo/wenget/compare/v3.7.0...v3.8.0
[3.8.1]: https://github.com/superyngo/wenget/compare/v3.8.0...v3.8.1
[3.8.2]: https://github.com/superyngo/wenget/compare/v3.8.1...v3.8.2
[3.8.3]: https://github.com/superyngo/wenget/compare/v3.8.2...v3.8.3
[3.8.4]: https://github.com/superyngo/wenget/compare/v3.8.3...v3.8.4
[3.8.5]: https://github.com/superyngo/wenget/compare/v3.8.4...v3.8.5
[3.8.6]: https://github.com/superyngo/wenget/compare/v3.8.5...v3.8.6
[3.8.7]: https://github.com/superyngo/wenget/compare/v3.8.6...v3.8.7
