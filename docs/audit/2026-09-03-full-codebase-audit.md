# Full Codebase Audit — 2026-09-03

Status: In progress (findings not yet addressed)
Scope: `wenget` v3.8.7 at commit `f0f8594`, all of `src/` (16,957 LOC, 41 files), plus
`Cargo.toml`, `.github/workflows/`, `install.sh`, `install.ps1`, and the `docs/` tree.
Dimensions: architecture, code quality, security, performance, testing, maintainability.

---

## Executive summary

**Overall health: 6/10.** The low-level engine is genuinely strong — platform/asset matching is
well-factored and heavily regression-tested, download and extraction are correctly streamed at
flat memory, `cargo fmt` and `cargo clippy` are clean, and all 41 files carry `//!` module docs.
The canonical reference doc (`docs/reference/RESOURCE_FILTERING_RULES.md`) was spot-checked
against `platform.rs` on five rules and matched on all five.

What drags the score down is the perimeter, not the core: **archive extraction has an exploitable
arbitrary-file-write hole**, the install/update/delete state machines are essentially untested,
and **no CI workflow runs tests, clippy, or fmt** — so the clean local state is unenforced.

### Top 3 priorities

1. **Fix tar + 7z path traversal** (Critical, ~half day). A malicious release archive writes
   files anywhere the process can write. Proven with a working exploit; see finding S-1.
2. **Stop `cargo test` from destroying live user state** (High, ~half day). Three tests in
   `core/config.rs` write to the real `~/.wenget/`. This fired during this audit and emptied the
   maintainer's `installed.json`; see finding T-1.
3. **Add a CI quality gate** (High, ~1h). No workflow runs `cargo test`/`clippy`/`fmt --check`
   on push or PR; see finding M-1.

### Counts

| Severity | Count |
|---|---|
| Critical | 2 |
| High | 8 |
| Medium | 11 |
| Low | 9 |

---

## Security

Verified present and correct, so the report does not imply gaps that aren't there: TLS defaults
are intact (no `danger_accept_invalid_certs` anywhere), reqwest's cross-host sensitive-header
stripping is not overridden, there are no hardcoded credentials, and **ZIP extraction is
correctly defended** via `ZipFile::enclosed_name()`.

### S-1. Arbitrary file write via tar path traversal (Zip Slip) — **PROVEN EXPLOITABLE**
- **Severity**: Critical
- **Location**: `src/installer/extractor.rs:218-254`
- **Evidence**: `extract_tar_archive` filters entry-path components for `Component::CurDir`
  only:
  ```rust
  let path: std::path::PathBuf = raw_path
      .components()
      .filter(|c| !matches!(c, std::path::Component::CurDir))   // line 223-226
      .collect();
  let dest_path = dest_dir.join(&path);                          // line 237
  entry.unpack(&dest_path)                                       // line 253
  ```
  `Component::ParentDir` (`..`) and `Component::RootDir` (`/`) are never filtered. Two things
  then go wrong: Rust's `Path::join` *replaces* the base when the joined path is absolute, and
  bare `Entry::unpack(dst)` passes `target_base = None` internally, so it performs **zero**
  containment, escape, or symlink validation (that behavior belongs to `unpack_in`, which is not
  used). Confirmed against crate source `tar-0.4.44/src/entry.rs`.

  This was confirmed empirically, not by inspection alone. Replicating the exact logic above and
  feeding it forged archives:
  ```
  === extracting evil_relative.tar.gz into .../run/apps/victimpkg ===
      entry "../../ESCAPED.txt" -> wrote .../run/apps/victimpkg/../../ESCAPED.txt
      extraction returned Ok (no traversal error raised)
  === extracting evil_absolute.tar.gz into .../run/apps/victimpkg ===
      entry "/tmp/.../run/ABSOLUTE.txt" -> wrote /tmp/.../run/ABSOLUTE.txt
      extraction returned Ok (no traversal error raised)
  *** ESCAPED: .../run/ESCAPED.txt exists OUTSIDE dest_dir
  *** ESCAPED: .../run/ABSOLUTE.txt exists OUTSIDE dest_dir
  ```
  Both vectors escape and extraction returns `Ok` — the failure is silent. Repro kept at
  `/tmp/claude-audit-scratch/tarslip/` (`forge.py` + `src/main.rs`).
- **Impact**: Attacker-controlled input is any `.tar.gz`/`.tar.xz`/`.tar.bz2` release asset —
  i.e. any of the ~500 third-party repos listed in `bucket/sources_repos.txt`, or any repo a user
  passes to `wenget add`. One compromised upstream release yields arbitrary file write: drop into
  `~/.zshrc`, `~/.local/bin`, or a cron directory to get code execution. Extraction also chmods
  executables to `0o755` (`extractor.rs:264`), so dropped payloads can be made executable. During
  a **system install running as root**, this is arbitrary root file write, i.e. trivial local
  privilege escalation. `panic = "abort"` is irrelevant here — nothing panics; it just succeeds.
- **Recommendation**: Use `entry.unpack_in(dest_dir)`, which strips root prefixes, rejects `..`,
  and validates symlink/hardlink targets. Keep the existing `CurDir` normalization for the
  recorded relative path, but derive the write path from `unpack_in`. Note `unpack_in` returns
  `Ok(false)` when it skips an unsafe entry — treat that as a hard error, not a silent skip.
- **Effort**: S

