# Implementation, Clarity, Simplicity & Optimization Audit — 2026-09-23

Status: In progress — fixed: IM-1–IM-5, OP-1, SI-1, SI-2, CL-1 (narrow: InstallOptions); CL-1 installer split and the rest open
Scope: `wenget` v3.9.0 at commit `529bfe2`, all of `src/` (19,036 LOC: 14,935 production, 4,101
test, 44 files). A follow-up to [2026-09-03-full-codebase-audit.md](2026-09-03-full-codebase-audit.md):
this pass re-checks that audit's findings and focuses on **implementation correctness, clarity,
simplicity, and optimization**. Security findings appear only where they are still open.

---

## Executive summary

**Overall health: 6.5/10** (up from 6/10). The 2026-09-03 Criticals are closed: tar/7z path
traversal is fixed with regression tests, `cargo test` is isolated, and CI gates fmt, clippy, and
tests on three OSes. The per-package record redesign (`store.rs`, `staging.rs`) is the cleanest
code in the tree.

The problems now sit at the **edges of the command layer**: what the user sees when something
fails, and how `add`/`update` are wired together. Four were reproduced on the real binary
(scripts in `docs/tmp/claude-scratch/audit-2026-09-23-repros.sh`):

- a failed install exits **0** (IM-1);
- every error message drops its root cause, so users see `Failed to create wenget root directory`
  and never `Not a directory` (IM-2);
- `--verbose` and `RUST_LOG` do nothing: all 71 `log::` sites are fixed at INFO (IM-3);
- `wenget rename` fails on every non-bash script package on Unix (IM-4).

### Top 3 priorities

1. **Make failure visible** (IM-1, IM-2, IM-3; ~2h total, mostly one-liners). Fix the exit code,
   print errors with `{:#}`, and fix the logger. Cheapest, and the most user-visible.
2. **Close the post-commit window in `install_package`** (IM-5, IM-6; half a day). The Package
   record is written *after* the staged swap and after launcher creation, so a launcher error
   deletes the previous install and its record and leaves an Untracked app directory. That breaks
   the promise in `staging.rs`'s header.
3. **Cut GitHub API calls per package from 4–8 to 1** (OP-1; 1–2 days). A single DirectRepo
   update makes 8 requests against a 60/hour unauthenticated limit.

### Counts (new findings only)

| Category | High | Medium | Low | Total |
|---|---|---|---|---|
| Implementation | 4 | 5 | 6 | 15 |
| Clarity | 1 | 1 | 5 | 7 |
| Simplicity | 1 | 6 | 5 | 12 |
| Optimization | 1 | 1 | 3 | 5 |

Effort key: **XS** < 1h · **S** < half day · **M** 1–2 days · **L** > 2 days.

---

## Status of 2026-09-03 findings

Checked against the source at `529bfe2`. "Not re-checked" means this pass did not look.

