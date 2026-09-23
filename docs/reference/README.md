# Reference

Current behavior only. Anything historical — a superseded design, a shipped plan, a resolved
investigation — lives in `../spec/`, `../plan/`, `../debug/`, or `../audit/`, not here.

- **[glossary.md](glossary.md)** — canonical vocabulary; read first.
- **[RESOURCE_FILTERING_RULES.md](RESOURCE_FILTERING_RULES.md)** — single source of truth for how
  release assets are filtered/scored into platform buckets, how the current platform is matched,
  and how the executable is selected from an extracted archive.
- **[changelog/](changelog/README.md)** — archived changelog series (v0.x–v2.x).

Machine-checked: none. No test asserts the claims in these documents; re-check them against
the code when the code changes.

See also [`../adr/`](../adr/README.md) for decision records.