### S-2. Arbitrary file write via 7z path traversal
- **Severity**: Critical
- **Location**: `src/installer/extractor.rs:140-146`
- **Evidence**: `extract_7z` delegates wholesale to `sevenz_rust::decompress_file(archive_path,
  dest_dir)`. Crate source `sevenz-rust-0.6.1/src/de_funcs.rs:101-146` computes
  `dest.join(entry.name())` and calls `File::create` on it with no sanitization of
  `entry.name()` — no `..` rejection, no containment check. (Source-verified; not separately
  exploited, since the tar proof already establishes the class.)
- **Impact**: Same as S-1, via a malicious `.7z` asset.
- **Recommendation**: Switch to `sevenz_rust::decompress_file_with_extract_fn` and validate each
  `entry.name()` (reject absolute paths, `..`, and leading separators) before writing.
- **Effort**: S

### S-3. Package names are not validated before being used to build — and delete — paths
- **Severity**: High
- **Location**: `src/core/paths.rs:32-34`, `src/commands/delete.rs:271-297`
- **Evidence**: The only sanitization is `pub fn sanitize_path_component(name: &str) -> String {
  name.replace("::", "-") }`. Path separators, `..`, and leading `-` all pass through.
  `app_dir(name)` joins that into `apps_dir()`, and `delete_package` calls
  `fs::remove_dir_all(&app_dir)` on the result.
- **Impact**: A hostile or typo'd bucket manifest entry can place an install outside
  `~/.wenget/apps/`, and `wenget delete` will then recursively delete an arbitrary directory.
- **Recommendation**: Validate names against `^[A-Za-z0-9._-]+$`, rejecting separators, `..`, and
  leading `-`; apply at manifest-parse time so bad names never reach path construction.
- **Effort**: XS

### S-4. PowerShell PATH scripts interpolate a path into generated source
- **Severity**: High
- **Location**: `src/commands/init.rs:508-525`, `src/commands/delete.rs:508-525`
- **Evidence**: `bin_dir` is `format!`-ed into a PowerShell script body inside single quotes,
  then run via `Command::new("powershell").args(["-NoProfile", "-Command", &ps_script])`. A
  quote, `$`, or `$(...)` in the path terminates the literal.
- **Impact**: Arbitrary PowerShell execution at user privilege. Reachable via a custom bin path
  (`core/preferences.rs` `custom_bin_path`) or an unusual profile directory.
- **Recommendation**: Don't generate PowerShell source. Pass the path via an environment variable
  and reference `$env:VAR` in a static script, or write the registry value directly.
- **Effort**: S

### S-5. Checksum verification is best-effort and silently downgrades
- **Severity**: Medium
- **Location**: `src/core/checksum.rs:185-217`
- **Evidence**: Verdict, confirmed by reading every call site (`add.rs:608`, `add.rs:1736`,
  `update.rs:777`): a **mismatch is correctly fatal** (`bail!` at :193, caller deletes the file),
  but three of four outcomes return `Ok(())`:
  `NotPublished` → skip (:206), `ProbeFailed` → skip (:213), `NotApplicable` → skip (:215).
  `NotApplicable` covers any URL lacking `/releases/download/` (:70-72), so direct-URL installs
  are never verified. `Package.checksum` in `core/manifest.rs:179` is parsed but ignored.
  The doc comment at :183-184 shows the skip-and-proceed behavior is deliberate.
- **Impact**: The `ProbeFailed` branch is the real weakness: an attacker who can drop or stall
  the `SHA256SUMS` request downgrades verification to nothing and still gets an install. A
  supply-chain guarantee that an attacker can turn off is not a guarantee.
- **Recommendation**: Distinguish "upstream publishes no checksum" (skip, as today) from "we
  could not reach the checksum" (fail, or require `--allow-unverified`). Honor
  `Package.checksum` when the manifest supplies one.
- **Effort**: S

### S-6. Windows PATH write downgrades `REG_EXPAND_SZ` to `REG_SZ`
- **Severity**: Medium
- **Location**: `src/core/registry.rs:65-66`
- **Evidence**: `env.set_value("Path", &new_path)` writes a `REG_SZ`. The system PATH key is
  conventionally `REG_EXPAND_SZ` so `%SystemRoot%`-style entries expand.
- **Impact**: A system-level PATH edit can break expansion for every newly launched process —
  potentially breaking `%SystemRoot%\system32` for the whole machine.
- **Recommendation**: Use `set_raw_value` with `REG_EXPAND_SZ`.
- **Effort**: S

### S-7. Generated shims interpolate paths without escaping
- **Severity**: Medium
- **Location**: `src/installer/shim.rs:16-19`, `src/installer/script.rs:348-360`
- **Evidence**: `script.rs:289` has an `escape_batch_path` helper, but `shim.rs:16-19` does not
  call it when building `.cmd` content, leaving `&`, `%`, `^`, `!` unescaped. On Unix,
  `create_script_shim_unix` formats `script_path.display()` into
  `exec python3 "{}" "$@"` — a `"` or `$(...)` in the path escapes the quoting.
- **Impact**: Command injection triggered when the *user later runs the shim*, from a package or
  path containing metacharacters.
