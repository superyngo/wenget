# Remove `installed.json`: Per-Package Meta Files
Status: Draft (2026-09-03)

## Problem Statement

All installed-package state lives in one file, `{root}/installed.json`, loaded and rewritten in
full by every mutating command. Three consequences:

1. **Single point of total loss.** The whole registry is serialized and written non-atomically
   (`src/core/config.rs:146-149`, `162-170`) from seven call sites (`add.rs:383,529,676,1589,1644`,
   `delete.rs:242`, `rename.rs:69`). A crash mid-write, or any bug that writes an empty manifest,
   loses the record of *every* package. This is audit finding A-1.
2. **It has already fired.** Audit finding T-1: `cargo test` truncated the maintainer's
   `installed.json` to `{"packages": {}}`. At the time of writing, `~/.wenget/installed.json` is
   20 bytes while `~/.wenget/apps/` still contains `fnm` and `lazyssh`. The packages survived;
   only the ledger did not.
3. **Records and payloads can diverge.** Deleting an app directory by hand leaves a stale entry;
   a failed manifest save after a successful extract leaves an untracked directory.

The information in each entry describes exactly one directory. Storing it anywhere else than in
that directory is what creates all three problems.

## Decision

`installed.json` is removed. Each installed package stores its own record in
`{install_path}/.wenget/package.json`. The set of installed packages is the set of app
directories containing a readable meta file. No global index file is written — not even a cache.

Rejected alternatives:

- **Keep `installed.json` as a rebuildable index cache.** Reintroduces two copies of the same
  state and the drift they permit, which is the class of bug being removed. The scan it would
  optimize costs tens of `read`s against a command that already makes network calls.
- **Keep `installed.json` authoritative, mirror a read-only copy per directory.** Leaves A-1 and
  T-1 untouched; buys only human readability.

## Preconditions Verified

- Every install path produces exactly one directory under `{root}/apps/`:
  binaries `add.rs:1743`, bucket scripts `add.rs:445`, direct scripts `add.rs:2241`,
  local files `installer/local.rs:39`, script payloads `installer/script.rs:202`.
- The installed key is recoverable from meta content, not from the directory name.
  `sanitize_path_component` (`paths.rs:38-56`) is lossy — `bun::baseline` becomes `bun-baseline` —
  but `repo_name` + `variant` are stored fields, and `generate_installed_key()`
  (`manifest.rs:922-...`) reconstructs the key from them.

## Design

### 1. On-disk format

Path: `{install_path}/.wenget/package.json`.

A `.wenget/` subdirectory, not a bare dotfile, so the meta never collides with an extracted
archive member and is trivially excluded from executable scanning.

Content is the existing `InstalledPackage` struct, serialized as today, plus one field:

```rust
/// Schema version of this meta file. Absent means version 1.
#[serde(default = "default_meta_version")]
pub meta_version: u32,
```

`install_path` is retained but becomes derived: on load it is overwritten with the directory the
meta was actually found in, so a manually moved or copied app directory self-corrects instead of
pointing at its old location.

The deprecated `command_names`, `command_name`, and `parent_package` fields keep their current
`serde` attributes; migration from `installed.json` runs the existing `migrate()` logic once, so
freshly written meta files never contain them.

### 2. `InstalledStore`

`InstalledManifest` stops being a serde DTO for one file and becomes an in-memory collection
loaded from and persisted to many. New module `src/core/store.rs`:

```rust
pub struct InstalledStore { paths: WenPaths }

impl InstalledStore {
    /// Scan {root}/apps/*/.wenget/package.json. Unreadable entries are quarantined
    /// (see §5) and skipped; one bad file never fails the load.
    pub fn load(&self) -> Result<InstalledManifest>;

    /// Write one package's meta atomically: tmp file in {install_path}/.wenget/,
    /// sync_all, rename over package.json.
    pub fn save_package(&self, key: &str, pkg: &InstalledPackage) -> Result<()>;

    /// Meta is removed with the directory; this is only for the rare case of
    /// dropping a record while keeping the payload.
    pub fn remove_package(&self, key: &str) -> Result<()>;
}
```

`InstalledManifest` keeps its current API surface (`get_package`, `upsert_package`,
`remove_package`, `group_by_repo`, `find_by_repo`, `is_command_taken`, `command_name_set`) so
read-side callers are unchanged. `packages` remains `HashMap<String, InstalledPackage>` keyed by
installed key, rebuilt during `load()` via `generate_installed_key(repo_name, variant)`.

### 3. Call-site changes

- `Config::load_installed` / `get_or_create_installed` delegate to `InstalledStore::load()`.
  Signatures and return types are unchanged, so `list.rs`, `update.rs`, `info.rs`,
  `package_resolver.rs`, and the read half of `add.rs` need no edits.
- `Config::save_installed(&InstalledManifest)` is **deleted**. The seven whole-registry writes are
  replaced by per-package writes at the point the package is installed or modified:
  - `add.rs` — after each successful install, `store.save_package(&key, &inst_pkg)`. The existing
    batching optimization is dropped: it exists only to amortize whole-file serialization, which
    no longer happens. The in-memory `installed` snapshot passed down for conflict resolution
    (`add.rs:1688-1692`) is still updated in memory and still must not be re-read from disk.
  - `delete.rs` — no meta write at all. `fs::remove_dir_all(app_dir)` (`delete.rs:294-297`)
    removes the record with the payload. Shim removal is unchanged.
  - `rename.rs` — writes only the one package whose `executables` map changed.