| ID | Finding | Status | Evidence |
|---|---|---|---|
| S-1 | tar path traversal | **Fixed** | `extractor.rs:254-288` rejects `..`/absolute and uses `unpack_in`; regression test `extractor.rs:882` |
| S-2 | 7z path traversal | **Fixed** | entry names pre-validated, `extractor.rs:149-159` |
| S-3 | unvalidated names → paths | **Fixed** | `sanitize_path_component` (`paths.rs:38`) plus collision guard `ensure_dir_available` (`store.rs:362`) |
| S-4 | PowerShell PATH interpolation | Open | `init.rs:500-525`, `delete.rs:508-525` |
| S-5 | checksum downgrades on probe failure | Open | `checksum.rs:208` returns `Ok(())`; `PlatformBinary.checksum` is never consulted |
| S-6 | PATH written as `REG_SZ` | Open | `registry.rs:65`; `broadcast_environment_change` (`registry.rs:111-116`) is still a no-op stub |
| S-7 | shim paths unescaped | Open | `shim.rs:16-19`; Unix wrapper `script.rs:350-365` |
| S-8 | Windows scripts misparse quoted paths | Open | `delete.rs:639`, `update.rs:876` |
| S-9 | `http://` accepted silently | Open | `input_detector.rs:48`, `add.rs:254` |
| S-10 | bootstrap installers unverified | Not re-checked | — |
| T-1 | tests overwrite live state | **Fixed** | `WenPaths::with_root`, `Config::with_paths` |
| T-2 | no integration tests | Open | no `tests/` directory |
| T-3 | install/update/delete untested | Open | 0 tests reach `install_packages`, `install_package`, `find_upgradeable` |
| T-4 | `delete.rs` test re-implements logic | Open | `delete.rs:709-753` |
| T-5 | no adversarial tests | Partly fixed | traversal tests exist; no tests for malformed records/manifests via the command layer |
| T-6 | untested command modules | Partly fixed | `repair`, `search` now tested; `init`, `bucket`, `info`, `list` still have none |
| A-1 | non-atomic `installed.json` | **Fixed** | superseded by per-package records |
| A-2 | unused `tokio` | **Fixed** | removed from `Cargo.toml` |
| A-3 | `add.rs` god module driven by a bool | Open | see CL-1 |
| A-4 | vestigial `SourceProvider` | Open | one implementor (`github.rs:215`) |
| A-5 | divergent HTTP clients | Open, worse | four construction sites, see SI-12 |
| A-6 | data model spawns processes / does I/O | Open | `manifest.rs:16-50` (`INTERPRETER_CACHE`), `manifest.rs:645-779` (`migrate`) |
| A-7 | `core/` depends upward | Open | `config.rs:13-14, 175-177` import `bucket`, `cache`, `utils` |
| Q-1 | byte/char panic | **Fixed** | `platform.rs:500-512` uses byte-boundary checks |
| Q-2 | `search` unwraps | **Fixed** | `search.rs:120-125` |
| Q-3 | two functions carry half of `add.rs` | Open | `install_packages` 879 lines, `install_package` 425 lines |
| Q-4 | duplicate asset scoring | Open, worse than reported | see SI-1: tests pin the path production doesn't use |
| Q-5 | PATH code duplicated init/delete | Open | `init.rs:500-530, 612-628` vs `delete.rs:505-546` |
| Q-6 | `bin_dir()` panics | Open | `paths.rs:269` `expect("Failed to determine home directory")` |
| Q-7 | `.to_str().unwrap()` on Windows paths | Open | `update.rs:876`, `delete.rs:639` |
| Q-8 | dead-code sites | Open | 52 `#[allow(dead_code)]`; compiler confirms 43 dead items, see SI-7 |
| Q-9 | backup failures swallowed | Open | `bucket.rs:74` `.ok()`, `repair.rs:320` `if let Ok` |
| P-1 | 2 API calls per update check | Open, worse | see OP-1: up to 8 per package |
| P-2 | cache parsed 3× per update | Open | see OP-2 |
| P-3 | whole-cache clone for `list --all` | Open | `list.rs:180` → `cache.rs:188` |
| P-4 | name lookups scan URL-keyed map | Partly fixed | `packages_by_name` used in `update.rs`; `package_resolver.rs:134-138` still scans |
| P-5 | `bucket create` sleeps 1s/package | Open | `bucket.rs:671, 697, 709` |
| P-6 | filename lowercased repeatedly | Open | `platform.rs:302, 325, 458, 480, 487, 1128` |
| M-1 | no CI gate | **Fixed** | `.github/workflows/ci.yml` (fmt, clippy `-D warnings`, tests × 3 OS) |
| M-2 | 4.4 MB binary committed each release | Open | `bucket/wenget` (4.7 MB), 40 commits; pack is 16 MB |
| M-3 | AGENTS.md structure map incomplete | **Fixed** | map lists every module |
| M-4 | AGENTS.md/CLAUDE.md overlap | Not re-checked | — |
| M-5 | aging deps | Open | `zip 0.6` (pulls a second `bzip2 0.4`), `sevenz-rust 0.6`, `is_elevated 0.1` |
| M-6 | README bin dir wrong | **Fixed** in README | still wrong in `preferences.rs`, see CL-4 |
| M-7 | rationale buried in changelog | **Fixed** | `docs/adr/`, `docs/spec/`, `docs/plan/` populated |

**Tally:** 12 fixed, 3 partly fixed, 28 open, 2 not re-checked (45 IDs).

---

## Implementation

### IM-1. Failed installs exit with status 0 — **REPRODUCED**
High · XS
**Where:** `commands/add.rs:392` (scripts), `:532` (local files), `:673` (URLs), `:1649` and
`:1665` (packages). Each prints `✗ N … failed` and then returns `Ok(())`.
**Evidence:** `wenget add -y https://wenget-audit.invalid/x.tar.gz` prints
`✗ 1 URL(s) failed` and exits `0`. Any script, CI job, or `wenget update && …` chain treats a
failed install as success. `update` inherits this because it ends in `add::run`.
**Recommendation:** Track failures in one place (see SI-11) and end with
`bail!("{n} of {total} install(s) failed")` when `n > 0`. The printed summary stays the same.

