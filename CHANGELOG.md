# Changelog

All notable changes to wenget will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 2026-10-01

- docs: translate the remaining Chinese docs to English: `RESOURCE_FILTERING_RULES.md` (section
  numbers unchanged), the downloader resume / segmented plan, and one line of the archived
  `changelog/v0.x.md`. Only the language changed; the content is the same.
- docs: audit every doc against the code after the downloader work
  (`docs/audit/2026-10-01-documentation-audit.md`). The README examples now use packages in the
  default bucket, multi-package installs are described as continuing past a failed package, and
  resumable and segmented downloads are listed under Features and How It Works. Also fixed: the
  stale `FallbackType` note in the filtering rules, the archived changelog range, and the
  downloader plan's `Status:` line. Opened backlog DL-1 (an invalid `download_connections` resets
  every preference).
- feat(downloader): interrupted downloads resume across runs. A failed or Ctrl-C'd download keeps
  `<file>.part` and a `<file>.part.json` record (URL, validator, size, completed segments);
  running the same command again continues where it stopped when the remote file is unchanged.
  The `.part` file is locked so two wenget processes cannot write it at once, and leftovers older
  than 7 days are deleted. Self-update still starts over (its scratch directory is removed).
- feat(downloader): segmented multi-connection downloads. Files of 16 MiB or more on servers that
  honour `Range` are fetched as 8 MiB segments over `download_connections` parallel connections
  (new `config.toml` key, 1-16, default 4; `1` disables it). Servers without range support,
  small files, or a file that changes mid-download fall back to one stream; the probe response is
  reused, so small files cost no extra request.
- feat(downloader): downloads stream into `<file>.part` and are renamed only after the byte count
  matches. Dropped connections, stalls, 5xx and 429 are retried (1 s/2 s/4 s backoff) and resume
  with `Range` + `If-Range` from where they stopped; a server that ignores the range or a changed
  file restarts from 0. Gives up after 3 consecutive attempts without progress. The shared HTTP
  client's 30 s stall timeout is now explicit and documented.

## [4.0.0] - 2026-09-29

### 2026-09-29

- test(windows): native Windows smoke checks verified system PATH type preservation, self-update
  and `del self` from a spaced Program Files path, script shim recreation through `repair -f`, and
  `install.ps1` SHA256SUMS verification against v3.9.0. S-4/SI-5's apostrophe-account check remains
  tracked in `docs/plan/BACKLOG.md`.
- docs(agents): prompt audit of the agent instruction files. The AGENTS.md release steps now state
  each gate once, in plain wording with its reason, and drop the duplicated "Important Notes"
  block. Every command and gate is unchanged. The audit is recorded in
  `docs/audit/2026-09-29-prompt-audit.md`.

### 2026-09-24

- fix(delete): `del self` removes the launchers in the bin directory that point into the wenget
  root (package launchers and the `wenget` launcher) before deleting the root, and honours
  `custom_bin_path`. Other files in the bin directory are left alone. Closes B-14.
