# Prompt Audit — 2026-09-29
Status: Resolved (2026-09-29)

Audit of the agent instruction files (`CLAUDE.md`, `AGENTS.md`) at `a5dda8e` for dated prompting
patterns, stale facts, and conflicts between instruction files. F2 and F3 are applied in this
commit. F1 is left to the owner of the user-level file. F4 and F5 are low-confidence flags with no
edit.

## Assumptions (Step 0)

- **Scope:** the repository's own prompt surface. That means `CLAUDE.md` (5 lines, imports
  `@AGENTS.md`) and `AGENTS.md` (319 lines). `CONTEXT.md` was read only to check the index claims.
  - `.claude/settings.local.json` was not read, because settings files can hold secrets.
  - `~/.claude/CLAUDE.md` is user-level and outside the scope. It was read only because it was
    already in the session context, and it is used only for a conflict flag (F1).
  - `src/utils/prompt.rs` handles terminal y/n prompts and has nothing to do with LLMs.
  - No code calls an LLM. The `anthropic`/`claude-` matches are package names in `CHANGELOG.md`,
    `bucket/` and `docs/audit/`. No non-Anthropic provider SDK markers were found.
- **Target model:** Claude Opus 5.5 (`claude-opus-5-5`). The request named no model, so the files
  are audited against the model that runs this audit and reads them.

## Summary

There are two findings worth acting on.

**Most important: the user-level `~/.claude/CLAUDE.md` and this repo disagree on the released
CHANGELOG heading.** The user-level file expects `## [vX.Y.Z]` and a CI grep for it. The repo uses
`## [X.Y.Z]`, and no workflow greps for any heading. The fix would be in a file outside the
project, so this is flag-only (F1).

**The Release Workflow section (`AGENTS.md:204-252`) is written in dated all-caps pressure
language.** It was written on 2026-01-14 (commits `7191522` and `9294127`) and stacks
`MANDATORY` ×4, `MUST` ×2 and `DO NOT proceed` ×2. It then restates the same gates a third time in
an "Important Notes" block (F2, F3). The gates are real and stay. The applied diff keeps them,
and every command, as plain statements with their reason.

Findings by group:

| Group | Count |
|---|---|
| Group 1 (dated prompt text) | 2 (medium) |
| Group 2 (config files) | 1 high flag (conflict with the user-level file) + 2 low flags. Stale facts: 0 |
| Group 3 (tool descriptions) | not applicable |
| Group 4 (request config) | not applicable, because there is no LLM request code |

## Findings

### F1 — Conflict between the user-level file and the repo on the release CHANGELOG heading. High, flag.

- **Locations:**
  - `~/.claude/CLAUDE.md`, "After Each Development Task" §1: *"release tooling rewrites it to
    `## [vX.Y.Z] - YYYY-MM-DD` and CI version gates grep for that result"*
  - `AGENTS.md:276`: `## [X.Y.Z] - YYYY-MM-DD`
- **Evidence from the repo:** `CHANGELOG.md:302` is `## [3.9.0] - 2026-09-21`, with no `v`. No
  workflow in `.github/workflows/` mentions `CHANGELOG` or greps a `## [` heading.
- **Pattern:** Group 2, instruction files that contradict each other.
- **Why flag-only:** the user-level file is outside the project and applies to every project, so a
  project file is never a reason to edit it. For this repo, `AGENTS.md` matches reality and is the
  file to follow.
- **What you need to decide:** whether the user-level sentence should be reworded as
  project-conditional (for example, "where the repo's release tooling uses a `v` prefix"). Its
  claim about a CI grep isn't true here.

### F2 — Pressure language on the release gates. Medium.

- **Location:** `AGENTS.md:204-205, 220, 225-226, 244, 267, 286`
- **Evidence:**
  - `### 1. Code Quality Checks (MANDATORY)` / `**MUST complete before proceeding with release:**`
  - `**DO NOT proceed with release if:**` (twice)
  - `**MUST ensure all changes are committed before proceeding:**`
  - `(MANDATORY)` on the README badge and on step 5b
- **Pattern:** 1a, pressure language (several `MUST`/`MANDATORY` markers, with no "because" next to
  most of them).