- `Config::init` no longer creates `installed.json`; `is_initialized` no longer tests for it.
  Initialization means: root and `apps/` exist.

### 4. Command-name reverse index

Command-name conflict resolution, `rename`, and `delete` need a command-name → package map. This
is built once per `InstalledStore::load()` from the loaded `executables` maps, exactly as
`command_name_set` does today. Cost is one `read_dir` plus one small `read` per app directory —
tens of syscalls against commands that already perform network I/O.

### 5. Failure handling

Per-package, replacing the current all-or-nothing repair:

| Condition | Behavior |
|---|---|
| App directory with no `.wenget/package.json` | Reported by `repair` as an untracked payload. Not loaded, not deleted. |
| Meta present but unparseable | Renamed to `package.json.corrupt-<timestamp>`, a Critical warning naming that one package is printed, load continues. |
| Meta parses but its directory no longer matches | `install_path` rewritten to the actual directory on load; not persisted unless the package is otherwise written. |

`wenget repair` is rewritten against this model. It reports, per app directory: OK / corrupt meta
/ missing meta, plus a new global check — **orphaned shims**: entries in `bin_dir` that no loaded
package claims. This check is new work required by the design, because with the global registry
gone, `bin_dir` is the only remaining unowned global state. `repair` reports orphans and offers
removal; it never removes without confirmation.

### 6. Directory-name collision

`sanitize_path_component` maps distinct keys onto one directory: package `foo-bar` and variant
`foo::bar` both resolve to `apps/foo-bar`. Today this is a latent bug — the two extracts already
overwrite each other while `installed.json` pretends the entries are distinct. Under this design
the second install would also overwrite the first's meta, making the collision destructive.

Guard: before extracting, if the target directory already contains a meta whose reconstructed key
differs from the key being installed, abort with an error naming both keys. Detection and refusal
only; a renaming scheme for colliding directories is out of scope.

### 7. Migration

On `InstalledStore::load()`, if `{root}/installed.json` exists:

1. Parse it with the current loader, including `migrate()` (`::` path rename, `command_names` →
   `executables` filesystem walk).
2. For each entry whose `install_path` directory exists, write `.wenget/package.json`. Entries
   whose directory is gone are dropped and listed in a one-time notice.
3. Rename `installed.json` to `installed.json.migrated-<timestamp>`. It is never deleted.
4. Continue with the normal scan.

A missing or empty `installed.json` with populated app directories — the current state of the
maintainer's machine — needs no special case: the scan finds `fnm` and `lazyssh` only if they
carry meta, which they do not, so `repair` reports them as untracked payloads and the user
reinstalls or is offered adoption. Adoption is out of scope for this spec.

Self-update is unaffected: wenget writes its own meta after replacing its binary, the same as any
other package.

## Files Touched

| File | Change |
|---|---|
| `src/core/store.rs` | New. `InstalledStore` load/save/remove, quarantine, migration. |
| `src/core/manifest.rs` | `meta_version` field; `InstalledManifest` loses its file-format role; `migrate()` becomes migration-only, called from `store.rs`. |
| `src/core/config.rs` | `load_installed` delegates; `save_installed` removed; `init`/`is_initialized` drop the `installed.json` check. |
| `src/core/paths.rs` | `installed_json()` retained for migration only; add `package_meta_path(&self, key)`. |
| `src/commands/add.rs` | Five batched saves become per-package saves; collision guard before extract. |
| `src/commands/delete.rs` | Drop the manifest save; directory removal is the record removal. |
| `src/commands/rename.rs` | Save only the modified package. |
| `src/commands/repair.rs` | Rewritten: per-directory status, orphaned-shim check. |
| `docs/reference/glossary.md` | "Installed package" no longer defined as an entry in `installed.json`. |
| `README.md`, `CLAUDE.md`, `AGENTS.md` | Directory-structure sections and the "always update `installed.json`" rule. |

## Testing

All against a `WenPaths::with_root(tempdir)`; no test may touch the real `~/.wenget/`.

- **Round trip**: save a package, reload, assert the key, `executables`, and `source` survive.
- **Key reconstruction**: a package with `variant: Some("baseline")` in `apps/bun-baseline/`
  reloads under key `bun::baseline`.
- **`install_path` self-correction**: move an app directory, reload, assert `install_path` points
  at the new location.
- **Corrupt-meta isolation**: two packages, one with truncated JSON — reload yields exactly the
  healthy one, and the corrupt file is renamed rather than deleted.
- **Delete removes the record**: `remove_dir_all` then reload yields no entry, no orphan.
- **Migration**: a fixture `installed.json` with two entries and matching directories produces two
  meta files and an `installed.json.migrated-*`; a second load is a no-op.
- **Migration drops dead entries**: an entry whose directory is absent does not appear after load.
- **Collision guard**: installing `foo::bar` into a directory holding `foo-bar`'s meta errors and
  leaves the existing meta intact.
- **Orphaned shim detection**: a shim in `bin_dir` with no owning package is reported by `repair`.
- **Atomicity**: `save_package` leaves no `.tmp` behind on success.

## Out of Scope

- Adopting untracked app directories into managed packages.
- A renaming scheme for colliding sanitized directory names.
- Any change to `buckets.json` or `manifest-cache.json`, which are legitimately global.
