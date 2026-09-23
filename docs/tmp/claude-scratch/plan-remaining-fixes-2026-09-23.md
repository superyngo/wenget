# Plan: remaining audit fixes (2026-09-23), awaiting approval

Order: A → B → C → D, one commit each, with the usual gates, a CHANGELOG entry, and a sandboxed real-binary check.

A. GITHUB_TOKEN for add/update/info: `GitHubProvider::new()` reads `GITHUB_TOKEN` (empty = none).
   The HttpClient already sends `Bearer`. The gist client in bucket.rs stays tokenless on purpose. Test: rate_limit.limit=5000 with a token.
B. OP-1 + SI-2: single `fetch_package(url, version: Option<&str>)`; drop SourceProvider trait.
   - repo info (description/license) fetched only for a DirectRepo first install; otherwise reuse the cached/installed metadata → 1 call.
   - add.rs:1287/1351 (install phase) reuse the Package fetched at :1029/:1051 (planning) instead of refetching.
   - resolver URL fetch result reused by planning (skip the :1051 refetch when source is DirectRepo just resolved).
   - Verify: count GETs with --verbose. Target: add <bucket pkg> 1, add <url> 2, update check 1.
C. SI-1: `select_for_platform` becomes a #[cfg(test)] wrapper over parse+score_parsed; delete score_asset,
   select_all_for_platform, detect_compiler_from_filename. All tests must pass unchanged. If some fail, that shows real divergence, which gets reported rather than patched over.
D. CL-1 (narrow): `InstallOptions` struct replaces add::run's positional flags (main.rs, update.rs call sites).
   The full Installer/InstallRequest split is out of scope and stays open.
