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

- Rate limit reported as "Not found": fixed (URL inputs now print the resolver error chain; 403/429
  with `x-ratelimit-remaining: 0` names the rate limit). `GITHUB_TOKEN` support for `add`/`update`
  still open as a feature proposal.

- ~~IM-5 error-message wording on the real binary~~ — verified 17:36: `✗ Installed fd but could
  not create its launcher(s): …/bin/fd: Failed to remove existing symlink: …: Operation not
  permitted (os error 1). Fix the path, then run `wenget del fd` and `wenget add` again`, exit 1,
  record kept at 10.5.0.

- During IM-5 verification, one shell step ran without `WENGET_ROOT` and touched the real
  `~/.wenget`: `del fd`, `add https://github.com/sharkdp/fd --ver v10.1.0`, `update fd`. End state
  is consistent: fd 10.5.0 (bucket `wenget`), launcher `~/.local/bin/fd`. Whether fd was installed
  before, and at which version, is unknown.
