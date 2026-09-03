# Remove `installed.json`: Per-Package Records

Status: Draft (2026-09-03)

## Problem Statement

All installed-package state lives in one file, `{root}/installed.json`, loaded and rewritten in
full by every mutating command. Three consequences:

1. **Single point of total loss.** The whole set is serialized and written non-atomically
   (`src/core/config.rs:146-149`, `162-170`) from eight call sites (`add.rs:383,529,676,1589,1644`,
   `delete.rs:242`, `rename.rs:69`, `repair.rs:103,113`). A crash mid-write, or any bug that writes
   an empty collection, loses the record of *every* package. This is audit finding A-1.
2. **It has already fired.** Audit finding T-1: `cargo test` truncated the maintainer's
   `installed.json` to `{"packages": {}}`. At the time of writing, `~/.wenget/installed.json` is
   20 bytes while `~/.wenget/apps/` still contains `fnm` and `lazyssh`. The packages survived;
   only the record of them did not.
3. **Records and app directories can diverge.** Deleting an app directory by hand leaves a stale
   entry; a failed save after a successful extract leaves an untracked app directory.

The information in each entry describes exactly one directory. Storing it anywhere else than in
that directory is what creates all three problems.

## Decision

`installed.json` is removed. Each installed package stores its own **package record** in
`{app_dir}/.wenget/package.json`. The set of installed packages is the set of app directories
containing a readable package record. No global index file is written — not even a cache.

Rejected alternatives:

- **Keep `installed.json` as a rebuildable index cache.** Reintroduces two copies of the same
  state and the drift they permit, which is the class of bug being removed. The scan it would
  optimize costs tens of `read`s against a command that already makes network calls.
- **Keep `installed.json` authoritative, mirror a read-only copy per directory.** Leaves A-1 and
  T-1 untouched; buys only human readability.

Recorded as [ADR 0001](../adr/0001-no-global-installed-index.md).

## Vocabulary

These terms are used consistently below and land in `docs/reference/glossary.md` with the
implementation:

- **Package record** — the `{app_dir}/.wenget/package.json` file. The authoritative statement that
  a package is installed. _Avoid_: meta, meta file, ledger, registry, index, install record.
- **App directory** — `{root}/apps/<sanitized-name>/`, the directory holding one package's files
  and its package record. _Avoid_: payload.
- **Untracked app directory** — a directory under `{root}/apps/` with no readable package record.
  wenget reports it and otherwise leaves it alone.
- **Installed key** — `{repo_name}` or `{repo_name}::{variant}`, produced by
  `generate_installed_key`. The identity of an installed package; reconstructed from record
  content, never parsed from the directory name.
- **`InstalledSet`** — the in-memory collection of loaded packages. Renamed from
  `InstalledManifest` so that **Manifest** keeps its glossary meaning: the `manifest.json`
  published by a bucket.

## Preconditions Verified

- Every install path produces exactly one directory under `{root}/apps/`:
  binaries `add.rs:1743`, bucket scripts `add.rs:445`, direct scripts `add.rs:2241`,
  local files `installer/local.rs:39`, script payloads `installer/script.rs:202`.
- The installed key is recoverable from record content, not from the directory name.
  `sanitize_path_component` (`paths.rs:38-56`) is lossy — `bun::baseline` becomes `bun-baseline` —
  but `repo_name` + `variant` are stored fields, and `generate_installed_key()`
  (`manifest.rs:936-940`) reconstructs the key from them.
- Every install path calls `fs::remove_dir_all(&app_dir)` *before* writing anything
  (`add.rs:1748-1749`, `installer/local.rs:48-49`). Colocating the record therefore requires §2.
- `bin_dir()` is `~/.local/bin` for user installs and `/usr/local/bin` for system installs
  (`paths.rs:212-232`) — directories shared with unrelated software. This constrains §6.
- `WenPaths::with_root` and `Config::with_paths` are `#[cfg(test)]`-only (`paths.rs:118`,
  `config.rs:52`), so the shipped binary can only address the real root. This requires §9.

## Design

### 1. On-disk format

Path: `{app_dir}/.wenget/package.json`.

A `.wenget/` subdirectory, not a bare dotfile, so the record never collides with an extracted
archive member and is trivially excluded from executable scanning.

Content is the existing `InstalledPackage` struct, serialized as today, with two changes:

```rust
/// Schema version of this record. Absent means version 1.
#[serde(default = "default_meta_version")]
pub meta_version: u32,

/// Where this package lives. Not serialized: the record's own location is the
/// answer, so the file cannot disagree with reality.
#[serde(skip)]
pub install_path: String,
```

