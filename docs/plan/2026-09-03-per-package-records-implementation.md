# Per-Package Records Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `subagent-driven-development` (recommended) or
> `executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.

**Goal:** Remove `{root}/installed.json` and make each installed package own its state in
`{app_dir}/.wenget/package.json`, so no single write can lose the record of every package.

**Architecture:** A new `src/core/store.rs` owns all package-record I/O. `InstalledStore::load()`
scans `{root}/apps/*/.wenget/package.json` and rebuilds the in-memory `InstalledSet`;
`save_package()` writes exactly one package's record atomically. Installs stage into
`{root}/apps/.staging/` and swap by `rename`, because every install path wipes the app directory
before writing and the record now lives inside it. `wenget repair` becomes the only place that
reports drift between records, app directories, and `bin_dir`.

**Tech Stack:** Rust 2021, `anyhow`, `serde`/`serde_json`, `chrono`, `colored`, `tempfile` (dev).
No new dependencies.

**Spec:** [`docs/spec/2026-09-03-per-package-meta-design.md`](../spec/2026-09-03-per-package-meta-design.md)
— read it before starting. Decision recorded in
[`docs/adr/0001-no-global-installed-index.md`](../adr/0001-no-global-installed-index.md).

## Global Constraints

- **Vocabulary is fixed** (spec §Vocabulary). Use these exact terms in code, output, and docs:
  **package record** (the `{app_dir}/.wenget/package.json` file), **app directory**, **untracked
  app directory**, **installed key**, **`InstalledSet`**. Never write "meta", "meta file",
  "ledger", "registry", "index", or "install record" in new code or user-facing strings. The
  struct field is `meta_version` — that one name is the on-disk key and does not change.
- **No global index is written, not even a cache.** Any commit that adds a file listing all
  packages violates ADR 0001.
- **No test may touch the real `~/.wenget/`.** Every test constructs paths via
  `WenPaths::with_root(tempdir.path().to_path_buf())`. This is the exact failure (audit T-1) that
  motivated the spec.
- **Reads never fail for a write reason** (spec §6). Quarantine renames and migration writes are
  best-effort: log one warning, continue with the in-memory result, exit 0.
- **A record with `meta_version` above the known maximum is skipped, never quarantined and never
  overwritten** (spec §1).
- **wenget never fabricates a record** for an untracked app directory (spec §8).
- **`repair` only reports or removes a `bin_dir` entry whose wenget provenance it can prove**
  (spec §6). `bin_dir` is `~/.local/bin` (user) or `/usr/local/bin` (system) — shared with
  unrelated software. Absence from the loaded set is not evidence.
- Current schema version constant: `pub const CURRENT_META_VERSION: u32 = 1;`
- After each task: `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must pass with no
  output before committing. `cargo test` must pass at the end of every task — the tree is never
  left red between commits.
- Do **not** append to `CHANGELOG.md` per task. Task 11 writes one entry for the whole change.

---

### Task 1: `WENGET_ROOT` override and record path helpers

Without this the release binary can only address the user's real `~/.wenget`, so nothing in this
plan can be verified end-to-end (spec §9). It comes first for that reason.

**Files:**
- Modify: `src/core/paths.rs:85-99` (`new_with_custom_bin`), `src/core/paths.rs:118-125`
  (`with_root`), and the path-getter block around `src/core/paths.rs:176-205`
- Test: `src/core/paths.rs` (existing `#[cfg(test)] mod tests` at the bottom of the file)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `WenPaths::with_root(root: PathBuf) -> Self` — now compiled in release builds too (the
    `#[cfg(test)]` attribute is dropped, replaced by `#[allow(dead_code)]`… no: it becomes
    reachable from `new_with_custom_bin`, so no attribute is needed).
  - `WenPaths::package_record_path(&self, key: &str) -> PathBuf` → `{app_dir}/.wenget/package.json`
  - `WenPaths::record_dir(&self, key: &str) -> PathBuf` → `{app_dir}/.wenget`
  - `WenPaths::staging_dir(&self) -> PathBuf` → `{root}/apps/.staging`
  - `WenPaths::installed_json(&self) -> PathBuf` — unchanged, retained for migration only.

- [ ] **Step 1: Write the failing tests**

Append to the `mod tests` block at the bottom of `src/core/paths.rs`:

```rust
    #[test]
    fn test_wenget_root_env_override() {
        // Serialized against other env-mutating tests by running in one test fn.
        let tmp = tempfile::TempDir::new().unwrap();
        let previous = std::env::var_os("WENGET_ROOT");

        std::env::set_var("WENGET_ROOT", tmp.path());
        let paths = WenPaths::new_with_custom_bin(None).unwrap();
        assert_eq!(paths.root(), tmp.path());
        assert_eq!(paths.bin_dir(), tmp.path().join("bin"));

        std::env::remove_var("WENGET_ROOT");
        let default_paths = WenPaths::new_with_custom_bin(None).unwrap();
        assert_ne!(default_paths.root(), tmp.path());

        if let Some(value) = previous {
            std::env::set_var("WENGET_ROOT", value);
        }
    }

    #[test]
    fn test_record_path_helpers() {
        let tmp = tempfile::TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());

        assert_eq!(
            paths.package_record_path("bun::baseline"),
            tmp.path()
                .join("apps")
                .join("bun-baseline")
                .join(".wenget")
                .join("package.json")
        );
        assert_eq!(
            paths.record_dir("bun::baseline"),
            tmp.path().join("apps").join("bun-baseline").join(".wenget")
        );
        assert_eq!(paths.staging_dir(), tmp.path().join("apps").join(".staging"));
    }
```

If `WenPaths` has no `root()` getter, add one next to the other getters:

```rust
    /// Get the root directory
    pub fn root(&self) -> &Path {
        &self.root
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib core::paths::tests::test_wenget_root_env_override core::paths::tests::test_record_path_helpers`
Expected: FAIL — compile error, `no function or associated item named package_record_path`, and
`with_root` not found outside test builds is *not* an issue here (tests are a test build), but the
env override assertion fails because `WENGET_ROOT` is ignored.

- [ ] **Step 3: Implement the override**

Replace `new_with_custom_bin` (`src/core/paths.rs:85-99`):

```rust
    /// Create a new WenPaths instance with optional custom bin directory
    ///
    /// `WENGET_ROOT`, when set and non-empty, overrides both the root and the bin
    /// directory. This exists so migration, quarantine, the collision guard, and
    /// orphaned-shim detection can be exercised on the shipped binary without
    /// pointing it at the user's real `~/.wenget/`.
    pub fn new_with_custom_bin(custom_bin_dir: Option<PathBuf>) -> Result<Self> {
        if let Some(root) = std::env::var_os("WENGET_ROOT") {
            if !root.is_empty() {
                let mut paths = Self::with_root(PathBuf::from(root));
                if let Some(bin) = custom_bin_dir {
                    paths.custom_bin_dir = Some(bin);
                }
                return Ok(paths);
            }
        }

        let is_system = is_elevated();

        let root = if is_system {
            Self::system_root_path()
        } else {
            Self::user_root_path()?
        };

        Ok(Self {
            root,
            is_system_install: is_system,
            custom_bin_dir,
        })
    }
```

Drop the `#[cfg(test)]` on `with_root` (`src/core/paths.rs:118`) so release builds can use it, and
extend its doc comment:

```rust
    /// Create a WenPaths instance rooted at an arbitrary directory
    ///
    /// Used by tests and by the `WENGET_ROOT` override, so both operate on a
    /// self-contained layout instead of the real `~/.wenget/`. `bin_dir` also
    /// resolves under `root`.
    pub fn with_root(root: PathBuf) -> Self {
```

- [ ] **Step 4: Implement the path helpers**

Insert after `app_bin_dir` (`src/core/paths.rs:205`):

```rust
    /// Get the `.wenget` directory holding a package's record
    pub fn record_dir(&self, key: &str) -> PathBuf {
        self.app_dir(key).join(".wenget")
    }

    /// Get a package's record path: `{app_dir}/.wenget/package.json`
    pub fn package_record_path(&self, key: &str) -> PathBuf {
        self.record_dir(key).join("package.json")
    }

    /// Get the staging directory used by install/update swaps
    ///
    /// Lives under `apps/` so the swap is a same-filesystem `rename`. The leading
    /// dot keeps it out of the app-directory scan.
    pub fn staging_dir(&self) -> PathBuf {
        self.apps_dir().join(".staging")
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib core::paths`
Expected: PASS, all tests in the module.

- [ ] **Step 6: Verify on the real binary**

```bash
cargo build
WENGET_ROOT=/tmp/wenget-probe ./target/debug/wenget list
ls -la /tmp/wenget-probe 2>/dev/null || echo "root not created by list (expected until Task 8)"
```
Expected: the command runs against `/tmp/wenget-probe`, not `~/.wenget`. Until Task 8 it may still
create the root; that is fine here.

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/core/paths.rs
git commit -m "feat(paths): honor WENGET_ROOT; add package-record and staging path helpers"
```

---

### Task 2: Rename `InstalledManifest` to `InstalledSet`

Purely mechanical, whole-tree, zero behavior change. It is its own commit so that the substantive
commits later are not buried in a rename diff. The rename matters because "manifest" already means
a bucket's `manifest.json` (spec §Vocabulary).

**Files:**
- Modify: `src/core/manifest.rs:473-478` (struct + doc comment), `src/core/manifest.rs:480`
  (`impl` block), and every reference across `src/core/config.rs`, `src/core/mod.rs`,
  `src/commands/add.rs`, `src/commands/delete.rs`, `src/commands/rename.rs`,
  `src/commands/list.rs`, `src/commands/update.rs`, `src/commands/info.rs`,
  `src/commands/repair.rs`, `src/package_resolver.rs`
- Test: existing tests only; they are renamed with everything else.

**Interfaces:**
- Consumes: nothing.
- Produces: `InstalledSet` — same fields (`packages: HashMap<String, InstalledPackage>`) and same
  methods (`new`, `is_installed`, `get_package`, `upsert_package`, `remove_package`,
  `installed_names`, `group_by_repo`, `find_by_repo`, `is_command_taken`, `command_name_set`,
  `migrate`) as `InstalledManifest` had. Every later task uses this name.

- [ ] **Step 1: Find every reference**

Run: `rg -n 'InstalledManifest' src/ --stats`
Expected: a list of files and a total count. Note the count; you will assert it reaches zero.

- [ ] **Step 2: Rename**

Replace the type name everywhere. Do **not** introduce a `pub type InstalledManifest = InstalledSet;`
alias — a half-renamed vocabulary is worse than either name.

```bash
rg -l 'InstalledManifest' src/ | xargs sed -i '' 's/InstalledManifest/InstalledSet/g'
```

(On Linux use `sed -i` without the `''`.)

Then fix the doc comment at `src/core/manifest.rs:473`, which still describes a file:

```rust
/// The set of installed packages, loaded from per-package records
///
/// This is an in-memory collection, not a file format: each package's state is
/// stored in its own `{app_dir}/.wenget/package.json` (see `src/core/store.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSet {
```

- [ ] **Step 3: Verify the rename is complete and behavior-neutral**

Run: `rg -n 'InstalledManifest' src/ ; cargo test`
Expected: no matches from `rg`; all tests PASS with no test-body changes other than the type name.

- [ ] **Step 4: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add -A src/
git commit -m "refactor(core): rename InstalledManifest to InstalledSet

Manifest means a bucket's manifest.json. The in-memory collection of
installed packages is a set, and after per-package records it is not a
file format at all."
```

---

### Task 3: `meta_version` on the record

**Files:**
- Modify: `src/core/manifest.rs:394-451` (`InstalledPackage`)
- Test: `src/core/manifest.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const CURRENT_META_VERSION: u32 = 1;` in `src/core/manifest.rs`
  - `InstalledPackage::meta_version: u32`, defaulting to 1 when the key is absent
  - `fn default_meta_version() -> u32`

`install_path` stays serialized in this task. It becomes `#[serde(skip)]` only in Task 8, when
`InstalledStore` is wired in to populate it — flipping it earlier would blank the field for every
read path while `installed.json` is still authoritative.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn test_meta_version_defaults_to_one_when_absent() {
        let json = r#"{
            "repo_name": "ripgrep",
            "version": "14.0.0",
            "platform": "linux-x86_64",
            "installed_at": "2026-01-01T00:00:00Z",
            "install_path": "/tmp/apps/ripgrep",
            "executables": {"rg": "rg"},
            "source": {"type": "bucket", "name": "main"},
            "description": "search tool",
            "asset_name": "ripgrep-linux.tar.gz"
        }"#;

        let pkg: InstalledPackage = serde_json::from_str(json).unwrap();
        assert_eq!(pkg.meta_version, 1);
        assert_eq!(pkg.meta_version, CURRENT_META_VERSION);
    }

    #[test]
    fn test_meta_version_round_trips() {
        let json = serde_json::to_string(&sample_package()).unwrap();
        assert!(json.contains("\"meta_version\":1"));
        let back: InstalledPackage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.meta_version, 1);
    }
```

`sample_package()` does not exist yet. Add it to the test module — later tasks reuse it:

```rust
    /// A minimal, valid installed package for tests.
    fn sample_package() -> InstalledPackage {
        InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: "ripgrep".to_string(),
            variant: None,
            version: "14.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: "/tmp/apps/ripgrep".to_string(),
            executables: HashMap::from([("bin/rg".to_string(), "rg".to_string())]),
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "search tool".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "ripgrep-linux.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        }
    }
```

If the `source` JSON shape in the first test does not match `PackageSource`'s serde
representation, run `cargo test test_meta_version_round_trips -- --nocapture` first, print the
serialized form, and use that shape verbatim in the hand-written JSON. Do not guess.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::manifest::tests::test_meta_version`
Expected: FAIL — `struct InstalledPackage has no field named meta_version`.

- [ ] **Step 3: Implement**

Add above `InstalledPackage` in `src/core/manifest.rs`:

```rust
/// Highest package-record schema version this build understands.
///
/// A record above this version is skipped on load, never rewritten: a downgrade
/// must be read-only toward data it cannot interpret.
pub const CURRENT_META_VERSION: u32 = 1;

fn default_meta_version() -> u32 {
    1
}
```

Add as the first field of `InstalledPackage` (before `repo_name`, `src/core/manifest.rs:397`):

```rust
    /// Schema version of this package record. Absent means version 1.
    #[serde(default = "default_meta_version")]
    pub meta_version: u32,
```

Then fix every struct-literal construction of `InstalledPackage` the compiler reports (in
`src/commands/add.rs`, `src/installer/local.rs`, `src/installer/script.rs`, and existing tests) by
adding `meta_version: CURRENT_META_VERSION,` as the first field.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib core::manifest`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add -A src/
git commit -m "feat(core): add meta_version to installed package records"
```

---

### Task 4: `InstalledStore` load and save

The core of the change. Not wired into `Config` yet — this task lands a tested module that nothing
calls, which keeps the wiring commit (Task 8) reviewable.

**Files:**
- Create: `src/core/store.rs`
- Modify: `src/core/mod.rs` (add `pub mod store;` and re-export `InstalledStore`)
- Test: `src/core/store.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `WenPaths::{app_dir, apps_dir, package_record_path, record_dir, staging_dir}` (Task 1),
  `InstalledPackage`, `InstalledSet`, `CURRENT_META_VERSION`, `generate_installed_key` (Tasks 2-3).
- Produces:
  - `InstalledStore::new(paths: WenPaths) -> Self`
  - `InstalledStore::load(&self) -> Result<InstalledSet>`
  - `InstalledStore::save_package(&self, key: &str, pkg: &InstalledPackage) -> Result<()>`
  - `InstalledStore::remove_package(&self, key: &str) -> Result<()>`
  - `InstalledStore::scan_app_dirs(&self) -> Result<Vec<ScanEntry>>` and
    `pub enum ScanEntry { Loaded { key: String, package: InstalledPackage, dir: PathBuf },
    Untracked(PathBuf), Corrupt(PathBuf), FutureVersion { dir: PathBuf, version: u32 },
    Residue(PathBuf) }` — Task 10's `repair` renders these variants; `load()` keeps only `Loaded`.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::manifest::{PackageSource, CURRENT_META_VERSION};
    use chrono::Utc;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn store(tmp: &TempDir) -> InstalledStore {
        InstalledStore::new(WenPaths::with_root(tmp.path().to_path_buf()))
    }

    fn pkg(repo: &str, variant: Option<&str>) -> InstalledPackage {
        InstalledPackage {
            meta_version: CURRENT_META_VERSION,
            repo_name: repo.to_string(),
            variant: variant.map(|v| v.to_string()),
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: Utc::now(),
            install_path: String::new(),
            executables: HashMap::from([("bin/x".to_string(), repo.to_string())]),
            source: PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "d".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "x.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        }
    }

    #[test]
    fn test_round_trip_preserves_key_executables_and_source() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("ripgrep", &pkg("ripgrep", None)).unwrap();

        let set = s.load().unwrap();
        let loaded = set.get_package("ripgrep").expect("package should load");
        assert_eq!(loaded.executables.get("bin/x").map(String::as_str), Some("ripgrep"));
        assert!(matches!(loaded.source, PackageSource::Bucket { .. }));
    }

    #[test]
    fn test_key_is_reconstructed_from_record_not_directory_name() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("bun::baseline", &pkg("bun", Some("baseline"))).unwrap();

        // The directory is the lossy `bun-baseline`...
        assert!(tmp.path().join("apps").join("bun-baseline").exists());
        // ...but the key comes back intact.
        let set = s.load().unwrap();
        assert!(set.get_package("bun::baseline").is_some());
        assert!(set.get_package("bun-baseline").is_none());
    }

    #[test]
    fn test_install_path_is_derived_not_stored() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("ripgrep", &pkg("ripgrep", None)).unwrap();

        let raw = std::fs::read_to_string(
            s.paths().package_record_path("ripgrep")
        ).unwrap();
        assert!(!raw.contains("install_path"), "record must not store its own location");

        let set = s.load().unwrap();
        assert_eq!(
            set.get_package("ripgrep").unwrap().install_path,
            tmp.path().join("apps").join("ripgrep").to_string_lossy()
        );
    }

    #[test]
    fn test_corrupt_record_is_quarantined_and_isolated() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("good", &pkg("good", None)).unwrap();
        s.save_package("bad", &pkg("bad", None)).unwrap();
        std::fs::write(s.paths().package_record_path("bad"), "{ truncated").unwrap();

        let set = s.load().unwrap();
        assert!(set.get_package("good").is_some());
        assert!(set.get_package("bad").is_none());
        assert!(!s.paths().package_record_path("bad").exists());

        let quarantined: Vec<_> = std::fs::read_dir(s.paths().record_dir("bad"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("package.json.corrupt-"))
            .collect();
        assert_eq!(quarantined.len(), 1, "corrupt record is renamed, never deleted");
    }

    #[test]
    fn test_future_version_is_skipped_untouched() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("futurepkg", &pkg("futurepkg", None)).unwrap();
        let path = s.paths().package_record_path("futurepkg");
        let mut value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        value["meta_version"] = serde_json::json!(99);
        let before = serde_json::to_string_pretty(&value).unwrap();
        std::fs::write(&path, &before).unwrap();

        let set = s.load().unwrap();
        assert!(set.get_package("futurepkg").is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn test_residue_is_not_loaded_as_a_package() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("real", &pkg("real", None)).unwrap();

        // Staging residue carrying a valid record.
        let staged = s.paths().staging_dir().join("real-123");
        std::fs::create_dir_all(staged.join(".wenget")).unwrap();
        std::fs::copy(
            s.paths().package_record_path("real"),
            staged.join(".wenget").join("package.json"),
        )
        .unwrap();
        // Old-install residue, likewise.
        let old = tmp.path().join("apps").join("real.old-123");
        std::fs::create_dir_all(old.join(".wenget")).unwrap();
        std::fs::copy(
            s.paths().package_record_path("real"),
            old.join(".wenget").join("package.json"),
        )
        .unwrap();

        let set = s.load().unwrap();
        assert_eq!(set.packages.len(), 1);
    }

    #[test]
    fn test_untracked_directory_is_not_loaded_and_not_deleted() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let orphan = tmp.path().join("apps").join("fnm");
        std::fs::create_dir_all(&orphan).unwrap();
        std::fs::write(orphan.join("fnm"), b"binary").unwrap();

        let set = s.load().unwrap();
        assert!(set.packages.is_empty());
        assert!(orphan.join("fnm").exists(), "untracked files are left alone");
    }

    #[test]
    fn test_duplicate_key_keeps_first_and_reports() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("dup", &pkg("dup", None)).unwrap();
        // A second directory whose record reconstructs the same key.
        let second = tmp.path().join("apps").join("dup-copy");
        std::fs::create_dir_all(second.join(".wenget")).unwrap();
        std::fs::copy(
            s.paths().package_record_path("dup"),
            second.join(".wenget").join("package.json"),
        )
        .unwrap();

        let set = s.load().unwrap();
        assert_eq!(set.packages.len(), 1, "no silent shadowing");

        let dup_dirs = s.duplicate_keys().unwrap();
        assert_eq!(dup_dirs.len(), 1);
        assert_eq!(dup_dirs[0].0, "dup");
        assert_eq!(dup_dirs[0].1.len(), 2, "both directories are reported");
    }

    #[test]
    fn test_load_on_missing_root_is_empty_and_creates_nothing() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("absent");
        let s = InstalledStore::new(WenPaths::with_root(root.clone()));

        assert!(s.load().unwrap().packages.is_empty());
        assert!(!root.exists(), "load must not initialize anything");
    }

    #[test]
    fn test_save_leaves_no_tmp_file() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("atomic", &pkg("atomic", None)).unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(s.paths().record_dir("atomic"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn test_remove_package_drops_record_only() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("gone", &pkg("gone", None)).unwrap();
        std::fs::write(tmp.path().join("apps").join("gone").join("payload"), b"x").unwrap();

        s.remove_package("gone").unwrap();
        assert!(s.load().unwrap().packages.is_empty());
        assert!(tmp.path().join("apps").join("gone").join("payload").exists());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::store`
Expected: FAIL — `file not found for module store` / `cannot find struct InstalledStore`.

- [ ] **Step 3: Implement `src/core/store.rs`**

```rust
//! Per-package record storage
//!
//! Each installed package owns its state in `{app_dir}/.wenget/package.json` —
//! its **package record**. The set of installed packages is the set of app
//! directories carrying a readable record; no global index exists (ADR 0001).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;
use colored::Colorize;

use crate::core::manifest::{
    generate_installed_key, InstalledPackage, InstalledSet, CURRENT_META_VERSION,
};
use crate::core::paths::WenPaths;

/// One entry found while scanning `{root}/apps/`
#[derive(Debug)]
pub enum ScanEntry {
    /// A directory with a readable, understood record
    Loaded {
        key: String,
        package: InstalledPackage,
        dir: PathBuf,
    },
    /// A directory under `apps/` with no package record
    Untracked(PathBuf),
    /// A record that could not be parsed; already quarantined by the scan
    Corrupt(PathBuf),
    /// A record written by a newer wenget; skipped and left untouched
    FutureVersion { dir: PathBuf, version: u32 },
    /// `apps/.staging/...` or `apps/*.old-*` left by an interrupted swap
    Residue(PathBuf),
}

/// Reads and writes package records
pub struct InstalledStore {
    paths: WenPaths,
}

impl InstalledStore {
    pub fn new(paths: WenPaths) -> Self {
        Self { paths }
    }

    pub fn paths(&self) -> &WenPaths {
        &self.paths
    }

    /// Load every readable package record under `{root}/apps/`
    ///
    /// One unreadable record never fails the load: it is quarantined and skipped.
    /// Performs no writes to `bin_dir` and never re-links a shim — drift is a
    /// `repair` finding, not a load-time repair.
    pub fn load(&self) -> Result<InstalledSet> {
        let mut set = InstalledSet::new();

        for entry in self.scan_app_dirs()? {
            if let ScanEntry::Loaded { key, package, .. } = entry {
                if set.packages.contains_key(&key) {
                    log::warn!(
                        "Two app directories describe package '{}'; keeping the first. \
                         Run `wenget repair` for details.",
                        key
                    );
                    continue;
                }
                set.upsert_package(key, package);
            }
        }

        Ok(set)
    }

    /// Classify every entry under `{root}/apps/`
    pub fn scan_app_dirs(&self) -> Result<Vec<ScanEntry>> {
        let apps_dir = self.paths.apps_dir();
        if !apps_dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        let read_dir = fs::read_dir(&apps_dir)
            .with_context(|| format!("Failed to read {}", apps_dir.display()))?;

        for dir_entry in read_dir {
            let dir_entry = match dir_entry {
                Ok(e) => e,
                Err(e) => {
                    log::warn!("Skipping unreadable entry under apps/: {}", e);
                    continue;
                }
            };

            let path = dir_entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = dir_entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || is_old_install_residue(&name) {
                entries.push(ScanEntry::Residue(path));
                continue;
            }

            entries.push(self.classify(&path));
        }

        Ok(entries)
    }

    fn classify(&self, dir: &Path) -> ScanEntry {
        let record_path = dir.join(".wenget").join("package.json");
        if !record_path.exists() {
            return ScanEntry::Untracked(dir.to_path_buf());
        }

        let content = match fs::read_to_string(&record_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Failed to read {}: {}", record_path.display(), e);
                return ScanEntry::Untracked(dir.to_path_buf());
            }
        };

        // Read the version before deserializing: a record from a newer wenget
        // must be skipped, not treated as corrupt.
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
            let version = value
                .get("meta_version")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            if version > CURRENT_META_VERSION {
                log::warn!(
                    "Skipping {}: package record version {} is newer than this wenget understands",
                    dir.display(),
                    version
                );
                return ScanEntry::FutureVersion {
                    dir: dir.to_path_buf(),
                    version,
                };
            }
        }

        match serde_json::from_str::<InstalledPackage>(&content) {
            Ok(mut package) => {
                package.install_path = dir.to_string_lossy().to_string();
                let key = generate_installed_key(&package.repo_name, package.variant.as_deref());
                ScanEntry::Loaded {
                    key,
                    package,
                    dir: dir.to_path_buf(),
                }
            }
            Err(e) => {
                self.quarantine(&record_path, &e.to_string());
                ScanEntry::Corrupt(dir.to_path_buf())
            }
        }
    }

    /// Rename an unparseable record aside. Best-effort: a read-only root logs and
    /// continues rather than failing the command.
    fn quarantine(&self, record_path: &Path, reason: &str) {
        let target = record_path.with_file_name(format!(
            "package.json.corrupt-{}",
            Utc::now().format("%Y%m%d%H%M%S")
        ));

        eprintln!(
            "{} Package record {} is corrupted ({}).",
            "Critical:".red().bold(),
            record_path.display(),
            reason
        );

        match fs::rename(record_path, &target) {
            Ok(()) => eprintln!("  Saved aside as {}", target.display()),
            Err(e) => log::warn!("Could not quarantine {}: {}", record_path.display(), e),
        }
    }

    /// Write one package's record atomically
    pub fn save_package(&self, key: &str, pkg: &InstalledPackage) -> Result<()> {
        let record_dir = self.paths.record_dir(key);
        fs::create_dir_all(&record_dir)
            .with_context(|| format!("Failed to create {}", record_dir.display()))?;

        let final_path = record_dir.join("package.json");
        write_record_atomically(&final_path, pkg)
    }

    /// Drop a record while keeping the files
    ///
    /// Uninstall does not use this: `remove_dir_all` on the app directory removes
    /// the record with the payload.
    pub fn remove_package(&self, key: &str) -> Result<()> {
        let record_path = self.paths.package_record_path(key);
        if record_path.exists() {
            fs::remove_file(&record_path)
                .with_context(|| format!("Failed to remove {}", record_path.display()))?;
        }
        Ok(())
    }

    /// Installed keys claimed by more than one app directory
    pub fn duplicate_keys(&self) -> Result<Vec<(String, Vec<PathBuf>)>> {
        let mut by_key: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for entry in self.scan_app_dirs()? {
            if let ScanEntry::Loaded { key, dir, .. } = entry {
                by_key.entry(key).or_default().push(dir);
            }
        }
        Ok(by_key.into_iter().filter(|(_, dirs)| dirs.len() > 1).collect())
    }
}

/// `foo.old-1730000000` — residue from an interrupted swap
fn is_old_install_residue(name: &str) -> bool {
    name.rsplit_once(".old-")
        .is_some_and(|(_, stamp)| !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()))
}

/// Serialize to a sibling `.tmp`, fsync, then rename over the target
pub(crate) fn write_record_atomically(final_path: &Path, pkg: &InstalledPackage) -> Result<()> {
    use std::io::Write;

    let tmp_path = final_path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(pkg).context("Failed to serialize package record")?;

    {
        let mut file = fs::File::create(&tmp_path)
            .with_context(|| format!("Failed to create {}", tmp_path.display()))?;
        file.write_all(json.as_bytes())
            .with_context(|| format!("Failed to write {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to flush {}", tmp_path.display()))?;
    }

    fs::rename(&tmp_path, final_path).with_context(|| {
        format!(
            "Failed to move {} into place at {}",
            tmp_path.display(),
            final_path.display()
        )
    })?;

    Ok(())
}
```

Note `with_extension("json.tmp")` on a path ending in `package.json` yields `package.json.tmp`
only if the file stem is `package`; verify with the `test_save_leaves_no_tmp_file` test and, if the
name comes out wrong, build the name explicitly with `final_path.with_file_name("package.json.tmp")`.

Add to `src/core/mod.rs`, following the existing module/re-export style in that file:

```rust
pub mod store;

pub use store::InstalledStore;
```

- [ ] **Step 4: Make `install_path` non-serialized for records only**

`test_install_path_is_derived_not_stored` fails until Task 8 flips the field. Mark it now so the
task is honest about it:

```rust
    #[test]
    #[ignore = "unignored in Task 8, when install_path becomes #[serde(skip)]"]
    fn test_install_path_is_derived_not_stored() {
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib core::store`
Expected: PASS, with one test reported as ignored.

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/core/store.rs src/core/mod.rs
git commit -m "feat(core): add InstalledStore for per-package records

Load scans apps/*/.wenget/package.json, reconstructs the installed key
from record content, quarantines unparseable records, and skips records
from a newer schema version. Not wired in yet."
```

---

### Task 5: Migration from `installed.json`

**Files:**
- Modify: `src/core/store.rs`
- Test: `src/core/store.rs` (`mod tests`)

**Interfaces:**
- Consumes: `InstalledStore` (Task 4), `WenPaths::installed_json`, `InstalledSet::migrate`.
- Produces: `InstalledStore::migrate_legacy(&self) -> Result<()>`, called at the top of `load()`.

- [ ] **Step 1: Write the failing tests**

```rust
    /// Writes a legacy installed.json holding `keys`, creating a matching app
    /// directory for each key in `with_dirs`.
    fn seed_legacy(tmp: &TempDir, keys: &[&str], with_dirs: &[&str]) {
        let root = tmp.path();
        std::fs::create_dir_all(root.join("apps")).unwrap();

        let mut packages = serde_json::Map::new();
        for key in keys {
            let (repo, variant) = match key.split_once("::") {
                Some((r, v)) => (r, Some(v)),
                None => (*key, None),
            };
            let dir = root.join("apps").join(key.replace("::", "-"));
            let mut p = pkg(repo, variant);
            p.install_path = dir.to_string_lossy().to_string();
            packages.insert(key.to_string(), serde_json::to_value(&p).unwrap());
        }
        for key in with_dirs {
            std::fs::create_dir_all(root.join("apps").join(key.replace("::", "-"))).unwrap();
        }

        std::fs::write(
            root.join("installed.json"),
            serde_json::to_string_pretty(&serde_json::json!({ "packages": packages })).unwrap(),
        )
        .unwrap();
    }

    fn migrated_marker(tmp: &TempDir) -> Option<String> {
        std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|n| n.starts_with("installed.json.migrated-"))
    }

    #[test]
    fn test_migration_writes_records_and_retires_the_file() {
        let tmp = TempDir::new().unwrap();
        seed_legacy(&tmp, &["fnm", "bun::baseline"], &["fnm", "bun::baseline"]);
        let s = store(&tmp);

        let set = s.load().unwrap();
        assert!(set.get_package("fnm").is_some());
        assert!(set.get_package("bun::baseline").is_some());
        assert!(s.paths().package_record_path("fnm").exists());
        assert!(s.paths().package_record_path("bun::baseline").exists());
        assert!(!tmp.path().join("installed.json").exists());
        assert!(migrated_marker(&tmp).is_some(), "the old file is renamed, never deleted");

        // Idempotent: a second load changes nothing and still sees both packages.
        let again = s.load().unwrap();
        assert_eq!(again.packages.len(), 2);
    }

    #[test]
    fn test_migration_drops_entries_whose_directory_is_gone() {
        let tmp = TempDir::new().unwrap();
        seed_legacy(&tmp, &["alive", "dead"], &["alive"]);
        let s = store(&tmp);

        let set = s.load().unwrap();
        assert!(set.get_package("alive").is_some());
        assert!(set.get_package("dead").is_none());
        assert!(!s.paths().package_record_path("dead").exists());
    }

    #[test]
    fn test_empty_legacy_file_with_untracked_dirs_needs_no_special_case() {
        // The maintainer's actual machine: 20-byte installed.json, populated apps/.
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("apps").join("fnm")).unwrap();
        std::fs::write(tmp.path().join("installed.json"), r#"{"packages": {}}"#).unwrap();
        let s = store(&tmp);

        let set = s.load().unwrap();
        assert!(set.packages.is_empty(), "wenget never fabricates a record");
        assert!(tmp.path().join("apps").join("fnm").exists());
        assert!(migrated_marker(&tmp).is_some());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::store::tests::test_migration test_empty_legacy_file`
Expected: FAIL — records are absent and `installed.json` is still in place.

- [ ] **Step 3: Implement**

Add to `impl InstalledStore` and call it as the first statement of `load()`:

```rust
    pub fn load(&self) -> Result<InstalledSet> {
        self.migrate_legacy();
        // ... existing scan loop unchanged
```

```rust
    /// Convert a legacy `{root}/installed.json` into per-package records, once
    ///
    /// Best-effort by design (spec §6): if any write fails, `installed.json` stays
    /// in place, one warning is logged, and the command proceeds from whatever the
    /// scan finds. The next writable run migrates.
    fn migrate_legacy(&self) {
        let legacy_path = self.paths.installed_json();
        if !legacy_path.exists() {
            return;
        }

        let content = match fs::read_to_string(&legacy_path) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Could not read {}: {}", legacy_path.display(), e);
                return;
            }
        };

        let mut legacy: InstalledSet = match serde_json::from_str(&content) {
            Ok(set) => set,
            Err(e) => {
                eprintln!(
                    "{} {} could not be parsed ({}); leaving it in place.",
                    "Warning:".yellow().bold(),
                    legacy_path.display(),
                    e
                );
                return;
            }
        };

        // Run the historical fixups (`::` path rename, command_names -> executables)
        // exactly once, here, so freshly written records never carry legacy fields.
        legacy.migrate();

        let mut written = 0usize;
        let mut dropped: Vec<String> = Vec::new();
        let mut failed = false;

        for (key, package) in &legacy.packages {
            let app_dir = if package.install_path.is_empty() {
                self.paths.app_dir(key)
            } else {
                PathBuf::from(&package.install_path)
            };

            if !app_dir.exists() {
                dropped.push(key.clone());
                continue;
            }

            let record_path = app_dir.join(".wenget").join("package.json");
            if record_path.exists() {
                continue;
            }

            let mut to_write = package.clone();
            to_write.meta_version = CURRENT_META_VERSION;

            if let Err(e) = fs::create_dir_all(app_dir.join(".wenget"))
                .and_then(|()| Ok(write_record_atomically(&record_path, &to_write)))
                .and_then(|r| r.map_err(std::io::Error::other))
            {
                log::warn!("Could not write record for '{}': {}", key, e);
                failed = true;
                continue;
            }
            written += 1;
        }

        if failed {
            eprintln!(
                "{} Some package records could not be written; {} is left in place \
                 and migration will retry on the next run.",
                "Warning:".yellow().bold(),
                legacy_path.display()
            );
            return;
        }

        let retired = legacy_path.with_file_name(format!(
            "installed.json.migrated-{}",
            Utc::now().format("%Y%m%d%H%M%S")
        ));
        if let Err(e) = fs::rename(&legacy_path, &retired) {
            log::warn!("Could not retire {}: {}", legacy_path.display(), e);
            return;
        }

        println!(
            "{} Migrated {} package(s) to per-package records. Previous file saved as {}.",
            "ℹ".cyan(),
            written,
            retired.display()
        );
        if !dropped.is_empty() {
            let mut names = dropped.clone();
            names.sort();
            println!(
                "  {} entr(ies) had no install directory and were dropped: {}",
                names.len(),
                names.join(" ")
            );
        }
    }
