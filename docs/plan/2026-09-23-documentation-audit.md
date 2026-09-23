# Documentation audit fixes
Status: Shipped (2026-09-23)

Fixes for [`../audit/2026-09-23-documentation-audit.md`](../audit/2026-09-23-documentation-audit.md).
Decisions taken with the user on 2026-09-23: `CLAUDE.md` becomes a pointer to `AGENTS.md`; archive
the 0.x–2.x changelog series; promote citable scratch, archive the rest, gitignore
`docs/tmp/*-scratch/`; rename plans to pair with their specs; seed the backlog with every open
finding.

One commit per step, each with its `CHANGELOG.md` entry under `[Unreleased]` → `### 2026-09-23`.
Docs-only: no code changes, so the gate per commit is link/index checks rather than `cargo test`.

1. **Land the audit and this plan.** Add both records and their index rows.
   Verify: both indexes list them.
2. **Structure (DS-2, DS-3, DS-4, DS-5, DS-6, DS-10).** Move `Status:` to line 2 in 5 records;
   `Implemented` → `Shipped` in the 2026-09-03 spec; ADR status `accepted` → `Implemented
   (2026-09-03)` in file and index; rename so each spec/plan pair shares one basename —
   `spec/2026-03-18-update-scripts-and-self-check-design.md` → `…-self-check.md`,
   `spec/2026-04-10-update-default-binary-asset-matching-design.md` → `…-asset-matching.md`,
   `spec/2026-09-03-per-package-meta-design.md` and
   `plan/2026-09-03-per-package-records-implementation.md` → `2026-09-03-per-package-records.md` —
   and rewrite only the path strings that cite them (indexes, ADR 0001, plan 2026-09-03, 2
   `CHANGELOG.md` lines); rewrite `CONTEXT.md` from
   the template (tmp row, backlog row, reading-order step); `reference/README.md` states that no
   document is machine-checked.
   Verify: `rg -L '^Status: '` over working records names nothing (except `BACKLOG.md`); every index row
   resolves.
3. **Scratch (DS-7, DS-8, DS-9).** `git mv` the repro script to
   `docs/audit/2026-09-23-implementation-simplicity-audit/repros.sh` and rewrite its two path
   strings in that audit; `git mv` `search-evaluation.md` to `docs/audit/2026-09-23-search-evaluation.md`
   with `Status: Resolved (2026-09-23)` on line 2, then add an index row. Tar the remaining files
   into `docs/tmp/archive/2026-09.tar.gz` and `git rm` them; add `docs/tmp/*-scratch/` to
   `.gitignore`.
   Verify: the tarball lists every removed file; no live doc cites a removed path.
4. **Backlog (DS-1).** Create `docs/plan/BACKLOG.md` from the template: one Open row per
   still-open finding in both code audits (merged where they are the same finding, e.g. A-5 =
   SI-12), the scratch follow-ups, and this audit's two code defects (dead `FallbackType` variants,
   `update self`). Each row carries evidence as file + symbol, `Verified` = the date the finding was
   last checked against the tree, priority, effort, and an acceptance criterion. Fixed findings go
   to Done with their closing commit. The Watching section holds the fd incident note. Both audits'
   `Status:` becomes plain `In progress`, and the per-ID state moves into the backlog.
   Verify: every open audit ID appears in exactly one row.
5. **Changelog (DS-11, DS-12).** Move the 0.x, 1.x and 2.x sections verbatim to
   `docs/reference/changelog/v{0,1,2}.x.md` with their link references, and add an index README.
   Remove the stray empty `[Unreleased]` and fix the link references for 0.3.0, 3.0.1 and 2.1.1.
   Verify: the moved sections are byte-identical (diff); every 3.x section is still in the root.
6. **Agent files (DS-13, DA-19–22).** `CLAUDE.md` → `@AGENTS.md` import plus a link to
   `CONTEXT.md`; move its still-true unique facts (gotchas, cross-platform notes) into `AGENTS.md`
   (conduct) or `docs/reference/` (behavior). In `AGENTS.md`, replace the project tree with a
   pointer to `CONTEXT.md`, fix the `SourceProvider` example, and align the release section with
   the `[Unreleased]` convention and the `v*.*.*` trigger. State the changelog archive rule there.
   Verify: no symbol named in either file is missing from `src/`.
7. **README (DA-1–DA-10).** Fix each claim; add the undocumented flags, aliases, `WENGET_ROOT` and
   exit-status behavior; replace the bucket examples with ones that parse (checked by feeding
   them to the real binary in a `WENGET_ROOT` sandbox).
   Verify: every command line in README runs `--help` cleanly on the real binary.
8. **Reference (DA-11–DA-18).** Correct `RESOURCE_FILTERING_RULES.md` §0, intro, §2.2, §3 and §6;
   glossary: fix **Provider**, **Manifest**, **Installed package**; add **Launcher**, **Staging**,
   **Manifest cache**.
   Verify: each named symbol exists (`rg`).
9. **Close.** The audit and this plan get `Status: Resolved/Shipped (date)`, and the index rows
   are updated to match.
