# Backlog
Status: In progress

The one living record of open work. Rows move to Done with the commit that closed them and are
never deleted. Evidence is file + symbol, never a line number. `Verified` is the date the row was
last checked against the tree — not when it was opened.

## Open

| ID | Opened | Verified | Pri | Finding | Evidence | Effort | Acceptance |
|---|---|---|---|---|---|---|---|
| S-4 | 2026-09-03 | 2026-09-23 | P1 | PowerShell PATH scripts interpolate path into generated source | `commands/init.rs` `setup_windows_path`, `commands/delete.rs` `clean_windows_path` | S | Windows PATH addition and removal use parameter passing or direct registry edits without string interpolation |
| S-5 | 2026-09-03 | 2026-09-23 | P2 | Checksum verification is best-effort and silently downgrades on probe failure | `core/checksum.rs` `verify_checksum`, `core/manifest.rs` `PlatformBinary.checksum` | S | Unreachable checksum manifests fail or require explicit flag; manifest-provided `PlatformBinary.checksum` verified when present |
| S-6 | 2026-09-03 | 2026-09-23 | P2 | Windows PATH write downgrades `REG_EXPAND_SZ` to `REG_SZ` and env broadcast is stubbed | `core/registry.rs` `add_to_system_path`, `broadcast_environment_change` | S | Windows system PATH preserves `REG_EXPAND_SZ`; environment changes broadcast properly to running shells |
| S-7 | 2026-09-03 | 2026-09-23 | P2 | Generated shims and Unix script wrappers interpolate paths without escaping | `installer/shim.rs` `create_shim`, `installer/script.rs` `create_script_shim_unix` | XS | Batch metacharacters escaped in `.cmd` shims; quotes and dollar signs escaped in Unix script wrapper |
| S-8 | 2026-09-03 | 2026-09-23 | P2 | Windows self-update and uninstall scripts misparse quoted paths | `commands/update.rs` `perform_windows_update`, `commands/delete.rs` `delete_windows_self` | S | `cmd /C start` calls specify empty window title (`""`) or avoid shell wrapper for paths containing spaces |
| S-9 | 2026-09-03 | 2026-09-23 | P3 | Plaintext `http://` download URLs accepted silently without warning | `downloader/mod.rs` `download_file`, `installer/input_detector.rs` `detect_input_type` | XS | Plaintext `http://` URLs rejected unless `--allow-http` flag passed |
| S-10 | 2026-09-03 | 2026-09-03 | P3 | Bootstrap installers download release binaries without verification | `install.sh`, `install.ps1` | S | Bootstrap scripts verify published `SHA256SUMS` before executing downloaded binary |
| T-2 | 2026-09-03 | 2026-09-23 | P1 | No end-to-end integration tests over install/update/delete lifecycle | `tests/` directory missing | M | Integration test suite executes lifecycle against isolated temporary root and mock server |
| T-3 | 2026-09-03 | 2026-09-23 | P1 | Install/update/delete state machines effectively untested | `commands/add.rs` `install_packages`, `commands/update.rs` `find_upgradeable`, `commands/delete.rs` `run` | M | Automated tests exercise state transitions and disk operations with mock paths |
| T-4 | 2026-09-03 | 2026-09-23 | P2 | `delete.rs` test re-implements dedup logic instead of calling production code | `commands/delete.rs` `test_specific_variant_not_duplicated_in_final_to_delete` | XS | Test calls public/internal candidate resolution helper rather than duplicated loop |
| T-5 | 2026-09-03 | 2026-09-23 | P2 | No adversarial tests for malformed records/manifests or non-tar formats | `installer/extractor.rs`, `core/checksum.rs` | S | Adversarial tests cover corrupt archives, zip/7z decompression error paths, and malformed JSON records |
| T-6 | 2026-09-03 | 2026-09-23 | P2 | Untested command modules (`init`, `bucket`, `info`, `list`) | `commands/init.rs`, `commands/bucket.rs`, `commands/info.rs`, `commands/list.rs` | S | Unit tests cover argument validation and execution logic for all command modules |
| CL-1 (A-3, Q-3) | 2026-09-03 | 2026-09-23 | P1 | `update` drives `add` through positional flags; Installer/InstallRequest split pending | `commands/add.rs` `install_packages`, `install_package`; `commands/update.rs` `run` | M | Headless `Installer` service separated from CLI UI, accepting structured `InstallRequest` without positional booleans |
| A-4 | 2026-09-03 | 2026-09-23 | P3 | Vestigial `SourceProvider` trait with single implementor | `providers/base.rs` `SourceProvider`, `providers/github.rs` `GitHubProvider` | XS | Trait removed or broadened for genuine multiple provider support (partly addressed in 3f56915) |
| SI-12 (A-5) | 2026-09-03 | 2026-09-23 | P2 | Divergent HTTP clients across downloader, checksum, and utils | `utils/http.rs` `HttpClient`, `downloader/mod.rs` `download_file`, `core/checksum.rs` `fetch_checksum_file` | S | Process-wide `reqwest::blocking::Client` instance shared with unified authentication and configurable timeouts |
| A-6 | 2026-09-03 | 2026-09-23 | P3 | Data model spawns processes and performs disk I/O | `core/manifest.rs` `INTERPRETER_CACHE`, `InstalledManifest::migrate` | S | Process execution moved to `installer/script.rs` and legacy migration isolated from core data definitions |
| A-7 | 2026-09-03 | 2026-09-23 | P3 | `core/` modules depend upward on root-level modules | `core/config.rs` imports `crate::bucket`, `crate::cache` | S | Upward dependencies relocated into `core/` or dependency direction inverted |
| SI-5 (Q-5) | 2026-09-03 | 2026-09-23 | P2 | PATH manipulation logic duplicated between `init` and `delete` | `commands/init.rs` `setup_windows_path`/rc helpers, `commands/delete.rs` `clean_windows_path` | S | Single `core::env_path` helper handles adding and removing PATH entries across platforms |
| Q-6 | 2026-09-03 | 2026-09-23 | P2 | `bin_dir()` panics when home directory is missing | `core/paths.rs` `WenPaths::bin_dir` | XS | Home directory resolved fallibly at `WenPaths::new` and errors propagated gracefully |
| Q-7 | 2026-09-03 | 2026-09-23 | P3 | `.to_str().unwrap()` on Windows paths during self-update and uninstall | `commands/delete.rs` `delete_windows_self`, `commands/update.rs` `perform_windows_update` | XS | Paths converted safely via `as_os_str()` or contextual error returned |
| SI-7 (Q-8) | 2026-09-03 | 2026-09-23 | P2 | 43 compiler-confirmed dead items masked by `#[allow(dead_code)]` | `core/platform.rs`, `core/manifest.rs`, `core/paths.rs`, `utils/http.rs` | S | Unused dead items and non-cfg `#[allow(dead_code)]` annotations removed |
| Q-9 | 2026-09-03 | 2026-09-23 | P3 | Backup failures swallowed before mutating repair actions | `commands/repair.rs` `repair_buckets`, `core/config.rs` `create_backup` | XS | Backup failure produces a visible warning or prompts user before destructive repair |
| OP-2 (P-2) | 2026-09-03 | 2026-09-23 | P2 | Cache and Installed set re-read and re-parsed multiple times per `update` | `commands/update.rs` `run`, `commands/add.rs` `run`, `update_cache_with_packages` | S | In-memory `InstalledSet` and `ManifestCache` passed down without redundant disk re-reads |
| P-3 | 2026-09-03 | 2026-09-23 | P2 | Whole-cache cloned in memory to display `list --all` | `commands/list.rs` `run`, `cache.rs` `to_source_manifest` | XS | Non-cloning borrowing iterator traverses cached packages directly |
| P-4 | 2026-09-03 | 2026-09-23 | P2 | Package name lookups scan URL-keyed map linearly in resolver | `package_resolver.rs` `resolve_package_input` | S | Name-to-URL index maintained for O(1) package name lookups |
| P-5 | 2026-09-03 | 2026-09-23 | P3 | `bucket create` sleeps 1s serially per package | `commands/bucket.rs` `run_create` | S | Rate-limit throttling checks response headers instead of unconditional 1s sleep |
| OP-4 (P-6) | 2026-09-03 | 2026-09-23 | P3 | Asset filenames lowercased repeatedly and keyword arrays rebuilt per call | `core/platform.rs` `BinarySelector::score_parsed`, `contains_unknown_arch_pattern` | XS | Filenames lowercased once in `ParsedAsset` and keyword tables hoisted to `const` slices |
| M-2 | 2026-09-03 | 2026-09-23 | P1 | Compiled 4.4MB `bucket/wenget` binary committed and force-pushed each release | `bucket/wenget`, `.github/workflows/release.yml`, `.github/workflows/update-manifest.yml` | S | Tracked `bucket/wenget` binary removed; CI builds or downloads verified release asset |
| M-4 | 2026-09-03 | 2026-09-23 | P2 | Redundant content between `AGENTS.md` and `CLAUDE.md` | `AGENTS.md`, `CLAUDE.md` | S | `CLAUDE.md` trimmed to reference pointer and overlapping instruction rules consolidated |
| M-5 | 2026-09-03 | 2026-09-23 | P2 | Aging dependencies (`zip 0.6`, `sevenz-rust 0.6`, `is_elevated 0.1`) | `Cargo.toml` | M | Dependencies updated to modern supported releases; `is_elevated` replaced with `windows-sys` API |
| IM-6 | 2026-09-23 | 2026-09-23 | P2 | Failure to save package record reported as success in batch summary | `commands/add.rs` `record_installed` | XS | Package record save errors return `Err` and fail the batch install |
| IM-7 | 2026-09-23 | 2026-09-23 | P2 | Invalid `config.toml` announced as using defaults but bad values still used | `core/config.rs` `Config::new` | XS | Validated preferences fallback to `Preferences::default()` when validation fails |
| IM-8 | 2026-09-23 | 2026-09-23 | P2 | Version comparison differs between API and cache update paths | `commands/update.rs` `is_newer_version`, `find_upgradeable` | S | Single version comparison routine uses `semver` crate with integer fallback for pre-releases |
| IM-9 | 2026-09-23 | 2026-09-23 | P3 | Downloaded archives leak in `downloads/` directory on error paths | `commands/add.rs` `install_packages`, `downloader/mod.rs` `download_file` | XS | Downloads write to `tempfile::NamedTempFile` cleaned up automatically on early drop |
| IM-10 | 2026-09-23 | 2026-09-23 | P3 | "Removed obsolete command" printed even when file removal failed | `commands/add.rs` `install_package` | XS | Removal message printed only when `fs::remove_file` succeeds; error printed on failure |
| IM-11 | 2026-09-23 | 2026-09-23 | P3 | Self-update skips executable permission and magic-byte checks | `commands/update.rs` `upgrade_self_with_provider`, `installer/extractor.rs` `find_executable` | XS | `find_executable_candidates` called with extraction directory during self-update |
| IM-12 | 2026-09-23 | 2026-09-23 | P2 | `repair` reports missing launchers but cannot restore them | `commands/repair.rs` `missing_shims`, `run` | S | `repair --force` recreates missing launchers from package record executables or provides re-install hint |
| IM-13 | 2026-09-23 | 2026-09-23 | P3 | `wenget config` fails when `$EDITOR` contains command-line arguments | `commands/config.rs` `edit_config` | XS | `$VISUAL` checked first; command string split by whitespace into binary and arguments |
| IM-14 | 2026-09-23 | 2026-09-23 | P3 | URL classification is substring-based matching non-GitHub domains | `installer/input_detector.rs` `detect_input_type` | XS | Strict host and path parsing distinguishes GitHub repos from arbitrary URLs |
| IM-15 | 2026-09-23 | 2026-09-23 | P3 | Download filename parsed from URL path instead of using known asset name | `commands/add.rs` `install_package`, `commands/update.rs` `upgrade_self_with_provider` | XS | Sanitized `PlatformBinary.asset_name` used directly as target download filename |
| CL-2 | 2026-09-23 | 2026-09-23 | P3 | `#[allow(dead_code)]` placed on active production methods | `core/config.rs` `Config::preferences`, `core/paths.rs` `staging_dir`, `core/store.rs` `ScanEntry` | XS | Dead code annotations removed from live symbols; compiler warns only on true dead code |
| CL-3 | 2026-09-23 | 2026-09-23 | P3 | Glossary drift in `meta_version` and misleading `get_or_create_installed` method name | `core/manifest.rs` `CURRENT_META_VERSION`, `core/config.rs` `get_or_create_installed` | XS | Fields renamed to `schema_version` with serde alias; legacy alias method removed |
| CL-4 | 2026-09-23 | 2026-09-23 | P3 | `config.toml` template comments document incorrect default bin directory | `core/preferences.rs` `DEFAULT_CONFIG_TEMPLATE` | XS | Comments in generated configuration template cite `~/.local/bin` matching implementation |
| CL-5 | 2026-09-23 | 2026-09-23 | P3 | Deprecated legacy fields (`parent_package`) still threaded through install path | `commands/add.rs` `install_packages`, `core/manifest.rs` `InstalledPackage` | XS | Deprecated fields given `#[serde(default, skip_serializing)]` and omitted from constructors |
| CL-6 | 2026-09-23 | 2026-09-23 | P2 | Oversized functions exceeding 100 LOC across command and core modules | `commands/delete.rs` `run`, `commands/bucket.rs` `run_create`, `commands/info.rs` `run` | M | Complex functions refactored into distinct compute and render/presentation passes |
| CL-7 | 2026-09-23 | 2026-09-23 | P3 | `thiserror` crate declared as dependency with zero usages in tree | `Cargo.toml` | XS | `thiserror` dependency removed or adopted systematically in provider error handling |
| SI-3 | 2026-09-23 | 2026-09-23 | P2 | Resolver uses custom glob implementation instead of `glob::Pattern` | `package_resolver.rs` `matches_pattern`, `delete.rs` | XS | Resolver uses `glob::Pattern` matching and honors variant specifications consistently |
| SI-4 | 2026-09-03 | 2026-09-23 | P3 | `init` command re-implements `installer::create_symlink` verbatim | `commands/init.rs` `create_wenget_symlink`, `installer/symlink.rs` `create_symlink` | XS | Duplicated symlink creation in `init` replaced by call to `installer::create_symlink` |
| SI-6 | 2026-09-23 | 2026-09-23 | P3 | Launcher creation contains `cfg` platform switches at every call site | `commands/add.rs`, `commands/rename.rs`, `installer/local.rs` | S | Unified `installer::create_launcher` and `remove_launcher` abstraction hides platform branches |
| SI-8 | 2026-09-23 | 2026-09-23 | P3 | Bucket subcommand enum manually mapped between CLI and command layers | `main.rs` `main`, `commands/bucket.rs` `BucketCommand` | XS | CLI `BucketCommands` enum consumed directly by `commands::run_bucket` |
| SI-9 | 2026-09-23 | 2026-09-23 | P3 | Numeric-suffix deduplication search loop copied three times | `commands/add.rs` `install_packages`, `install_scripts` | XS | Shared `first_free(base, taken)` helper finds next unused command name |
| SI-10 | 2026-09-23 | 2026-09-23 | P3 | `extract_variant_from_asset` uses 141-step substring replacement cascade | `core/manifest.rs` `extract_variant_from_asset` | S | Token-based asset name parser drops version/platform noise; verified with golden tests |
| SI-11 | 2026-09-23 | 2026-09-23 | P2 | Four separate batch result tally and summary loops in `add.rs` | `commands/add.rs` `install_scripts`, `install_local_files`, `install_packages` | S | Unified `BatchReport` struct manages collection, formatting, and non-zero exit decisions |
| OP-3 | 2026-09-23 | 2026-09-23 | P3 | `repair` scans and parses `apps/` directory three times sequentially | `commands/repair.rs` `run`, `core/store.rs` `duplicate_keys` | XS | Scan entries cached from initial pass and reused for duplicate detection |
| OP-5 | 2026-09-23 | 2026-09-23 | P3 | Candidate executables opened multiple times for permissions, magic bytes, shebang | `installer/extractor.rs` `find_executable_candidates` | XS | Single file handle and initial buffer read verify magic bytes, shebang, and permissions |
| B-1 | 2026-09-23 | 2026-09-23 | P2 | `repair` misses blocked launcher path when directory exists at `bin/<cmd>` | `commands/repair.rs` `missing_shims` | XS | `missing_shims` checks that path is symlink or file; directory at launcher path reported as blocked |
| B-2 | 2026-09-23 | 2026-09-23 | P3 | `update` may recreate launcher under original name after `wenget rename` | `commands/update.rs` `run`, `commands/rename.rs` `rename_command` | S | Regression test verifies whether `update` preserves renamed launcher or restores original manifest name |
| B-3 | 2026-09-23 | 2026-09-23 | P3 | `FallbackType` enum variants for libc and Windows compiler are dead code | `core/platform.rs` `FallbackType::MuslOnGnu`, `GnuOnMusl`, `WindowsCompilerVariant` | XS | Unconstructed fallback enum variants removed or wired into platform matching |
| B-4 | 2026-09-23 | 2026-09-23 | P3 | `wenget update self` treats `self` as a package name and reports not installed | `commands/update.rs` `run` | XS | `update self` alias explicitly supported as trigger for self-update or dropped from help |