```

If the `and_then` chain above fights the type checker, write it plainly instead — the requirement
is only that a failed write sets `failed = true` and does not abort the loop:

```rust
            if let Err(e) = fs::create_dir_all(app_dir.join(".wenget")) {
                log::warn!("Could not create record dir for '{}': {}", key, e);
                failed = true;
                continue;
            }
            if let Err(e) = write_record_atomically(&record_path, &to_write) {
                log::warn!("Could not write record for '{}': {}", key, e);
                failed = true;
                continue;
            }
            written += 1;
```

- [ ] **Step 4: Add the read-only degradation test**

```rust
    #[test]
    #[cfg(unix)]
    fn test_read_only_root_still_loads() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("ro", &pkg("ro", None)).unwrap();
        // A corrupt second record: quarantine will be refused, load must not fail.
        s.save_package("broken", &pkg("broken", None)).unwrap();
        std::fs::write(s.paths().package_record_path("broken"), "{ nope").unwrap();

        let record_dir = s.paths().record_dir("broken");
        let original = std::fs::metadata(&record_dir).unwrap().permissions();
        let mut locked = original.clone();
        locked.set_mode(0o500);
        std::fs::set_permissions(&record_dir, locked).unwrap();

        let result = s.load();

        std::fs::set_permissions(&record_dir, original).unwrap();

        let set = result.expect("load must not fail because a write was refused");
        assert!(set.get_package("ro").is_some());
    }
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib core::store`
Expected: PASS (one ignored).

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/core/store.rs
git commit -m "feat(core): migrate installed.json into per-package records on load

Idempotent and best-effort: entries whose directory is gone are dropped
and listed, the old file is renamed rather than deleted, and a refused
write leaves it in place for the next run."
```

