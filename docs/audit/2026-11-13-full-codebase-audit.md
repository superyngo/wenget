# Full Codebase Audit — 2026-11-13
Status: Resolved (2026-11-14)

Six-dimension sweep of v4.1.0 (`3194a74`, clean tree). Findings already closed in
[`../plan/BACKLOG.md`](../plan/BACKLOG.md) are not repeated. Evidence is file + symbol.
**PROVEN** means reproduced on the real debug binary under `WENGET_ROOT=/tmp/wg-audit-*`;
**REASONED** means read from the code only.

## Resolution (2026-11-14)

Fixed: D-1, S-1 to S-6, R-1 to R-4, A-1, M-1 (see `CHANGELOG.md`). A-1 was resolved by
classifying zst and other unextractable formats as unsupported rather than adding zstd extraction.
P-1 is largely removed by S-2. Opened in the backlog: A-2 → LK-1, T-1/T-2/T-3 → TS-1, M-2 → AD-1.
The modules listed under Coverage limits are still unreviewed.

## Executive summary

- Overall health: **B+**. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and
  `cargo test` (253 passed, 4 network tests ignored) are green; no `TODO`/`FIXME`; three small
  `unsafe` blocks; docs match the tree.
- Critical: 0. High: 1. Medium: 6. Low: 12.
- Top 3 priorities:
  1. **D-1** `cargo audit` reports 14 vulnerabilities and nothing in CI would have caught them.
  2. **S-1** a checksum probe answered 403/429/5xx is reported as "No checksum published" and the
     install proceeds unverified.
  3. **R-1** `wenget repair` deletes the only surviving copy of a package after an interrupted swap.

## Coverage limits

Three delegated reviewers (installer/security, commands, core) failed on API rate limits before
reporting. The main pass read `downloader/`, `core/checksum.rs`, `installer/staging.rs`, the
extraction dispatch and zip/tar loops, self-update and `del self`, the HTTP client, and the
workflows. **Not reviewed in depth:** `core/platform.rs` scoring, `core/store.rs`,
`core/registry.rs`, `commands/{add,rename,bucket,init,info,list,search}.rs` beyond the cited
lines, `installer/script.rs`, `install.sh`, `install.ps1`. Test coverage was not measured (no
`cargo-llvm-cov`/`tarpaulin` installed).

## Security

### D-1 (High) 14 RustSec vulnerabilities in the lockfile; no advisory gate in CI
- Evidence: `cargo audit` on `Cargo.lock` — `zip 8.6.0` (RUSTSEC-2026-0369 critical 9.1,
  -0429 high, -0367 high, -0368), `rustls 0.23.45` (4 advisories, ≤ medium),
  `rustls-webpki 0.103.15` (RUSTSEC-2026-0382, distrusted intermediates accepted),
  `webpki-roots 1.0.6` (RUSTSEC-2026-0381), `zstd`/`zstd-safe`/`zstd-sys` (4), plus 14
  unsound/unmaintained warnings. `.github/workflows/ci.yml` has no audit job and there is no
  `.github/dependabot.yml`.
- Reachability: the four `zip` advisories are in `ZipArchive::extract`; `extractor::extract_zip`
  iterates entries with `enclosed_name()` and never calls it, and writes symlink entries as
  regular files, so they are **not reachable as written** (REASONED). The `rustls-webpki` and
  `webpki-roots` ones apply to every HTTPS request wenget makes.
- Fix: `cargo update` for rustls / rustls-webpki / webpki-roots / zstd (semver-compatible);
  bump `zip` to 9; add a `cargo audit` (or `cargo deny`) CI job and Dependabot for cargo and
  actions. Effort: S for the lockfile, M for `zip` 9.

### S-1 (Medium) Checksum probe treats every non-2xx as "not published" — PROVEN
- Evidence: `core::checksum::probe` — `Ok(_) => Probe::NotFound`.
- Scenario: `add` of a `/releases/download/` URL whose three checksum candidates answer 503,
  429 or 403 prints `No checksum published, skipping verification`, installs, exits 0. The
  module doc promises a failed probe aborts unless `--skip-checksum`; only transport errors do.
  Residual of S-5.
- Fix: map only 404/410 to `NotFound`; everything else to `NetworkError`. Add a fixture test.
  Effort: S.

### S-2 (Medium) GitHub's per-asset `digest` is ignored
- Evidence: `providers::github::GitHubAsset` has no `digest` field;
  `extract_platform_binaries` sets `checksum: None`.
- Impact: the releases API has returned `digest: "sha256:…"` for assets since mid-2025.
  Using it would verify most installs with zero extra requests, instead of three sequential
  probes that miss common names (`*_checksums.txt` from goreleaser, `SHA256SUMS.txt`,
  `sha256sums.txt`). REASONED.