## Pending verification

| Item | Closed by | Verifies when | Fallback |
|---|---|---|---|
| IM-4 Windows script shim recreation | 563c5fc | Tested on native Windows environment | Re-open IM-4 Windows half if script wrappers fail on Windows |

## Awaiting external

| Item | Blocked on | Ready when |
|---|---|---|

## Watching

| Item | Why not now | Trigger | Re-read |
|---|---|---|---|
| Real `~/.wenget` state after IM-5 test | State is consistent (fd 10.5.0 from bucket wenget, launcher `~/.local/bin/fd`), prior state unknown | User reports fd issues | 2026-09-23 |

## Done

| ID | Finding | Closed by |
|---|---|---|
| S-1 | Tar path traversal arbitrary file write vulnerability (Zip Slip) | 67c0bb9 |
| S-2 | 7z path traversal arbitrary file write vulnerability | 67c0bb9 |
| S-3 | Unvalidated package names allowing path traversal during deletion | fb2d2e3 |
| T-1 | `cargo test` overwriting user's live `~/.wenget/installed.json` | fb2d2e3 |
| A-1 | Non-atomic writes to `installed.json` | 82a21ee |
| A-2 | Unused `tokio` dependency with `full` features in `Cargo.toml` | fb2d2e3 |
| Q-1 | Byte-offset vs character-index panic on non-ASCII asset names | fb2d2e3 |
| Q-2 | Unchecked `unwrap()` on empty platform binaries in `search` | fb2d2e3 |
| M-1 | Absence of CI workflow running fmt, clippy, and tests across OSes | fb2d2e3 |
| M-3 | Structure map in `AGENTS.md` omitting modules | before 2026-09-23 (see audit status table) |
| M-6 | README documenting incorrect bin directory | before 2026-09-23 (see audit status table) |
| M-7 | Architectural rationale buried in changelog rather than `docs/` | before 2026-09-23 (see audit status table) |
| IM-1 | Failed installs exiting with status code 0 | fbfbef0 |
| IM-2 | Error reporting dropping underlying root cause chain | 4038c74 |
| IM-3 | `--verbose` and `RUST_LOG` ignored; INFO logs leaking into output | 5da4d1d |
| IM-4 | `rename` failing on non-bash script packages on Unix | 563c5fc |
| IM-5 | Package record written after staged swap risking untracked installs | a923375 |
| SI-1 (Q-4) | Duplicate asset scoring engines in `platform.rs` | 6e5279a |
| SI-2 | Duplicated `fetch_package` and `fetch_package_by_version` methods | 3f56915 |
| OP-1 (P-1) | Excessive GitHub API calls per package (4–8 calls reduced to 1) | 3f56915 |
| Rate limit reported as "Not found" | Unfetchable GitHub URLs reporting "Not found" instead of rate limit error | eb19b8b |
| GITHUB_TOKEN support | `add`, `update`, `info` support for `GITHUB_TOKEN` authentication | d787223 |