- **Why it's obsolete:** Opus 5.5 follows instructions in `AGENTS.md` closely. When every step is
  marked mandatory, the markers stop telling the model which step matters most. Written at normal
  volume with its reason, the same constraint is followed just as reliably. The steps are a fragile
  procedure (keep-list item 3), so the commands and the gate conditions stay. Only the register
  changes, and the reason for gate 1 (`release.yml` runs no tests) moves to the gate itself.
- **Action:** rewrite (hunks 1, 3 and 4 of the patch).

### F3 — The "Important Notes" block repeats sections 1 and 2. Medium.

- **Location:** `AGENTS.md:248-252`, plus the in-block comments at `208-216` that repeat the
  `DO NOT` list.
- **Evidence:**
  - `All code changes must be committed before starting the release process` duplicates line 226.
  - `Verify code quality checks pass (fmt, clippy, and tests)` duplicates section 1.
  - `Organize commit messages clearly describing the updates` is a generic virtue.
  - `# Fix all clippy warnings before proceeding` is the third statement of the clippy gate.
- **Pattern:** 1c, padding (repetition used as reinforcement, generic virtues). Keep-list item 10
  covers only a single deliberate recap at the end of a prompt. This block sits in the middle of
  the procedure, and the gates are stated three times.
- **Why it's obsolete:** duplicated rules make the model reconcile several wordings of one rule.
  The one live fact in the block, that the release adds its own commits, moves into section 2's
  reason ("so the release commits contain only the version bump").
- **Action:** remove, folded into hunks 1–2.

### F4 — Generic cargo commands in "Build & Test Commands". Low, flag.

- **Location:** `AGENTS.md:8-41`
- **Why:** `cargo build`, `cargo check` and `cargo test -- --nocapture` are general knowledge (Group 2,
  "Verbose … explaining things the model already knows"). The CI-matching clippy line and the
  real test and module names are project context, though. The block loads every session but is
  harmless and accurate (keep-list items 1, 2 and 8). No edit is proposed.

### F5 — Generic Rust conventions in "Code Style Guidelines". Low, flag.

- **Location:** `AGENTS.md:86-99, 121-125`
- **Why:** PascalCase/snake_case, `///` docs and "test both success and error paths" are defaults
  the model already follows. They sit next to real project rules, which stay:
  - import grouping
  - anyhow only, no `thiserror`
  - the clippy `too_many_lines` CI gate
  - `Default` delegating to `new()`, which matches `src/core/bucket.rs:146` and others
  - `create_launcher`

  No edit is proposed, because the generic lines are working redundancy and cause no errors.

### Not findings (checked and clean)

- `CLAUDE.md`: a one-line pointer plus an `@AGENTS.md` import. It is clean.
- Every path, symbol, test name, CLI subcommand, CI invocation and release-workflow claim in
  `AGENTS.md` matches the code. For example:
  - `too_many_lines` is in `src/main.rs`.
  - The 86400 s TTL is in `src/core/cache.rs`.
  - `GITHUB_TOKEN` is read in `src/providers/github.rs:29`.
  - `release.yml` triggers on `v*.*.*`, reads notes from `%(contents)`, and has no `cargo test`.
  - CI runs clippy with `-D warnings`.
- Every named path exists, including `docs/reference/changelog/README.md`, the `src/core/*` and
  `src/commands/*` modules, `installer::{symlink,shim}` and `package_resolver.rs`. Every named
  symbol exists too.

## Applied diff

The four hunks below are applied to `AGENTS.md` in the same commit as this record:

1. §1 gate rewrite plus the §2 header (F2 and F3).
2. §2 closing gate plus removal of "Important Notes" (F2 and F3).
3. README badge `(MANDATORY)` removed (F2).
4. 5b `(MANDATORY)` removed (F2).

F1 needs no repo edit. `AGENTS.md` already matches the repo. Any fix belongs in the user-level
`~/.claude/CLAUDE.md`, which is outside this project.

## Verification

These are wording-only changes to a procedure. To check them, run the next release (or a dry run
in a scratch clone) with the edited file. Confirm the agent still:

- stops when clippy or a test fails,
- stops on a dirty tree,
- commits the version bump before tagging.