- docs: documentation audit fixes. README: `bucket refresh` (not `update`) refreshes package
  metadata, `info <name>` costs 1 API call, `del -f` / `repair -f` / `del self` described as they
  behave, Windows system `bin\` holds `.cmd` shims, `path.json` and `RUST_LOG` documented, the
  v0.3.0 migration notice removed. `bucket/README.md` drops the removed binary and release
  trigger. AGENTS.md release steps commit the version bump before tagging and run `cargo test`.
  CHANGELOG: `3.2.0` placed above `3.1.0`, compare links sorted, `[Unreleased]` link added.
  Reference: `RESOURCE_FILTERING_RULES.md` describes the token-based variant extraction and the
  filter / dedupe / select step between platform matching and extraction; the glossary gains
  Format score, Staged swap and Script. Audit record: `docs/audit/2026-09-24-documentation-audit.md`.
- fix(add): when a release ships the same build in several archive formats (fastfetch: `.tar.gz`
  and `.zip`), `add -y` installs one (preferred format) instead of installing twice under the same
  name, and the interactive picker lists it once.
- fix(list): a package's command names are shown sorted in `list`, `info`, `rename` and the `add`
  summary instead of in random order per run.
- build: `src/main.rs` enables `clippy::too_many_lines`, so CI (clippy `-D warnings`) rejects any
  new function over 100 lines.
- refactor: the 13 remaining functions over 100 lines (clippy `too_many_lines`) are split into
  phase helpers: `add` (`install_scripts`, `plan_packages`, `install_plan_item`), `update` (`run`,
  `find_upgradeable`, `upgrade_self_with_provider`), `installer` (`install`, `select_executables`,
  `find_executable_candidates`), `del self`, `bucket create`, `repair` and `search`. Real-binary
  output is unchanged (CL-9).
- fix(add): when the GitHub API lookup fails and the bucket's cached download links are used,
  `add`/`update` now say so ("⚠ Using cached download links (GitHub API unavailable)") and no longer
  write the stale package back into the cache. Before 158afd5 this fallback was silent.
- fix(delete): `del` lists and deletes matches in a stable sorted order and labels each group
  with its repo (`regclient`), instead of a random variant key (`regclient::regbot`) that changed
  between runs (B-11).
- fix(add): the upgrade plan preview (`add`/`update`) now prints only the binary URL matching the
  previously installed asset (variant-filtered), instead of every release asset for the platform.
  A repo that ships many components per platform (e.g. `codex`: `codex`, `codex-app-server`,
  `codex-npm-darwin-arm64`, ...) no longer floods the upgrade line for one package with every
  other component's download URL; the new-install preview still lists all variants so `add` users
  can see what they're choosing between.

### 2026-09-23

- refactor(delete, info): `delete::run` split into `match_installed`, `print_delete_plan` and
  `choose_variants`; the platform list of `info` moved to `print_platforms` with a pure
  `binary_status` helper. Output is unchanged (CL-6).
- refactor(installer): command-name planning and obsolete-launcher cleanup moved out of
  `PackageInstaller::install` into `plan_launchers` and `remove_obsolete_commands` (CL-6).
- refactor(add): `install_packages` (~650 lines) split into phases: `resolve_inputs`,
  `plan_packages`, `print_script_plan`, `install_plan_item`, `install_bucket_scripts`, with a
  shared `Session` for platform matching. Output is unchanged (CL-6).
- refactor(manifest): `extract_variant_from_asset` is now a token parser instead of a substring
  replacement cascade: platform words no longer eat parts of other words (`winget`, `denort`),
  platform words match case-insensitively (`Linux`, `Darwin`), and extensions like `.tar.zst` no
  longer leak into names. Some variant names change (e.g. `codex::apple` → `codex`). Packages
  installed under an old variant name may need a reinstall to update. Golden test covers all 1178
  bucket assets (SI-10).
- refactor(core): `bucket` and `cache` modules moved under `core/`, so `core::config` no longer
  depends upward on root-level modules (A-7).
- refactor(core): interpreter probing moved from `core/manifest.rs` to `installer::script`
  (`is_interpreter_available`, `installable_script`); legacy `InstalledSet::migrate` moved to
  `core/migrate.rs`, so the manifest data definitions no longer spawn processes or touch disk (A-6).
- fix(delete): `del self` now removes only the PATH entries `wenget init` actually added, recorded
  in `~/.wenget/path.json` (shell rc block or Windows registry entry). It no longer removes the
  default bin dir when `custom_bin_path` is set, and no longer strips user-owned lines that mention
  the bin dir. Installs initialized by older versions have no record, so PATH is left untouched with
  a hint to remove it manually (B-6).
- fix(checksum): a checksum lookup that fails on the network now aborts the install instead of
  proceeding unverified; `add`/`update --skip-checksum` installs anyway (a mismatch still always
  aborts). A manifest `PlatformBinary.checksum` (`<hex>` or `sha256:<hex>`) is now verified and takes
  precedence over probing release checksum files (S-5).
- refactor(installer): one `installer::create_launcher` replaces the per-platform symlink/shim
  `cfg` switches in install, local install, rename and repair (SI-6).
- refactor(manifest): the deprecated `parent_package` field is gone from `InstalledPackage`; legacy
  `installed.json` migration reads it from the raw JSON, and new records no longer write it (CL-5).
- refactor(manifest): `InstalledPackage::meta_version` is now `schema_version` in code (the on-disk
  key stays `meta_version` for older builds); the `get_or_create_installed` alias is removed (CL-3).
- fix(launcher): Unix script wrappers single-quote the script path (no `$`/backtick expansion);
  Windows `.cmd` shims only double `%` inside the quoted path, and package shims now escape it too.
  The old `^` escapes were kept literally inside quotes and broke such paths (S-7).
- fix(repair): a corrupt `buckets.json` is never reset without a backup; if the backup fails,
  `repair` stops with an error and normal commands warn and leave the file untouched (Q-9).
- fix(download): downloads are saved under the sanitized release asset name instead of the last
  URL path segment (IM-15).
- fix(update): self-update picks the new wenget executable with on-disk permission and magic-byte
  checks, like package installs (IM-11).
- perf(platform): asset names are lowercased once per asset and the arch/exclude keyword tables
  are `const` slices (OP-4).
- perf(extractor): executable candidates are opened once; permission, magic bytes and shebang
  come from one handle and one 128-byte read (OP-5).
- perf(repair): `repair` scans `apps/` once (`InstalledStore::load_scanned`) instead of three times;
  `duplicate_keys` now works on that scan (OP-3).
- fix(list): `list --all` sorts scripts by name, so their order no longer changes between runs.
- perf(list): `list --all` borrows packages from the cache instead of cloning it; removed the
  now-unused `Config::get_packages_from_cache` and `ManifestCache::{to_source_manifest,get_packages,get_scripts}` (P-3).
- fix(variant): asset names containing `x86_64` no longer yield bogus variants (confy on
  windows-x86_64 gave `desktop-64`/`64`; now `desktop`/none). Installs recorded under the old names
  need a reinstall.
- fix(info): `wenget info` exits non-zero when any requested name is not found (it still prints the
  names that were found).
- perf(update): `update` hands its loaded Installed set and synced cache to the install step, and
  `add` reuses its in-memory cache when recording new versions, so neither is re-read from disk (OP-2).
- refactor: remove dead code previously hidden by `#[allow(dead_code)]`; the remaining allows are
  scoped to the platform or test build that needs them (SI-7).
