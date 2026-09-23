# `wenget search` evaluation (2026-09-23)
Status: Resolved (2026-09-23)

Implemented in commit 529bfe2 (2026-09-23). Deviations: subsequence span cap 3x; typo tolerance only for 4+ chars; description/repo use word-prefix match (not raw substring) to avoid noise like rip→JavaScript.

## Current implementation (`src/commands/search.rs`)

- Each term is compiled as a `glob::Pattern` and matched against the **whole name**, case-sensitive.
- Only `name` is searched. `description` and `repo` are ignored.
- Results are `HashMap::values()`, so the order is random from run to run.
- There is no fuzzy matching, typo tolerance, or ranking.

## Evidence (real binary, 70 packages + 12 scripts in cache)

| Query | Result | Expected |
|---|---|---|
| `rip` | nothing | ripgrep |
| `RIPGREP` | nothing | ripgrep |
| `ripgrp` (typo) | nothing | ripgrep |
| `json`, `finder` (keywords) | nothing | description hits (fzf is a "fuzzy finder") |
| `*a*` run twice | different order each time | stable, ranked |

- The README advertises `wenget search rust` and `wenget search <keyword>`, but neither works
  unless the name matches exactly.
- `resolve_from_cache` tells the user "Use 'wenget search X' to find similar packages", but
  search with the same X finds nothing similar.
- `truncate()` slices at a byte index (`&s[..max_len-3]`). A CJK or emoji description can land
  mid-character and panic, and the release build uses `panic = "abort"`.
- Packages filtered out by platform disappear silently. The user can't tell "doesn't exist"
  apart from "not available on this OS".
- Performance: not a bottleneck. The dataset is about 80 entries, so any scorer is O(N·T·L²)
  and takes microseconds. Loading the cache dominates. Libraries like nucleo or skim would be
  overkill.

## Proposed plan (pending approval)

Scorer, case-insensitive, taking the best tier over all terms (multiple terms stay OR, as today):

1. A term containing `*`, `?`, or `[` means glob mode: keep the current glob behavior, now
   case-insensitive.
2. Exact name match.
3. Name prefix (shorter names rank higher).
4. Name substring, with a word-boundary hit (after `-`, `_`, `.`) above a mid-word hit.
5. Name subsequence, fzf-style (e.g. `rgp` → ripgrep). Only for terms of 3+ chars, with the
   span capped to limit noise.
6. Typo tolerance: OSA/Damerau distance ≤1 for 3–5 chars, ≤2 for 6+ chars. Compare against the
   full name and against the name prefix of the same length.
7. Substring in description or repo path (lowest tier, terms of 3+ chars).

Output: sort by score desc, then name. Fix `truncate` to be char-safe. Print a count of matches
hidden by the platform filter.
No new dependency. OSA is about 20 lines. The release profile is size-optimized.
Tests: unit tests for the scorer tiers and truncate. Then verify on the real binary with the
queries in the table above.

Open questions for user: scope of description search, extra flags, and a "did you mean" hint in
`add`.