---

### Task 6: Stage-and-swap installs

Must land **before** the record becomes authoritative (Task 8): every install path calls
`remove_dir_all(&app_dir)` before writing (`add.rs:1748-1749`, `installer/local.rs:48-49`), so a
colocated record would be destroyed at the start of every update (spec §2).

**Files:**
- Create: `src/installer/staging.rs`
- Modify: `src/installer/mod.rs` (declare and re-export), `src/commands/add.rs:1742-1752`
  (binary install), `src/installer/local.rs:39-52`, `src/installer/script.rs` around `:202`
- Test: `src/installer/staging.rs`

**Interfaces:**
- Consumes: `WenPaths::{app_dir, staging_dir}` (Task 1).
- Produces:
  - `pub struct StagedInstall { staging_path: PathBuf, app_dir: PathBuf }`
  - `StagedInstall::begin(paths: &WenPaths, key: &str) -> Result<StagedInstall>` — creates a fresh
    empty `{root}/apps/.staging/{sanitized-key}-{pid}` and returns the handle
  - `StagedInstall::path(&self) -> &Path` — extract here, write the record here
  - `StagedInstall::commit(self) -> Result<PathBuf>` — swaps into place, returns the app directory
  - `Drop` removes an uncommitted staging directory

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_commit_swaps_staged_content_into_place() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());

        let staged = StagedInstall::begin(&paths, "tool").unwrap();
        std::fs::write(staged.path().join("tool"), b"new").unwrap();
        let app_dir = staged.commit().unwrap();

        assert_eq!(app_dir, paths.app_dir("tool"));
        assert_eq!(std::fs::read(app_dir.join("tool")).unwrap(), b"new");
        assert!(!paths.staging_dir().join("tool").exists());
    }

    #[test]
    fn test_commit_replaces_a_previous_install_completely() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        let app_dir = paths.app_dir("tool");
        std::fs::create_dir_all(&app_dir).unwrap();
        std::fs::write(app_dir.join("stale"), b"old").unwrap();

        let staged = StagedInstall::begin(&paths, "tool").unwrap();
        std::fs::write(staged.path().join("tool"), b"new").unwrap();
        staged.commit().unwrap();

        assert!(!app_dir.join("stale").exists(), "old payload must not survive");
        assert!(app_dir.join("tool").exists());

        let residue: Vec<_> = std::fs::read_dir(paths.apps_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".old-"))
            .collect();
        assert!(residue.is_empty(), "committed swaps clean up their own .old- dir");
    }

    #[test]
    fn test_abandoned_staging_leaves_the_previous_install_intact() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        let app_dir = paths.app_dir("tool");
        std::fs::create_dir_all(app_dir.join(".wenget")).unwrap();
        std::fs::write(app_dir.join(".wenget").join("package.json"), b"{}").unwrap();
        std::fs::write(app_dir.join("tool"), b"old").unwrap();

        {
            let staged = StagedInstall::begin(&paths, "tool").unwrap();
            std::fs::write(staged.path().join("half"), b"partial").unwrap();
            // Dropped without commit, as an extraction failure would.
        }

        assert_eq!(std::fs::read(app_dir.join("tool")).unwrap(), b"old");
        assert!(app_dir.join(".wenget").join("package.json").exists());
        assert!(!paths.staging_dir().join("tool").exists());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib installer::staging`
Expected: FAIL — module does not exist.

- [ ] **Step 3: Implement `src/installer/staging.rs`**

```rust
//! Staged install/swap
//!
//! A package's record lives inside its app directory, and every install path
//! wipes that directory before writing. Extracting into a staging directory and
//! swapping by `rename` means every crash point leaves either the complete old
//! install or the complete new one.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::core::paths::{sanitize_path_component, WenPaths};