- refactor(http): one process-wide `reqwest` client shared by `HttpClient`, downloads, and checksum
  probes, with per-request timeouts; checksum probes now send the `wenget/<version>` User-Agent (SI-12).
- test: cover `init`, `bucket`, `info`, and `list` with unit tests and offline end-to-end tests
  in `tests/cli.rs` (T-6).
- refactor(add): `install_package` becomes `installer::package::PackageInstaller::install`, taking
  an `InstallRequest`; executable selection is its own method with scripted-UI tests (CL-8, 3/3).
- refactor(add): the install flow asks and reports through an `InstallUi` trait
  (`utils::prompt::TerminalUi` in the terminal), so its prompts can be scripted in tests (CL-8, 2/3).
- refactor(add): release selection and binary filtering move to headless
  `installer::package` (`target_package`, `filter_binaries`); the release is chosen once during
  planning instead of twice, so install time no longer re-queries the GitHub API (CL-8, 1/3).
- test(update): end-to-end `update` tests against a local HTTP fixture (bucket + GitHub API), with
  the API base overridable via the `WENGET_GITHUB_API` test hook (T-3).
- refactor(add): `install_packages`/`install_package` take `&InstallOptions` instead of positional
  flags, and the unused `_parent_package` argument is gone (CL-1; Installer split tracked as CL-8).
- refactor(paths): resolve `~/.local/bin` when `WenPaths` is built, removing the `expect` in
  `bin_dir()` (Q-6; the panic was unreachable, no behavior change).
- test: add `tests/lifecycle.rs`, offline end-to-end tests that run the real binary in a
  `WENGET_ROOT` sandbox (script and archive install, rename, repair, failed install, reinstall,
  delete); `assert_cmd` dev-dependency (T-2; T-3 narrowed to `update`).