- **Recommendation**: Apply the existing `escape_batch_path` in `shim.rs`; escape `"`/`$`/`` ` ``
  (or single-quote) in the Unix wrapper.
- **Effort**: XS

### S-8. Windows self-update/uninstall scripts misparse quoted paths
- **Severity**: Medium
- **Location**: `src/commands/update.rs:863-877`, `src/commands/delete.rs:616-642`
- **Evidence**: `Command::new("cmd").args(["/C", "start", "/B", path])` — `start` treats its
  first quoted argument as a *window title*, so paths with spaces are misparsed. Batch
  metacharacters in `old_exe.display()` are also uninterpolated-unescaped.
- **Impact**: Self-update cleanup silently fails, or launches the wrong executable, for users
  whose install path contains a space.
- **Recommendation**: Pass an explicit empty title (`start "" "<path>"`), or replace the
  binary without shelling out to `cmd`.
- **Effort**: S

### S-9. `http://` download URLs accepted without warning
- **Severity**: Low
- **Location**: `src/downloader/mod.rs:21-35`, `src/installer/input_detector.rs:48-57`
- **Evidence**: No scheme check; plaintext URLs for archives, binaries, and scripts are accepted.
- **Impact**: MITM tampering, compounded by S-5 (such URLs are also `NotApplicable` for
  checksums, so they get no verification at all).
- **Recommendation**: Reject `http://` unless an explicit `--allow-http` flag is passed.
- **Effort**: XS

### S-10. Bootstrap installers don't verify what they download
- **Severity**: Low
- **Location**: `install.sh:158-187`, `install.ps1:139-165`
- **Evidence**: Both fetch a release asset and run `wenget init` without fetching `SHA256SUMS`
  or checking a signature — even though the project *does* publish `SHA256SUMS`.
- **Recommendation**: Verify the published SHA-256 before executing. Low cost, since the input
  already exists.
- **Effort**: S

---

## Testing

### T-1. `cargo test` overwrites the developer's real `~/.wenget/installed.json`
- **Severity**: High
- **Location**: `src/core/config.rs:307-331`
- **Evidence**: `test_config_creation`, `test_init`, and `test_manifest_round_trip` all call
  `Config::new()`, which builds `WenPaths::new()` → `dirs::home_dir().join(".wenget")`. There is
  no path injection; `TempDir` is imported at :294 and the helper at :296-305 admits in a comment
  that overriding paths isn't wired up. `test_manifest_round_trip` then does:
  ```rust
  let config = Config::new().unwrap();
  config.init().unwrap();
  let manifest = InstalledManifest::new();      // empty
  config.save_installed(&manifest).unwrap();    // writes real ~/.wenget/installed.json
  ```
- **Impact**: **This is not theoretical — it fired during this audit.** Running `cargo test` on
  this machine truncated the maintainer's `installed.json` to `{"packages": {}}` (mtime jumped to
  the test run; every sibling file in `~/.wenget/` was months older). The two tracked packages,
  `fnm` and `lazyssh`, are still present on disk under `~/.wenget/apps/`, so nothing was
  uninstalled — only the registry that tracks them was lost. No backup was written, because
  `create_backup` (`config.rs:103`) only fires on a *parse* failure and this was a valid write.
  Any contributor running the standard test command silently loses their package registry.
- **Recommendation**: Add `WenPaths::with_root(PathBuf)` and `Config::with_paths(WenPaths)`, and
  point these three tests at a `TempDir`. This is also the prerequisite for T-2.
- **Effort**: S

### T-2. No integration tests, and the code is not currently injectable enough to add them
- **Severity**: High
- **Location**: `src/core/paths.rs:49-75`, `src/downloader/mod.rs:10-18`, `src/core/checksum.rs:57-65`
- **Evidence**: No `tests/` directory exists. The full journey — resolve → download → verify →
  extract → link/shim → record → list → update → delete — has no end-to-end coverage. The
  blockers are concrete: `WenPaths` hardwires `dirs::home_dir()`, `downloader` uses a private
  `static OnceLock<reqwest::blocking::Client>`, and `checksum.rs` constructs a client inline
  with hardcoded GitHub URL probing. Because nothing can be pointed at a temp root or a mock
  server, the tests that would matter most are marked `#[ignore]`
  (`checksum.rs:307`, `:319`, `downloader/mod.rs:94`) — 5 ignored in total.
- **Impact**: The whole test suite runs in **0.02s** for 125 tests, which is the tell: it only
  covers pure leaf helpers. Multi-step regressions — a broken rollback, a stale shim, a bad
  manifest write — cannot be caught before release.
- **Recommendation**: Land the `with_root` seam from T-1, put the HTTP client behind a small
  trait or allow a base-URL override, then add `tests/` covering install→list→update→delete
  against a `TempDir` and a local mock server.
- **Effort**: M

### T-3. The install/update/delete state machines (~4,200 LOC) are effectively untested
- **Severity**: High
- **Location**: `src/commands/add.rs` (2,426 LOC / 9 tests), `src/commands/delete.rs`
  (754 / 1), `src/commands/update.rs` (1,020 / 3)
- **Evidence**: The existing tests cover string helpers (`normalize_asset_for_matching`,
  `resolve_command_name`, version comparison, winget-path detection). Zero tests touch
  extraction, shim/symlink creation, manifest mutation, or rollback.
- **Recommendation**: Extract the state transitions into pure functions over `WenPaths` +
  `InstalledManifest` and test both happy and failure paths.
- **Effort**: M

### T-4. `delete.rs`'s only test re-implements the logic instead of calling it
- **Severity**: Medium
- **Location**: `src/commands/delete.rs:712-754`
- **Evidence**: `test_specific_variant_not_duplicated_in_final_to_delete` copies ~20 lines of
  dedup loop into the test body and asserts on locals; it calls nothing from the module. A
  comment at :727 even marks where the original bug was. If the real implementation regresses,
  this test still passes.
- **Impact**: Coverage theater — `delete.rs` reads as "1 test" but is effectively at zero.
- **Recommendation**: Expose the candidate-resolution helper and test that.
- **Effort**: XS