### IM-2. Every user-facing error drops its cause — **REPRODUCED**
High · XS
**Where:** `main.rs:123` `eprintln!("{} {}", "Error:".red().bold(), e)`; also the per-item
failure lines in `add.rs` (`println!("  {} {}", "✗".red(), e)`).
**Evidence:** Nowhere in `src/` is there a `{:#}`, `.chain()`, or `root_cause`. With
`WENGET_ROOT` pointing under a regular file, `wenget bucket list` prints only
`Error: Failed to create wenget root directory`; the `Not a directory (os error 20)` it wraps is
lost. The same thing hides `Invalid argument` in IM-4 and every network cause (DNS, TLS, 403 rate
limit) behind `Failed to download from …`. The `.with_context` calls spread across 26 files add
context, but the underlying cause never reaches the user.
**Recommendation:** Use `{:#}` (prints `outer: inner: root`) in `main.rs` and in the per-item
failure prints. One-character change per site.

### IM-3. `--verbose` and `RUST_LOG` have no effect; INFO leaks into normal output — **REPRODUCED**
Medium · XS
**Where:** `main.rs:20-30`.
```rust
env_logger::Builder::from_default_env()
    .filter_level(log::LevelFilter::Info)   // overrides RUST_LOG's global directive
    .init();
if cli.verbose { log::set_max_level(log::LevelFilter::Debug); } // env_logger's filter still says Info
```
**Evidence:** `--verbose` produces 0 DEBUG lines, `RUST_LOG=debug` produces 0, and
`RUST_LOG=error` still prints `INFO wenget::downloader] Downloading: …`. Every `log::debug!` in
the tree is unreachable, and the INFO line duplicates the user-facing
`Downloading from …` printed just above it (`add.rs:1709`).
**Recommendation:** Pick the level before building the logger: `Warn` by default, `Debug` when
`--verbose`, and let `RUST_LOG` override via `parse_default_env()` called *after*
`filter_level`. Demote `downloader/mod.rs:22` to `debug!`.

### IM-4. `rename` fails on non-bash script packages (Unix) and breaks them (Windows) — **REPRODUCED on Unix**
High · S
**Where:** `commands/rename.rs:256-295`.
**Evidence:** `rename_command` recovers the target by reading the old launcher. On Unix it calls
`fs::read_link`, but Python/PowerShell/Batch scripts get a regular wrapper file
(`script.rs:350-375`), not a symlink. `wenget rename hello hey` on a `.py` package fails with
`Failed to read symlink`. On Windows it always recreates a *binary* shim (`create_shim`), which
drops the interpreter wrapper that `create_script_shim` wrote (inferred from code, not run).
Separately, the old launcher is deleted (`:274-278`) before the new one is created, so any
failure loses the command entirely.
**Recommendation:** Derive the target from the Package record (`install_path` +
`executables[old_cmd]`) instead of parsing launchers. Dispatch on `package.source`
(`Script { script_type, .. }` → `create_script_shim`, otherwise `create_symlink`/`create_shim`).
Create the new launcher before removing the old one. This deletes `read_shim_target`
(`rename.rs:322-360`).

### IM-5. The Package record is written after the swap, so a launcher error erases the package
High · S
**Where:** `commands/add.rs:1945` (`staged.commit()?`), `:2047`/`:2052` (launchers, `?`), record
written later by `record_installed` (`:2192`).
**Evidence:** `commit()` moves the old app directory aside and deletes it (`staging.rs:70-115`),
and the old directory holds the old record. If `create_symlink`/`create_shim` then fails, the
function returns early. The previous install is gone, the new directory has no record (an
**Untracked app directory**), and the command disappears from `list`/`update`/`del`.
`staging.rs`'s header says "every crash point leaves either the complete old install or the
complete new one", and `StagedInstall::path()` is documented as "where to extract files **and
write the package record**", but `add.rs` never writes the record there.
**Recommendation:** Build the `InstalledPackage` from `staged.target()` (the final path is known
before commit) and write it into `staged.path()/.wenget/package.json` *before* `commit()`. Create
launchers after commit. A launcher failure then leaves a tracked package that `repair` reports as
"missing launcher" (see IM-12).

### IM-6. Failure to save a Package record is reported as success
Medium · XS
**Where:** `commands/add.rs:2192-2207`.
**Evidence:** `record_installed` catches the `save_package` error, prints it, then
`upsert_package`s the in-memory set anyway and returns `()`. The batch counts the package as
installed and the summary says `✓`, but on disk it is an Untracked app directory.
**Recommendation:** Return `Result<()>` and propagate. This disappears naturally with IM-5's fix.

### IM-7. Invalid `config.toml` is announced as ignored, then used
Medium · XS
**Where:** `core/config.rs:37-43`.
```rust
if let Err(e) = preferences.validate() {
    log::warn!("Invalid preferences in config.toml: {}", e);
    log::warn!("Using default preferences instead");
}
let paths = WenPaths::new_with_custom_bin(preferences.custom_bin_path.clone())?;
```
**Evidence:** A relative `custom_bin_path` or a malformed `preferred_platform` is still used (the
latter at `add.rs:824`, `update.rs:706`). Because of IM-3, the warning is visible only at INFO+
anyway.
**Recommendation:** `let preferences = if preferences.validate().is_ok() { preferences } else
{ Preferences::default() };`, or fail hard. Either way the message should match what happens.

