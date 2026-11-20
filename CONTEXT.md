# CONTEXT

Entry point for all documentation. Root-level files (`README.md`, `CHANGELOG.md`, `CONTEXT.md`,
`LICENSE`, and the agent instruction files `AGENTS.md` / `CLAUDE.md`) stay here; everything else
lives under `docs/`.

Profile: dev

| Folder | Holds | Canonical? | Lifecycle |
|---|---|---|---|
| [`docs/reference/`](docs/reference/README.md) | Current behavior: glossary, resource-filtering rules, architecture notes, archived changelog series | Yes — the only source of truth | Kept in sync with the code |
| [`docs/adr/`](docs/adr/README.md) | Decisions that were expensive to reach and would be expensive to reverse | No — historical | Never edited; superseded by a new ADR |
| [`docs/spec/`](docs/spec/README.md) | Design records written before implementation | No — historical | Frozen once approved; only `Status:` changes |
| [`docs/plan/`](docs/plan/README.md) | Task-by-task implementation plans derived from a spec | No — historical | Frozen once shipped; only `Status:` changes |
| [`docs/plan/BACKLOG.md`](docs/plan/BACKLOG.md) | The one living tracker of open work, pending verification, external blockers, and watched items | No — live state | Never frozen while anything is open |
| [`docs/debug/`](docs/debug/README.md) | Handoff notes from investigations, with repro scripts | No — historical | Frozen once resolved; only `Status:` changes |
| [`docs/audit/`](docs/audit/README.md) | Point-in-time sweeps for bugs, dead code, inconsistency, plus assessment / verification runs | No — historical | Frozen once findings are addressed; only `Status:` changes |
| [`docs/memo/`](docs/memo/README.md) | The human's notes: ideas still forming, loose thinking, background | No — not evidence of behavior | Free-form; edited by people, by agents only on request |
| `docs/tmp/` | Scratch; `<agent>-scratch/` is gitignored | No | Swept at each release: promoted, archived to `docs/tmp/archive/YYYY-MM.tar.gz`, or trashed (see `AGENTS.md` release step 8) |

## Reading order

1. [`docs/reference/glossary.md`](docs/reference/glossary.md) — the vocabulary every other file uses.
2. [`docs/reference/README.md`](docs/reference/README.md) — index of the reference documents
   (asset-filtering rules, architecture notes, archived changelogs); module structure lives in
   each file's `//!` docs.
3. [`docs/adr/README.md`](docs/adr/README.md) — why the shape is what it is.
4. `CHANGELOG.md` — what changed recently; older series in
   [`docs/reference/changelog/`](docs/reference/changelog/README.md).
5. [`docs/plan/BACKLOG.md`](docs/plan/BACKLOG.md) — what is still open.
