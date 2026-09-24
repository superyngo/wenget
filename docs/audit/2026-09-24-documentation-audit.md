# Documentation Audit — 2026-09-24
Status: Resolved (2026-09-24)

Sweep of every tracked Markdown file at `dc78cc6` (v3.9.0 plus the 2026-09-23/24 backlog fixes),
after the backlog's Open table emptied. Structure was checked mechanically (`Status:` lines, index
rows, relative links, `file:line` citations in living documents, scratch ignore rule). Accuracy was
checked by reading `README.md`, `bucket/README.md`, `AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`,
`CHANGELOG.md`, `docs/reference/*`, and ADR 0001 against `src/` and `.github/workflows/`. Every
High claim was re-checked by hand against the code before fixing.

Frozen records (`spec/`, `plan/`, `adr/`, earlier audits) were checked for structure only.

Checked and clean: every spec/plan/audit has `Status:` on line 2; every relative link resolves; no
`file:line` citations in living documents; `docs/tmp/*-scratch/` is gitignored; ADR 0001 matches
the code; `CLAUDE.md` only points at `AGENTS.md` and `CONTEXT.md`.

## Structure

| ID | Finding | Fix |
|---|---|---|
| DC-1 | The 2026-09-03 and 2026-09-23 audits were still `In progress` although every finding was closed in the backlog (only real-Windows verification rows remain, tracked there). | `Resolved (2026-09-24)`, moved to Landed in `audit/README.md` |
| DC-2 | Landed tables sorted oldest-first in `spec/` and `plan/`, newest-first in `audit/`. | All newest-first |
| DC-3 | `docs/tmp/archive/2026-09.tar.gz` contains `claude-scratch/` files, which the archiving rule excludes. | Left as is (frozen archive) |

## Accuracy

| ID | Sev | Claim → actual | Evidence | Fixed in |
|---|---|---|---|---|
| DC-4 | High | README "Update metadata" (Examples, Troubleshooting) runs `wenget update` → `update` only upgrades installed packages and exits before any refresh when none are installed; metadata is refreshed by `bucket refresh`. | `commands::update::run` | `bc1078c` |
| DC-5 | High | README lists `info <name>` as 0 API calls → it calls `fetch_latest_version` (1 call; scripts 0). | `commands::info::display_package_info` | `bc1078c` |
| DC-6 | High | README: `del -f` deletes "without interactive confirmation" → `-f` only allows deleting the `wenget` package; `-y` skips prompts. | `commands::delete::run` | `bc1078c` |
| DC-7 | High | `AGENTS.md` release steps tag right after editing the version files, with no commit in between. | `AGENTS.md` Release Workflow | `f46cea5` |
| DC-8 | High | `RESOURCE_FILTERING_RULES.md` §4 describes the old substring cascade → token parser over `ASSET_EXTENSIONS` / `PLATFORM_TOKENS`. | `core::manifest::extract_variant_from_asset` | `76eb95f` |
| DC-9 | High | `RESOURCE_FILTERING_RULES.md` has no step for choosing among a platform's binaries (`filter_binaries`, the new `dedupe_same_variant`, `select_packages_for_platform`). | `commands::add::prepare_plan_binaries` | `76eb95f` |
| DC-10 | High | `bucket/README.md` lists a committed `wenget` binary and an "On Release" trigger → both removed; CI downloads the release asset and checks `SHA256SUMS`. | `.github/workflows/update-manifest.yml` | `3090dc1` |
| DC-11 | Med | README `repair` = "config files only" → also records, launchers, staging residue; `-f` applies fixes without prompting. | `commands::repair::apply_repairs` | `bc1078c` |
| DC-12 | Med | README Windows system tree shows `bin\*.exe` → `.cmd` shims. | `install.ps1`; `installer::create_launcher` | `bc1078c` |
| DC-13 | Med | README rate-limit table says searches are limited → `search` is offline. v0.2→v0.3 migration notice still in Quick Install. | `commands::search::run` | `bc1078c` |
| DC-14 | Med | Reference: executable selection attributed to `add.rs` → `PackageInstaller::select_executables*`; globs support `?` and `[...]`, not only `*`. | `installer::package`; `package_resolver::is_glob` | `76eb95f` |
| DC-15 | Med | Glossary: Manifest omits scripts; Launcher cites per-OS modules; no entries for Format score, Staged swap, Script. | `installer::create_launcher`; `FileExtension::format_score`; `StagedInstall` | `76eb95f` |
| DC-16 | Med | `AGENTS.md`: mentions `thiserror` (dependency removed); launcher example bypasses `create_launcher`; points at a non-existent architecture doc (also `CONTEXT.md` "subsystem map"); three notes duplicated between Key Implementation Notes and Gotchas; "rename the day's heading" fails with several Unreleased days; archiving rule duplicated; no `cargo test` in the release gate. | `Cargo.toml`; `installer::create_launcher`; `docs/reference/` | `f46cea5` |
| DC-17 | Med | `CHANGELOG.md`: `3.1.0` above `3.2.0`; compare links out of order; no `[Unreleased]` link. | `CHANGELOG.md` | `1fbbb20` |
| DC-18 | Low | README: install-method numbering broken by System-Level Installation; `path.json` and `RUST_LOG` undocumented; `del self` menu and leftover launchers undocumented. `AGENTS.md`: bare `cargo clippy`, stale example `install_package`, ambiguous `repair.rs`. `CONTEXT.md`: archive path missing `docs/`. | — | `bc1078c`, `f46cea5` |

Not fixed, by decision: README Examples partly repeat Quick Start; FreeBSD is not in the platform
table (wenget ships no FreeBSD build); historical 3.x entries use mixed prefix styles (released
entries are not rewritten); moving the release runbook out of `AGENTS.md` (it is agent conduct).

### Code defect found while checking docs

- `del self` removes the wenget root but leaves the `wenget` and package launchers in the bin dir
  dangling → backlog **B-14**. README now describes the current behavior.
