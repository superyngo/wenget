# Audits

Point-in-time sweeps for bugs, dead code, and inconsistency. Every file here is a historical
record: frozen once findings are addressed, dated by when it was written, never rewritten.
Current behavior lives in [`../reference/`](../reference/README.md).

## In progress

- **[2026-09-23-implementation-simplicity-audit.md](2026-09-23-implementation-simplicity-audit.md)**
  — implementation, clarity, simplicity, and optimization sweep of v3.9.0, plus a status re-check
  of every 2026-09-03 finding. 7 High among 39 new findings. Headlines: failed installs exit 0,
  error causes and `--verbose` output are swallowed, `rename` breaks script packages, and a
  launcher error after the staged swap erases the package.
- **[2026-09-03-full-codebase-audit.md](2026-09-03-full-codebase-audit.md)** — full six-dimension
  sweep of v3.8.7 (architecture, quality, security, performance, testing, maintainability).
  2 Critical / 8 High. Headlines: tar + 7z path traversal (proven exploitable), `cargo test`
  overwrites the real `~/.wenget/installed.json`, and no CI runs tests/clippy/fmt.

## Landed

None yet.