### IM-8. Update detection uses two different version comparisons
Medium · S
**Where:** `commands/update.rs:429` (`inst_version != latest_version`) vs `:217` and `:467`
(`is_newer_version`). `is_newer_version` (`:92-113`) drops any segment that isn't a plain integer.
**Evidence:** On the API path, any difference counts as an upgrade: a user who pinned a newer
pre-release with `--ver` is offered a "downgrade" to the latest stable release. On the cache
paths, `1.0.0-rc.2` parses as `[1,0,2]`, which compares newer than `1.0.1` (`[1,0,1]`), so that
user is never offered 1.0.1.
**Recommendation:** Route every comparison through one function. Parse with the `semver` crate and
fall back to the numeric split only when parsing fails; add the rc cases to
`test_is_newer_version`.

### IM-9. Downloaded archives leak on most error paths
Low · XS
**Where:** `commands/add.rs:1723` (download) → `:2075` (only unconditional cleanup); only the
checksum branch (`:1728`) cleans up early. `downloader/mod.rs:56` also writes straight to `dest`,
so an interrupted download leaves a partial file.
**Evidence:** Any `?` between those lines (`ensure_dir_available`, `StagedInstall::begin`,
extraction, a cancelled `dialoguer` prompt) leaves the archive in `downloads/`.
**Recommendation:** Download into a `tempfile::NamedTempFile` inside `downloads_dir()` (already a
dev-dependency; promote it) so the file is removed on drop.

### IM-10. "Removed obsolete command" printed when removal failed
Low · XS
**Where:** `commands/add.rs:2066-2069` — `fs::remove_file(&old_bin).ok();` followed by an
unconditional `println!`.
**Recommendation:** Print success only on `Ok`; print the error otherwise.

### IM-11. Self-update skips the executable checks that `add` uses
Low · XS
**Where:** `commands/update.rs:791` calls `find_executable(&files, "wenget")`, which passes
`extract_dir = None` (`extractor.rs:753`), so the permission and magic-byte rules are skipped.
`add.rs:1746` and `local.rs:54` pass the directory.
**Recommendation:** Call `find_executable_candidates(&files, "wenget", Some(extract_dir))`, then
delete `find_executable`, which is used only by tests after this change.

### IM-12. `repair` reports missing launchers but cannot fix them or say how
Medium · S
**Where:** `commands/repair.rs:120-128` (reported); the apply phase `:153-176` handles residue,
orphans, and JSON files but not `missing`. The per-package-records plan only ever reported them.
**Evidence:** Untracked directories get a hint (`Re-install … wenget add <name>`, `:47-54`);
missing launchers get none, and `repair --force` ends with `Repair complete.` while they are
still missing.
**Recommendation:** Recreate the launcher from `install_path` + `executables`, reusing the
unified launcher function from SI-6; at minimum print the re-install hint.

### IM-13. `wenget config` fails when `$EDITOR` has arguments
Low · XS
**Where:** `commands/config.rs:31`, `:86-92` — `Command::new(&editor)` with `EDITOR="code --wait"`
looks for an executable literally named `code --wait`. `$VISUAL` is not consulted.
**Recommendation:** Check `VISUAL` then `EDITOR`; split on whitespace (program + args).

### IM-14. URL classification is substring-based
Low · XS
**Where:** `installer/input_detector.rs:19-39`. `contains("github.com")` matches
`notgithub.com` and `gist.github.com`. `github.com/o/r/archive/refs/tags/v1.tar.gz` is treated
as a package name and then fails in the resolver.
**Recommendation:** Parse the host and path: a repo URL is exactly `github.com/{owner}/{repo}[/]`;
anything longer is a direct URL.

### IM-15. Download filename parsed from the URL even though the asset name is known
Low · XS
**Where:** `add.rs:1715-1719`, `update.rs:760-763` (`url.split('/').next_back()`, query string
kept). `add.rs:569` strips `?…` for the same job; `PlatformBinary.asset_name` already holds the
answer.
**Recommendation:** Use `binary.asset_name` (sanitized) for the download filename.

---

## Clarity

