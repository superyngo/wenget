# Follow-ups from the IM-1..IM-5 fix session (2026-09-23)

## New findings (not yet in an audit)

- **Rate limit reported as "Not found".** When the unauthenticated GitHub API limit (60/h) is
  exhausted, `wenget add https://github.com/sharkdp/fd` prints `Error …: Not found` instead of a
  rate-limit message. `add`/`update` also ignore `GITHUB_TOKEN` (only `bucket create` reads it).
  Related: audit OP-1 (4–8 API calls per package).
- **`repair` misses a blocked launcher path.** `missing_shims` uses `Path::exists()`, so a
  directory sitting at `bin/<cmd>` counts as a launcher and `repair` reports "all launchers match".
- **Rename vs update (unverified).** After `wenget rename`, does `update` recreate the launcher
  under the original name? Not checked.

## Pending verification

- IM-5 error-message wording on the real binary (blocked by the rate limit at 16:44; the resets
  17:11 CST). Behavior was verified: `update` exits 1, record kept at the new version, and
  `del` + `add` recovers. Repro: install fd `--ver v10.1.0` in a `WENGET_ROOT` sandbox, replace
  `bin/fd` with a directory, run `update fd -y`.

## Incident

- During IM-5 verification, one shell step ran without `WENGET_ROOT` and touched the real
  `~/.wenget`: `del fd`, `add https://github.com/sharkdp/fd --ver v10.1.0`, `update fd`. End state
  is consistent: fd 10.5.0 (bucket `wenget`), launcher `~/.local/bin/fd`. Whether fd was installed
  before, and at which version, is unknown.