### T-5. No adversarial tests on the highest-risk code
- **Severity**: Medium
- **Location**: `src/installer/extractor.rs:760-1032`, `src/core/checksum.rs:293-325`
- **Evidence**: Only one test actually runs `extract_archive`
  (`test_extract_tar_gz_strips_curdir_prefix`) — the other 11 are in-memory heuristics. There are
  **zero** tests for `.zip`, `.tar.xz`, `.tar.bz2`, or `.7z` decompression, zero for corrupt or
  truncated archives, and **zero for path traversal** — which is precisely why S-1 and S-2 went
  unnoticed. Offline checksum-mismatch coverage is absent (the mismatch test is `#[ignore]`).
- **Recommendation**: Add synthesized-archive tests per format, plus traversal cases
  (`../../evil`, absolute paths) as regression tests for S-1/S-2, and offline checksum
  mismatch/malformed cases.
- **Effort**: S

### T-6. Six command modules (~2,800 LOC) have no tests at all
- **Severity**: Medium
- **Location**: `commands/init.rs` (736), `bucket.rs` (927), `info.rs` (471), `list.rs` (298),
  `repair.rs` (198), `search.rs` (162)
- **Evidence**: No `#[cfg(test)]` module in any of them.
- **Effort**: S

Strong tests worth preserving, for balance: `extractor.rs:765-814` builds a real `.tar.gz` and
asserts on-disk results; `manifest.rs:1179-1227` exercises a real legacy-schema migration with
`0o755` files on disk; `platform.rs:2088-2127` are sharp regression tests against real-world
asset names (the "mac" inside "komac" false positive).

### Coverage vs. risk

| Module | LOC | Tests | Verdict |
|---|---|---|---|
| `commands/add.rs` | 2,426 | 9 | Severely under-tested vs. risk |
| `commands/update.rs` | 1,020 | 3 | Severely under-tested vs. risk |
| `commands/delete.rs` | 754 | 1 | Effectively zero (see T-4) |
| `installer/extractor.rs` | 1,032 | 12 | 1 real extraction test; no adversarial cases |
| `core/checksum.rs` | 332 | 9 | Offline failure paths untested |
| `core/config.rs` | 332 | 3 | Destructive (see T-1) |
| `core/repair.rs` | 322 | 6 | Happy path only |
| `core/platform.rs` | 2,185 | 32 | Well tested |
| `core/preferences.rs` | 219 | 7 | Well tested, properly isolated |
| 6 command modules | ~2,800 | 0 | Untested |

---

## Architecture & design

### A-1. `installed.json` writes are non-atomic
- **Severity**: High
- **Location**: `src/core/config.rs:149-157`; mutated from `add.rs:383,529,676,1589,1644`,
  `delete.rs:242`, `rename.rs:69`
- **Evidence**: `save_json` does `fs::write(path, json)` — truncate-in-place, no temp file, no
  rename, no fsync. Seven separate call sites mutate and persist installed state with no single
  choke point.
- **Impact**: Ctrl-C or power loss mid-write leaves a truncated file. On next run
  `load_installed` (:99-127) resets to an empty manifest. Mitigating factor, verified: it *does*
  back up the corrupt file first (:103) and prints a Critical warning (:120), so the state is
  recoverable by hand — this is degradation, not silent loss. Still, the window is avoidable.
- **Recommendation**: Write to `installed.json.tmp` in the same directory, `sync_all()`, then
  atomic `fs::rename`. Consider funnelling the seven mutation sites through one method.
- **Effort**: S

### A-2. `tokio` is a declared dependency with zero usage
- **Severity**: Medium
- **Location**: `Cargo.toml:20`
- **Evidence**: `tokio = { version = "1.35", features = ["full"] }`, yet a search across all of
  `src/` for `tokio`, `.await`, and `block_on` returns **no matches**. `main.rs` is a plain
  `fn main()`; all networking is `reqwest::blocking`; concurrency is `std::thread::scope`.
- **Impact**: `features = ["full"]` drags in mio, socket2, signal/process/fs reactors — pure
  build-time and binary-size cost, directly opposing a release profile deliberately tuned with
  `opt-level = "z"`, `lto`, `codegen-units = 1`, `strip`.
- **Recommendation**: Delete the dependency. Highest value-per-keystroke fix in this report.
- **Effort**: XS

### A-3. `add.rs` is a god module that `update.rs` drives through a boolean
- **Severity**: Medium
- **Location**: `src/commands/add.rs:31-2200`, `src/commands/update.rs:286`
- **Evidence**: `add.rs` mixes dialoguer/colored CLI presentation with the actual install engine
  (`install_packages`, `install_package`, `install_scripts`, `install_local_files`,
  `install_from_urls`). Because that engine is trapped in a command module, `update.rs` upgrades
  by calling `add::run(to_run, yes, None, platform, None, None, false, true)` — the trailing
  `true` being `update_mode`.
- **Impact**: Rigid coupling; the install pipeline can't be tested without going through the CLI
  (this is the structural reason for T-3).
- **Recommendation**: Extract a headless installer service under `src/installer/`, leaving both
  commands as thin wrappers. Pairs naturally with T-2/T-3.
- **Effort**: M

### A-4. `SourceProvider` is a vestigial abstraction
- **Severity**: Low
- **Location**: `src/providers/base.rs:6-20`
- **Evidence**: A 20-line trait with two methods and exactly one implementor. No `Box<dyn
  SourceProvider>` or generic bound anywhere; every consumer builds a concrete `GitHubProvider`
  and calls methods *not* on the trait (`fetch_package_by_version`, `extract_platform_binaries`,
  `parse_github_url`).
