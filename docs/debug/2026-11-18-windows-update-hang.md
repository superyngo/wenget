# 2026-11-18 Windows `update` hangs / loses resume state mid-download
Status: In progress

The original "hang" hypothesis below is **refuted**; see
[Correction](#correction-same-day-re-test). The real defect is that a bucket fetch failure is
swallowed and reported as "All packages are up to date".

## Symptom
On ezpad (Windows, system install under `C:\Program Files\wenget`), `wenget update -y`
upgraded `uv` (zip, segmented download) successfully but **hung indefinitely** downloading
`oh-my-pi`'s release asset `omp-windows-x64.exe` (a single ~243 MB executable, not an archive).

## Repro
```
wenget update -y
```
with `uv` and `oh-my-pi` both outdated. `uv` finished; `oh-my-pi` printed
`Downloading from https://github.com/can1357/oh-my-pi/releases/download/v18.22.2/omp-windows-x64.exe...`
and never progressed further.

## Evidence
- `tasklist`: `wenget.exe` stayed resident (not crashed) while stuck.
- `Get-NetTCPConnection -OwningProcess <pid>`: 4 `Established` connections to the asset CDN
  (`185.199.110.133:443`) plus 1 to `20.27.177.113:443` — consistent with the segmented
  downloader's default `download_connections = 4`.
- `C:\Program Files\wenget\cache\downloads\omp-windows-x64.exe.part`: **255,011,840 bytes**,
  unchanged across two checks several minutes apart — genuinely stalled, not just slow.
- The SSH session driving the command was killed (client-side timeout) while it was still
  stuck; after that, `omp-windows-x64.exe.part` and `.part.json` were both **gone** — on this
  forced disconnect the resume state was not kept, unlike the documented behavior ("a failed or
  Ctrl-C'd download keeps `.part`+`.part.json` so a rerun resumes it", CHANGELOG 4.1.0).
- `wenget list` afterward: `uv` shows `0.13.9` (upgraded), `oh-my-pi` still `18.12.6` (never
  completed).

## Working hypothesis (not yet root-caused in code)
- One of the 4 parallel range-request workers in `src/downloader/segmented.rs` got stuck inside
  `attempt()`'s `resp.read(&mut buffer)` (or in `send(req)` waiting on headers) without the
  per-read/connect timeout firing, so `retry_loop` never saw an error and never retried — the
  other workers finish their chunks and exit, leaving one worker blocking forever with no overall
  wall-clock timeout on the whole segmented job.
- Separately: on process kill (SIGHUP-equivalent from the dropped SSH session), the regular
  (non-self-update) download's `.part`/`.part.json` were removed instead of preserved — contrary
  to the resume feature. Needs tracing through `src/downloader/resume.rs` / whatever owns cleanup
  on drop/interrupt for plain (non-self-update) downloads.

## Next steps
- Reproduce locally against a throttled/slow server (via `test_server.rs`) to isolate which
  worker stalls and why the per-read timeout doesn't fire.
- Check whether `write_at`/`seek_write` or the shared `reqwest` client's timeout applies per
  low-level socket read or only around the whole `.send()` — SI-12 backlog already flags the
  client-level-only 30 s timeout bounding "headers and each read rather than the whole body"; may
  not be reaching a stuck read for another reason (e.g. a half-closed TCP connection that is
  still `Established` but never returns ACKs, so OS-level reads also never see EOF or error).
- Confirm whether regular downloads clean up `.part` on `Drop`/signal the same way self-update's
  `CleanupGuard` does (self-update is intentionally restart-from-scratch per 4.1.0 notes, but
  plain package downloads are documented to resume — the removal here contradicts that).

## Correction (same-day re-test)

### Why the evidence above does not show a hang
- `segmented::download` calls `file.set_len(plan.total)` before fetching anything, so the
  `.part` length is the **final size** from the first moment. A constant 255,011,840 bytes says
  nothing about progress.
- `.part`/`.part.json` disappearing matches `fetch`'s designed error path: on `Err`, if
  `State::has_progress` is false (segmented: `done_chunks` empty), both are removed. So the
  transfer **ended with an error** before any 8 MiB chunk completed. Not a resume regression.
- The 300 s "hang" was the agent's own SSH command deadline, not wenget.

### Re-test, detached from the SSH session (`Start-Process ... -Wait`)
```
exit=0
Checking for wenget updates...
Refreshing bucket cache...

All packages are up to date
--- stderr ---
WARN wenget::core::cache] Failed to fetch bucket 'wenget': Failed to fetch bucket from https://raw.githubusercontent.com/superyngo/Wenget/refs/heads/main/bucket/manifest.json
Warning: Package herdr not found in bucket wenget cache, skipping update check
... (same for every bucket package, incl. oh-my-pi still at 18.12.6)
```

### Network cause (transient, GitHub/Fastly side)
- From ezpad, `curl` to the bucket URL hit edge `185.199.110.133` and got **HTTP 503**
  (0.6 s). The other three `raw.githubusercontent.com` edges (`.108/.109/.111`) returned 200,
  and a few seconds later the default resolve returned 200 too. The Mac (IPv6 edge) got 200.
- `185.199.110.133` is the same edge the stuck `omp-windows-x64.exe` connections were on, so
  the earlier segmented-download failure is most likely the same degraded edge, which the
  downloader's retries could not get past (3 stalled attempts → error).

### Actual wenget defects
1. **One failed bucket fetch empties that bucket from the cache.** `cache::build_cache_from_results`
   only logs the `Err` and builds the new cache without it, then the rebuilt cache is saved
   (`bucket refresh` printed "Total packages in cache: 0 / Cache refreshed successfully!"). The
   previous good copy is thrown away.
2. **`update` then reports success.** `update::prepare_upgrade_jobs` warns "not found in bucket
   cache, skipping update check" per package, `resolve_all_targets` sees an empty list and
   prints "All packages are up to date"; exit code 0. A transient CDN error looks like "nothing
   to update".
3. **No retry for a 5xx on metadata GETs.** `utils::http::HttpClient::get_text`/`get_json`
   fail on the first 503, unlike the downloader's `retry_loop`.

## Fix
- `core::cache::build_cache_from_results` takes the previous cache; a failed bucket carries over
  its packages/scripts/source entry and the new cache is marked expired.
- `commands::update::find_upgradeable` returns the packages it could not check;
  `resolve_all_targets` bails when nothing is upgradeable and some were unchecked, else warns.
  Version-`local` direct-URL installs (no update source) are not counted.
- `utils::http::HttpClient::get_ok` retries 5xx / non-exhausted 429 twice.
- Sandbox repro on the real binary (`WENGET_ROOT=/tmp/wg-dl2`, local bucket server toggled to
  503, script in `docs/debug/2026-11-18-windows-update-hang/srv.py`): before `All packages are up to date`
  exit 0; after two logged retries, `Error: Could not check 1 package(s) for updates: foo` exit 1,
  and `manifest-cache.json` still holds the bucket's package. Pending: ezpad run.