- fix(staging): remove the empty `apps/.staging/` root after an install commits or aborts (B-10).
- fix(add): local archives strip their extension when naming the app, and the record is keyed by the app dir, so `repair`/`del` see one package (B-9).
- fix(delete): `del self` under `WENGET_ROOT` keeps an executable that lives outside the root (B-8).
- fix(init): with `WENGET_ROOT` set, `init` no longer edits the real shell rc files or registry
  PATH (prints the PATH line instead), and `del self` no longer strips PATH entries (B-7).
- docs(backlog): close 20 rows from waves A–D (Windows fixes S-4/S-6/S-8/SI-5/S-10 sit in
  Pending verification until tested on real Windows); open B-7 (`init` edits real rc files under
  `WENGET_ROOT`). CI workflow re-enabled on GitHub.
- chore(deps): `zip` 0.6 → 8.6 (deflate, bzip2, zstd), `bzip2` 0.5 → 0.6 (one copy in the tree),
  and the unmaintained `is_elevated` replaced by a `windows-sys` token check (M-5).
- test(extractor): zip extraction (contents, exec bit, `../` rejection) and corrupt/truncated
  zip, tar.gz, tar.xz, tar.bz2 and 7z archives are covered; zip paths use `/` on Windows (T-5).
- test(delete): variant grouping moved into `group_delete_candidates`; tests call it instead of a
  copied loop (T-4).
- refactor(add): one `BatchReport` replaces five hand-rolled success/failure tallies and summary
  blocks; failed packages are now always named in the summary (SI-11).
- refactor: `bucket` subcommands consume the clap `BucketCommands` enum directly (SI-8); one
  `first_free` helper replaces three numeric-suffix loops (SI-9); `init` reuses
  `installer::create_symlink` (SI-4).
- ci: stop committing the 4.5 MB `bucket/wenget` binary on every release; `update-manifest.yml`
  now downloads the latest release asset and checks it against `SHA256SUMS` (M-2). Existing
  history is not rewritten.
- feat(install): `install.sh` and `install.ps1` verify the downloaded release asset against the
  release's `SHA256SUMS` and abort on mismatch; `install.ps1` downloads to a temp file first
  (S-10).
- feat(add): downloads over plain `http://` print a warning (S-9; the cheap "warn" variant, not a
  hard reject).
- fix(update): `wenget update self` no longer warns "'self' is not installed"; it says wenget is
  checked on every update (B-4).
- fix: legacy record migration writes `/`-separated executable keys on Windows too.
- feat(repair): `repair` offers to recreate missing launchers from the package record (`--force`
  recreates them without prompting); a directory blocking the path is reported, never removed
  (IM-12).
- fix(update): updating a bucket script keeps the command name set by `wenget rename` instead of
  recreating the original launcher and leaving the renamed one stale (B-2).
- fix(add): package name patterns use `glob::Pattern` like `del`, so `?` and `[...]` work
  (`add 'f[d]'`) (SI-3).
- fix: 7z extraction records `/`-separated paths on Windows like tar/zip; tests fixed for Windows
  paths; code updated for the latest clippy lints (CI green on all platforms).
- fix(add,update): downloaded archives and the self-update scratch directory are removed on every
  error path, not only after success (IM-9).
- fix(update): the GitHub API update path and self-update only offer strictly newer numeric
  versions, matching the cache path; they no longer offer downgrades (IM-8).
- fix(windows): user PATH is edited in the registry instead of generated PowerShell source, so
  usernames containing `'` work and entries match exactly; PATH changes are broadcast
  (`WM_SETTINGCHANGE`); `del self` also removes the system PATH entry for system installs
  (S-4, SI-5).
- fix(windows): self-update cleanup and `del self` scripts now run when the path contains spaces
  (`start` got the quoted path as its window title); non-UTF-8 paths no longer panic (S-8, Q-7).
- fix(windows): admin `init` keeps the system `Path` value type (`REG_EXPAND_SZ`) when adding to
  PATH, so `%SystemRoot%`-style entries keep expanding (S-6).
