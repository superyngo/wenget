# Backlog
Status: In progress

The one living record of open work. Rows move to Done with the commit that closed them and are
never deleted. Evidence is file + symbol, never a line number. `Verified` is the date the row was
last checked against the tree — not when it was opened.

## Open

| ID | Opened | Verified | Pri | Finding | Evidence | Effort | Acceptance |
|---|---|---|---|---|---|---|---|
| S-4 | 2026-09-03 | 2026-09-23 | P1 | PowerShell PATH scripts interpolate path into generated source (breaks on `'` in username; not attacker-controlled) | `commands/init.rs` `setup_path_windows`, `commands/delete.rs` `remove_from_path_windows` | S | Windows PATH addition and removal use parameter passing or direct registry edits without string interpolation |
| S-5 | 2026-09-03 | 2026-09-23 | P3 | Checksum verification is best-effort and downgrades (with warning) on probe failure; `PlatformBinary.checksum` unused, and no bucket manifest sets it | `core/checksum.rs` `verify_download`, `core/manifest.rs` `PlatformBinary.checksum` | S | Unreachable checksum manifests fail or require explicit flag; manifest-provided `PlatformBinary.checksum` verified when present |
| S-6 | 2026-09-03 | 2026-09-23 | P1 | Admin `init` rewrites system PATH as `REG_SZ` (was `REG_EXPAND_SZ`), so `%SystemRoot%` entries stop expanding; env broadcast is stubbed | `core/registry.rs` `modify_system_path_inner`, `broadcast_environment_change` | S | Windows system PATH preserves `REG_EXPAND_SZ`; environment changes broadcast properly to running shells |
| S-7 | 2026-09-03 | 2026-09-23 | P3 | Generated shims and Unix script wrappers interpolate paths without escaping (robustness: the bucket already supplies the executed script) | `installer/shim.rs` `create_shim`, `installer/script.rs` `create_script_shim_unix` | XS | Batch metacharacters escaped in `.cmd` shims; quotes and dollar signs escaped in Unix script wrapper |
| S-8 (+Q-7) | 2026-09-03 | 2026-09-23 | P1 | Windows self-update cleanup and uninstall scripts never run when path has spaces (`start` takes quoted path as title); same lines `.to_str().unwrap()` | `commands/update.rs` `replace_exe_windows`, `commands/delete.rs` `delete_executable_windows` | XS | `cmd /C start` calls pass empty title (`""`) and no `unwrap` on path conversion |
| S-9 | 2026-09-03 | 2026-09-23 | P3 | Plaintext `http://` download URLs accepted silently without warning | `downloader/mod.rs` `download_file`, `installer/input_detector.rs` `detect_input_type` | XS | Plaintext `http://` URLs rejected unless `--allow-http` flag passed |
| S-10 | 2026-09-03 | 2026-09-03 | P3 | Bootstrap installers download release binaries without verification | `install.sh`, `install.ps1` | S | Bootstrap scripts verify published `SHA256SUMS` before executing downloaded binary |
| T-2 | 2026-09-03 | 2026-09-23 | P1 | No end-to-end integration tests over install/update/delete lifecycle | `tests/` directory missing | M | Integration test suite executes lifecycle against isolated temporary root and mock server |
| T-3 | 2026-09-03 | 2026-09-23 | P1 | Install/update/delete state machines effectively untested | `commands/add.rs` `install_packages`, `commands/update.rs` `find_upgradeable`, `commands/delete.rs` `run` | M | Automated tests exercise state transitions and disk operations with mock paths |
| T-4 | 2026-09-03 | 2026-09-23 | P2 | `delete.rs` test re-implements dedup logic instead of calling production code | `commands/delete.rs` `test_specific_variant_not_duplicated_in_final_to_delete` | XS | Test calls public/internal candidate resolution helper rather than duplicated loop |
| T-5 | 2026-09-03 | 2026-09-23 | P2 | No adversarial tests for malformed records/manifests or non-tar formats | `installer/extractor.rs`, `core/checksum.rs` | S | Adversarial tests cover corrupt archives, zip/7z decompression error paths, and malformed JSON records |
| T-6 | 2026-09-03 | 2026-09-23 | P2 | Untested command modules (`init`, `bucket`, `info`, `list`) | `commands/init.rs`, `commands/bucket.rs`, `commands/info.rs`, `commands/list.rs` | S | Unit tests cover argument validation and execution logic for all command modules |
| CL-1 (A-3, Q-3) | 2026-09-03 | 2026-09-23 | P1 | `update` drives `add` through positional flags; Installer/InstallRequest split pending | `commands/add.rs` `install_packages`, `install_package`; `commands/update.rs` `run` | M | Headless `Installer` service separated from CLI UI, accepting structured `InstallRequest` without positional booleans |
| SI-12 (A-5) | 2026-09-03 | 2026-09-23 | P2 | Divergent HTTP clients across downloader, checksum, and utils | `utils/http.rs` `HttpClient`, `downloader/mod.rs` `download_file`, `core/checksum.rs` `fetch_checksum_file` | S | Process-wide `reqwest::blocking::Client` instance shared with unified authentication and configurable timeouts |
| A-6 | 2026-09-03 | 2026-09-23 | P3 | Data model spawns processes and performs disk I/O | `core/manifest.rs` `INTERPRETER_CACHE`, `InstalledManifest::migrate` | S | Process execution moved to `installer/script.rs` and legacy migration isolated from core data definitions |
| A-7 | 2026-09-03 | 2026-09-23 | P3 | `core/` modules depend upward on root-level modules | `core/config.rs` imports `crate::bucket`, `crate::cache` | S | Upward dependencies relocated into `core/` or dependency direction inverted |
| SI-5 (Q-5) | 2026-09-03 | 2026-09-23 | P2 | PATH manipulation logic duplicated between `init` and `delete`; delete never cleans system PATH | `commands/init.rs` `setup_path_windows`/rc helpers, `commands/delete.rs` `remove_from_path_windows` | S | Single `core::env_path` helper handles adding and removing PATH entries across platforms |
| Q-6 | 2026-09-03 | 2026-09-23 | P2 | `bin_dir()` panics when home directory is missing | `core/paths.rs` `WenPaths::bin_dir` | XS | Home directory resolved fallibly at `WenPaths::new` and errors propagated gracefully |
| SI-7 (Q-8) | 2026-09-03 | 2026-09-23 | P2 | 43 compiler-confirmed dead items masked by `#[allow(dead_code)]` | `core/platform.rs`, `core/manifest.rs`, `core/paths.rs`, `utils/http.rs` | S | Unused dead items and non-cfg `#[allow(dead_code)]` annotations removed |
| Q-9 | 2026-09-03 | 2026-09-23 | P3 | Backup failures swallowed before mutating repair actions | `commands/repair.rs` `repair_buckets`, `core/config.rs` `create_backup` | XS | Backup failure produces a visible warning or prompts user before destructive repair |
| OP-2 (P-2) | 2026-09-03 | 2026-09-23 | P2 | Cache and Installed set re-read and re-parsed multiple times per `update` | `commands/update.rs` `run`, `commands/add.rs` `run`, `update_cache_with_packages` | S | In-memory `InstalledSet` and `ManifestCache` passed down without redundant disk re-reads |
| P-3 | 2026-09-03 | 2026-09-23 | P2 | Whole-cache cloned in memory to display `list --all` | `commands/list.rs` `run`, `cache.rs` `to_source_manifest` | XS | Non-cloning borrowing iterator traverses cached packages directly |
| OP-4 (P-6) | 2026-09-03 | 2026-09-23 | P3 | Asset filenames lowercased repeatedly and keyword arrays rebuilt per call | `core/platform.rs` `BinarySelector::score_parsed`, `contains_unknown_arch_pattern` | XS | Filenames lowercased once in `ParsedAsset` and keyword tables hoisted to `const` slices |
| M-2 | 2026-09-03 | 2026-09-23 | P1 | Compiled 4.4MB `bucket/wenget` binary committed and force-pushed each release | `bucket/wenget`, `.github/workflows/release.yml`, `.github/workflows/update-manifest.yml` | S | Tracked `bucket/wenget` binary removed; CI builds or downloads verified release asset |
| M-5 | 2026-09-03 | 2026-09-23 | P2 | Aging dependencies (`zip 0.6`, `is_elevated 0.1`); `sevenz-rust` replaced by `sevenz-rust2` | `Cargo.toml` | M | Dependencies updated to modern supported releases; `is_elevated` replaced with `windows-sys` API |
| IM-8 | 2026-09-23 | 2026-09-23 | P2 | Version comparison differs between API and cache update paths | `commands/update.rs` `is_newer_version`, `find_upgradeable` | S | Single version comparison routine uses `semver` crate with integer fallback for pre-releases |
| IM-9 | 2026-09-23 | 2026-09-23 | P3 | Downloaded archives leak in `downloads/` directory on error paths | `commands/add.rs` `install_packages`, `downloader/mod.rs` `download_file` | XS | Downloads write to `tempfile::NamedTempFile` cleaned up automatically on early drop |
| IM-11 | 2026-09-23 | 2026-09-23 | P3 | Self-update skips executable permission and magic-byte checks | `commands/update.rs` `upgrade_self_with_provider`, `installer/extractor.rs` `find_executable` | XS | `find_executable_candidates` called with extraction directory during self-update |
| IM-12 | 2026-09-23 | 2026-09-23 | P2 | `repair` reports missing launchers but cannot restore them | `commands/repair.rs` `missing_shims`, `run` | S | `repair --force` recreates missing launchers from package record executables or provides re-install hint |
| IM-15 | 2026-09-23 | 2026-09-23 | P3 | Download filename parsed from URL path instead of using known asset name | `commands/add.rs` `install_package`, `commands/update.rs` `upgrade_self_with_provider` | XS | Sanitized `PlatformBinary.asset_name` used directly as target download filename |
| CL-3 | 2026-09-23 | 2026-09-23 | P3 | Glossary drift in `meta_version` and misleading `get_or_create_installed` method name | `core/manifest.rs` `CURRENT_META_VERSION`, `core/config.rs` `get_or_create_installed` | XS | Fields renamed to `schema_version` with serde alias; legacy alias method removed |
| CL-5 | 2026-09-23 | 2026-09-23 | P3 | Deprecated legacy fields (`parent_package`) still threaded through install path | `commands/add.rs` `install_packages`, `core/manifest.rs` `InstalledPackage` | XS | Deprecated fields given `#[serde(default, skip_serializing)]` and omitted from constructors |
| CL-6 | 2026-09-23 | 2026-09-23 | P3 | 24 functions exceed 100 LOC (largest `install_packages` ~900) | `commands/add.rs` `install_packages`, `install_package`; `commands/delete.rs` `run`; `commands/info.rs` `display_package_info` | M | Complex functions refactored into distinct compute and render/presentation passes |
| SI-3 | 2026-09-23 | 2026-09-23 | P2 | Resolver uses custom glob implementation instead of `glob::Pattern` (`add 'x[12]*'` fails, `del` works) | `package_resolver.rs` `glob_match` | XS | Resolver uses `glob::Pattern` matching and honors variant specifications consistently |
| SI-4 | 2026-09-03 | 2026-09-23 | P3 | `init` command re-implements `installer::create_symlink` verbatim | `commands/init.rs` `create_wenget_symlink`, `installer/symlink.rs` `create_symlink` | XS | Duplicated symlink creation in `init` replaced by call to `installer::create_symlink` |
| SI-6 | 2026-09-23 | 2026-09-23 | P3 | Launcher creation contains `cfg` platform switches at every call site | `commands/add.rs`, `commands/rename.rs`, `installer/local.rs` | S | Unified `installer::create_launcher` and `remove_launcher` abstraction hides platform branches |
| SI-8 | 2026-09-23 | 2026-09-23 | P3 | Bucket subcommand enum manually mapped between CLI and command layers | `main.rs` `main`, `commands/bucket.rs` `BucketCommand` | XS | CLI `BucketCommands` enum consumed directly by `commands::run_bucket` |
| SI-9 | 2026-09-23 | 2026-09-23 | P3 | Numeric-suffix deduplication search loop copied three times | `commands/add.rs` `resolve_command_name` | XS | Shared `first_free(base, taken)` helper finds next unused command name |
| SI-10 | 2026-09-23 | 2026-09-23 | P3 | `extract_variant_from_asset` uses 141-step substring replacement cascade | `core/manifest.rs` `extract_variant_from_asset` | S | Token-based asset name parser drops version/platform noise; verified with golden tests |
| SI-11 | 2026-09-23 | 2026-09-23 | P2 | Four separate batch result tally and summary loops in `add.rs` | `commands/add.rs` `install_scripts`, `install_local_files`, `install_packages` | S | Unified `BatchReport` struct manages collection, formatting, and non-zero exit decisions |
| OP-3 | 2026-09-23 | 2026-09-23 | P3 | `repair` scans and parses `apps/` directory three times sequentially | `commands/repair.rs` `run`, `core/store.rs` `duplicate_keys` | XS | Scan entries cached from initial pass and reused for duplicate detection |
| OP-5 | 2026-09-23 | 2026-09-23 | P3 | Candidate executables opened multiple times for permissions, magic bytes, shebang | `installer/extractor.rs` `find_executable_candidates` | XS | Single file handle and initial buffer read verify magic bytes, shebang, and permissions |
| B-2 | 2026-09-23 | 2026-09-23 | P3 | `update` may recreate launcher under original name after `wenget rename` | `commands/update.rs` `run`, `commands/rename.rs` `rename_command` | S | Regression test verifies whether `update` preserves renamed launcher or restores original manifest name |
| B-3 | 2026-09-23 | 2026-09-23 | P3 | `FallbackType` enum variants for libc and Windows compiler are dead code | `core/platform.rs` `FallbackType::MuslOnGnu`, `GnuOnMusl`, `WindowsCompilerVariant` | XS | Unconstructed fallback enum variants removed or wired into platform matching |
| B-4 | 2026-09-23 | 2026-09-23 | P3 | `wenget update self` prints "'self' is not installed" (`update self` intentionally removed in 2026-03; self-check always runs) | `commands/update.rs` `run` | XS | `self` accepted silently as no-op or rejected with a clear message |
| B-6 | 2026-09-23 | 2026-09-23 | P3 | `del self` removes the default bin dir from PATH, not `custom_bin_path`; `init` only adds a dir that is not already on PATH, so switching blindly could strip a user-owned entry | `commands/delete.rs` `delete_self`, `commands/init.rs` PATH planning | S | Decide ownership (e.g. record the PATH entry `init` added); `del self` removes exactly that entry |

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
| `number_prefix` unmaintained (RUSTSEC-2025-0119), pulled in by `indicatif` | Warning only, no vulnerability; needs an `indicatif` upgrade | Advisory escalates or `indicatif` bump lands | 2026-09-23 |

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
| DEP-1 | Dependabot alerts #1–#10 (`tar`, `rustls-webpki`, `quinn-proto`, `rand`) plus `rustls` RUSTSEC-2026-0285 | bf4bfc5 |
| DEP-2 | `sevenz-rust` abandoned with unpatched path traversal (RUSTSEC-2026-0245); `extract_7z` let rooted/drive-prefixed names through on Windows | commit "fix(deps): replace sevenz-rust with sevenz-rust2" |
| A-4 | Vestigial `SourceProvider` trait | 3f56915 (verified 2026-09-23) |
| M-4 | `AGENTS.md`/`CLAUDE.md` redundancy — `CLAUDE.md` is a 5-line pointer | before 2026-09-23 (verified 2026-09-23) |
| Q-7 | `.to_str().unwrap()` in Windows self-update/uninstall | merged into S-8 (2026-09-23) |
| P-4 | Linear name lookup in resolver | dropped 2026-09-23: 68 packages, µs-scale scan |
| P-5 | `bucket create` 1 s sleep per package | dropped 2026-09-23: intentional guard against GitHub secondary rate limits on maintainer-only command |
| IM-13 | `wenget config` fails when `$EDITOR` has arguments; `$VISUAL` ignored | db83e37 |
| IM-7 | Invalid `config.toml` preferences used despite "using defaults" warning | 5a6748a |
| IM-6 | Package record save failure reported as success | e7839a2 |
| B-1 | `repair` treats a directory or dangling symlink at launcher path as healthy | 60c9dfe |
| IM-10 | "Removed obsolete command" printed when removal failed | eed670d |
| IM-14 | GitHub repo URLs classified by substring (source archives kept as repo URLs on purpose) | f761851 |
| CL-4 | `config.toml` template documents wrong user bin dir | c3fa271 |
| CL-7 / CL-2 | Unused `thiserror`; `#[allow(dead_code)]` on live items | d1cc80d |
| B-5 | `add`/`delete` ignore `custom_bin_path` (`del self` split out as B-6) | d201b7a |