- Fix: deserialize `digest`, pass it as `PlatformBinary.checksum` (the parser already accepts
  `sha256:<hex>`). Effort: S.

### S-3 (Medium) `del self` runs a script from a predictable shared temp path
- Evidence: `commands::delete::delete_executable_unix` writes
  `env::temp_dir()/wenget_uninstall.sh` with `fs::write`, chmods it, then runs `sh` on it.
  The path is interpolated as `rm -rf "{}"` without escaping.
- Scenario: system-level install, root runs `wenget del self` on Linux. Another local user
  pre-creates `/tmp/wenget_uninstall.sh`; root writes into the attacker-owned file and the
  attacker rewrites it before `sh` reads it → code runs as root. Blocked where
  `fs.protected_regular`/`protected_symlinks` are on (systemd distros), not on Alpine, older
  kernels or BSD. A root path containing `"`, `$` or a backtick also breaks out of the quotes.
  REASONED.
- Fix: create the script with `tempfile` (`O_EXCL`, 0700) or inside the wenget root's parent,
  and quote with the existing `sh_single_quote`. Effort: S.

### S-4 (Low) Workflow supply chain
- Evidence: `release.yml` — `cargo install cross --git https://github.com/cross-rs/cross`
  (unpinned HEAD builds release binaries), `softprops/action-gh-release@v1`;
  `publish-winget.yml` — `vedantmgoyal9/winget-releaser@v2` receives the `WINGET_TOKEN` PAT;
  all actions are tag-pinned, none SHA-pinned.
- Fix: pin `cross` to a rev/tag and third-party actions to commit SHAs. Effort: S.

### S-5 (Low) Direct-URL `add` still derives the file name from the URL
- Evidence: `commands::add` URL loop — `url.split('/').next_back()` then `split('?')`, joined
  onto the downloads dir without `sanitize_path_component` (IM-15 fixed the other two paths).