- docs(backlog): close B-5; open B-6 (`del self` PATH cleanup with `custom_bin_path`).
- fix(add,delete): `add` and `delete` honor `custom_bin_path`, creating and removing launchers in
  the configured bin directory like `repair`/`rename`/`init` (B-5).
- docs(backlog): close IM-6/IM-7/IM-10/IM-13/IM-14/B-1/CL-2/CL-4/CL-7; open B-5 (`add`/`delete`
  ignore `custom_bin_path`).
- chore: drop unused `thiserror` dependency and stale `#[allow(dead_code)]` on live items
  (`Config::preferences`, `WenPaths::staging_dir`, `ScanEntry`) (CL-7, CL-2).
- docs(config): the generated `config.toml` now documents the real default user bin directory
  `~/.local/bin` (CL-4).
- fix(add): URLs are treated as GitHub repos only when the host is github.com; other URLs that
  merely contain "github.com" are downloaded directly (IM-14).
- fix(add): "Removed obsolete command" is printed only when removal succeeds; failures now
  show a warning with the reason (IM-10).
- fix(repair): a directory or dangling symlink at a launcher path is reported instead of
  counting as a healthy launcher (B-1).
- fix(add): a failed package-record save now counts the install as failed (non-zero exit)
  instead of reporting success (IM-6).
- fix(config): invalid `config.toml` preferences now really fall back to defaults instead of
  being used after the "using defaults" warning (IM-7).
- fix(config): `wenget config` honours `$VISUAL` and editor commands with arguments such as
  `code --wait` (IM-13).
- docs(backlog): re-verify all open rows; close A-4/M-4, merge Q-7 into S-8, drop P-4/P-5,
  re-prioritise S-5/S-6/S-7 and fix stale evidence symbols.
- fix(deps): replace abandoned `sevenz-rust` (unpatched path traversal RUSTSEC-2026-0245) with
  `sevenz-rust2` 0.23; `extract_7z` also rejects rooted and drive-prefixed entry names, which
  could escape the extraction directory on Windows. Adds 7z extraction tests.

- fix(deps): bump locked crates past security advisories — `tar` 0.4.46, `rustls` 0.23.45,
  `rustls-webpki` 0.103.15, `quinn-proto` 0.11.18 (`rand` 0.10.3), `anyhow` 1.0.104. Closes
  Dependabot alerts #1–#10 and RUSTSEC-2026-0285/-0190.

- docs: close the 2026-09-23 documentation audit (Resolved) and its plan (Shipped).

- docs(readme): `del` replaces the nonexistent `delete`; only `--verbose` is global; bucket
  manifest examples now parse; `update self` is gone (`update` always checks wenget first);
  `apps`, `manifest-cache.json` and bin-dir paths, the `GITHUB_TOKEN` rate-limit text and Windows
  ARM64 are corrected; aliases, `add`/`del`/`repair` flags, `WENGET_ROOT`, exit status, checksum
  verification and stage-and-swap installs are documented (documentation audit DA-1–DA-10).

- docs(reference): `RESOURCE_FILTERING_RULES.md` describes the single scoring engine
  `score_parsed`, the fallbacks `fallback_identifiers` actually produces, and multi-executable
  selection; the glossary drops the removed `SourceProvider`, fixes **Manifest**, and adds
  **Launcher**, **Staging** and **Manifest cache** (documentation audit DA-11–DA-18).

- docs: `CLAUDE.md` is now a pointer that imports `AGENTS.md`; `AGENTS.md` drops its project
  tree and the removed `SourceProvider` example, adds a Gotchas section, and its release steps
  match the `[Unreleased]` convention, the `v*.*.*` tag trigger and the changelog archive rule
  (documentation audit DS-13, DA-19–DA-22).

- docs: add `docs/plan/BACKLOG.md`, the one living tracker of open work, seeded with every
  still-open finding from the 2026-09-03 and 2026-09-23 audits (documentation audit DS-1).

- docs: the 0.x, 1.x and 2.x changelog series move verbatim to `docs/reference/changelog/`;
  the root file keeps `[Unreleased]` and 3.x. A stray empty `[Unreleased]` heading and broken
  compare links are fixed (documentation audit DS-11, DS-12).

