//! End-to-end lifecycle tests over the real `wenget` binary
//!
//! Every test runs the compiled binary against a throwaway `WENGET_ROOT` and
//! `HOME`, installing from local files only, so no network access is needed
//! and the user's real `~/.wenget/` and shell rc files are never touched.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

/// A sandboxed wenget root plus a directory for source files
struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        Self { dir }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    fn src(&self, name: &str) -> PathBuf {
        self.dir.path().join("src").join(name)
    }

    fn app_dir(&self, name: &str) -> PathBuf {
        self.root().join("apps").join(name)
    }

    fn record(&self, name: &str) -> PathBuf {
        self.app_dir(name).join(".wenget").join("package.json")
    }

    /// Launcher for `name` in the bin dir, whatever its platform extension
    fn launcher(&self, name: &str) -> Option<PathBuf> {
        let bin = self.root().join("bin");
        fs::read_dir(&bin)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_stem().and_then(|s| s.to_str()) == Some(name) && p.symlink_metadata().is_ok()
            })
    }

    fn wenget(&self) -> Command {
        let mut cmd = Command::cargo_bin("wenget").unwrap();
        cmd.env("WENGET_ROOT", self.root())
            .env("HOME", self.dir.path().join("home"))
            .env_remove("GITHUB_TOKEN")
            .write_stdin("");
        cmd
    }

    /// Write a tiny script that prints `hello-from-script`
    fn script(&self) -> PathBuf {
        if cfg!(windows) {
            let p = self.src("hello.cmd");
            fs::write(&p, "@echo off\r\necho hello-from-script\r\n").unwrap();
            p
        } else {
            let p = self.src("hello.sh");
            fs::write(&p, "#!/bin/sh\necho hello-from-script\n").unwrap();
            p
        }
    }

    /// Pack a copy of the wenget binary, renamed to `mytool`, into a tar.gz
    fn tool_archive(&self) -> PathBuf {
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_wenget"));
        let name = if cfg!(windows) {
            "mytool.exe"
        } else {
            "mytool"
        };
        let path = self.src("mytool.tar.gz");
        let file = fs::File::create(&path).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        tar.append_path_with_name(&exe, name).unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        path
    }
}

fn arg(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn script_install_list_delete() {
    let sb = Sandbox::new();
    let script = sb.script();

    sb.wenget()
        .args(["add", "-y", arg(&script)])
        .assert()
        .success();
    assert!(sb.record("hello").is_file(), "package record written");
    let launcher = sb.launcher("hello").expect("launcher created");

    #[cfg(unix)]
    {
        let out = std::process::Command::new(&launcher).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            "hello-from-script"
        );
    }
    let _ = launcher;

    let list = sb.wenget().arg("list").assert().success();
    let stdout = String::from_utf8_lossy(&list.get_output().stdout).to_string();
    assert!(stdout.contains("hello"), "list shows package: {stdout}");

    sb.wenget().args(["info", "hello"]).assert().success();

    sb.wenget().args(["del", "-y", "hello"]).assert().success();
    assert!(!sb.app_dir("hello").exists(), "app dir removed");
    assert!(sb.launcher("hello").is_none(), "launcher removed");
}

#[test]
fn archive_install_runs_extracted_binary() {
    let sb = Sandbox::new();
    let archive = sb.tool_archive();

    sb.wenget()
        .args(["add", "-y", arg(&archive)])
        .assert()
        .success();
    assert!(sb.record("mytool").is_file(), "package record written");
    let launcher = sb.launcher("mytool").expect("launcher created");

    let out = std::process::Command::new(&launcher)
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("wenget "));

    sb.wenget().args(["del", "-y", "mytool"]).assert().success();
    let left: Vec<_> = fs::read_dir(sb.root().join("apps"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "apps/ empty after delete: {left:?}");
    assert!(sb.launcher("mytool").is_none());
}

#[test]
fn rename_then_delete_cleans_new_name() {
    let sb = Sandbox::new();
    let script = sb.script();
    sb.wenget()
        .args(["add", "-y", arg(&script)])
        .assert()
        .success();

    sb.wenget()
        .args(["rename", "hello", "hi"])
        .assert()
        .success();
    assert!(sb.launcher("hi").is_some(), "renamed launcher exists");
    assert!(sb.launcher("hello").is_none(), "old launcher gone");

    sb.wenget().args(["del", "-y", "hello"]).assert().success();
    assert!(sb.launcher("hi").is_none(), "renamed launcher removed");
    assert!(!sb.app_dir("hello").exists());
}

#[test]
fn repair_recreates_missing_launcher() {
    let sb = Sandbox::new();
    let archive = sb.tool_archive();
    sb.wenget()
        .args(["add", "-y", arg(&archive)])
        .assert()
        .success();

    let launcher = sb.launcher("mytool").unwrap();
    fs::remove_file(&launcher).unwrap();

    sb.wenget().args(["repair", "-f"]).assert().success();
    assert!(sb.launcher("mytool").is_some(), "launcher recreated");
}

#[test]
fn failed_install_exits_nonzero_and_leaves_no_state() {
    let sb = Sandbox::new();
    let broken = sb.src("broken.tar.gz");
    fs::write(&broken, "not an archive").unwrap();

    sb.wenget()
        .args(["add", "-y", arg(&broken)])
        .assert()
        .failure();
    assert!(!sb.app_dir("broken").exists(), "no app dir left behind");
    assert!(sb.launcher("broken").is_none(), "no launcher left behind");
}

#[test]
fn reinstall_keeps_single_record() {
    let sb = Sandbox::new();
    let script = sb.script();

    sb.wenget()
        .args(["add", "-y", arg(&script)])
        .assert()
        .success();
    sb.wenget()
        .args(["add", "-y", arg(&script)])
        .assert()
        .success();

    let apps: Vec<_> = fs::read_dir(sb.root().join("apps"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(apps, vec![std::ffi::OsString::from("hello")]);
    assert!(sb.record("hello").is_file());
    assert!(sb.launcher("hello").is_some());
}