/// An in-progress install, extracted aside and not yet swapped into place
pub struct StagedInstall {
    staging_path: PathBuf,
    app_dir: PathBuf,
    committed: bool,
}

impl StagedInstall {
    /// Create an empty staging directory for `key`
    pub fn begin(paths: &WenPaths, key: &str) -> Result<Self> {
        let staging_root = paths.staging_dir();
        fs::create_dir_all(&staging_root)
            .with_context(|| format!("Failed to create {}", staging_root.display()))?;

        let staging_path = staging_root.join(format!(
            "{}-{}",
            sanitize_path_component(key),
            std::process::id()
        ));
        if staging_path.exists() {
            fs::remove_dir_all(&staging_path).with_context(|| {
                format!("Failed to clear stale staging dir {}", staging_path.display())
            })?;
        }
        fs::create_dir_all(&staging_path)
            .with_context(|| format!("Failed to create {}", staging_path.display()))?;

        Ok(Self {
            staging_path,
            app_dir: paths.app_dir(key),
            committed: false,
        })
    }

    /// Where to extract files and write the package record
    pub fn path(&self) -> &Path {
        &self.staging_path
    }

    /// The app directory this will become, for messages before the swap
    pub fn target(&self) -> &Path {
        &self.app_dir
    }

    /// Swap the staged directory into place, returning the app directory
    pub fn commit(mut self) -> Result<PathBuf> {
        if let Some(parent) = self.app_dir.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let retired = if self.app_dir.exists() {
            let retired = self.app_dir.with_file_name(format!(
                "{}.old-{}",
                self.app_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                Utc::now().timestamp()
            ));
            fs::rename(&self.app_dir, &retired).with_context(|| {
                format!(
                    "Failed to move the previous install aside: {} -> {}",
                    self.app_dir.display(),
                    retired.display()
                )
            })?;
            Some(retired)
        } else {
            None
        };

        if let Err(e) = fs::rename(&self.staging_path, &self.app_dir) {
            // Put the previous install back before reporting failure.
            if let Some(ref retired) = retired {
                let _ = fs::rename(retired, &self.app_dir);
            }
            return Err(e).with_context(|| {
                format!("Failed to move the new install into {}", self.app_dir.display())
            });
        }

        self.committed = true;

        if let Some(retired) = retired {
            if let Err(e) = fs::remove_dir_all(&retired) {
                log::warn!(
                    "Installed successfully, but could not remove {}: {}. \
                     `wenget repair` will sweep it.",
                    retired.display(),
                    e
                );
            }
        }

        Ok(self.app_dir.clone())
    }
}

