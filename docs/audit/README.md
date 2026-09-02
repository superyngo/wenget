# Audits

Point-in-time sweeps for bugs, dead code, and inconsistency. Every file here is a historical
record: frozen once findings are addressed, dated by when it was written, never rewritten.
Current behavior lives in [`../reference/`](../reference/README.md).

## In progress

- **[2026-09-03-full-codebase-audit.md](2026-09-03-full-codebase-audit.md)** — full six-dimension
  sweep of v3.8.7 (architecture, quality, security, performance, testing, maintainability).
  2 Critical / 8 High. Headlines: tar + 7z path traversal (proven exploitable), `cargo test`
  overwrites the real `~/.wenget/installed.json`, and no CI runs tests/clippy/fmt.

## Landed

None yet.