### CL-1. `update` drives `add` through positional flags; the core functions take 8–13 parameters
High · M (A-3 and Q-3, still open)
**Where:** `update.rs:289` `add::run(to_run, yes, None, platform, None, None, false, true)`;
`main.rs:78-87` passes the same trailing `false`; `add::run` 8 params (`add.rs:31`),
`install_packages` 11 (`:804`), `install_package` 13 (`:1684`); `update_mode` branches at ≥9
sites inside them.
**Impact:** `update` re-parses its own already-resolved packages as CLI strings, and `add`
reloads config, the Installed set, and the cache (OP-2) and re-fetches from GitHub (OP-1).
**Recommendation** (sketch; do it after IM-5 and T-3 tests exist):
```rust
pub struct InstallOptions { yes: bool, platform: Option<String>, version: Option<String>,
                            variant: Option<String>, command_name: Option<String>, no_suffix: bool }
pub enum Intent { Install, Upgrade { key: String } }
pub struct InstallRequest { package: Package, source: PackageSource, intent: Intent }

pub struct Installer<'a> { paths: &'a WenPaths, installed: &'a mut InstalledSet,
                           cache: &'a mut ManifestCache, github: &'a GitHubProvider }
impl Installer<'_> { pub fn run(&mut self, reqs: Vec<InstallRequest>, opts: &InstallOptions)
                         -> Result<BatchReport>; }
```
Split `install_packages` along its existing seams: resolve targets (`~804-948`), plan versions
(`~949-1085`), confirm (`~1086-1114`), filter binaries (`~1115-1404`), execute + report
(`~1405-1682`). Split `install_package` into download → stage/extract → choose executables →
write record → commit → link.

### CL-2. `#[allow(dead_code)]` no longer carries information
Low · XS
**Where:** At least 7 annotations sit on code that is used: `Config::preferences` and its field
(`config.rs:22, 67`; used at `add.rs:824`, `update.rs:706`), `WenPaths::staging_dir`
(`paths.rs:242`, whose comment "First caller lands with stage-and-swap installs" is stale),
`WenPaths::record_dir` (`paths.rs:226`), `ScanEntry` (`store.rs:26`), `duplicate_keys`
(`store.rs:344`), `write_record_atomically` (`store.rs:434`), `ManifestCache::find_script`
(`cache.rs:214`). Meanwhile the real dead code (SI-7) hides behind the same attribute.
**Recommendation:** Delete every non-`cfg` `#[allow(dead_code)]`, then remove what the compiler
flags. Keep the attribute only on platform stubs, or better, remove those stubs (SI-6).

### CL-3. Glossary drift in identifiers and messages
Low · XS
**Where:** `meta_version` / `CURRENT_META_VERSION` (`manifest.rs:409`; the glossary says avoid
"meta"); `Config::get_or_create_installed` (`config.rs:113`) is a pure alias of `load_installed`
whose name implies creation it no longer does; the Installed set is bound as `manifest`
(`list.rs:44`); errors say `Package not found in manifest` (`rename.rs:231`).
**Recommendation:** Rename to `schema_version` with `#[serde(alias = "meta_version")]` to keep
existing records readable; delete the alias method; rename the binding and messages.

### CL-4. `config.toml` template documents the wrong default bin directory
Low · XS
**Where:** `preferences.rs:23` and the generated template (`preferences.rs:88-92`) say
`~/.wenget/bin`; `paths.rs:268-270` uses `~/.local/bin` (README is already correct).

### CL-5. Deprecated fields are still threaded through the install path
Low · XS
**Where:** `parent_key` is computed per variant (`add.rs:1514-1539`), passed to
`install_package` as `_parent_package`, ignored, and `parent_package: None` is written
(`:2103`). Every `InstalledPackage` literal must spell out `command_names: vec![]`,
`command_name: None`, `parent_package: None` (`add.rs:2100-2103`, `:430-453`, `:2253-2274`,
`rename.rs:374`, `repair.rs:402`).
**Recommendation:** Drop the `parent_key` loop and the parameter; give the legacy fields
`#[serde(default, skip_serializing)]` plus a constructor so callers stop naming them.

### CL-6. Long functions
Medium · (covered by CL-1 for `add`/`update`)
Clippy `too_many_lines` (>100): `add.rs` ×4 (largest 879), `update.rs` ×3, `delete.rs:12` (247),
`delete.rs:349`, `bucket.rs:725`, `info.rs:94` (173), `repair.rs:20` (164), `search.rs:8` (168),
`manifest.rs:806`, `extractor.rs:560`. For the non-`add` ones, the scouts' splits all follow one
pattern: separate *compute* (targets, report, scores) from *print*. That split is also what makes
them testable (T-6).

### CL-7. `thiserror` is a dependency with zero uses; AGENTS.md prescribes it
Low · XS
**Where:** `Cargo.toml` (`thiserror = "1.0"`); `rg thiserror src` → 0 hits; AGENTS.md "Use
`thiserror` for defining custom error types".
**Recommendation:** Remove the dependency and the AGENTS.md line, or adopt it where callers need
to branch on an error kind (e.g. rate limit vs not found in `providers/github.rs`).

---

## Simplicity