- docs: the 2026-09-23 audit's repro script moves beside its audit, the `search` evaluation
  becomes `docs/audit/2026-09-23-search-evaluation.md`, the rest of `docs/tmp/claude-scratch/`
  is archived to `docs/tmp/archive/2026-09.tar.gz`, and `docs/tmp/*-scratch/` is gitignored
  (documentation audit DS-7–DS-9).

- docs: records put `Status:` on line 2 with values from the fixed set; ADR 0001 is
  `Implemented`; spec/plan pairs share a basename (`2026-03-18-update-scripts-and-self-check`,
  `2026-04-10-update-default-binary-asset-matching`, `2026-09-03-per-package-records`);
  `CONTEXT.md` lists `docs/tmp/` and the backlog (documentation audit DS-2–DS-6, DS-10).

- docs(audit): add the 2026-09-23 documentation audit and its fix plan.

- fix(cli): error messages now include the full cause chain (e.g. `Failed to create wenget root
  directory: Not a directory`) instead of only the outermost context (audit IM-2).

- fix(cli): `--verbose` now shows wenget's debug logs and `RUST_LOG` is honored; they were
  previously overridden by a hard-coded Info filter. Default level is now Warn, so INFO lines that
  duplicated normal output are no longer printed (audit IM-3).

- fix(add): `add` and `update` now exit 1 when any install fails, including inputs that are not
  found or do not support the platform. All inputs in a batch are still attempted before the
  failure is reported; a user cancellation or "already up to date" still exits 0 (audit IM-1).

- fix(rename): renaming a Python/PowerShell script package no longer fails with "Failed to read
  symlink" on Unix; script launchers are rebuilt from the package record. The new launcher is now
  created before the old one is removed, so a failed rename keeps the old command (audit IM-4).

- fix(add): a launcher failure after the staged swap no longer erases the package. Command names
  are resolved and executables checked before the swap; after it, launcher errors are collected,
  the new package record is still written, and the install then fails with the list of launchers
  that could not be created (audit IM-5).

- fix(add): a GitHub URL that cannot be fetched now reports the real cause (network error, HTTP
  404, …) instead of "Not found", and an exhausted GitHub API rate limit is named as such with its
  reset time: `GitHub API rate limit exceeded (resets at HH:MM)`.

- feat: `add`, `update` and `info` use `GITHUB_TOKEN` when set, raising the GitHub API limit from
  60 to 5000 requests per hour.

- perf: fewer GitHub API calls (audit OP-1). Installing a bucket package takes 1 request,
  a GitHub URL 2, and each `update` check 1 (the audit counted 4, 6 and 2): repository description/license are
  fetched only for a URL's first resolution, and a just-resolved URL is not fetched again.
  `fetch_package` and `fetch_package_by_version` are merged, and the single-implementor
  `SourceProvider` trait is removed (SI-2).

- refactor(platform): one asset-scoring engine (audit SI-1). The unused `score_asset`,
  `select_all_for_platform` and `detect_compiler_from_filename` are removed; the test helper
  `select_for_platform` now scores through production's `score_parsed`, and every existing
  platform test passes unchanged against it.

- refactor(add): `add::run` takes an `InstallOptions` struct instead of seven positional flags
  (audit CL-1, narrow part).

- feat(search): fuzzy, ranked, case-insensitive search. Terms match names by tier (exact >
  prefix > word-boundary substring > substring > subsequence such as `rgp` → ripgrep > typo
  tolerance for 4+ char terms), then word prefixes in descriptions and repo URLs (e.g. `json`,
  `burntsushi`). Terms containing `*`, `?`, or `[` keep glob matching, now case-insensitive.
  Results are sorted by relevance instead of random order, and the output reports how many
  matches are hidden because they are unavailable on this platform.
- feat(add): an unknown name now prints `Did you mean: …?` suggestions from bucket packages and
  scripts.
- fix(search): truncating descriptions no longer panics on multi-byte (CJK/emoji) text.

[Unreleased]: https://github.com/superyngo/wenget/compare/v4.0.0...HEAD
[4.0.0]: https://github.com/superyngo/wenget/compare/v3.9.0...v4.0.0