impl Drop for StagedInstall {
    fn drop(&mut self) {
        if !self.committed && self.staging_path.exists() {
            if let Err(e) = fs::remove_dir_all(&self.staging_path) {
                log::warn!(
                    "Could not clean up staging dir {}: {}",
                    self.staging_path.display(),
                    e
                );
            }
        }
    }
}
```

Add to `src/installer/mod.rs`, matching the existing style there:

```rust
pub mod staging;

pub use staging::StagedInstall;
```

- [ ] **Step 4: Convert the binary install path**

In `src/commands/add.rs`, replace the extract block (`:1742-1752`):

```rust
    // Stage the extraction beside the app directory and swap it in on success, so
    // a failed install leaves the previous install and its record untouched.
    let staged = crate::installer::StagedInstall::begin(paths, installed_key)?;
    let app_dir = staged.target().to_path_buf();

    println!("  Extracting to {}...", app_dir.display());

    let extracted_files = extract_archive(&download_path, staged.path())?;
```

Everything downstream in `install_package` that inspects extracted files must now look inside
`staged.path()`, not `app_dir`, until the swap. Update the executable-candidate call
(`add.rs:1755`) accordingly:

```rust
    let candidates = find_executable_candidates(&extracted_files, &pkg.name, Some(staged.path()));
```

Then, at the point where the function has finished touching files and is ready to return the
`InstalledPackage` (immediately before it builds the value it returns), swap:

```rust
    let app_dir = staged.commit()?;
```

Any code between extraction and commit that uses `app_dir` for chmod, shim creation, or path
recording must use `staged.path()` before the commit, or run after it. Work through the compiler
and the `install_path` value: the returned `InstalledPackage.install_path` must be the committed
`app_dir`, never the staging path.

- [ ] **Step 5: Convert the local-file and script paths the same way**

`src/installer/local.rs:39-52` currently does `remove_dir_all` then writes into `app_dir`. Replace
with `StagedInstall::begin` / write into `staged.path()` / `staged.commit()`. Do the same at
`src/installer/script.rs:202`. Both keep their existing logic; only the destination and the
directory-clearing change.

- [ ] **Step 6: Run the whole suite plus a real install**

```bash
cargo test
cargo build
export WENGET_ROOT=/tmp/wenget-stage
rm -rf "$WENGET_ROOT"
./target/debug/wenget add ripgrep -y
ls "$WENGET_ROOT/apps"
./target/debug/wenget add ripgrep -y   # update path: swap over an existing install
ls -a "$WENGET_ROOT/apps"
```
Expected: tests PASS; `apps/ripgrep` present after both runs; no `.staging` and no `*.old-*` left
behind; `rg` runs from the bin directory.

- [ ] **Step 7: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add -A src/
git commit -m "feat(installer): stage installs and swap by rename

Extract into apps/.staging and rename into place, moving any previous
install aside first. A failed install can no longer leave a half-written
app directory - required before the package record moves inside it."
```

---

### Task 7: Directory-name collision guard

**Files:**
- Modify: `src/core/store.rs` (the check), `src/commands/add.rs` (call it before staging)
- Test: `src/core/store.rs`

**Interfaces:**
- Consumes: `InstalledStore::classify` internals (Task 4).
- Produces: `InstalledStore::ensure_dir_available(&self, key: &str) -> Result<()>` — `Ok(())` when
  the target app directory is free or already belongs to `key`; otherwise an error naming both keys
  and the way out.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn test_collision_guard_rejects_a_foreign_occupant() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        // `foo-bar` the package occupies apps/foo-bar ...
        s.save_package("foo-bar", &pkg("foo-bar", None)).unwrap();

        // ... which is also where `foo::bar` the variant would go.
        let err = s.ensure_dir_available("foo::bar").unwrap_err().to_string();
        assert!(err.contains("foo::bar"), "names the package being installed: {err}");
        assert!(err.contains("foo-bar"), "names the occupant: {err}");
        assert!(err.contains("wenget delete foo-bar"), "offers a way out: {err}");

        // The existing record is untouched.
        assert!(s.load().unwrap().get_package("foo-bar").is_some());
    }

    #[test]
    fn test_collision_guard_allows_reinstall_of_the_same_key() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("bun::baseline", &pkg("bun", Some("baseline"))).unwrap();
        s.ensure_dir_available("bun::baseline").unwrap();
    }

    #[test]
    fn test_collision_guard_allows_untracked_and_empty_targets() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.ensure_dir_available("fresh").unwrap();

        std::fs::create_dir_all(tmp.path().join("apps").join("untracked")).unwrap();
        s.ensure_dir_available("untracked").unwrap();
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::store::tests::test_collision_guard`
Expected: FAIL — `no method named ensure_dir_available`.

- [ ] **Step 3: Implement**

```rust
    /// Refuse to install `key` into an app directory another package occupies
    ///
    /// `sanitize_path_component` is lossy: `foo::bar` and `foo-bar` both map to
    /// `apps/foo-bar`. Only detected, not resolved - see spec §7.
    pub fn ensure_dir_available(&self, key: &str) -> Result<()> {
        let app_dir = self.paths.app_dir(key);
        if !app_dir.exists() {
            return Ok(());
        }

        if let ScanEntry::Loaded { key: existing, .. } = self.classify(&app_dir) {
            if existing != key {
                anyhow::bail!(
                    "Cannot install {key}: {} is occupied by package {existing}.\n\
                     Run `wenget delete {existing}` first, or install {key} under a \
                     different name.",
                    app_dir.display()
                );
            }
        }

        Ok(())
    }
```

- [ ] **Step 4: Call it before staging**

In `src/commands/add.rs`, immediately before `StagedInstall::begin` in `install_package`:

```rust
    let store = crate::core::InstalledStore::new(paths.clone());
    store.ensure_dir_available(installed_key)?;
```

`WenPaths` derives `Clone` (`src/core/paths.rs:59`), so this is cheap. Add the same guard to the
local-file and script install paths converted in Task 6.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --lib core::store`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add -A src/
git commit -m "feat(add): refuse to install over another package's app directory

Sanitized directory names collide (foo::bar and foo-bar both map to
apps/foo-bar). Detect it before staging and name both keys plus the way
out, instead of silently overwriting the occupant's record."
```

---

### Task 8: Wire it in — `installed.json` stops being authoritative

The irreducible commit. `save_installed` disappears, so every writer changes at once, and
`repair.rs` will not compile until its `installed.json` handling is removed. Do not split this:
the tree is red in the middle of it.

**Files:**
- Modify: `src/core/config.rs:88-149` (`load_installed`, `save_installed`),
  `src/core/config.rs:172-178` (`get_or_create_installed`), `src/core/config.rs` (`init`,
  `is_initialized`), `src/core/manifest.rs:416-417` (`install_path`),
  `src/commands/add.rs:382-386,528-532,675-679,1588-1592,1643-1647` (batched saves),
  `src/commands/delete.rs:241-242`, `src/commands/rename.rs:68-69`,
  `src/commands/repair.rs:22-34,39-41,66-69,87-122`, `src/commands/init.rs:99-104` and `:318`
- Test: `src/core/config.rs`, `src/core/store.rs` (unignore one test)

**Interfaces:**
- Consumes: everything from Tasks 1-7.
- Produces:
  - `Config::load_installed(&self) -> Result<InstalledSet>` — delegates to `InstalledStore::load()`
  - `Config::get_or_create_installed(&self) -> Result<InstalledSet>` — no longer calls `init()`
  - `Config::store(&self) -> InstalledStore` — the handle write sites use
  - `Config::save_installed` — **deleted**
  - `InstalledPackage::install_path` — `#[serde(skip)]`

- [ ] **Step 1: Write the failing tests**

Unignore the Task 4 test:

```rust
    #[test]
    fn test_install_path_is_derived_not_stored() {
```

Add to `src/core/config.rs`'s test module:

```rust
    #[test]
    fn test_load_installed_reads_per_package_records() {
        let tmp = TempDir::new().unwrap();
        let config = Config::with_paths(WenPaths::with_root(tmp.path().to_path_buf()));

        config
            .store()
            .save_package("ripgrep", &sample_installed_package())
            .unwrap();

        let set = config.load_installed().unwrap();
        assert!(set.get_package("ripgrep").is_some());
        assert!(
            !tmp.path().join("installed.json").exists(),
            "no global index is ever written"
        );
    }

    #[test]
    fn test_get_or_create_installed_does_not_initialize() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("absent");
        let config = Config::with_paths(WenPaths::with_root(root.clone()));

        assert!(config.get_or_create_installed().unwrap().packages.is_empty());
        assert!(!root.exists(), "a read must not create the root");
    }

    #[test]
    fn test_is_initialized_ignores_installed_json() {
        let tmp = TempDir::new().unwrap();
        let config = Config::with_paths(WenPaths::with_root(tmp.path().to_path_buf()));
        assert!(!config.is_initialized());

        config.init().unwrap();
        assert!(config.is_initialized());
        assert!(tmp.path().join("apps").exists());
        assert!(
            !tmp.path().join("installed.json").exists(),
            "init no longer creates a global index"
        );
    }
```

