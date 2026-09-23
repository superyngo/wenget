# Documentation Audit — 2026-09-23
Status: Resolved (2026-09-23)

Two-pass sweep of every tracked Markdown file against `wens-dev-principles docs` (structure) and
against the code at `aca10c9` (accuracy). Pass 1 was mechanical (index, filename, `Status:`, link,
and code-span checks). Pass 2 read `README.md`, `bucket/README.md`, `AGENTS.md`, `CLAUDE.md`,
`CONTEXT.md`, `docs/reference/RESOURCE_FILTERING_RULES.md`, and `docs/reference/glossary.md`
against `src/`. Every High-severity accuracy claim was re-checked by hand against the code.

Frozen records (`spec/`, `plan/`, `adr/`, and the two earlier audits) were checked for structure
only; their content is history and is not expected to match the current code.

## Pass 1 — structure

| ID | Principle | Finding | Evidence |
|---|---|---|---|
| DS-1 | docs 17 | No living backlog. Open work is spread over two frozen audits (≈30 open findings in the 2026-09-23 audit, 28 open + 3 partly fixed in the 2026-09-03 audit) and a scratch note. Nothing can be ticked off. | no `docs/plan/BACKLOG.md`; `docs/tmp/claude-scratch/fix-followups-2026-09-23.md` |
| DS-2 | docs 8 | `Status:` is on line 3, not line 2 (a blank line after the H1), in 5 records. | `spec/2026-09-03-per-package-meta-design.md`, `plan/2026-09-03-per-package-records-implementation.md`, `audit/2026-09-03-full-codebase-audit.md`, `audit/2026-09-23-implementation-simplicity-audit.md`, `adr/0001-no-global-installed-index.md` |
| DS-3 | docs 8 | `Status:` values are outside the fixed set: `Implemented (2026-09-03)` in a spec (should be `Shipped`), and both audits carry free-text progress after `In progress` that belongs in the backlog. | `spec/2026-09-03-per-package-meta-design.md`; both audits |
| DS-4 | docs 14 | ADR index status is `accepted`; the value set is `Proposed` / `Implemented (date)` / `Superseded by NNNN`. | `adr/README.md`, `adr/0001-*.md` |
| DS-5 | docs 1 | `CONTEXT.md` has no `docs/tmp/` row, no backlog row or reading-order step, and its `audit/` row omits assessment/verification runs. | `CONTEXT.md` |
| DS-6 | docs 6, 18 | `reference/README.md` does not say which documents are machine-checked (none are; it should say so). | `docs/reference/README.md` |
| DS-7 | docs 10 | The 2026-09-23 audit's repro script lives in `docs/tmp/claude-scratch/`, which is scratch. The frozen audit cites it by that path, so it depends on a scratch file surviving. | `audit-2026-09-23-repros.sh`, cited twice in the 2026-09-23 audit |
| DS-8 | docs 20 | `search-evaluation.md` is an assessment someone would cite (it records the evidence behind commit `529bfe2`), but it sits in scratch without a dated name or index row. | `docs/tmp/claude-scratch/search-evaluation.md` |
| DS-9 | docs 12 | `docs/tmp/claude-scratch/` is tracked; the principle keeps `<agent>-scratch/` gitignored and the rest of `tmp/` committed. Loose files in it are stale: the done fix plan, the follow-up note (superseded by DS-1), and a WINGET_TOKEN wizard unrelated to docs. | `.gitignore`; `docs/tmp/claude-scratch/*` |
| DS-10 | docs 9 | Spec/plan pairs do not share a kebab title: `…-self-check-design` ↔ `…-self-check`, `…-matching-design` ↔ `…-matching`, `…-per-package-meta-design` ↔ `…-per-package-records-implementation`. | `docs/spec/`, `docs/plan/` |
| DS-11 | docs 16 | `CHANGELOG.md` is 1,389 lines and holds four series (0.x: 18 releases, 1.x: 8, 2.x: 10, 3.x: 25). Only 3.x plus `[Unreleased]` belong in the root file. | `CHANGELOG.md` |
| DS-12 | — | `CHANGELOG.md` has a second, empty `## [Unreleased]` heading between 3.1.0 and 3.0.4. Link references are missing for 0.3.0 and 3.0.1, and one exists for 2.1.1, which has no section. | `CHANGELOG.md` |
| DS-13 | docs 3 | Two agent instruction files. `CLAUDE.md` (277 lines) is nearly all reference material (module map, data flow, directory layout, target matrix) and never links `CONTEXT.md`. `AGENTS.md` duplicates build commands with it and restates a project tree. | `CLAUDE.md`, `AGENTS.md` |

Checked and clean: every `docs/` folder has a `README.md` and every index row resolves; every
working-record filename is dated; no `reference/` file carries a History section; no `file:line`
citations in living documents (the frozen records have them, as expected).

## Pass 2 — accuracy

### README.md