### SI-1. Two asset-scoring engines; most of the tests pin the one production doesn't use
High · S (Q-4, understated in the prior audit)
**Where:** `platform.rs:1001-1078` `score_asset` vs `:1204-1288` `score_parsed`.
**Evidence:** Production reaches scoring only through `BinarySelector::extract_platforms` →
`score_parsed` (`github.rs:194`). `score_asset` is reached only from `select_for_platform`
(`#[allow(dead_code)]`, `:925`) and `select_all_for_platform` (`:952`), which the compiler
confirms are dead outside tests. **19 of the platform tests call `select_for_platform`; 5 call
`extract_platforms`.** The doc comment "behavior is identical to `score_asset`" (`:1203`) is
unenforced, so the heaviest regression suite in the repo guards a copy.
**Recommendation:** Make `select_for_platform` a `#[cfg(test)]` wrapper over the production path
(parse once, then `score_parsed`), delete `score_asset`, `select_all_for_platform`, and
`detect_compiler_from_filename`. About −150 LOC, and the tests then exercise real behavior.

### SI-2. `fetch_package` and `fetch_package_by_version` are the same function twice
Medium · XS
**Where:** `providers/github.rs:114-174` and `:216-268` differ only in which release they fetch.
`SourceProvider` (`providers/base.rs`) has one implementor and no dynamic dispatch (A-4).
**Recommendation:** One `fn fetch_package(&self, url, version: Option<&str>)`, delete the trait
(−~90 LOC). Combine with OP-1.

### SI-3. Hand-rolled glob in the resolver; `glob` is already a dependency
Medium · XS
**Where:** `package_resolver.rs:237-291` (55 lines, `*` only). `glob::Pattern` is already used in
`delete.rs:6` and `fuzzy.rs:8`. The resolver also strips `::variant` before matching
(`:118-122`), so `add 'bun*::baseline'` ignores the variant, while `delete` honors it.
**Recommendation:** Use `glob::Pattern`, and share one "match user input against packages"
helper between `add`, `delete`, `rename`, and `info` (each has its own today: `delete.rs:41-140`,
`rename.rs:77-133`, `info.rs:40-80`).

### SI-4. `init` re-implements `installer::create_symlink` verbatim
Low · XS
**Where:** `init.rs:365-390` vs `installer/symlink.rs:10-35`, identical line for line (the
Windows `create_wenget_shim` differs on purpose: absolute path).
**Recommendation:** Call `installer::create_symlink`; delete the copy.

### SI-5. PATH editing duplicated between `init` and `delete` (Q-5)
Medium · S
**Where:** shell-rc lists `init.rs:613` / `delete.rs:541-544`; PowerShell user-PATH scripts
`init.rs:500-525` / `delete.rs:508-525`.
**Recommendation:** One `core::env_path` module with `add`/`remove` for rc files, user PATH, and
system PATH (`registry.rs`). Fix S-4 and S-6 once there.

### SI-6. Launcher creation is `cfg`-split at every call site
Low · S
**Where:** `add.rs:2045-2053`, `rename.rs:281-295`, `local.rs:105-113` each write
`#[cfg(unix)] create_symlink` / `#[cfg(windows)] create_shim`. The non-native stubs
(`shim.rs:34-39`, `symlink.rs:37-43`) never compile, because `installer/mod.rs` gates the modules.
**Recommendation:** `installer::create_launcher(target, command) -> Result<PathBuf>` and
`remove_launcher(command)`, with script handling folded in (fixes the root cause of IM-4). Delete
the stubs.

### SI-7. 43 dead items, compiler-confirmed
Medium · S (Q-8)
**Method:** A copy of `src/` with every `#[allow(dead_code)]` removed, run through `cargo check`
(production target).
**Result:** `bucket.rs` `find_bucket`, `find_bucket_mut`, `set_enabled` · `cache.rs`
`find_package`, `packages_by_source`, `scripts_by_source` · `commands/bucket.rs:277` `new` ·
`config.rs` `load_json` · `manifest.rs` `available_platforms`, `get_platform`,
`packages_for_platform`, `scripts_for_current_platform`, `installed_names`, `is_command_taken` ·
`paths.rs` `new_user`, `new_system`, `app_bin_dir`, `package_record_path`, `executable_name` ·
`platform.rs` `MuslOnGnu`, `GnuOnMusl`, `WindowsCompilerVariant`, `Musl`/`Glibc` variants,
`is_exact`, `with_compiler`, `select_for_platform`, `select_all_for_platform`,
`detect_compiler_from_filename`, `score_asset` · `preferences.rs` `save` · `registry.rs`
non-Windows stubs · `core/repair.rs` `CreatedNew`, `Deleted`, `value` · `store.rs` `paths`,
`remove_package` · `script.rs` `get_powershell_command` · `providers/base.rs` `name` ·
`utils/http.rs` `RateLimit`, `check_rate_limit`, `is_low`, `warning_message`.
**Recommendation:** Delete them (CL-2 first). Where a test is the only user (`is_command_taken`,
`remove_package`), repoint the test at the live API.