- **Impact**: Suggests pluggability that doesn't exist.
- **Recommendation**: Either make `fetch_package` an inherent method and drop the trait, or widen
  the trait if GitLab/Gitea support is genuinely planned. Do not leave it as-is.
- **Effort**: XS

### A-5. Three network clients with divergent configuration
- **Severity**: Low
- **Location**: `src/utils/http.rs:11-60`, `src/downloader/mod.rs:10-18`, `src/installer/script.rs:178-185`
- **Evidence**: `utils::HttpClient` supports a GitHub token and custom timeouts;
  `downloader` uses a private `OnceLock` client that ignores both; `script::download_script`
  builds an unauthenticated client per call.
- **Impact**: The GitHub token can't authenticate asset downloads — which matters directly for
  P-1's rate-limit problem.
- **Recommendation**: Thread one configured client through.
- **Effort**: S

### A-6. Data model performs process spawning and disk I/O
- **Severity**: Low
- **Location**: `src/core/manifest.rs:16-50`, `:670-745`
- **Evidence**: `manifest.rs` defines the data structs but also holds a `static
  INTERPRETER_CACHE` that spawns `pwsh`/`bash`/`python3`, and an `InstalledManifest::migrate()`
  that walks and renames directories during load.
- **Recommendation**: Move interpreter probing to `installer/script.rs` and migration to a
  dedicated service.
- **Effort**: S

### A-7. `core/` depends upward on root-level modules
- **Severity**: Low
- **Location**: `src/core/config.rs:12-13` importing `crate::bucket`, `crate::cache`
- **Evidence**: `bucket.rs`, `cache.rs`, `package_resolver.rs` sit beside `main.rs` yet are
  consumed by `core/`. (No true cycle — `core/`, `installer/`, `providers/` contain no
  `use crate::commands`.)
- **Recommendation**: Relocate into `src/core/`.
- **Effort**: S

---

## Code quality

`cargo clippy` (default lints) and `cargo fmt --check` are both clean; with `-W
clippy::pedantic -W clippy::nursery` there are 596 advisory hits (notably `too_many_lines`,
`redundant_clone` ×2, `needless_collect`). Module docs: 41/41 files. The findings below are
things clippy cannot see.

### Q-1. Byte-offset vs. char-index confusion panics on non-ASCII asset names — **PROVEN**
- **Severity**: High
- **Location**: `src/core/platform.rs:498-504`
- **Evidence**: `contains_unknown_arch_pattern` gets a **byte** offset from `lower.find(p)` and
  feeds it to `.chars().nth(...)`, which expects a **character** index, then unwraps:
  ```rust
  let before = pos == 0 || !lower.chars().nth(pos - 1).unwrap().is_alphanumeric();
  let after_pos = pos + p.len();
  let after = after_pos >= lower.len()
      || !lower.chars().nth(after_pos).unwrap().is_alphanumeric()
      || lower[after_pos..].starts_with("64")
  ```
  With any multi-byte prefix the byte offset exceeds the char count, `nth` yields `None`, and
  `unwrap` aborts. Confirmed by running the exact logic:
  ```
  ascii  'tool-ppc.tar.gz' -> true
  bytes=16 chars=8
  thread 'main' panicked at repro.rs:15:66:
  called `Option::unwrap()` on a `None` value
  === exit code: 101 ===
  ```
  Repro at `/tmp/claude-audit-scratch/repro.rs`. The `lower[after_pos..]` slices on lines 503-504
  are a second latent panic (non-char-boundary slicing).
- **Impact**: A release asset whose name contains non-ASCII text plus an arch-like token crashes
  `add`, `update`, `search`, or `bucket create`. Because the release profile sets
  `panic = "abort"`, this is an immediate process abort with no unwinding and no error message.
  Reachable from ordinary non-English release names, not just hostile input.
- **Recommendation**: Operate on bytes throughout — use `lower.as_bytes()` for the boundary
  checks, or `char_indices()`. Replace both `unwrap()`s with `map_or(true, ...)`.
- **Effort**: XS

### Q-2. Unchecked `unwrap()` on manifest data in `search`
- **Severity**: High
- **Location**: `src/commands/search.rs:99-104`
- **Evidence**:
  ```rust
  let platform_binaries = platform_ids.iter().find_map(|id| pkg.platforms.get(id)).unwrap();
  let first_binary = platform_binaries.first().unwrap();
  ```
  Both unwrap remote manifest data; an entry with an empty binary array (`"linux-x86_64": []`)
  panics.
- **Impact**: `wenget search` aborts on a sparse or malformed bucket entry.
- **Recommendation**: `if let Some(b) = ...and_then(|b| b.first())`, with a fallback display.
- **Effort**: XS

### Q-3. Two functions carry half of `add.rs`
- **Severity**: High
- **Location**: `src/commands/add.rs:821-1692` (872 lines, 11 params), `:1694-2125` (432 lines, 10 params)
- **Evidence**: `install_packages` reaches nesting depth 8 and needs
  `#[allow(clippy::too_many_arguments)]`; it handles cache resolution, API queries, interactive
  MultiSelect, variant filtering, dry-run, batch download, error aggregation, and cache sync in
  one block.
- **Recommendation**: Split into `resolve_plan` / `prompt_selections` / `execute_install` /
  `sync_cache`. Same work as A-3.
- **Effort**: M

Top offenders by length: `install_packages` 872 · `install_package` 432 · `delete::run` 245 ·
`find_upgradeable` 214 · `display_package_info` 185 · `bucket::run_create` 178 ·
`update::run` 177 · `install_scripts` 170.