`install_path` is populated on load from the directory the record was found in. It is not written,
so a moved or copied app directory cannot carry a stale location.

**Version policy.** The version is read *before* deserialization: parse to `serde_json::Value`,
read `meta_version`, then deserialize. A record whose version exceeds the known maximum is
**skipped with a one-line warning naming the directory** — never quarantined, never overwritten.
A downgrade must be read-only toward records it does not understand; treating a valid future
record as corrupt would turn a harmless downgrade into data damage.

The deprecated `command_names`, `command_name`, and `parent_package` fields keep their current
`serde` attributes; migration from `installed.json` runs the existing `migrate()` logic once, so
freshly written records never contain them.

### 2. Install and update are staged

Because the record lives inside the app directory, the current
`remove_dir_all` → `extract` → `save` order would destroy the old record before the new payload
exists: a failed update would lose both the files *and* the record of what was there.

New order for every install path:

1. Extract into `{root}/apps/.staging/<dir>-<pid>/`.
2. Write the package record inside the staging directory.
3. `rename(app_dir, {root}/apps/<dir>.old-<timestamp>)` if the app directory exists.
4. `rename(staging, app_dir)`.
5. `remove_dir_all(<dir>.old-<timestamp>)`.

Every crash point leaves either the complete old install or the complete new one. The cost is
transiently doubled disk use for one package and three residue patterns for `repair` to sweep:
`apps/.staging/`, `apps/*.old-*`, and `package.json.corrupt-*`.

`.staging/` and `*.old-*` are not app directories and are skipped by the scan in §3.

### 3. `InstalledStore`

`InstalledManifest` stops being a serde DTO for one file and becomes `InstalledSet`, an in-memory
collection loaded from and persisted to many. New module `src/core/store.rs`:

```rust
pub struct InstalledStore { paths: WenPaths }

impl InstalledStore {
    /// Scan {root}/apps/*/.wenget/package.json, skipping `.staging`, `*.old-*`,
    /// and dotted entries. Unreadable records are quarantined (see §6) and
    /// skipped; one bad file never fails the load.
    pub fn load(&self) -> Result<InstalledSet>;

    /// Write one package's record atomically: tmp file in {app_dir}/.wenget/,
    /// sync_all, rename over package.json.
    pub fn save_package(&self, key: &str, pkg: &InstalledPackage) -> Result<()>;

    /// The record is removed with the directory; this is only for the rare case
    /// of dropping a record while keeping the files.
    pub fn remove_package(&self, key: &str) -> Result<()>;
}
```

`InstalledSet` keeps `InstalledManifest`'s API surface (`get_package`, `upsert_package`,
`remove_package`, `group_by_repo`, `find_by_repo`, `is_command_taken`, `command_name_set`) so
read-side callers change only in the type name. `packages` remains
`HashMap<String, InstalledPackage>` keyed by installed key, rebuilt during `load()` via
`generate_installed_key(repo_name, variant)`.

`load()` performs no writes to `bin_dir` and never re-links a shim. Every drift it notices is a
`repair` finding (§6), not a load-time repair.

### 4. Call-site changes

- `Config::load_installed` / `get_or_create_installed` delegate to `InstalledStore::load()`.
  Signatures are unchanged apart from the `InstalledSet` rename, so `list.rs`, `update.rs`,
  `info.rs`, `package_resolver.rs`, and the read half of `add.rs` need no logic edits.
- `Config::save_installed(&InstalledManifest)` is **deleted**. The eight whole-file writes are
  replaced by per-package writes at the point the package is installed or modified:
  - `add.rs` — after each successful staged install, `store.save_package(&key, &inst_pkg)`. The
    existing batching optimization is dropped: it exists only to amortize whole-file
    serialization, which no longer happens. The in-memory snapshot passed down for conflict
    resolution (`add.rs:1688-1692`) is still updated in memory and still must not be re-read from
    disk.
  - `delete.rs` — no record write at all. `fs::remove_dir_all(app_dir)` (`delete.rs:294-297`)
    removes the record with the files. Shim removal is unchanged.
  - `rename.rs` — writes only the one package whose `executables` map changed.
  - `repair.rs` — both `save_installed` calls (`:103`, `:113`) disappear with the rewrite in §6.
    This file does not compile until then.
- `Config::init` no longer creates `installed.json`; `is_initialized` no longer tests for it.
  Initialization means: root and `apps/` exist.
- **Read commands no longer initialize.** With no file that must exist for reads to work, an
  absent root means "nothing installed": `list`, `info`, `search`, and `update --check` report
  that and create no directories. `get_or_create_installed`'s `init()` call
  (`config.rs:173-178`) is removed; initialization happens on `add` and on `wenget init` only.
  `init.rs:100-103` and `:318` drop their `installed.json` lines.