`sample_installed_package()` is a local helper in the config test module; copy the body of
`sample_package()` from Task 3, changing `install_path` to `String::new()`.

`Config::with_paths` is `#[cfg(test)]` (`src/core/config.rs:52`) and stays that way — only
`WenPaths::with_root` was un-gated, in Task 1.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib core::config core::store`
Expected: FAIL — `no method named store`, and the init/`install_path` assertions fail.

- [ ] **Step 3: Flip `install_path` and delegate the loader**

`src/core/manifest.rs:416-417`:

```rust
    /// Where this package is installed
    ///
    /// Not serialized: a package record's own location is the answer, so the file
    /// cannot disagree with reality. Populated on load by `InstalledStore`.
    #[serde(skip)]
    pub install_path: String,
```

`src/core/config.rs` — replace `load_installed` (`:88-143`) and delete `save_installed`
(`:145-149`) entirely:

```rust
    /// A handle for reading and writing per-package records
    pub fn store(&self) -> InstalledStore {
        InstalledStore::new(self.paths.clone())
    }

    /// Load the set of installed packages from per-package records
    ///
    /// Migration from a legacy `installed.json`, quarantine of unparseable
    /// records, and skipping of future-version records all happen inside
    /// `InstalledStore::load`.
    pub fn load_installed(&self) -> Result<InstalledSet> {
        self.store().load()
    }
```

Replace `get_or_create_installed` (`:172-178`):

```rust
    /// Load the set of installed packages
    ///
    /// No longer initializes: with no file that must exist for reads to work, an
    /// absent root simply means nothing is installed. Initialization happens on
    /// `add` and on `wenget init`.
    pub fn get_or_create_installed(&self) -> Result<InstalledSet> {
        self.load_installed()
    }
```

Then find `Config::init` and `Config::is_initialized` in the same file and remove any
`installed_json` creation or existence check. `init` must still create the root, `apps/`, and the
other directories; `is_initialized` tests the root only (`WenPaths::is_initialized`,
`src/core/paths.rs:293-295`, already does exactly that — make `Config::is_initialized` defer to it
if it does not already).

Keep `WenPaths::installed_json()` — Task 5's migration is its only remaining caller. Add to its doc
comment:

```rust
    /// Get the legacy installed.json path
    ///
    /// Retained only so `InstalledStore` can migrate and retire the file. wenget
    /// never writes it.
```

- [ ] **Step 4: Convert the write sites**

`src/commands/add.rs` — each of the five `config.save_installed(installed)` calls disappears. The
package is written where it is installed. At the script-install site (`:368-386`), the loop becomes:

```rust
            Ok(inst_pkg) => {
                if let Err(e) = store.save_package(&name, &inst_pkg) {
                    eprintln!("{} Failed to save package record: {}", "✗".red(), e);
                }
                installed.upsert_package(name.clone(), inst_pkg);
                println!("  {} Installed successfully", "✓".green());
                success_count += 1;
                successful_scripts.push(name);
            }
```

with `let store = config.store();` hoisted above the loop, and the trailing
`if success_count > 0 { config.save_installed(installed) }` block deleted. Apply the same shape at
the other four sites. The in-memory `installed.upsert_package` call **stays**: the snapshot passed
to `install_package` (`add.rs:1694-1695`) is what command-name conflict resolution reads, and per
the doc comment at `add.rs:1688-1692` it must not be re-read from disk.

`src/commands/delete.rs` — delete the `config.save_installed(&installed)?;` at `:242`. Nothing
replaces it: `fs::remove_dir_all(&app_dir)` at `:296` removes the record with the payload. Keep
`installed.remove_package(name)` at `:300` — the in-memory set is still used for the summary. Add a
comment where the save used to be:

```rust
    // No record write: `delete_package` removed each app directory, and the
    // package record lived inside it.
```

`src/commands/rename.rs` — replace `config.save_installed(&installed)?;` at `:69`:

```rust
    // Only the renamed package's record changed.
    let renamed = installed
        .get_package(&pkg_key)
        .context("renamed package vanished from the installed set")?;
    config.store().save_package(&pkg_key, renamed)?;
```

`src/commands/repair.rs` — remove `installed.json` from this command for now: drop the
`installed_path`/`installed_status` bindings (`:22`, `:26`), their display line (`:32`), their issue
count (`:39-41`), the dispatch (`:66-69`), and the whole `repair_installed` function (`:87-122`).
Leave `buckets.json` and `manifest-cache.json` handling exactly as it is. Task 10 rebuilds this
command around app directories.

`src/commands/init.rs` — delete the `installed_json` block at `:99-104` and its line in the summary
at `:318`. Check the surrounding text: if the summary enumerates created files, `installed.json`
must not appear.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test`
Expected: PASS. `rg -n 'save_installed' src/` must print nothing.

- [ ] **Step 6: Verify on the real binary**

```bash
cargo build
export WENGET_ROOT=/tmp/wenget-wire
rm -rf "$WENGET_ROOT"

# A read against a nonexistent root must create nothing.
./target/debug/wenget list
test -e "$WENGET_ROOT" && echo "FAIL: read created the root" || echo "OK: no init on read"

# Install, inspect the record, rename, delete.
./target/debug/wenget add ripgrep -y
cat "$WENGET_ROOT/apps/ripgrep/.wenget/package.json"
test -e "$WENGET_ROOT/installed.json" && echo "FAIL: global index written" || echo "OK: no index"
./target/debug/wenget list
./target/debug/wenget rename rg rgx -y 2>/dev/null || ./target/debug/wenget rename rg rgx
./target/debug/wenget list
./target/debug/wenget delete ripgrep -y
./target/debug/wenget list
```
Expected: `list` on an absent root prints "nothing installed" and creates nothing; the record
contains no `install_path` key; no `installed.json` ever appears; rename updates the record; delete
leaves no entry.

- [ ] **Step 7: Verify migration against the real truncated file**

```bash
export WENGET_ROOT=/tmp/wenget-migrate
rm -rf "$WENGET_ROOT" && mkdir -p "$WENGET_ROOT/apps"
cp ~/.wenget/installed.json "$WENGET_ROOT/installed.json"
cp -R ~/.wenget/apps/fnm "$WENGET_ROOT/apps/" 2>/dev/null || true
./target/debug/wenget list
ls "$WENGET_ROOT"
```
Expected: the 20-byte `{"packages": {}}` is retired to `installed.json.migrated-*`, `fnm` is
reported as nothing (it carries no record, and wenget does not fabricate one), and the command
exits 0. **Do not run this against `~/.wenget` itself.**

- [ ] **Step 8: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add -A src/
git commit -m "feat(core): make per-package records authoritative; drop installed.json

Config::save_installed is deleted; add/rename write one record each and
delete removes the record with the app directory. install_path is no
longer serialized, reads no longer initialize the root, and init stops
creating a global index. repair loses its installed.json branch, which
Task 10 replaces."
```

---

### Task 9: Delete-path and residue regression tests

Task 8's write sites have no automated coverage; this adds it before `repair` is rebuilt on top.

**Files:**
- Test: `src/core/store.rs` (`mod tests`)

**Interfaces:**
- Consumes: `InstalledStore`, `StagedInstall`.
- Produces: nothing.

- [ ] **Step 1: Write the tests**

```rust
    #[test]
    fn test_removing_the_app_directory_removes_the_record() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("doomed", &pkg("doomed", None)).unwrap();
        assert_eq!(s.load().unwrap().packages.len(), 1);

        std::fs::remove_dir_all(s.paths().app_dir("doomed")).unwrap();

        assert!(s.load().unwrap().packages.is_empty());
        assert!(s.duplicate_keys().unwrap().is_empty());
    }

    #[test]
    fn test_failed_swap_leaves_the_previous_record_intact() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        let mut first = pkg("tool", None);
        first.version = "1.0.0".to_string();
        s.save_package("tool", &first).unwrap();

        {
            let staged =
                crate::installer::StagedInstall::begin(s.paths(), "tool").unwrap();
            let mut second = pkg("tool", None);
            second.version = "2.0.0".to_string();
            crate::core::store::write_record_atomically(
                &{
                    let dir = staged.path().join(".wenget");
                    std::fs::create_dir_all(&dir).unwrap();
                    dir.join("package.json")
                },
                &second,
            )
            .unwrap();
            // Dropped without commit: the install failed after writing its record.
        }

        let set = s.load().unwrap();
        assert_eq!(set.get_package("tool").unwrap().version, "1.0.0");
    }
```

`write_record_atomically` is `pub(crate)`, so this test reaches it from within the crate. If the
path expression above is awkward, create the directory on a separate line first — the assertion is
what matters.

- [ ] **Step 2: Run**

Run: `cargo test --lib core::store`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add src/core/store.rs
git commit -m "test(core): cover record removal by directory delete and failed swaps"
```

---

### Task 10: Rebuild `wenget repair`

**Files:**
- Modify: `src/commands/repair.rs` (rewrite `run` and add the app-directory and shim checks; keep
  `repair_buckets` and `repair_cache` untouched)
- Modify: `src/core/store.rs` (add the residue sweep)
- Test: `src/core/store.rs`, `src/commands/repair.rs`

**Interfaces:**
- Consumes: `InstalledStore::{scan_app_dirs, duplicate_keys, load}`, `ScanEntry` (Task 4),
  `WenPaths::{bin_dir, apps_dir, root, bin_shim_path}`.
- Produces:
  - `InstalledStore::sweep_residue(&self) -> Result<Vec<PathBuf>>` — deletes `apps/.staging/*` and
    `apps/*.old-*`, returning what it removed
  - `pub fn provable_orphan_shims(paths: &WenPaths, set: &InstalledSet) -> Result<Vec<PathBuf>>` in
    `src/commands/repair.rs`
  - `pub fn missing_shims(paths: &WenPaths, set: &InstalledSet) -> Vec<(String, PathBuf)>` in
    `src/commands/repair.rs`

- [ ] **Step 1: Write the failing tests**

In `src/core/store.rs`:

