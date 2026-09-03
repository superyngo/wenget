# No global installed-package index

Status: accepted (2026-09-03)

wenget kept every installed-package record in one file, `{root}/installed.json`, rewritten in full
and non-atomically by every mutating command. That made the whole set of installed packages
destroyable by a single bad write, and it did not stay theoretical: audit finding T-1 records
`cargo test` truncating the maintainer's real `installed.json` to `{"packages": {}}` while
`~/.wenget/apps/` still held working installs of `fnm` and `lazyssh`. The packages survived; the
record of them did not.

We decided that each installed package owns its own record, stored inside its own app directory at
`{app_dir}/.wenget/package.json`, and that **no global index exists — not even as a cache**. The
set of installed packages is discovered by scanning `{root}/apps/` for directories carrying a
readable record. Each entry describes exactly one directory; keeping that description anywhere
else is what allowed both total loss and drift between records and the directories they describe.

## Considered options

- **Keep `installed.json` as a rebuildable index cache.** Rejected: it reintroduces two copies of
  the same state and the drift between them, which is the class of bug being removed. The scan it
  would optimize is one `read_dir` plus one small read per package, against commands that already
  make network calls.
- **Keep `installed.json` authoritative and mirror a read-only copy into each directory.**
  Rejected: leaves the single-point-of-loss and the divergence intact and buys only human
  readability.

## Consequences

- Deleting an app directory by hand is now a complete, correct uninstall of the record. There is
  no stale entry to clean up.
- `{root}/bin`'s contents (`~/.local/bin`, or `/usr/local/bin` for system installs) become the
  only remaining unowned global state, and it is shared with unrelated software. `wenget repair`
  may therefore only report or remove a bin entry whose wenget provenance it can prove — a symlink
  resolving under `{root}/apps/`, or a wenget-generated shim naming such a path. Absence from the
  loaded set is not evidence.
- Because the record lives inside the directory that installs wipe first, install and update must
  stage into a scratch directory and swap, or a failed update would destroy the record of what was
  previously there.
- No lock is taken, and per-package writes change the concurrency failure mode rather than
  removing it: concurrent installs can each claim the same command name and leave two records
  disagreeing with the filesystem.

The file format, migration path, staging protocol, and failure handling are specified in
[`docs/spec/2026-09-03-per-package-meta-design.md`](../spec/2026-09-03-per-package-meta-design.md).