### 5. Command-name reverse index

Command-name conflict resolution, `rename`, and `delete` need a command-name → package map. This
is built once per `InstalledStore::load()` from the loaded `executables` maps, exactly as
`command_name_set` does today. Cost is one `read_dir` plus one small `read` per app directory —
tens of syscalls against commands that already perform network I/O.

### 6. Failure handling

Per-package, replacing the current all-or-nothing repair:

| Condition | Behavior |
|---|---|
| Directory under `apps/` with no `.wenget/package.json` | Reported by `repair` as an untracked app directory. Not loaded, not deleted. |
| Record present but unparseable | Renamed to `package.json.corrupt-<timestamp>`, a Critical warning naming that one package is printed, load continues. |
| `meta_version` above the known maximum | Skipped with a warning. Not quarantined, not overwritten (§1). |
| Two directories reconstructing the same installed key | Both reported by `repair`. `load()` keeps the first and warns; it does not silently shadow one. |
| `.staging/`, `*.old-*` residue | Skipped by the scan; swept by `repair`. |

Writes on the read path — quarantine renames and the §8 migration — are **best-effort**. If the
filesystem refuses the write, wenget logs one warning, proceeds with the in-memory result, and
exits successfully. A read command never fails for a write reason.

`wenget repair` is rewritten against this model. Per app directory it reports OK / corrupt record /
missing record / duplicate key, and it sweeps staging and `.old-*` residue. It gains one global
check — **orphaned shims** — with a hard rule: *wenget only ever reports or removes a bin entry it
can prove it created.* Because `bin_dir` is shared with unrelated software, absence from the
loaded set is not evidence. Proof means:

- Unix: the entry is a symlink whose target resolves under `{root}/apps/`.
- Windows: the entry matches wenget's generated shim signature (`installer/shim.rs`) and names a
  path under `{root}/apps/`.

Anything unresolvable is left silent. Provable orphans are reported, and removal always requires
confirmation. `repair` additionally validates the reverse direction: a package whose recorded
command has no shim, and a wenget-owned shim pointing at a path no loaded package occupies — the
breakage a hand-moved app directory actually causes.

### 7. Directory-name collision

`sanitize_path_component` maps distinct keys onto one directory: package `foo-bar` and variant
`foo::bar` both resolve to `apps/foo-bar`. Today this is a latent bug — the two extracts already
overwrite each other while `installed.json` pretends the entries are distinct. Under this design
the second install would also overwrite the first's record, making the collision destructive.

Guard: before staging, if the target app directory already holds a record whose reconstructed key
differs from the key being installed, abort with an error naming both keys *and the way out*:

```
Cannot install foo::bar: apps/foo-bar is occupied by package foo-bar.
Run `wenget delete foo-bar` first, or install foo::bar under a different name.
```

Without the second line the user meets an install that can never succeed and no way to learn why.

**Known limitation, not out of scope.** The collision is only detected, never resolved. The fix is
to make the mapping lossless — append a short hash of the installed key when
`sanitize_path_component(key) != key`, giving `apps/bun-baseline-a3f1/` — which would remove this
guard entirely. It is deferred because it renames the app directory of every existing variant
install and so requires directory renames plus shim rewrites during migration.

### 8. Migration

On `InstalledStore::load()`, if `{root}/installed.json` exists:

1. Parse it with the current loader, including `migrate()` (`::` path rename, `command_names` →
   `executables` filesystem walk).
2. For each entry whose `install_path` directory exists, write `.wenget/package.json`. Entries
   whose directory is gone are dropped and listed in a one-time notice.
3. Rename `installed.json` to `installed.json.migrated-<timestamp>`. It is never deleted.
4. Continue with the normal scan.

Migration is idempotent and best-effort per §6: a failed write leaves `installed.json` in place,
warns, and serves the command from memory, so the next writable run migrates.

A missing or empty `installed.json` with populated app directories — the current state of the
maintainer's machine — needs no special case. The scan finds `fnm` and `lazyssh` only if they
carry a record, which they do not, so `repair` reports them as untracked app directories and the
message tells the user to re-add them: `wenget add fnm`. `add` already wipes the directory before
installing, so re-adding is safe.

wenget will **not** fabricate a record for an untracked directory. A record that claims a version
and source it never verified is worse than no record; adoption stays out of scope.

Self-update is unaffected: wenget writes its own record after replacing its binary, the same as
any other package.

### 9. Root override