```rust
    #[test]
    fn test_sweep_residue_removes_only_residue() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("keep", &pkg("keep", None)).unwrap();
        std::fs::create_dir_all(s.paths().staging_dir().join("keep-99")).unwrap();
        std::fs::create_dir_all(tmp.path().join("apps").join("keep.old-123")).unwrap();
        std::fs::create_dir_all(tmp.path().join("apps").join("untracked")).unwrap();

        let swept = s.sweep_residue().unwrap();
        assert_eq!(swept.len(), 2, "staging entry and .old- dir: {swept:?}");
        assert!(s.paths().app_dir("keep").exists());
        assert!(tmp.path().join("apps").join("untracked").exists());
        assert!(!tmp.path().join("apps").join("keep.old-123").exists());
    }
```

In `src/commands/repair.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths::WenPaths;
    use crate::core::InstalledStore;
    use tempfile::TempDir;

    #[test]
    #[cfg(unix)]
    fn test_orphan_detection_requires_provenance() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        std::fs::create_dir_all(paths.apps_dir()).unwrap();

        // 1. An unrelated regular file in bin_dir. Not ours, no provenance.
        std::fs::write(paths.bin_dir().join("system-tool"), b"#!/bin/sh\n").unwrap();

        // 2. A symlink to somewhere outside the wenget root. Not ours.
        let foreign = tmp.path().join("elsewhere");
        std::fs::write(&foreign, b"x").unwrap();
        std::os::unix::fs::symlink(&foreign, paths.bin_dir().join("foreign")).unwrap();

        // 3. A symlink into apps/ with no owning package. Provably ours, orphaned.
        let dangling_target = paths.apps_dir().join("ghost").join("bin").join("ghost");
        std::fs::create_dir_all(dangling_target.parent().unwrap()).unwrap();
        std::fs::write(&dangling_target, b"x").unwrap();
        std::os::unix::fs::symlink(&dangling_target, paths.bin_dir().join("ghost")).unwrap();

        let set = InstalledStore::new(paths.clone()).load().unwrap();
        let orphans = provable_orphan_shims(&paths, &set).unwrap();

        let names: Vec<String> = orphans
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["ghost".to_string()], "got {names:?}");
    }

    #[test]
    #[cfg(unix)]
    fn test_missing_shim_is_reported() {
        let tmp = TempDir::new().unwrap();
        let paths = WenPaths::with_root(tmp.path().to_path_buf());
        std::fs::create_dir_all(paths.bin_dir()).unwrap();
        let store = InstalledStore::new(paths.clone());

        let mut p = crate::core::manifest::InstalledPackage {
            meta_version: crate::core::manifest::CURRENT_META_VERSION,
            repo_name: "tool".to_string(),
            variant: None,
            version: "1.0.0".to_string(),
            platform: "linux-x86_64".to_string(),
            installed_at: chrono::Utc::now(),
            install_path: String::new(),
            executables: std::collections::HashMap::new(),
            source: crate::core::manifest::PackageSource::Bucket {
                name: "main".to_string(),
            },
            description: "d".to_string(),
            command_names: vec![],
            command_name: None,
            asset_name: "tool.tar.gz".to_string(),
            parent_package: None,
            download_url: None,
        };
        p.executables
            .insert("bin/tool".to_string(), "tool".to_string());
        store.save_package("tool", &p).unwrap();

        let set = store.load().unwrap();
        let missing = missing_shims(&paths, &set);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].0, "tool");
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib commands::repair core::store::tests::test_sweep_residue`
Expected: FAIL — `cannot find function provable_orphan_shims`, `no method named sweep_residue`.

- [ ] **Step 3: Implement the residue sweep**

In `src/core/store.rs`:

