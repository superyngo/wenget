# Changelog

All notable changes to wenget will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### 2026-09-23

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

## [3.9.0] - 2026-09-21

### Security

- fix(installer): reject archive entries that escape the destination directory (Zip Slip).
  `extract_tar_archive` normalized only `Component::CurDir`, then joined the raw entry path onto
  `dest_dir` and called `Entry::unpack`, which performs no containment checking — so a `.tar.gz`
  /`.tar.xz`/`.tar.bz2` entry named `../../x` or `/etc/x` wrote outside the target directory (and
  was chmod'd `0o755`), giving arbitrary file write from a malicious release asset, or arbitrary
  root file write during a system install. Entries with `..` or an absolute path are now rejected
  with an explicit error, and the write goes through `Entry::unpack_in`, which strips root
  prefixes, refuses `..`, and validates symlink/hardlink targets. `extract_7z` likewise validated
  nothing before handing the destination to `sevenz_rust::decompress_file`; entry names are now
  checked up front. `.zip` was already correctly defended via `ZipFile::enclosed_name()` and is
  unchanged. Regression tests cover both the relative and absolute vectors (2026-09-03).
- fix(paths): `sanitize_path_component` now neutralizes path traversal, not just `::`. Package
  and command names arrive from remote bucket manifests and the GitHub API and are joined onto
  the apps/bin directories; the resulting path is also what `delete` hands to `remove_dir_all`,
  so a name containing `/`, `\`, or `..` could place an install outside `~/.wenget/apps/` and
  have it recursively deleted later. Separators are replaced, `..` is collapsed, and an all-dots
  or empty component becomes `_` (2026-09-03).

### Fixed

- fix(platform): `contains_unknown_arch_pattern` fed the byte offset from `str::find` into
  `chars().nth(..).unwrap()`, so any release asset whose name contained multi-byte characters
  pushed the offset past the character count and panicked — an immediate abort in release builds
  (`panic = "abort"`), reachable from `add`, `update`, `search`, and `bucket create`. Boundary
  checks now operate on bytes throughout, removing both the panic and the adjacent
  non-char-boundary slicing hazard (2026-09-03).
- fix(search): `wenget search` unwrapped the platform lookup and the first binary of each result,
  both of which come from a remote bucket manifest, so an entry with a missing or empty binary
  list aborted the whole search. Such entries now render as `0.0 MB` instead (2026-09-03).
- fix(core): `cargo test` no longer overwrites the developer's real `~/.wenget/installed.json`.
  Three tests in `core/config.rs` constructed `Config::new()`, which resolves the live home
  directory, and `test_manifest_round_trip` then wrote an empty manifest over it, silently
  destroying the local package registry on any contributor's machine. Adds test-only
  `WenPaths::with_root` / `Config::with_paths` seams and points those tests at a `TempDir`
  (2026-09-03).

### Changed

- refactor(core)!: replace `installed.json` with per-package records at
  `{app_dir}/.wenget/package.json`. All installed-package state lived in one file, rewritten in
  full and non-atomically by eight call sites, so a crash mid-write or any bug that serialized an
  empty collection lost the record of *every* package — which already happened once: `cargo test`
  truncated the maintainer's `installed.json` while the app directories survived. Each package now
  owns its own record inside its own app directory, and the set of installed packages is the set
  of directories carrying a readable one; no global index is written, not even as a cache. Installs
  stage into `apps/.staging/` and swap by `rename`, so a failed update leaves the previous install
  and its record intact. `wenget repair` reports per-directory status, sweeps interrupted-install
  leftovers, and only reports or removes a launcher in the bin directory whose wenget provenance it
  can prove. A legacy `installed.json` is migrated on first load and renamed to
  `installed.json.migrated-<timestamp>`, never deleted; entries whose directory is gone are
  dropped and listed. `WENGET_ROOT` now overrides the root in release builds so this can be
  verified without touching a real `~/.wenget`. See
  `docs/adr/0001-no-global-installed-index.md` (2026-09-03).
- refactor(core): rename `InstalledManifest` to `InstalledSet`. "Manifest" already means a
  bucket's `manifest.json`, and the in-memory collection of installed packages is no longer a file
  format at all (2026-09-03).
- refactor(core): two deviations from the written plan, both found by tests rather than review.
  `install_path` is `#[serde(default, skip_serializing)]`, not `#[serde(skip)]`: a record must
  never store its own location, but a legacy `installed.json` carries it and migration reads it to
  find the app directory — `skip` broke that. And launcher provenance is resolved *lexically*, not
  by absolute-path substring match: both the Windows `.cmd` shim and the Unix symlink store the
  target relative to the bin directory (`%~dp0..\apps\...`), so the planned absolute match would
  have detected nothing and left orphan detection silently dead (2026-09-03).
- docs(plan): add `docs/plan/2026-09-03-per-package-records.md` — an eleven-task,
  TDD, commit-per-task plan implementing the per-package-records spec. Ordering is forced by two
  constraints found while planning: `WENGET_ROOT` lands first because nothing else can be verified
  on the release binary without it, and stage-and-swap lands before the record becomes
  authoritative because every install path wipes the app directory before writing. Task 8 (delete
  `Config::save_installed`, flip `install_path` to `#[serde(skip)]`, convert all eight writers,
  strip `repair`'s `installed.json` branch) is called out as irreducible: it cannot be split
  without leaving the tree red, and Tasks 1-7 exist to shrink it. Two steps deliberately send the
  implementer to read reality first rather than guess — the Windows shim signature in
  `installer/shim.rs`, and `PackageSource`'s serde shape (2026-09-03).
- docs(spec): settle the design for removing `installed.json` in favor of per-package **package
  records** at `{app_dir}/.wenget/package.json`, and record the decision as
  `docs/adr/0001-no-global-installed-index.md`. The spec now covers what the draft left implicit:
  install/update must stage-and-swap because every install path wipes the app directory before
  writing (so a colocated record would be destroyed at the start of each update); `install_path`
  becomes `#[serde(skip)]` rather than a written-then-ignored field; a record with a
  `meta_version` above the known maximum is skipped, never quarantined, so a downgrade cannot
  damage future data; read-path writes (migration, quarantine) are best-effort and never fail a
  read command; orphaned-shim detection must prove wenget provenance because `bin_dir` is
  `~/.local/bin`, shared with unrelated software; the single-writer assumption and its new
  "two packages claim one command name" race are stated in Out of Scope; and a `WENGET_ROOT`
  override is added so the behavior can be verified on the release binary instead of against the
  maintainer's real `~/.wenget`. Also corrects the call-site count (`repair.rs:103,113` was the
  missing eighth `save_installed` caller) and fixes the vocabulary against the glossary:
  `InstalledManifest` → `InstalledSet`, so "manifest" keeps meaning a bucket's `manifest.json`
  (2026-09-03).
- ci: add `.github/workflows/ci.yml` — quality gate running `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets` on
  ubuntu/windows/macos for every `pull_request` and push to `main`. No workflow previously ran
  tests, clippy, or fmt: the existing workflows are tag-triggered release builds, a publish
  gate, and a scheduled manifest update, so the clean lint/test state was unenforced and
  platform-gated `#[cfg(windows)]`/`#[cfg(unix)]` code was never exercised in CI (2026-09-03).
- chore(deps): drop the `tokio` dependency. It was declared with `features = ["full"]` but the
  codebase contains no `tokio::`, `async fn`, `.await`, or `block_on` — all networking is
  `reqwest::blocking` and concurrency is `std::thread::scope`. It now appears only as a
  transitive dependency of `reqwest`, with just the features reqwest needs, which suits a
  release profile deliberately tuned for size (`opt-level = "z"`, `lto`, `strip`) (2026-09-03).
- docs: restructure `docs/` per wens-dev-principles (docs domain) — added `CONTEXT.md` root
  index; split `docs/` into `reference/`, `adr/`, `spec/`, `plan/`, `debug/`, `audit/`; moved
  `docs/RESOURCE_FILTERING_RULES.md` to `docs/reference/`; moved `docs/superpowers/specs|plans/*`
  to `docs/spec/`/`docs/plan/` with `Status: Shipped (...)` lines added; added
  `docs/reference/glossary.md`; each folder gained an indexing `README.md`; `AGENTS.md` now
  points at `CONTEXT.md` (2026-09-02).
- docs: add `docs/audit/2026-09-03-full-codebase-audit.md` — point-in-time six-dimension audit of
  v3.8.7 (architecture, code quality, security, performance, testing, maintainability) covering
  16,957 LOC across 41 files; 2 Critical / 8 High / 11 Medium / 9 Low findings with `path:line`
  evidence, effort estimates, and a prioritized action plan. Four findings were verified by
  execution rather than inspection: tar (`extractor.rs:218-254`) and 7z (`:140-146`) archive path
  traversal allowing arbitrary file write; a byte-offset/char-index `unwrap` panic on non-ASCII
  asset names (`platform.rs:498-504`); `cargo test` overwriting the real
  `~/.wenget/installed.json` via non-injectable paths in `core/config.rs:307-331`; and the
  absence of any CI workflow running `cargo test`/`clippy`/`fmt`. Indexed in
  `docs/audit/README.md` (2026-09-03).
- docs: add `docs/spec/2026-09-03-per-package-records.md` (Draft) — design record for removing
  `installed.json` and storing each package's record in `{install_path}/.wenget/package.json`.
  Motivated by audit findings A-1 (non-atomic whole-registry writes from seven call sites) and
  T-1 (a test wiping the real registry): both have a blast radius of every installed package
  because one file describes all of them. Covers the `InstalledStore` load/save API, per-package
  atomic writes, corrupt-meta quarantine, the new orphaned-shim check in `repair`, the
  sanitized-directory collision guard, and one-time migration off `installed.json`. Not
  implemented (2026-09-03).

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

[3.0.4]: https://github.com/superyngo/wenget/compare/v3.0.3...v3.0.4
[3.0.3]: https://github.com/superyngo/wenget/compare/v3.0.2...v3.0.3
[3.0.2]: https://github.com/superyngo/wenget/compare/v3.0.1...v3.0.2
[3.0.1]: https://github.com/superyngo/wenget/compare/v3.0.0...v3.0.1
[3.0.0]: https://github.com/superyngo/wenget/compare/v2.3.1...v3.0.0
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
[3.9.0]: https://github.com/superyngo/wenget/compare/v3.8.7...v3.9.0
