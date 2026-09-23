# Changelog archives

The root [`CHANGELOG.md`](../../../CHANGELOG.md) carries **`[Unreleased]` plus the current version
series only**. Completed series are moved here verbatim — same format, same ordering, no edits.

| Archive | Covers |
|---|---|
| [`v2.x.md`](v2.x.md) | v2.0.0 (2026-01-16) … v2.3.1 (2026-02-01) |
| [`v1.x.md`](v1.x.md) | v1.0.0 (2026-01-05) … v1.3.3 (2026-01-16) |
| [`v0.x.md`](v0.x.md) | v0.1.0 (2025-01-21) … v0.9.1 (2026-01-05) |

**When to archive.** On the first release of a new major series (v4.0.0), move the whole
preceding series into `v3.x.md` here and add a row above. Never archive the series the next tag
belongs to.

The 2026-09-23 move also removed a stray empty `## [Unreleased]` heading from the 3.x series and
fixed link references (duplicate 2.x block, a link for the untagged 2.1.1, missing 3.0.1). 0.3.0
has no tag, so it has no compare link.