### SI-8. The bucket subcommand enum is mirrored and mapped by hand
Low · XS
**Where:** `cli::BucketCommands` ↔ `commands::bucket::BucketCommand`, with a 25-line
variant-by-variant copy in `main.rs:43-68`.
**Recommendation:** Have `commands::run_bucket` take `cli::BucketCommands`.

### SI-9. Numeric-suffix search copied three times
Low · XS
**Where:** `add.rs:162-167`, `:194-199`, `:209-214` (`for i in 1..=99 { format!("{}-{}", …) }`).
**Recommendation:** A single `fn first_free(base, taken) -> String`.

### SI-10. `extract_variant_from_asset` is a 141-step replace cascade
Low · S
**Where:** `manifest.rs:806-938` (133 lines). For 47 patterns it runs three `replace` calls each,
then loops `while result.contains("--")`.
**Recommendation:** Tokenize on `-`/`_`, drop tokens that are versions or OS/arch/compiler/noise
keywords (reuse `platform.rs`'s tables, see OP-4), and rejoin. Behavior-sensitive: this decides
Installed keys, so pin current outputs for every asset name in `bucket/manifest.json` as a
golden test first.

### SI-11. Four copies of the batch tally/summary loop in `add.rs`
Medium · S
**Where:** `install_scripts` (`:358-401`), `install_local_files` (`:476-541`),
`install_from_urls` (`:563-682`), `install_packages` (`:1405-1682`), each with its own
`success_count`/`fail_count`/vectors and summary print.
**Recommendation:** One `BatchReport { ok: Vec<String>, failed: Vec<(String, anyhow::Error)> }`
with `print()` and `into_result()`. This is where IM-1 is fixed once.

### SI-12. Four HTTP clients with different configuration (A-5)
Medium · S
**Where:** `utils/http.rs:33` (30s timeout, token), `downloader/mod.rs:13` (no token; reqwest's
default 30s per-read timeout), `checksum.rs:77` (built per download), `config.rs:203` (built per
bucket thread).
**Impact:** A `GITHUB_TOKEN` helps API calls but not downloads; each checksum probe pays a new TLS
handshake.
**Recommendation:** One process-wide client (`OnceLock`) in `utils::http`; add a streaming `get`
for the downloader; per-call timeouts via `RequestBuilder::timeout`.

---

## Optimization

Scale for context: `bucket/manifest.json` is 329 KB with 86 packages, and a typical Installed set
is tens of packages. At that size the CPU items below cost microseconds. **Network round trips
and GitHub rate limits dominate.** Priorities follow that.

### OP-1. 4–8 GitHub API calls per package where 1 suffices
High · S–M (P-1, worse than reported)
**Where:** `fetch_package` always makes 2 requests (`github.rs:224` repo info + `:227` latest
release). Call sites stack:

| Operation | Calls per package | Sites |
|---|---|---|
| `update` check | 2 | `update.rs:72` |
| `add <bucket pkg>` | 4 | `add.rs:1029`, `:1328` |
| `add <github url>` | 6 | resolver `package_resolver.rs:213`, `add.rs:1029`, `:1328` |
| `update` of a DirectRepo pkg | 8 | all of the above |

Unauthenticated GitHub allows 60/hour, so a 10-package DirectRepo update consumes the whole hour.
**Recommendation:** (1) Version checks call `fetch_latest_release` only; repo info (description,
license) is needed only on first install of a DirectRepo package. (2) Carry the planning-phase
`Package` into the install phase instead of re-fetching (`:1328`). (3) `update` hands `add`
resolved packages (CL-1) so the resolver doesn't fetch again. Then consider `If-None-Match` ETags
on the cache (304s don't count against the limit).

### OP-2. One `update` loads the Installed set twice and the cache three times
Medium · (falls out of CL-1)
**Where:** Installed set: `update.rs:123`, then `add.rs:49`. Cache: rebuilt `update.rs:132`,
saved `:279`, re-read `add.rs:827`, re-read `add.rs:2168`, saved again `:2183`.
**Recommendation:** Pass `&mut InstalledSet` / `&mut ManifestCache` through the installer; save
the cache once at the end.

### OP-3. `repair` scans and parses `apps/` three times
Low · XS
**Where:** `repair.rs:31` `scan_app_dirs`, `:81` `duplicate_keys` (scans again, `store.rs:347`),
`:103` `store.load()` (scans again).
**Recommendation:** Derive duplicates and the set from the first `Vec<ScanEntry>`.

### OP-4. Asset parsing re-lowercases and rebuilds keyword tables per call
Low · XS (P-6)
**Where:** lowercasing at `platform.rs:302, 325, 458, 480, 487, 1128`; keyword arrays built inside
functions at `:490, 561, 598, 1081`, and `vec![…]` of platforms per call at `:1142`.
**Recommendation:** Lowercase once in `ParsedAsset::from_filename` and pass `&str`; hoist tables
to `const`. Matters only for `bucket create` (hundreds of releases); do it with SI-10.

### OP-5. Each executable candidate is opened up to three times
Low · XS
**Where:** `extractor.rs:608-648`: `metadata` for permissions, open+read 4 bytes for magic, open+read
128 bytes for shebang.
**Recommendation:** One open, one 128-byte read, then check both from the buffer.

**Looked at and not worth changing:** `fuzzy.rs` allocations (`osa_distance` matrix) are a few
thousand small allocations per search over 86 packages, well under 1 ms. `list --all` whole-cache
clone (P-3) is 329 KB once per command, also under 1 ms. Fix those only if the manifest grows by
two orders of magnitude.

---

## Claims checked and rejected

Scout reports were verified before inclusion. These were wrong and are excluded:

- *"Wrap tar decoders in `BufReader`"*: `flate2`, `xz2`, and `bzip2` read decoders already buffer
  internally.
- *"Downloader client has no timeout"*: reqwest's blocking client defaults to 30s (applied per
  read, so large downloads are fine).
- *"`ensure_dir_available` is skipped for scripts"*: it runs in `install_script`
  (`installer/script.rs:202`).
- *"`Config::preferences()` has no callers"*: used at `add.rs:824` and `update.rs:706` (see CL-2).
- *"`--ver` should be `--version`"*: clap reserves `--version` for the binary itself.

---

## Prioritized action plan

**Quick wins (< 1 day; mostly independent one- to ten-line changes)**
1. IM-2 `{:#}` in error prints.
2. IM-3 logger fix.
3. IM-1 non-zero exit on any failure (via SI-11's `BatchReport`, or a direct `bail!` first).
4. IM-7 honor the "using defaults" message.
5. IM-10, IM-11, IM-13, IM-15, CL-4, CL-7, SI-4, SI-8, SI-9, OP-3.
6. CL-2 then SI-7: strip non-`cfg` `#[allow(dead_code)]`, delete the 43 dead items.

**Medium term (1–5 days)**
7. IM-5 + IM-6: record written into staging before commit; propagate save errors.
8. IM-4 + SI-6: record-driven `rename` on a unified launcher API; IM-12 reuses it.
9. SI-1: collapse to one scoring engine; repoint the 19 tests.
10. OP-1 + SI-2: one fetch function, 1 API call per version check, no re-fetch between planning
    and install.
11. IM-8: single `semver`-based comparison.
12. SI-3, SI-5, SI-12: shared input matching, `env_path` module, one HTTP client.

**Longer term (> 5 days)**
13. T-3 first: integration tests for install/update/delete behind `WENGET_ROOT` plus an HTTP
    seam. Then CL-1 + OP-2: the `Installer` extraction from `add.rs`.
14. SI-10 behind golden tests.
15. Open security items from the prior audit (S-4 through S-9), fixed inside the modules that
    SI-5 and SI-6 create.

---

## Metrics

| Metric | 2026-09-03 | 2026-09-23 |
|---|---|---|
| LOC (`src/`) | 16,957 | 19,036 (14,935 prod / 4,101 test) |
| Files | 41 | 44 |
| Tests | 135 (125 pass, 5 ignored) | 176 (171 pass, 5 ignored, 0.11s) |
| Integration tests | 0 | 0 |
| CI quality gates | none | fmt, clippy `-D warnings`, test × 3 OS |
| `cargo clippy -D warnings` | clean | clean |
| clippy pedantic warnings | 596 (pedantic+nursery) | 375 (pedantic only, production target; 158 are `uninlined_format_args`) |
| Production `unwrap` / `expect` / `panic!` | 18 / 5 / 2 | 18 / 5 / 3 |
| `#[allow(dead_code)]` | 46 | 52 (43 items actually dead; ≥7 annotations on live code) |
| Functions > 100 lines | — | 15 (largest `install_packages`, 879) |
| Largest file | `add.rs` 2,426 | `add.rs` 2,444 |
| Git pack | 15.86 MiB | 16 MiB (`bucket/wenget` 4.7 MB × 40 commits) |

## Method note

Four parallel read-only scouts each covered one slice (add/update, engine, state/core,
commands/installers). Every finding above was re-read against the source by the lead auditor, or
reproduced on the debug binary. Reproduced findings are marked. Reproductions run in a
`WENGET_ROOT` sandbox (bin directory included) and live in
`docs/tmp/claude-scratch/audit-2026-09-23-repros.sh`. Dead code was measured by compiling a copy
of `src/` in `/tmp` with the allow attributes stripped; the working tree was not modified.