`WenPaths::new_with_custom_bin` honors `WENGET_ROOT` when set, in release builds as well as tests.
Without it, migration, quarantine, the collision guard, and orphaned-shim detection cannot be
exercised on the shipped binary against anything but the user's real `~/.wenget` — which is
exactly how T-1 fired. With it, every scenario in Testing is reproducible on the release binary,
and the migration path can be demonstrated against a copy of the maintainer's truncated
`installed.json` before shipping.

## Files Touched

| File | Change |
|---|---|
| `src/core/store.rs` | New. `InstalledStore` load/save/remove, staging swap, quarantine, migration. |
| `src/core/manifest.rs` | `meta_version`; `install_path` becomes `#[serde(skip)]`; `InstalledManifest` → `InstalledSet`, loses its file-format role; `migrate()` becomes migration-only, called from `store.rs`. |
| `src/core/config.rs` | `load_installed` delegates; `save_installed` removed; `get_or_create_installed` no longer inits; `init`/`is_initialized` drop the `installed.json` check. |
| `src/core/paths.rs` | `WENGET_ROOT` override; `installed_json()` retained for migration only; add `package_record_path()`, `staging_dir()`. |
| `src/commands/add.rs` | Five batched saves become per-package saves; staged install/swap; collision guard. |
| `src/commands/delete.rs` | Drop the save; directory removal is the record removal. |
| `src/commands/rename.rs` | Save only the modified package. |
| `src/commands/repair.rs` | Rewritten: per-directory status, residue sweep, provenance-based shim checks. Will not compile until rewritten. |
| `src/commands/init.rs` | Drop `installed.json` from the creation plan (`:100-103`) and the summary (`:318`). |
| `docs/reference/glossary.md` | Add Package record, App directory, Untracked app directory, Installed key; redefine "Installed package" away from `installed.json`; drop `_Avoid_: Install record`. Lands with the code, not before. |
| `README.md`, `CLAUDE.md`, `AGENTS.md` | Directory-structure sections and the "always update `installed.json`" rule. |

## Testing

Unit tests run against `WenPaths::with_root(tempdir)`; no test may touch the real `~/.wenget/`.

- **Round trip**: save a package, reload, assert the key, `executables`, and `source` survive.
- **Key reconstruction**: a package with `variant: Some("baseline")` in `apps/bun-baseline/`
  reloads under key `bun::baseline`.
- **`install_path` is derived**: a record file contains no `install_path`; after moving the app
  directory, the reloaded value points at the new location.
- **Corrupt-record isolation**: two packages, one with truncated JSON — reload yields exactly the
  healthy one, and the corrupt file is renamed rather than deleted.
- **Future version**: a record with `meta_version: 99` is skipped, still present and byte-identical
  after load.
- **Duplicate key**: two directories reconstructing one key produce a `repair` finding, not a
  silent shadow.
- **Read-only degradation**: with the root read-only, `load()` returns the packages and does not
  error.
- **Staged swap**: an extraction failure mid-install leaves the previous app directory and its
  record intact.
- **Residue skipping**: `apps/.staging/x` and `apps/foo.old-123` are not loaded as packages.
- **Delete removes the record**: `remove_dir_all` then reload yields no entry, no orphan.
- **Migration**: a fixture `installed.json` with two entries and matching directories produces two
  records and an `installed.json.migrated-*`; a second load is a no-op.
- **Migration drops dead entries**: an entry whose directory is absent does not appear after load.
- **Collision guard**: installing `foo::bar` into a directory holding `foo-bar`'s record errors,
  names both keys, and leaves the existing record intact.
- **Orphan detection is provenance-based**: a plain unrelated file and a foreign symlink in
  `bin_dir` are *not* reported; a symlink into `{root}/apps/` with no owning package is.
- **No init on read**: `list` against a nonexistent root creates nothing.
- **Atomicity**: `save_package` leaves no `.tmp` behind on success.

Manual verification on the release binary, via `WENGET_ROOT` (§9): migration from a copy of the
real truncated `installed.json`; install, rename, delete round trip; `repair` output against a
seeded untracked directory.

## Out of Scope

- Adopting untracked app directories into managed packages, in any form.
- A renaming scheme for colliding sanitized directory names (see §7, known limitation).
- **Concurrency.** wenget assumes a single writer and takes no lock. Per-package writes change the
  failure mode rather than removing it: today two concurrent mutations last-write-wins the whole
  file and one package's record vanishes; afterwards both records survive, but each process
  resolved its command name from a stale snapshot (`add.rs:1695`), so two packages can claim one
  command name, one shim wins, and two records disagree with the filesystem until `repair` runs.
  A `{root}/.wenget.lock` around mutating commands is the fix when this is worth addressing.
- Any change to `buckets.json` or `manifest-cache.json`, which are legitimately global.