| ID | Sev | Claim → actual | Evidence |
|---|---|---|---|
| DA-1 | High | `wenget delete <name>` (Quick Start, Commands, Rate Limits, Examples) → no such subcommand; it is `del` (aliases `remove`, `rm`, `uninstall`). | `cli.rs` `Commands::Del` |
| DA-2 | High | Global Options lists `--yes, -y` and `--verbose, -v` → only `--verbose` is global and has no short form; `-y` is per subcommand. | `cli.rs` `Cli` |
| DA-3 | High | Bucket Structure examples: package `platforms.<id>` is one object without `asset_name`; script entries are flat `url`/`script_type` → `platforms` is `Vec<PlatformBinary>` (with required `asset_name`); scripts use `platforms: {<ScriptType>: ScriptPlatform}`. A bucket written from the README fails to parse. | `manifest.rs` `Package`, `PlatformBinary`, `ScriptItem` |
| DA-4 | Med | `wenget update self` upgrades wenget → `update` always checks for a wenget update first; `self` is treated as a package name and reported as not installed. | `update.rs` `run`, `check_and_upgrade_self` |
| DA-5 | Med | System install dir `/opt/wenget/app`, `%ProgramW6432%\wenget\app` → `apps`. | `paths.rs` `WenPaths::apps_dir`; `install.sh`, `install.ps1` |
| DA-6 | Med | Directory tree shows `cache/manifest-cache.json` → `<root>/manifest-cache.json`. | `paths.rs` `WenPaths::manifest_cache_json` |
| DA-7 | Med | Rate Limits: token auth is a "future feature"; `add <name>` makes no API calls; `search` is rate-limited; `bucket remove` → `GITHUB_TOKEN` is honored by `add`/`update`/`info`; `add <name>` makes one API call; `search` makes none; the subcommand is `bucket del`. | `github.rs` `GitHubProvider::new`; `add.rs` `run`; `search.rs` `run`; `cli.rs` `BucketCommands::Del` |
| DA-8 | Low | Platform table omits Windows ARM64; project tree omits `main.rs`, `package_resolver.rs`. | `platform.rs` `Arch`; `src/` |
| DA-9 | — | Undocumented: `add` flags `-c`, `-v/--ver`, `-p`; `del --force`/`--variant`; `repair --force`; command aliases; `WENGET_ROOT`; non-zero exit on failed installs; checksum verification; stage-and-swap installs. | `cli.rs`; `checksum.rs`; `staging.rs` |

`bucket/README.md` (DA-10, Low): the manual-generation example runs `./wenget` from the repo root,
but the binary is `bucket/wenget`; it also repeats the `bucket create` usage that README.md carries.

### Reference

| ID | Sev | Claim → actual | Evidence |
|---|---|---|---|
| DA-11 | High | `RESOURCE_FILTERING_RULES.md` says `score_asset` / `select_all_for_platform` are kept-in-sync compatibility copies and the checklist requires editing both → deleted in `6e5279a`; `score_parsed` is the only engine, `select_for_platform` is a test helper over it. | `platform.rs` `BinarySelector::score_parsed` |
| DA-12 | Med | §2.2 lists `MuslOnGnu` (500), `GnuOnMusl` (400), `WindowsCompilerVariant` (450) as Phase 2 fallbacks → `fallback_identifiers` produces only `Arch32On64` and `X64OnArm`; libc/compiler variants are Phase 1 exact matches (`possible_identifiers`, scores 1000/999/998). The three variants are never constructed. | `platform.rs` `Platform::fallback_identifiers`, `possible_identifiers`, `FallbackType` |
| DA-13 | Med | §0 cites `fetch_package_by_version` → merged into `fetch_package` in `3f56915`. | `github.rs` `GitHubProvider::fetch_package` |
| DA-14 | Med | §3: "`find_executable` takes the top one" for installs → `add` installs from `find_executable_candidates` (several executables possible); `find_executable` is used only by self-update. | `add.rs`; `update.rs` `upgrade_self_with_provider` |
| DA-15 | Low | Intro says "two stages" then lists three. | `RESOURCE_FILTERING_RULES.md` intro |
| DA-16 | High | Glossary **Provider** is "a `SourceProvider` implementation" → the trait was removed in `3f56915`. | `github.rs` `GitHubProvider` |
| DA-17 | Low | Glossary **Manifest** maps names to `DirectRepo`/`DirectUrl` sources → `DirectUrl` is a `PackageInput` variant, not a `PackageSource`. **Installed package** has no `_Avoid_:` line. | `manifest.rs` `PackageSource`; `package_resolver.rs` `PackageInput` |
| DA-18 | Med | Glossary lacks terms used throughout code and docs: **Launcher**, **Staging**, **Manifest cache**. | `installer/shim.rs`, `installer/symlink.rs`, `installer/staging.rs`, `cache.rs` |

### Agent instruction files

| ID | Sev | Claim → actual | Evidence |
|---|---|---|---|
| DA-19 | High | `AGENTS.md` project tree lists `providers/base.rs` and its import example uses `super::base::SourceProvider` → both deleted in `3f56915`. `bucket.rs` is described as add/remove/create → subcommands are add/del/list/refresh/create. | `src/providers/`; `cli.rs` `BucketCommands` |
| DA-20 | High | `CLAUDE.md` lists a `LocalScript { original_path }` source and omits `store.rs`, `checksum.rs`, `fuzzy.rs`, `preferences.rs` → the variant is `Script { origin, script_type }`; `store.rs` is the record store. | `manifest.rs` `PackageSource`; `core/store.rs` `InstalledStore` |
| DA-21 | Med | `CLAUDE.md` rate-limit note has no `GITHUB_TOKEN`. | `github.rs` `GitHubProvider::new` |
| DA-22 | Med | `AGENTS.md` release step 5 writes `## [x.x.x] - date` with Added/Changed/Fixed by hand; the repo convention is `## [Unreleased]` → `### YYYY-MM-DD` entries rewritten at release. Tag trigger is `v*.*.*`, not `v*`. `release.yml` takes notes from the tag and does not read `CHANGELOG.md`. | `.github/workflows/release.yml`; `CHANGELOG.md` |

### Code defects found while checking docs

Not documentation fixes; they go to the backlog:

- The `FallbackType::MuslOnGnu`, `GnuOnMusl` and `WindowsCompilerVariant` variants are dead (DA-12).
- `update self` silently does nothing special (DA-4): either support it or drop it from help/docs.

## Proposed fixes

See [`../plan/2026-09-23-documentation-audit.md`](../plan/2026-09-23-documentation-audit.md).