```rust
    /// Remove `apps/.staging/*` and `apps/*.old-*` left by interrupted swaps
    pub fn sweep_residue(&self) -> Result<Vec<PathBuf>> {
        let mut removed = Vec::new();

        let staging = self.paths.staging_dir();
        if staging.exists() {
            for entry in fs::read_dir(&staging)
                .with_context(|| format!("Failed to read {}", staging.display()))?
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                let result = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                match result {
                    Ok(()) => removed.push(path),
                    Err(e) => log::warn!("Could not remove {}: {}", path.display(), e),
                }
            }
        }

        let apps_dir = self.paths.apps_dir();
        if apps_dir.exists() {
            for entry in fs::read_dir(&apps_dir)
                .with_context(|| format!("Failed to read {}", apps_dir.display()))?
                .filter_map(|e| e.ok())
            {
                let name = entry.file_name().to_string_lossy().to_string();
                if !is_old_install_residue(&name) {
                    continue;
                }
                let path = entry.path();
                match fs::remove_dir_all(&path) {
                    Ok(()) => removed.push(path),
                    Err(e) => log::warn!("Could not remove {}: {}", path.display(), e),
                }
            }
        }

        Ok(removed)
    }
```

- [ ] **Step 4: Implement the shim checks**

In `src/commands/repair.rs`:

```rust
/// Bin entries wenget can prove it created and no loaded package owns
///
/// `bin_dir` is `~/.local/bin` (or `/usr/local/bin`), shared with unrelated
/// software, so absence from the loaded set is not evidence. Proof means a
/// symlink resolving under `{root}/apps/` (Unix), or a wenget-generated shim
/// naming such a path (Windows). Anything unresolvable is left silent.
pub fn provable_orphan_shims(paths: &WenPaths, set: &InstalledSet) -> Result<Vec<PathBuf>> {
    let bin_dir = paths.bin_dir();
    if !bin_dir.exists() {
        return Ok(Vec::new());
    }

    let owned: std::collections::HashSet<String> = set
        .packages
        .values()
        .flat_map(|p| p.executables.values().cloned())
        .collect();

    let apps_dir = paths.apps_dir();
    let mut orphans = Vec::new();

    for entry in std::fs::read_dir(&bin_dir)
        .with_context(|| format!("Failed to read {}", bin_dir.display()))?
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let command = match path.file_stem().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };
        if owned.contains(&command) {
            continue;
        }
        if points_into_apps(&path, &apps_dir) {
            orphans.push(path);
        }
    }

    orphans.sort();
    Ok(orphans)
}

/// Whether this bin entry is provably a wenget launcher into `apps_dir`
fn points_into_apps(path: &Path, apps_dir: &Path) -> bool {
    #[cfg(unix)]
    {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return false;
        };
        if !meta.file_type().is_symlink() {
            return false;
        }
        let Ok(target) = std::fs::read_link(path) else {
            return false;
        };
        let absolute = if target.is_absolute() {
            target
        } else {
            match path.parent() {
                Some(parent) => parent.join(target),
                None => return false,
            }
        };
        // Compare lexically: the target may not exist, which is exactly the
        // dangling case worth reporting.
        absolute.starts_with(apps_dir)
    }

    #[cfg(windows)]
    {
        // wenget's generated launchers are text (`.cmd`) or a shim naming the
        // target path; either way the app path appears verbatim in the file.
        let Ok(content) = std::fs::read(path) else {
            return false;
        };
        let haystack = String::from_utf8_lossy(&content);
        let needle = apps_dir.to_string_lossy().to_string();
        haystack.contains(&needle) || haystack.contains(&needle.replace('\\', "\\\\"))
    }
}

/// Packages whose recorded command has no launcher in `bin_dir`
pub fn missing_shims(paths: &WenPaths, set: &InstalledSet) -> Vec<(String, PathBuf)> {
    let mut missing = Vec::new();
    for package in set.packages.values() {
        for command in package.executables.values() {
            let shim = paths.bin_shim_path(command);
            if !shim.exists() {
                missing.push((command.clone(), shim));
            }
        }
    }
    missing.sort();
    missing
}
```

The Windows branch of `points_into_apps` must match what `src/installer/shim.rs` actually writes.
Read that file before implementing: if the shim is a binary that stores the target path in a way
the substring check misses, use the same accessor the shim writer uses instead of a text search.
Never widen the check to "any entry not in the loaded set".

- [ ] **Step 5: Rewrite `run`**

Replace `run` in `src/commands/repair.rs` (`:14-85`), keeping `repair_buckets` and `repair_cache`
and their dispatch:

```rust
/// Run the repair command
pub fn run(force: bool) -> Result<()> {
    println!("{}", "Checking wenget state...".cyan());
    println!();

    let config = Config::new()?;
    let paths = config.paths().clone();
    let store = InstalledStore::new(paths.clone());

    let mut issues = 0usize;

    println!("{}", "Installed packages:".bold());
    let entries = store.scan_app_dirs()?;
    let mut residue = Vec::new();

    for entry in &entries {
        match entry {
            ScanEntry::Loaded { key, .. } => {
                println!("  {} {}", "✓".green(), key);
            }
            ScanEntry::Untracked(dir) => {
                println!(
                    "  {} {} - untracked app directory (no package record)",
                    "!".yellow(),
                    dir.display()
                );
                println!(
                    "      Re-install to bring it under management: {}",
                    format!(
                        "wenget add {}",
                        dir.file_name().unwrap_or_default().to_string_lossy()
                    )
                    .cyan()
                );
                issues += 1;
            }
            ScanEntry::Corrupt(dir) => {
                println!(
                    "  {} {} - package record was corrupt and has been saved aside",
                    "✗".red(),
                    dir.display()
                );
                issues += 1;
            }
            ScanEntry::FutureVersion { dir, version } => {
                println!(
                    "  {} {} - record version {} is newer than this wenget; skipped",
                    "!".yellow(),
                    dir.display(),
                    version
                );
            }
            ScanEntry::Residue(path) => residue.push(path.clone()),
        }
    }

    if entries.is_empty() {
        println!("  (none)");
    }

    for (key, dirs) in store.duplicate_keys()? {
        println!(
            "  {} {} is claimed by {} directories:",
            "✗".red(),
            key,
            dirs.len()
        );
        for dir in dirs {
            println!("      {}", dir.display());
        }
        issues += 1;
    }

    if !residue.is_empty() {
        println!();
        println!("{}", "Interrupted installs:".bold());
        for path in &residue {
            println!("  {} {}", "!".yellow(), path.display());
        }
        issues += residue.len();
    }

    let set = store.load()?;

    println!();
    println!("{}", "Command launchers:".bold());
    let orphans = provable_orphan_shims(&paths, &set)?;
    let missing = missing_shims(&paths, &set);
    if orphans.is_empty() && missing.is_empty() {
        println!("  {} all launchers match installed packages", "✓".green());
    }
    for path in &orphans {
        println!(
            "  {} {} - points into apps/ but no installed package owns it",
            "!".yellow(),
            path.display()
        );
        issues += 1;
    }
    for (command, path) in &missing {
        println!(
            "  {} {} - recorded command has no launcher at {}",
            "!".yellow(),
            command,
            path.display()
        );
        issues += 1;
    }

    // Global config files that legitimately remain global.
    println!();
    println!("{}", "Configuration files:".bold());
    let buckets_path = paths.buckets_json();
    let cache_path = paths.manifest_cache_json();
    let buckets_status = check_json_file::<BucketConfig>(&buckets_path);
    let cache_status = check_json_file::<ManifestCache>(&cache_path);
    println!("  buckets.json:        {}", buckets_status);
    println!("  manifest-cache.json: {}", cache_status);

    if matches!(buckets_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }
    if matches!(cache_status, FileStatus::Corrupted(_)) {
        issues += 1;
    }

    println!();
    if issues == 0 && !force {
        println!("{}", "Everything looks healthy.".green());
        return Ok(());
    }

    if !residue.is_empty()
        && (force || crate::utils::prompt::confirm("Remove interrupted-install leftovers?")?)
    {
        for path in store.sweep_residue()? {
            println!("  {} Removed {}", "✓".green(), path.display());
        }
    }

    for path in &orphans {
        let question = format!("Remove orphaned launcher {}?", path.display());
        if force || crate::utils::prompt::confirm_no_default(&question)? {
            match std::fs::remove_file(path) {
                Ok(()) => println!("  {} Removed {}", "✓".green(), path.display()),
                Err(e) => println!("  {} {}: {}", "✗".red(), path.display(), e),
            }
        }
    }

    if force || matches!(buckets_status, FileStatus::Corrupted(_)) {
        repair_buckets(&config, &buckets_path, &buckets_status)?;
    }
    if force || matches!(cache_status, FileStatus::Corrupted(_)) {
        repair_cache(&config, &cache_path, &cache_status, force)?;
    }

    println!();
    println!("{}", "Repair complete.".green());

    Ok(())
}
```

Removal always asks, even for provable orphans — only `--force` skips the prompt. The helpers are
`confirm(message)` (defaults to yes, `src/utils/prompt.rs:22`) for the residue sweep and
`confirm_no_default(message)` (defaults to no, `:41`) for deleting a launcher; neither takes a
default argument. `confirm_no_default` currently carries `#[allow(dead_code)]` — drop that
attribute, since this is its first caller.

Update the imports at the top of the file: drop `InstalledSet` from the `check_json_file` usage,
and add
`use crate::core::store::{InstalledStore, ScanEntry};`, `use crate::core::manifest::InstalledSet;`,
`use crate::core::paths::WenPaths;`, `use std::path::{Path, PathBuf};`.

- [ ] **Step 6: Run to verify pass**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 7: Verify on the real binary**

```bash
cargo build
export WENGET_ROOT=/tmp/wenget-repair
rm -rf "$WENGET_ROOT"
./target/debug/wenget add ripgrep -y

# Seed every finding class.
mkdir -p "$WENGET_ROOT/apps/untracked-thing"
mkdir -p "$WENGET_ROOT/apps/ripgrep.old-123"
mkdir -p "$WENGET_ROOT/apps/.staging/leftover-1"
echo '{ broken' > "$WENGET_ROOT/apps/ripgrep/.wenget/package.json.bak" # not a record; must be ignored
printf '#!/bin/sh\necho hi\n' > "$WENGET_ROOT/bin/unrelated-tool"

./target/debug/wenget repair
```
Expected: `untracked-thing` reported as an untracked app directory with an `wenget add` hint; the
`.old-123` and `.staging` entries reported as interrupted installs and swept on confirmation;
`unrelated-tool` **not** mentioned anywhere; `package.json.bak` ignored; `ripgrep` reported OK.

- [ ] **Step 8: Commit**

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
git add -A src/
git commit -m "feat(repair): report per-directory status, sweep residue, prove shim provenance

repair now walks apps/ and reports OK/untracked/corrupt/future-version/
duplicate-key per directory, sweeps .staging and *.old- leftovers, and
only reports or removes a bin entry it can prove wenget created - bin_dir
is shared with unrelated software, so absence from the loaded set is not
evidence."
```

---

### Task 11: Documentation

The spec requires the glossary to land with the code, not before (spec §Files Touched).

**Files:**
- Modify: `docs/reference/glossary.md`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `CHANGELOG.md`,
  `src/core/paths.rs:1-16` (module doc listing `installed.json`)
- Modify: `docs/spec/2026-09-03-per-package-meta-design.md` (Status line)

**Interfaces:**
- Consumes: the shipped behavior from Tasks 1-10.
- Produces: nothing code-facing.

- [ ] **Step 1: Add the new glossary terms**

Append to `docs/reference/glossary.md`, matching the existing entry format exactly (bold term,
colon, definition, `_Avoid_:` line):

```markdown
**Package record**:
The `{app_dir}/.wenget/package.json` file. The authoritative statement that a package is
installed, holding its version, platform, source, and executable map. Written by
`InstalledStore::save_package` in `src/core/store.rs`. There is no global index.
_Avoid_: Meta, meta file, ledger, registry, index, install record, `installed.json`.

**App directory**:
`{root}/apps/<sanitized-name>/` — the directory holding one package's files and its **Package
record**. Named by `sanitize_path_component`, which is lossy, so the directory name is never
parsed for identity.
_Avoid_: Payload, install dir.

**Untracked app directory**:
A directory under `{root}/apps/` with no readable **Package record**. `wenget repair` reports it
and otherwise leaves it alone; wenget never fabricates a record for it.
_Avoid_: Orphan (reserved for launchers in the bin directory).

**Installed key**:
`{repo_name}` or `{repo_name}::{variant}` — the identity of an installed package, produced by
`generate_installed_key`. Reconstructed from **Package record** content on load, never parsed
from the **App directory** name.
_Avoid_: Package id, slug.

**Installed set**:
The in-memory collection of every loaded **Package record** (`InstalledSet`). Rebuilt on each
command from a scan of `{root}/apps/`; never serialized as a whole.
_Avoid_: Installed manifest, installed.json.
```

Then check the existing **Manifest** entry: it must still describe a bucket's `manifest.json`, and
now that the collision is gone, no entry should mention `installed.json` as current behavior.

- [ ] **Step 2: Update the code-facing docs**

- `src/core/paths.rs:1-16` — the module doc lists `Installed manifest: ~/.wenget/installed.json`.
  Replace that line with `Package records: ~/.wenget/apps/<name>/.wenget/package.json`.
- `AGENTS.md` — the Project Structure block and the Key Implementation Notes line "Always update
  `installed.json` after install/remove operations". Replace with "Write one package record per
  install/rename; deleting an app directory removes its record." Add `store.rs` to the `src/core/`
  listing.
- `CLAUDE.md` — same edits if it carries the same text; `rg -n 'installed.json' CLAUDE.md README.md`
  first and fix every hit.
- `README.md` — any directory-layout or state-file section.

- [ ] **Step 3: Freeze the spec**

Change the spec's status line from `Status: Draft (2026-09-03)` to
`Status: Implemented (2026-09-03)`. Do not edit its body: it is the record of what was decided.

- [ ] **Step 4: Write the changelog entry**

Append under `## Unreleased`, in a `### Changed` subsection, following the existing entry style
(imperative summary, then the reasoning, then the date):

```markdown
- refactor(core)!: replace `installed.json` with per-package records at
  `{app_dir}/.wenget/package.json`. All installed-package state lived in one file, rewritten in
  full and non-atomically by eight call sites, so a crash mid-write or any bug that serialized an
  empty collection lost the record of *every* package — which already happened once: `cargo test`
  truncated the maintainer's `installed.json` while the app directories survived. Each package now
  owns its own record inside its own app directory, and the set of installed packages is the set
  of directories carrying a readable one; no global index is written, not even as a cache. Installs
  stage into `apps/.staging/` and swap by `rename`, so a failed update leaves the previous install
  and its record intact. `wenget repair` reports per-directory status, sweeps interrupted-install
  leftovers, and only reports or removes a launcher in the bin directory whose wenget provenance it
  can prove. A legacy `installed.json` is migrated on first load and renamed to
  `installed.json.migrated-<timestamp>`, never deleted; entries whose directory is gone are
  dropped and listed. `WENGET_ROOT` now overrides the root in release builds so this can be
  verified without touching a real `~/.wenget`. See
  `docs/adr/0001-no-global-installed-index.md` (2026-09-03).
```

- [ ] **Step 5: Verify no stale references remain**

Run: `rg -n 'installed\.json' src/ README.md AGENTS.md CLAUDE.md docs/reference/`
Expected: hits only where `installed.json` is described as legacy — `paths.rs`'s
`installed_json()` doc, `store.rs`'s migration, the glossary's `_Avoid_` lines. No hit describes it
as current state.

Run: `rg -ni 'meta file|installed manifest|the ledger' src/ docs/reference/ README.md AGENTS.md`
Expected: no matches. `meta_version` is the one permitted "meta" — the regex above will not match it.

- [ ] **Step 6: Commit**

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
git add -A
git commit -m "docs: record per-package records in the glossary and project docs

Adds Package record, App directory, Untracked app directory, Installed
key, and Installed set to the glossary; removes the 'always update
installed.json' rule; freezes the spec as implemented."
```

---

## Self-Review

**1. Spec coverage.** §1 on-disk format → Tasks 3, 8 (`serde(skip)`), 4 (version gate). §2 staging
→ Task 6. §3 `InstalledStore` → Tasks 2, 4. §4 call sites → Task 8. §5 reverse index → covered by
Task 4's `load()` rebuilding `executables` into `InstalledSet`, which `command_name_set` already
consumes; no new code needed. §6 failure handling → Tasks 4, 5 (best-effort writes), 10 (repair).
§7 collision → Task 7. §8 migration → Task 5. §9 `WENGET_ROOT` → Task 1. All 17 Testing bullets map
to a named test except "Atomicity" (Task 4, `test_save_leaves_no_tmp_file`) and "No init on read"
(Task 8, `test_get_or_create_installed_does_not_initialize`) — both present.

**2. Known gaps, stated rather than hidden.**
- Task 8 is large and cannot be split: `save_installed`'s removal breaks every writer plus
  `repair.rs` in one step. Tasks 4-7 exist to shrink it as far as it goes.
- Two steps ask the implementer to check reality before writing code: the Windows shim signature in
  Task 10 Step 4, and the `confirm` helper's real signature in Task 10 Step 5. Both are marked
  inline. Do not guess at either.
- Task 3's hand-written JSON depends on `PackageSource`'s serde shape; Step 1 says to print the
  real serialization rather than assume the `{"type": ...}` form.

**3. Type consistency.** `InstalledSet` from Task 2 onward, never `InstalledManifest`.
`InstalledStore::{new, paths, load, scan_app_dirs, save_package, remove_package, duplicate_keys,
ensure_dir_available, sweep_residue}` — every call in later tasks resolves to one of these.
`StagedInstall::{begin, path, target, commit}` used consistently in Tasks 6, 7, 9.
`write_record_atomically` is `pub(crate)` and used in Tasks 4, 5, 9. `CURRENT_META_VERSION` from
Task 3 used in Tasks 4, 5, 10.