- Scenario: on Windows a `\..\` segment in the last URL component escapes the downloads dir; a
  URL ending in `/` yields an empty name and a confusing rename error. User-supplied URL, so
  low. REASONED.
- Fix: sanitize, reject empty. Effort: S.

### S-6 (Low) `WENGET_GITHUB_API` is honoured by release builds
- Evidence: `providers::github::api_base`. The "test hook" redirects API calls, with the
  `GITHUB_TOKEN` bearer header, to any host named in the environment. Also
  `bucket::ManifestGenerator::fetch_raw_script` sends the token to whatever raw URL the sources
  file lists (maintainer-only command). REASONED.
- Fix: attach the token only when the request host is `api.github.com` (or the override is
  loopback). Effort: S.

## Code quality / correctness

### R-1 (Medium) `repair` deletes the only copy of a package after an interrupted swap — PROVEN
- Evidence: `installer::staging::StagedInstall::commit` renames `app_dir → app_dir.old-<ts>`,
  then `staging → app_dir`; `core::store::InstalledStore::sweep_residue` removes every
  `*.old-*` and every `.staging/*` unconditionally.
- Scenario: a kill between the two renames leaves only `apps/tool.old-<ts>`. `wenget repair`
  lists it under "Interrupted installs", and on Enter (or stdin EOF, see R-4) removes it; the
  package and its record are gone. The staging module doc ("every crash point leaves either the
  complete old install or the complete new one") does not hold for that window. The same sweep
  also removes the staging dir of a concurrently running install.
- Fix: in `sweep_residue`, rename `X.old-<ts>` back to `X` when `X` is missing; skip staging
  dirs whose pid suffix is alive. Effort: S–M.

### R-2 (Low) Segmented resume trusts the sidecar over the `.part` file — PROVEN
- Evidence: `downloader::resume::Sidecar::done_mask` (same-`chunk_size` branch ignores
  `part_len`); `segmented::download` then `set_len(total)`.
- Scenario: `<asset>.part.json` listing all chunks done with no `.part` beside it → wenget
  prints `Resuming previous download at 16.2 MB`, `✓ Downloaded`, having fetched nothing; the
  file is all zeros. Extraction or a published checksum then fails, so the result is a
  confusing error, not a bad install. Reachable by deleting `*.part` by hand or a kill between
  `rename` and `state.remove()` in `fetch`. Chunks are also recorded done before their data is
  synced, so power loss gives the same shape.
- Fix: discard the previous record when `part_len != total` for segmented sidecars. Effort: S.

### R-3 (Low) `update self` promises a resume it then deletes
- Evidence: `commands::update::download_and_extract_self` guards the whole `self-upgrade` dir
  with `CleanupGuard`; `downloader::fetch` prints "Partial download kept; run the same command
  again to resume" on failure. The guard then removes the `.part`. REASONED.
- Fix: guard only the extraction dir and final file. Effort: S.

### R-4 (Low) `confirm` answers Yes on stdin EOF — PROVEN
- Evidence: `utils::prompt::confirm` — an empty read is "yes". `repair </dev/null` performed a
  deletion in the R-1 reproduction.
- Fix: treat a zero-byte read (EOF) as "no". Effort: S.

## Architecture & design

### A-1 (Medium) Two disagreeing archive-format tables; the shipped bucket lists assets wenget cannot install — PROVEN
- Evidence: `core::platform::FileExtension::from_filename` (lowercases; anything with a
  platform keyword and an unknown extension becomes `UncompressedBinary`) versus
  `installer::extractor::extract_archive` / `is_standalone_executable` (case-sensitive,
  different list, includes `.tbz`).
- Scenario: `x-aarch64-apple-darwin.tar.zst` → "Unsupported archive format";
  `x-aarch64-apple-darwin.zst` → copied as a "standalone executable", then "Failed to find
  executable". `bucket/manifest.json` carries 20 `.tar.zst` and 32 single-file `.zst` assets
  (codex, mise, bwrap). Today a `.tar.gz` sibling usually wins `dedupe_same_variant`, so it
  bites when a release ships zst only. Plain `.tar`, single-file `.gz`/`.xz` and upper-case
  extensions fail the same way.
- Fix: one shared format enum used by both sides; unknown compressed extensions classify as
  `Unsupported` so they never reach the manifest; add `.tar.zst` (zstd is already linked via
  `zip`). Effort: M.

### A-2 (Low) No cross-process lock on the wenget root
- Evidence: the only lock in `src/` is `File::try_lock` on a download `.part`
  (`downloader::fetch`). Two concurrent `add`/`update`/`repair` runs share `apps/`, `bin/` and
  the cache with no coordination (see R-1's sweep). REASONED.
- Fix: advisory lock file under the root for mutating commands. Effort: M.

## Performance

### P-1 (Low) Up to three sequential checksum probes per install
- Evidence: `core::checksum::lookup_checksum` (3 s timeout each); the segmented probe response
  body is also opened and dropped in `segmented::probe`. S-2 removes the probes for most assets.

No other measurable issue found in the code read; earlier P-/OP- items are closed.

## Testing

- 241 unit + 12 integration tests, all offline; the downloader has a fault-injecting fixture
  server and is the best-tested risky module (20 tests).
- **T-1 (Medium)** no test covers the non-2xx checksum probe (S-1), the interrupted-swap
  recovery (R-1), or a sidecar without its `.part` (R-2); each fix above should land with one.
- **T-2 (Low)** thin relative to size and risk: `commands/add.rs` (2 tests / 1,738 lines),
  `commands/update.rs` (4 / 1,196, self-update replacement untested), `commands/info.rs` (0),
  `core/migrate.rs` (0), `installer/local.rs` (1).
- **T-3 (Low)** coverage is unmeasured; add `cargo llvm-cov` to CI to get a number.

## Maintainability

- **M-1 (Low)** `Cargo.toml` declares no `rust-version`, while `File::try_lock` needs Rust
  1.89+; an older toolchain fails with an unhelpful error.
- **M-2 (Low)** `commands::add` URL loop nests seven levels deep with cleanup done by hand at
  the bottom instead of `CleanupGuard`, unlike the other two download sites.
- Positive: module `//!` docs throughout, an honest backlog, enforced `too_many_lines`.

## Prioritized action plan

1. Quick wins (< 1 day): D-1 lockfile bump and CI audit job; S-1; S-2; S-3; R-2; R-3; R-4;
   S-4; S-5; M-1.
2. Medium-term (1–5 days): R-1 with a crash-window test; A-1 shared format table and
   `.tar.zst`; `zip` 9; T-1/T-2 tests; S-6.
3. Long-term (> 5 days): A-2 root lock; coverage measurement in CI; finish the review of the
   modules listed under Coverage limits.

## Metrics

- Files analyzed: 53 Rust files (50 in `src/`, 3 in `tests/`), 5 workflows; about a third read
  in full
- Lines of code: 22,815
- Tests: 253 passing, 4 ignored (network); coverage not measured
- Dependencies: 26 direct, 236 in the tree; 14 vulnerabilities, 14 warnings
- Functions over 100 lines: 0 (clippy-enforced)

## Method note

Reproductions used `target/debug/wenget` with `WENGET_ROOT` under `/tmp/wg-audit-dl` and
`/tmp/wg-audit-zst`, and a local Range/ETag HTTP fixture at
`docs/audit/2026-11-13-full-codebase-audit/range_server.py` (optional third argument sets the status returned
for checksum-file probes). No source file was changed.
