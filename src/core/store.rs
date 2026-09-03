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
// `Loaded` carries an InstalledPackage, which is much larger than the path-only
// variants. Boxing it would buy nothing: the enum is built and consumed in one
// scan, never stored in bulk.
#[allow(clippy::large_enum_variant)]
#[allow(dead_code)]
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

// Callers land as the write sites are converted.
#[allow(dead_code)]
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
        self.migrate_legacy();

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

    /// Convert a legacy `{root}/installed.json` into per-package records, once
    ///
    /// Best-effort by design: if any write fails, `installed.json` stays in place,
    /// one warning is logged, and the command proceeds from whatever the scan
    /// finds. The next writable run migrates.
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
            "*".cyan(),
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

            // The staging directory itself is normal furniture; only its
            // contents are residue from an interrupted swap.
            if path == self.paths.staging_dir() {
                if let Ok(children) = fs::read_dir(&path) {
                    entries.extend(
                        children
                            .filter_map(|e| e.ok())
                            .map(|e| ScanEntry::Residue(e.path())),
                    );
                }
                continue;
            }

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
    #[allow(dead_code)]
    pub fn remove_package(&self, key: &str) -> Result<()> {
        let record_path = self.paths.package_record_path(key);
        if record_path.exists() {
            fs::remove_file(&record_path)
                .with_context(|| format!("Failed to remove {}", record_path.display()))?;
        }
        Ok(())
    }

    /// Installed keys claimed by more than one app directory
    #[allow(dead_code)]
    pub fn duplicate_keys(&self) -> Result<Vec<(String, Vec<PathBuf>)>> {
        let mut by_key: HashMap<String, Vec<PathBuf>> = HashMap::new();
        for entry in self.scan_app_dirs()? {
            if let ScanEntry::Loaded { key, dir, .. } = entry {
                by_key.entry(key).or_default().push(dir);
            }
        }
        Ok(by_key
            .into_iter()
            .filter(|(_, dirs)| dirs.len() > 1)
            .collect())
    }

    /// Refuse to install `key` into an app directory another package occupies
    ///
    /// `sanitize_path_component` is lossy: `foo::bar` and `foo-bar` both map to
    /// `apps/foo-bar`. Only detected, not resolved.
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
}

/// `foo.old-1730000000` — residue from an interrupted swap
fn is_old_install_residue(name: &str) -> bool {
    name.rsplit_once(".old-")
        .is_some_and(|(_, stamp)| !stamp.is_empty() && stamp.chars().all(|c| c.is_ascii_digit()))
}

/// Serialize to a sibling `.tmp`, fsync, then rename over the target
#[allow(dead_code)]
pub(crate) fn write_record_atomically(final_path: &Path, pkg: &InstalledPackage) -> Result<()> {
    use std::io::Write;

    let tmp_path = final_path.with_file_name("package.json.tmp");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::manifest::PackageSource;
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
        assert_eq!(
            loaded.executables.get("bin/x").map(String::as_str),
            Some("ripgrep")
        );
        assert!(matches!(loaded.source, PackageSource::Bucket { .. }));
    }

    #[test]
    fn test_key_is_reconstructed_from_record_not_directory_name() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("bun::baseline", &pkg("bun", Some("baseline")))
            .unwrap();

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

        let raw = std::fs::read_to_string(s.paths().package_record_path("ripgrep")).unwrap();
        assert!(
            !raw.contains("install_path"),
            "record must not store its own location"
        );

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
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("package.json.corrupt-")
            })
            .collect();
        assert_eq!(
            quarantined.len(),
            1,
            "corrupt record is renamed, never deleted"
        );
    }

    #[test]
    fn test_future_version_is_skipped_untouched() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("futurepkg", &pkg("futurepkg", None))
            .unwrap();
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
        assert!(
            orphan.join("fnm").exists(),
            "untracked files are left alone"
        );
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
        assert!(tmp
            .path()
            .join("apps")
            .join("gone")
            .join("payload")
            .exists());
    }

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
        assert!(
            migrated_marker(&tmp).is_some(),
            "the old file is renamed, never deleted"
        );

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

    #[test]
    fn test_collision_guard_rejects_a_foreign_occupant() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        // `foo-bar` the package occupies apps/foo-bar ...
        s.save_package("foo-bar", &pkg("foo-bar", None)).unwrap();

        // ... which is also where `foo::bar` the variant would go.
        let err = s.ensure_dir_available("foo::bar").unwrap_err().to_string();
        assert!(
            err.contains("foo::bar"),
            "names the package being installed: {err}"
        );
        assert!(err.contains("foo-bar"), "names the occupant: {err}");
        assert!(
            err.contains("wenget delete foo-bar"),
            "offers a way out: {err}"
        );

        // The existing record is untouched.
        assert!(s.load().unwrap().get_package("foo-bar").is_some());
    }

    #[test]
    fn test_collision_guard_allows_reinstall_of_the_same_key() {
        let tmp = TempDir::new().unwrap();
        let s = store(&tmp);
        s.save_package("bun::baseline", &pkg("bun", Some("baseline")))
            .unwrap();
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
            let staged = crate::installer::StagedInstall::begin(s.paths(), "tool").unwrap();
            let mut second = pkg("tool", None);
            second.version = "2.0.0".to_string();
            let dir = staged.path().join(".wenget");
            std::fs::create_dir_all(&dir).unwrap();
            write_record_atomically(&dir.join("package.json"), &second).unwrap();
            // Dropped without commit: the install failed after writing its record.
        }

        let set = s.load().unwrap();
        assert_eq!(set.get_package("tool").unwrap().version, "1.0.0");
    }

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
}
