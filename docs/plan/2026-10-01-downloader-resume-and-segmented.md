# Downloader: retry resume + segmented multi-connection + cross-run resume
Status: Shipped (2026-10-01)

Commits `3c844fe`, `b415ac3`, `6535ca1`; measured results are in "Verification results" at the end.

## Goal

Pain point (confirmed by the user): a single large file downloads slowly, and a failure means
starting over.

- When a transfer fails midway, continue from what was already downloaded instead of from 0
  (within the same run, and on the next run).
- When the server supports Range, download large files over several connections in segments; the
  connection count comes from `config.toml`, with a default when unset.

Non-goals: downloading several packages at once (direction D), HTTP/2, new dependencies, changes
to `HttpClient` / token behavior.

## Confirmed constraints

- **Download requests must never set a per-request `.timeout()`**: the blocking request timeout is
  carried into the async layer's `total_timeout` and applies to the whole body
  (`reqwest-0.12.28/src/async_impl/client.rs:3090`), so large files get cut off at 30 seconds.
  Today's stall protection comes from the blocking **client-level** default of 30 seconds (which
  only applies before headers arrive and to each `read()`). Keep this unchanged; just make it
  explicit and document it in a comment.
- GitHub assets 302-redirect to signed URLs that expire after about 33 minutes → every retry and
  every segment re-requests the **original URL**.
- reqwest does not enable `http2` → each segment is its own HTTP/1.1 TCP connection (which is
  exactly what's needed to get around per-connection rate limits).
- Same-named assets from different packages share `downloads_dir/<asset_name>` → resume state must
  match on the URL.

## Phase 1: in-process retry + Range resume (commit 1)

`src/downloader/mod.rs`
1. Write to `<dest>.part`; when done, check length == the total from `Content-Length` /
   `Content-Range`, then rename to `dest`.
2. Retry loop: retry on transport errors, read errors, stall timeouts, 5xx, and 429; fail
   immediately on other 4xx. Backoff 1s → 2s → 4s; give up after **3 consecutive attempts with no
   progress** (any progress resets the counter).
3. Resume request: `Range: bytes=<n>-` plus `If-Range: <ETag or Last-Modified>`.
   - 206 with `Content-Range` start == n → append.
   - 200 (server doesn't support Range, or the file changed) → truncate to 0 and write from the
     start.
   - 416 → start over.
4. Print `Resuming at X MB (attempt k)`; the progress bar continues from the current position.
5. When this phase gives up, the downloader deletes `.part` itself (Phase 3 changes this to keep it).

`src/utils/http.rs`: set `.timeout(30s)` explicitly in `shared_client()` with a comment on the
blocking semantics (behavior unchanged), and fix the wrong "no total timeout" description; update
the description of `docs/plan/BACKLOG.md` SI-12 to match.

Tests (no new dependencies; a minimal HTTP/1.1 server written with `TcpListener` in the tests,
configurable for Range support, ETag, dropping the connection at byte K, ignoring Range, and
changing the ETag midway):
- Drop midway → the next request carries `Range: bytes=K-`, and the result is byte-identical to the
  original
- Server ignores Range → download from the start, correct result
- 404 → no retry, error immediately
- No progress at all → give up after 3 attempts, and `.part` is removed
- Length mismatch → error
- No `dest` is created on failure

## Phase 2: segmented multi-connection + config (commit 2)

`src/core/preferences.rs`
- Add `download_connections: Option<u8>` (None is not serialized); unset = 4. Valid range 1..=16;
  1 = segmentation disabled. `validate()` checks the range. Document it in a comment in the default
  `config.toml` template.

Callers pass `DownloadOptions { connections }`: `PackageInstaller` (new field, populated by `add`
from `config.preferences()`), `install_from_urls`, self-update (`Config::new().ok()`, falling back
to the default on failure).

`src/downloader/` (split out `segmented.rs`)
1. Probe: send `GET Range: bytes=0-`. Got 206, a total length, total ≥ 16 MiB, and connections > 1
   → segmented mode; otherwise use the Phase 1 single stream.
2. Preallocate `.part` with `set_len(total)`; split into 8 MiB chunks in a queue; start N workers
   with `std::thread::scope`, each taking the next chunk via an `AtomicUsize` (same pattern as
   `update.rs`).
3. Each chunk requests `Range: bytes=a-b` + `If-Range` on its own. It must get a 206 with the right
   start, and is written in place with `write_all_at` (a small helper over unix `FileExt` / windows
   `seek_write`); per-chunk retries follow the Phase 1 rules.
4. Any chunk getting a 200 or a changed ETag → abort all segments and fall back to a single stream
   from the start.
5. One shared progress bar (`inc`).

Tests: N=4 with a tiny chunk size reassembles a file identical to the original; no Range support /
small file → single stream; ETag change midway → restart from the beginning with a correct result;
config parsing (unset = 4, 0 and 17 invalid, 1 = single stream).

## Phase 3: cross-run resume (commit 3)

1. When a download fails (network error, Ctrl-C, retries exhausted), keep `<dest>.part` and add
   `<dest>.part.json` = `{url, validator, total, chunk_size, done_chunks}` next to it, updated
   atomically via tmp+rename after each completed chunk. Single-stream mode simply resumes from the
   length of `.part`.
2. When a download starts, if the sidecar exists, the url matches, and the probed validator matches
   → resume; otherwise discard and download again.
3. Take `File::try_lock()` on `.part` (std, Rust ≥ 1.89; released automatically when the process
   exits): another wenget downloading the same file → a clear error.
4. At the start of every download, delete `.part` / `.part.json` files in `downloads_dir` older than
   7 days.
5. Checksum failure → delete the final file (`CleanupGuard` already does this; unchanged).
   Self-update's whole scratch directory is still removed by the guard → no cross-run resume there
   (wenget itself is about 2 MB, which is acceptable).

Tests: retries exhausted → `.part` and the sidecar both remain; a second call fetches only the
remainder (the server sees the right Range start); url or validator mismatch → download from
scratch; lock held → error; stale files are cleaned up.

## Verification (real binary, sandbox)

`cargo build --release`, then with `env WENGET_ROOT=/tmp/wg-dl ...`:
- Install a >100 MB package and time `download_connections = 1` and `4` at least 3 times each.
- Press Ctrl-C midway, rerun the same command → see `Resuming at …`, and the checksum passes.
- At every phase run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Docs (updated with each phase's commit)

`CHANGELOG.md` (`## [Unreleased]` → `### 2026-10-01`), the settings section of `README.md`,
`docs/reference/glossary.md` (for any new terms), the index in `docs/plan/README.md`, BACKLOG SI-12.

## Verification results (2026-10-01, release build, sandbox)

Test file: hadolint v2.15.1 `hadolint-macos-arm64` (102,616,248 bytes), installed by direct URL.

| Connections | Full download time | Approx. speed |
|---|---|---|
| 4 | 132 s, 99 s, 74 s, 86 s | 0.7–1.3 MiB/s |
| 1 | 310 s, 734 s, 736 s, 718 s | 0.13–0.31 MiB/s |

- Segmented mode Ctrl-C (45 s, 4/13 chunks done), then rerun → `Resuming previous download at
  32.0 MB`, finished in 54 s.
- Single-stream mode Ctrl-C (60 s, 21 MB), then rerun → `Resuming previous download at 21.0 MB`,
  finished in 271 s.
- Every result's SHA-256 matched the asset digest published by GitHub; no leftover files in
  `cache/downloads/` afterwards.
- Note: background processes in a non-interactive shell script ignore SIGINT, so `kill -INT` had no
  effect in the first batch run; those runs were used as full 1-connection timings instead.
