# Documentation Audit — 2026-10-01
Status: Resolved (2026-10-01)

Sweep of every tracked Markdown file at `a6c1721` (v4.0.0 plus the downloader resume / segmented
download work). Structure was checked mechanically (`Status:` lines, index rows, relative links,
changelog headings and link refs). Accuracy was checked by reading `README.md`, `bucket/README.md`,
`AGENTS.md`, `CLAUDE.md`, `CONTEXT.md`, `CHANGELOG.md`, `docs/plan/BACKLOG.md`, and
`docs/reference/*` against `src/`, `bucket/`, install scripts, and `.github/workflows/`. Every
finding was re-checked by hand before fixing.

Frozen records (`spec/`, `plan/`, `adr/`, earlier audits, archived changelogs) were checked for
structure only.

Checked and clean: version badge matches `Cargo.toml`; every CLI command, alias, and flag in the
README matches `cli`; config keys (`preferred_platform`, `custom_bin_path`,
`download_connections`), environment variables, checksum and rate-limit sections match the code;
`AGENTS.md` symbols and example test names exist; glossary downloader terms match
`downloader::resume` / `downloader::segmented`; BACKLOG SI-12 matches `utils::http::shared_client`;
every relative link resolves; every index lists each dated file once, newest-first.

## Structure

| ID | Finding | Fix |
|---|---|---|
| DD-1 | `plan/2026-10-01-downloader-resume-and-segmented.md` had no `Status:` on line 2 (a Chinese status sentence on line 3 instead) and cited a gitignored scratch file. | `Status: Shipped (2026-10-01)` on line 2; scratch citation dropped |
| DD-2 | `reference/README.md` described the archived changelogs as v0.x–v2.x; v3.x was archived at v4.0.0. | v0.x–v3.x |

## Accuracy

| ID | Sev | Claim → actual | Evidence |
|---|---|---|---|
| DD-3 | High | README Examples install `gitui`, `lazygit`, `starship`, `zoxide`, `tokei` → none is in the default bucket, so each fails with "Package not found". | `bucket/manifest.json`; `bucket/sources_repos.txt` (archived section) |
| DD-4 | Med | README "wenget aborts" when one package of a multi-package install fails → the batch continues, then exits 1 with "N install(s) failed". | `commands::add::run_with`; `BatchReport` |
| DD-5 | Med | README never mentions the 30 s stall retry, and "How It Works" / Features omit segmented and resumable downloads. | `utils::http::shared_client`; `downloader::download_file` |
| DD-6 | Low | README Features list scripts as PowerShell, Bash, Python → Batch is supported too. | `core::manifest::ScriptType::Batch` |
| DD-7 | Low | README user-level tree shows `cache/` without `downloads/` (the system-level tree has it). | `WenPaths::downloads_dir` |
| DD-8 | Low | `RESOURCE_FILTERING_RULES.md` §2.2 says `FallbackType` has unused `MuslOnGnu`, `GnuOnMusl`, `WindowsCompilerVariant` variants tracked in the backlog → those variants were removed; the enum has only `Arch32On64` and `X64OnArm`. | `core::platform::FallbackType` |

All findings were fixed in the commit that adds this record.

## Opened

| ID | Finding |
|---|---|
| DL-1 | An out-of-range `download_connections` makes `Preferences::validate` fail, and `validated_or_default` then drops **every** preference (also `preferred_platform`, `custom_bin_path`) with only a warning. Tracked in [`../plan/BACKLOG.md`](../plan/BACKLOG.md). |
