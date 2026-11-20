# Architecture notes

Current behavior of the core storage and launcher layers. Module-level detail lives in each
module's `//!` docs; vocabulary in [glossary.md](glossary.md).

## Configuration and paths

- `Config` coordinates path management, preferences, and bucket/manifest caching.
- `WenPaths` manages all directory paths (user vs system level); `WENGET_ROOT` overrides the root
  (used for tests and sandboxed manual runs).
- `src/core/bucket.rs` (bucket configuration and models) and `src/commands/bucket.rs` (CLI bucket
  subcommands) are distinct modules.

## Manifest cache

- The manifest cache has a 24-hour TTL (`core::cache`). Rebuild or invalidate it
  (`wenget bucket refresh` or `Config::invalidate_cache`) after bucket changes.
- Platform detection uses fuzzy matching for binary names; see
  [RESOURCE_FILTERING_RULES.md](RESOURCE_FILTERING_RULES.md).

## Package records

- One package record per install/rename (`InstalledStore::save_package`) at
  `{app_dir}/.wenget/package.json`. Deleting an app directory removes its record with it. There is
  no global installed index ([ADR 0001](../adr/0001-no-global-installed-index.md)).
- JSON config files auto-repair on parse errors with a backup (`src/core/repair.rs`; the
  `wenget repair` command is `src/commands/repair.rs`); corrupt package records are quarantined per
  package (`src/core/store.rs`).

## Launchers

- User installs place launchers in `~/.local/bin/` (or the custom bin dir) via
  `installer::create_launcher`: Unix uses symlinks (`installer::symlink`), Windows uses `.cmd` shims
  (`installer::shim`). Shims quote paths to handle spaces.
- `wenget del self` uninstalls wenget itself, with platform-specific handling for the running
  executable.
