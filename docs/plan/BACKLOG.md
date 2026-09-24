# Backlog
Status: In progress

The one living record of open work. Rows move to Done with the commit that closed them and are
never deleted. Evidence is file + symbol, never a line number. `Verified` is the date the row was
last checked against the tree — not when it was opened.

## Open

| ID | Opened | Verified | Pri | Finding | Evidence | Effort | Acceptance |
|---|---|---|---|---|---|---|---|
| CL-6 | 2026-09-23 | 2026-09-23 | P3 | 24 functions exceed 100 LOC (largest `install_packages` ~650 after CL-8; its plan/render phase is still inline) | `commands/add.rs` `install_packages`; `installer/package.rs` `PackageInstaller::install` (~280); `commands/delete.rs` `run`; `commands/info.rs` `display_package_info` | M | Complex functions refactored into distinct compute and render/presentation passes |
| SI-10 | 2026-09-23 | 2026-09-23 | P3 | `extract_variant_from_asset` uses 141-step substring replacement cascade | `core/manifest.rs` `extract_variant_from_asset` | S | Token-based asset name parser drops version/platform noise; verified with golden tests |

## Pending verification

| Item | Closed by | Verifies when | Fallback |
|---|---|---|---|
| S-6 system PATH keeps `REG_EXPAND_SZ` | a155341 | Admin `wenget init` on a real Windows machine leaves `%SystemRoot%` entries expanding (CI covers the registry helper under HKCU) | Re-open S-6 |
| S-8 + Q-7 detached scripts run from paths with spaces | bc1b7bd | `wenget update` self-update and `del self` from `C:\Program Files\...` on real Windows (CI runs a script from a spaced path) | Re-open S-8 |
| S-4 + SI-5 user PATH via registry, `del self` cleans system PATH | 3252241 | `init`/`del self` on a real Windows account whose name contains `'` | Re-open S-4 / SI-5 |
| S-10 bootstrap scripts verify `SHA256SUMS` | c46f16c | `install.ps1` run on real Windows (`install.sh` verified on macOS against the v3.9.0 release) | Re-open S-10 for `install.ps1` |
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
| S-9 | Plaintext `http://` accepted silently — now warns (warn variant, not reject) | ed6bd77 |
| T-4 | `delete.rs` test re-implemented dedup logic | f099e1c |
| T-5 | No adversarial archive tests (zip, corrupt tar/7z) | 3cae2b4 |
| M-2 | 4.5 MB `bucket/wenget` committed each release (history not rewritten) | c8b8115 |
| M-5 | `zip 0.6`, duplicate `bzip2`, unmaintained `is_elevated` | d5cdbfe |
| IM-8 | API update path offered downgrades | 7258539 |
| IM-9 | Downloads leaked on error paths | 495271b |
| IM-12 | `repair` could not recreate missing launchers | 7abe3dc |
| SI-3 | Resolver used a hand-rolled glob (`add 'x[12]*'` failed) | 01287ac |
| SI-4 | `init` copied `installer::create_symlink` | b842c67 |
| SI-8 | Bucket subcommand enum mapped by hand | b842c67 |
| SI-9 | Numeric-suffix loop copied three times | b842c67 |
| SI-11 | Five batch tally/summary blocks in `add.rs` | bae251a |
| B-2 | `update` restored original script name after `wenget rename` | f16063e |
| B-4 | `update self` warned "'self' is not installed" | 34a8258 |
| B-7 | `init`/`del self` under `WENGET_ROOT` edited the real rc files/registry PATH | 248afa2 |
| B-8 | `del self` under `WENGET_ROOT` deleted an executable outside the root | 33315ef |
| B-10 | Empty `apps/.staging/` left behind after install | df5be2e |
| T-2 | No end-to-end integration tests (added `tests/lifecycle.rs`, 6 offline scenarios; `update` split into T-3) | 205ab40 |
| Q-6 | `bin_dir()` `expect` on missing home (unreachable in practice; user bin dir now resolved at construction) | 9fc727e |
| CL-1 (A-3, Q-3) | Positional flags from `update` into `add` (`InstallOptions` in aca10c9; internals now take `&InstallOptions`, unused `_parent_package` dropped; Installer split moved to CL-8) | 4583858 |
| T-3 | `update` untested end to end (`tests/update.rs` runs it against a local HTTP fixture via the `WENGET_GITHUB_API` test hook: upgrade, up-to-date, API-failure fallback) | f7f809e |
| CL-8 | No headless `Installer` (`installer::package`: `target_package`, `filter_binaries`, `PackageInstaller::install` + `select_executables` behind the `InstallUi` trait; `install_package` and its 9-param allow gone; the plan/render phase of `install_packages` stays in CL-6) | b94e499 |
| T-6 | Untested `init`/`bucket`/`info`/`list` (unit tests for `ManifestGenerator` helpers, `merge_manifests`, `truncate_desc`, `detect_shell_configs`, `update_shell_config`; `tests/cli.rs` runs bucket add/list/del, `list --all`, `info`, and idempotent `init -y` offline) | 230f01c |
| SI-12 (A-5) | Divergent HTTP clients (`utils::http::shared_client` is the one process-wide client; `HttpClient`, downloader, and checksum probes use it with per-request timeouts; token stays `HttpClient`-only by design so asset hosts never see it) | 105d9dc |
| SI-7 (Q-8) | Dead items masked by `#[allow(dead_code)]` (dead helpers, enum variants, `is_exact`, `RateLimit`, non-Windows registry stubs removed; test-fixture accessors kept as `#[cfg(test)]`; remaining allows are platform- or test-scoped `cfg_attr`) | 21b5b67 |
| OP-2 (P-2) | Cache and Installed set re-read during `update`/`add` (`add::run_with` takes the caller's `InstalledSet` and `ManifestCache`; `update_cache_with_packages` updates the in-memory cache; `update` now loads the cache 0 extra times and the Installed set once, `add` loads the cache once) | d1a58be |
| P-3 | Whole cache cloned to display `list --all` (`list_all_packages` borrows from `ManifestCache`; clone helpers removed) | 32ba581 |
| BUG (info) | `info <missing>` exited 0 (`info::run` now fails when any name is not found) | 66f9afa |
| BUG (variant) | `extract_variant_from_asset` turned `x86_64` into variant `64` (patterns normalized like the name) | f112c57 |
| BUG (list order) | `list --all` printed scripts in random HashMap order (now sorted by name) | e8dfe08 |
| OP-3 | `repair` scanned `apps/` three times (`InstalledStore::load_scanned` returns set and entries; `duplicate_keys` takes the entries) | 8cb3ed3 |
| OP-5 | Candidate executables opened up to three times (`probe_file` reads permission and head bytes through one handle) | c763b1f |
| OP-4 (P-6) | Asset names lowercased repeatedly, keyword arrays rebuilt per call (`ParsedAsset::from_lower`; helpers take lowercase; `const` tables) | 1b24ee2 |
| IM-11 | Self-update skipped permission/magic-byte checks (`find_executable` takes the extraction dir) | 375da56 |
| IM-15 | Download filename parsed from the URL (`sanitize_path_component(asset_name)` in `PackageInstaller::install` and self-update) | 6d9d97d |
| Q-9 | Backup failure swallowed before resetting `buckets.json` (`repair_buckets` errors, `BucketConfig::load` warns and leaves the file) | 3766ba6 |
| S-7 | Launcher paths interpolated unescaped (`sh_single_quote` for Unix wrappers; `escape_cmd_quoted` doubles `%` in `.cmd` shims) | 289ea13 |
| B-3 | Dead `FallbackType` variants (already removed by the dead-code sweep; only `Arch32On64`/`X64OnArm` remain) | 21b5b67 |
| CL-3 | `meta_version` glossary drift and `get_or_create_installed` alias (field is `schema_version` in code, serialized as `meta_version` so older builds keep their version gate; alias removed) | 902deb0 |
| CL-5 | Deprecated `parent_package` threaded through constructors (field removed; `InstalledSet::migrate` takes legacy parents read from raw JSON) | c2a9cb8 |
| SI-6 | Launcher creation `cfg` switches duplicated across install/rename/repair (unified in `installer::create_launcher`) | a273e99 |
| S-5 | Best-effort checksum downgraded on probe failure; `PlatformBinary.checksum` unused (probe failure aborts unless `--skip-checksum`; mismatch never overridable; manifest checksum verified first) | ecaea8e |
| B-6 | `del self` removed the default bin dir from PATH, not `custom_bin_path`, and could strip user-owned entries (`init` records each PATH entry it adds in `path.json` via `core::path_record`; `del self` removes exactly those, otherwise leaves PATH alone) | 351b704 |
| A-6 | Data model spawned processes and performed disk I/O (interpreter probing now `installer::script::is_interpreter_available`/`installable_script`; legacy migration in `core/migrate.rs`) | 629967c |
| A-7 | `core/` modules depended upward on root-level `bucket`/`cache` (both moved into `core/`; they only depended on `core` themselves) | 3e65ac9 |
| B-9 | Local archive without platform keywords split payload and record across two app dirs | a92ed79 |