### Q-4. `score_asset` and `score_parsed` duplicate ~78 lines of scoring
- **Severity**: Medium
- **Location**: `src/core/platform.rs:997-1074` and `:1200-1288`
- **Evidence**: Both independently implement the same extension gate, OS match, arch match with
  default-arch fallback, FreeBSD explicit-arch rule, compiler priority, and format score.
- **Impact**: A scoring rule fixed in one will silently diverge in the other — a real hazard
  given `RESOURCE_FILTERING_RULES.md` is the declared source of truth for this behavior.
- **Recommendation**: Have `score_asset` parse then delegate to `score_parsed`.
- **Effort**: XS

### Q-5. PATH manipulation duplicated between `init.rs` and `delete.rs`
- **Severity**: Medium
- **Location**: `init.rs:500-530` / `delete.rs:505-535` (PowerShell), `init.rs:615-635` /
  `delete.rs:541-546` (shell rc file lists)
- **Evidence**: Near-identical embedded PowerShell and duplicated
  `[".bashrc", ".bash_profile", ".zshrc", ".profile"]` lists. Tellingly,
  `registry.rs:104`'s `remove_from_system_path` is `#[allow(dead_code)]` *because* `delete.rs`
  reimplemented it.
- **Impact**: Add/remove PATH behavior can drift; also doubles the S-4 injection surface.
- **Recommendation**: One helper pair in `core/paths.rs` or `core/registry.rs`, used by both.
- **Effort**: S

### Q-6. `bin_dir()` panics instead of returning an error
- **Severity**: Medium
- **Location**: `src/core/paths.rs:193`
- **Evidence**: `dirs::home_dir().expect("Failed to determine home directory")`, while the
  sibling `user_root_path()` correctly returns `Result`.
- **Impact**: Aborts in containers/CI without `$HOME`.
- **Recommendation**: Resolve and cache home in `WenPaths::new()`, which is already fallible.
- **Effort**: XS

### Q-7. `.to_str().unwrap()` on paths in Windows self-update/uninstall
- **Severity**: Low
- **Location**: `src/commands/delete.rs:640`, `src/commands/update.rs:876`
- **Recommendation**: Use `as_os_str()`, or return a contextual error.
- **Effort**: XS

### Q-8. 46 `#[allow(dead_code)]` sites, clustered around abandoned work
- **Severity**: Low
- **Location**: `platform.rs:12,56,192,681,920,947,973`; `utils/http.rs:95,122,131,137`;
  `cache.rs:194,214,220,229`; `bucket.rs:128,134,145`; `manifest.rs:309,320,353,362,509,562`;
  `paths.rs:82,94,166,281`
- **Evidence**: Three clusters: orphaned platform selectors from the parse-once rewrite, an
  entire unused `RateLimit` type in `http.rs` (relevant to P-1 — the rate-limit handling was
  built and never wired up), and unused cache/manifest query helpers.
- **Recommendation**: Confirm each is genuinely unreachable, then remove. Flagging, not deleting,
  per project convention.
- **Effort**: S

### Q-9. Backup failures swallowed before destructive repair
- **Severity**: Low
- **Location**: `src/commands/repair.rs:94,131`, `src/core/config.rs:108`
- **Evidence**: `create_backup` errors are discarded via `if let Ok(...)` / `.ok()`, so repair
  proceeds to overwrite even when the backup didn't happen.
- **Recommendation**: Warn or prompt when the backup fails.
- **Effort**: XS

Production panic inventory: 18 `unwrap()`, 5 `expect()`, 2 `panic!` (vs. 38/2/2 in tests).

---

## Performance

Verified good: downloads and extraction are **correctly streamed** at a flat 64 KiB buffer
(`downloader/mod.rs:60-75` loop; `extractor.rs` uses `entry.unpack` and `io::copy` per entry), so
peak memory is independent of asset size. No whole-body `bytes()`/`read_to_end` buffering.

### P-1. Update checks burn two GitHub API calls per package, with no conditional requests
- **Severity**: High
- **Location**: `src/providers/github.rs:215-228`, `src/commands/update.rs:60-70`
- **Evidence**: Update checking parallelizes across 8 threads, but each `fetch_package` makes
  **two** sequential API calls:
  ```rust
  let repo_info = self.fetch_repo_info(&owner, &repo)?;      // description/license
  let release = self.fetch_latest_release(&owner, &repo)?;   // the part actually needed
  ```
  No `If-None-Match`/ETag, and no rate-limit inspection — `utils/http.rs`'s `check_rate_limit`
  exists but is `#[allow(dead_code)]` (see Q-8).
- **Impact**: Unauthenticated GitHub allows 60 requests/hour/IP. At 2 calls per package, **30
  installed packages exhausts the entire hourly quota in one `wenget update`**, after which
  everything fails with 403. `fetch_repo_info` is pure waste here — update checks need only the
  release. Worsened by A-5: the token that would raise the limit to 5,000/hr isn't used on all
  paths.
- **Recommendation**: Drop `fetch_repo_info` from the update path (halves the cost immediately),
  store ETags in the cache and send `If-None-Match` (304s are free), and wire up the existing
  rate-limit warning.
- **Effort**: S

### P-2. The 302 KB manifest cache is read and parsed three times per `update`
- **Severity**: Medium
- **Location**: `update.rs:126`, `:245`, `:250`; `add.rs:844`, `:2172`
- **Evidence**: `update::run` loads the cache (:126), saves it (:245), then delegates to
  `add::run`, which loads it again (`add.rs:844`), and `update_cache_with_packages` loads it a
  third time (`add.rs:2172`) before writing back.
- **Impact**: 3 disk reads + 3 full JSON parses of hundreds of package structs per invocation.
- **Recommendation**: Pass the loaded cache down, or memoize it in `Config`.
- **Effort**: S

### P-3. Whole-cache deep clone to render `list`
- **Severity**: Medium
- **Location**: `src/cache.rs:185-190`, `src/core/config.rs:284-288`, `src/commands/list.rs:180`
- **Evidence**: `get_packages_from_cache()` → `to_source_manifest()` clones every package, asset
  vector, URL, and platform map, and `list.rs` then merely `.iter().filter(...)`s over it.
- **Recommendation**: Add a borrowing iterator over `cache.packages.values()`.
- **Effort**: XS

### P-4. Name lookups linear-scan a URL-keyed map
- **Severity**: Medium
- **Location**: `src/package_resolver.rs:114-125`, `src/cache.rs:75-80`
- **Evidence**: `ManifestCache.packages` is `HashMap` keyed by *repo URL*, so exact-name
  resolution falls back to `.values().filter(|c| c.package.name == base_name)` — O(N) per
  argument. N≈500 today, so this is structural rather than urgent.
- **Recommendation**: Maintain a name→URL index.
- **Effort**: S

### P-5. `bucket create` sleeps 1s per package, serially
- **Severity**: Low
- **Location**: `src/commands/bucket.rs:250`, `:667-712`
- **Evidence**: `RATE_LIMIT_DELAY_MS = 1000` with `thread::sleep` after every package, processed
  sequentially.
- **Impact**: ~100s of pure sleep for a 100-repo bucket — and it still exhausts the 60/hr
  unauthenticated limit after 30 packages, so the sleep doesn't buy what it's meant to.
- **Recommendation**: Reuse the worker pool when a token is present; throttle off
  `x-ratelimit-remaining` instead of a fixed sleep.
- **Effort**: S

### P-6. Same filename lowercased up to four times per asset
- **Severity**: Low
- **Location**: `src/core/platform.rs:1120-1136`, `:435-460`, `:470-490`
- **Recommendation**: Lowercase once and pass `&str` down.
- **Effort**: XS

---

## Maintainability

### M-1. No CI quality gate — tests, clippy, and fmt never run automatically
- **Severity**: High
- **Location**: `.github/workflows/{release,publish-gate,publish-winget,update-manifest}.yml`
- **Evidence**: Verified by searching all four workflows for `cargo test|clippy|fmt`: **no
  matches**. Triggers are tag pushes (`v*.*.*`), `workflow_run`, a weekly cron, and manual
  dispatch — there is **no `pull_request` or `push: branches: [main]` workflow at all**.
  `release.yml` builds 13 targets and never tests any of them.
- **Impact**: The clean clippy/fmt state and 125 passing tests rest entirely on local discipline
  and will regress unnoticed. Platform-gated `#[cfg(windows)]` / `#[cfg(unix)]` code is never
  exercised on any OS but the committer's — and `registry.rs:132`'s test module explicitly
  declines to test. Releases ship untested.
- **Recommendation**: Add `ci.yml` on `pull_request` + `push: main` running `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` on ubuntu/windows/macos. Gate this
  behind T-1 first, or CI will wipe `~/.wenget` on the runner (harmless there, but the fix order
  matters for contributors).
- **Effort**: S

### M-2. A 4.4 MB compiled binary is committed and force-pushed on every release
- **Severity**: High
- **Location**: `bucket/wenget`, `.github/workflows/release.yml:358-375`, `update-manifest.yml:30-36`
- **Evidence**: `git ls-files` confirms `bucket/wenget` is tracked. `release.yml:369` runs
  `git add -f bucket/wenget` and pushes to `main` every release; `git log --oneline --
  bucket/wenget` shows **38 rewrites**. Repo pack size is already **15.86 MiB** for a 17k-LOC
  project. `update-manifest.yml:31-33` then `chmod +x`es and *executes* this checked-in binary to
  regenerate `manifest.json`.
- **Impact**: Clone size grows by ~4.4 MB of incompressible data per release, permanently and
  irreversibly. Separately, executing an opaque committed binary in CI — rather than building
  from source or downloading a verified release asset — is a supply-chain weak point.
- **Recommendation**: Untrack `bucket/wenget`; have `update-manifest.yml` either
  `cargo build --release` or `gh release download` a verified asset; drop the force-add step.
  History rewriting is optional and disruptive — stopping the bleeding is the valuable part.
- **Effort**: S

### M-3. `AGENTS.md`'s structure map omits 11 modules
- **Severity**: Medium
- **Location**: `AGENTS.md:40-75`
- **Evidence**: Missing `core/checksum.rs`, `core/preferences.rs`, `installer/local.rs`,
  `installer/input_detector.rs`, and 7 command modules (`init`, `rename`, `repair`, `info`,
  `search`, `config`, `commands/bucket.rs`) — the last of which also shadows the root
  `src/bucket.rs`.
- **Impact**: This file exists to orient agents and contributors; an inaccurate map invites
  duplicate implementations of things like `repair.rs` or `checksum.rs`.
- **Recommendation**: Regenerate the tree.
- **Effort**: XS

### M-4. `AGENTS.md` and `CLAUDE.md` are ~50% redundant and unlinked
- **Severity**: Medium
- **Location**: `AGENTS.md` (8.9 KB), `CLAUDE.md` (10.3 KB)
- **Evidence**: Both duplicate build commands, subsystem breakdowns, and release steps.
  `CLAUDE.md` uniquely holds data-flow diagrams and path tables; `AGENTS.md` uniquely holds style
  rules. Neither references the other, and `CONTEXT.md` doesn't index `CLAUDE.md` at all. No
  direct contradiction found, but nothing keeps them aligned.
- **Recommendation**: Make `CLAUDE.md` a thin pointer to `AGENTS.md` + `CONTEXT.md`, or merge the
  unique content into `docs/reference/`.
- **Effort**: S

### M-5. Aging dependencies, including an unmaintained crate on the privilege path
- **Severity**: Medium
- **Location**: `Cargo.toml:28,32,43,50,54,65`
- **Evidence**: `zip 0.6` (ecosystem is on 2.x), `sevenz-rust 0.6`, `thiserror 1.0`, `dirs 5.0`,
  and `is_elevated 0.1` — a 0.1.x crate last published ~5 years ago, load-bearing for
  Administrator detection at `core/privilege.rs:27`. No `cargo audit`/`cargo deny` anywhere.
- **Impact**: The archive crates matter most given S-1/S-2 — upgrading `zip` and `sevenz-rust` is
  partly a security action, since newer versions harden extraction.
- **Recommendation**: Upgrade `zip`, `sevenz-rust`, `thiserror`; replace `is_elevated` with a
  direct `OpenProcessToken`/`GetTokenInformation` call via `windows-sys`; add `cargo audit` to
  the M-1 workflow.
- **Effort**: M

### M-6. `README.md` contradicts the code on the bin directory
- **Severity**: Low
- **Location**: `README.md:183-193`, `:66-80`
- **Evidence**: The directory tree shows `~/.wenget/bin/`, but `README.md:68`, `CLAUDE.md:112`,
  and `core/paths.rs:77` all say `~/.local/bin/`. A v0.2.x→v0.3.0 migration warning is still
  featured at the top, at version 3.8.7.
- **Effort**: XS

### M-7. Architectural rationale is buried in a 62 KB changelog while `docs/` sits empty
- **Severity**: Low
- **Location**: `CHANGELOG.md` (1,215 lines); `docs/adr/`, `docs/spec/`, `docs/plan/`,
  `docs/debug/`, `docs/audit/` all stubs reading "None yet."
- **Evidence**: Entries for v3.8.4/v3.8.6 contain design essays on the parse-once selector
  rewrite and checksum design — exactly ADR material. `CONTEXT.md`'s reading order points at
  directories that are currently dead ends.
- **Recommendation**: Backfill 2-3 ADRs from the changelog. Note `docs/reference/` itself is in
  good shape: five filtering rules spot-checked against `platform.rs` and `extractor.rs` all
  matched, so the one document declared canonical is genuinely trustworthy.
- **Effort**: S

---

## Prioritized action plan

**Quick wins (< 1 day, do first)**
1. S-1 + S-2 — switch tar to `unpack_in`, add a validating 7z extract fn. *Critical.*
2. T-1 — add `WenPaths::with_root` and point `config.rs` tests at a `TempDir`. *Stops data loss.*
3. M-1 — add `ci.yml` (after T-1). *Locks in everything else.*
4. Q-1 + Q-2 — fix the byte/char panic and the `search` unwraps. Both XS, both proven.
5. A-2 — delete `tokio`. One line.
6. S-3 — validate package names. XS, and closes an arbitrary-deletion path.
7. T-5 (partial) — add traversal regression tests for S-1/S-2 so they can't come back.

**Medium term (1-5 days)**
8. P-1 — drop `fetch_repo_info` from update checks; add ETags. Most user-visible perf win.
9. S-5 — fail on `ProbeFailed`; honor manifest checksums.
10. A-1 — atomic `installed.json` writes.
11. S-4, S-6, S-7, S-8 — Windows PATH/shim/script hardening; fold in Q-5's deduplication.
12. M-2 — untrack `bucket/wenget`, build or download it in CI instead.
13. M-3, M-4, M-6 — documentation corrections.
14. P-2, P-3 — stop re-parsing and deep-cloning the cache.

**Longer term (> 5 days)**
15. A-3 + Q-3 — extract a headless installer service from `add.rs`.
16. T-2 + T-3 — HTTP injection seam, then real integration tests over the full lifecycle.
17. M-5 — dependency upgrades (`zip`, `sevenz-rust`, replace `is_elevated`) + `cargo audit`.
18. Q-8 — confirm and retire the 46 dead-code sites.

---

## Metrics

| Metric | Value |
|---|---|
| Files analyzed | 41 `.rs` (+ 4 workflows, 2 installers, docs) |
| Lines of code | 16,957 |
| Tests | 135 defined; 125 pass, 5 ignored, 0 fail — in 0.02s |
| Integration tests | 0 (no `tests/` directory) |
| `cargo fmt --check` | Clean |
| `cargo clippy` (default) | Clean |
| `cargo clippy` (pedantic+nursery) | 596 advisory warnings |
| Production panic sites | 18 `unwrap`, 5 `expect`, 2 `panic!` |
| `#[allow(dead_code)]` | 46 |
| Largest function | `install_packages`, 872 lines, 11 params |
| Largest file | `commands/add.rs`, 2,426 lines |
| Git pack size | 15.86 MiB (4.4 MB binary × 38 rewrites) |
| CI quality gates | None |

## Method note

Findings were produced by six parallel dimension audits, then independently re-verified against
the source. Four claims were confirmed by execution rather than inspection: the tar traversal
exploit (S-1), the non-ASCII panic (Q-1), the live-state overwrite (T-1), and the absence of CI
gates (M-1). Reproductions live under `/tmp/claude-audit-scratch/` and are not part of the repo.
One dimension agent's first report was lost to a serialization failure and was re-run.
