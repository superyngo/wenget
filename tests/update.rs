//! End-to-end `update` tests over the real `wenget` binary
//!
//! A local HTTP fixture stands in for both the bucket manifest host and the GitHub API
//! (`WENGET_GITHUB_API`), so no network access is needed.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use assert_cmd::Command;
use tempfile::TempDir;

type Routes = Arc<Mutex<HashMap<String, (u16, Vec<u8>)>>>;

/// Minimal HTTP/1.1 server answering from a mutable path -> (status, body) map
struct Fixture {
    base: String,
    routes: Routes,
}

impl Fixture {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes: Routes = Arc::default();
        let shared = routes.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let mut line = String::new();
                while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                    line.clear();
                }
                let mut parts = request_line.split_whitespace();
                let method = parts.next().unwrap_or("");
                let path = parts.next().unwrap_or("").to_string();
                let (status, body) = shared
                    .lock()
                    .unwrap()
                    .get(&path)
                    .cloned()
                    .unwrap_or((404, b"not found".to_vec()));
                let mut out = &stream;
                let _ = write!(
                    out,
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if method != "HEAD" {
                    let _ = out.write_all(&body);
                }
            }
        });
        Self { base, routes }
    }

    fn set(&self, path: &str, status: u16, body: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.to_string(), (status, body.into()));
    }

    /// Publish `demo` at `version` in both the bucket manifest and the release API
    fn publish(&self, version: &str) {
        self.publish_to(version, true);
    }

    /// Publish `demo` at `version` in the release API (and bucket when `bucket`)
    fn publish_to(&self, version: &str, bucket: bool) {
        let asset = format!("demo-{version}-{}.tar.gz", target_triple());
        let asset_path = format!("/fx/demo/releases/download/v{version}/{asset}");
        let archive = demo_archive(version);
        let url = format!("{}{}", self.base, asset_path);
        let size = archive.len();
        self.set(&asset_path, 200, archive);

        let manifest = serde_json::json!({
            "last_updated": "2026-09-23T00:00:00Z",
            "packages": [{
                "name": "demo",
                "description": "fixture package",
                "repo": "https://github.com/fx/demo",
                "version": version,
                "platforms": {
                    platform_key(): [{ "url": url, "size": size, "asset_name": asset }]
                }
            }],
            "scripts": []
        });
        if bucket {
            self.set("/bucket/manifest.json", 200, manifest.to_string());
        }

        let release = serde_json::json!({
            "tag_name": format!("v{version}"),
            "assets": [{ "name": asset, "browser_download_url": url, "size": size }]
        });
        self.set(
            "/api/repos/fx/demo/releases/latest",
            200,
            release.to_string(),
        );
    }
}

fn platform_key() -> String {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    if cfg!(windows) {
        format!("{arch}-pc-windows-msvc")
    } else if cfg!(target_os = "macos") {
        format!("{arch}-apple-darwin")
    } else {
        format!("{arch}-unknown-linux-gnu")
    }
}

/// tar.gz holding a `demo` executable: a script printing `version` on Unix,
/// a copy of the wenget binary on Windows (only the record version is checked there)
fn demo_archive(version: &str) -> Vec<u8> {
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(gz);
    if cfg!(windows) {
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_wenget"));
        tar.append_path_with_name(&exe, "demo.exe").unwrap();
    } else {
        let body = format!("#!/bin/sh\necho demo-{version}\n");
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "demo", body.as_bytes())
            .unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

struct Sandbox {
    dir: TempDir,
    fx: Fixture,
}

impl Sandbox {
    /// Sandbox with the fixture bucket added and `demo` 1.0.0 installed
    fn with_demo_installed() -> Self {
        let sb = Self {
            dir: TempDir::new().unwrap(),
            fx: Fixture::start(),
        };
        fs::create_dir_all(sb.dir.path().join("home")).unwrap();
        sb.fx.publish("1.0.0");
        let bucket = format!("{}/bucket/manifest.json", sb.fx.base);
        sb.wenget()
            .args(["bucket", "add", "fx", &bucket])
            .assert()
            .success();
        sb.wenget().args(["add", "-y", "demo"]).assert().success();
        assert_eq!(sb.version(), "1.0.0");
        sb
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    fn wenget(&self) -> Command {
        let mut cmd = Command::cargo_bin("wenget").unwrap();
        cmd.env("WENGET_ROOT", self.root())
            .env("HOME", self.dir.path().join("home"))
            .env("WENGET_GITHUB_API", format!("{}/api", self.fx.base))
            .env_remove("GITHUB_TOKEN")
            .write_stdin("");
        cmd
    }

    /// Installed version from the package record
    fn version(&self) -> String {
        let record = self.root().join("apps/demo/.wenget/package.json");
        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(record).unwrap()).unwrap();
        json["version"].as_str().unwrap().to_string()
    }

    /// Output of running the installed `demo` launcher (Unix only)
    #[cfg(unix)]
    fn run_demo(&self) -> String {
        let out = std::process::Command::new(self.root().join("bin/demo"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn update(&self) -> String {
        let out = self
            .wenget()
            .args(["update", "-y", "demo"])
            .assert()
            .success();
        String::from_utf8_lossy(&out.get_output().stdout).to_string()
    }
}

#[test]
fn update_upgrades_to_new_release() {
    let sb = Sandbox::with_demo_installed();
    #[cfg(unix)]
    assert_eq!(sb.run_demo(), "demo-1.0.0");

    // Only the GitHub release moves; the bucket manifest still says 1.0.0
    sb.fx.publish_to("1.1.0", false);
    let stdout = sb.update();

    assert_eq!(sb.version(), "1.1.0", "record upgraded: {stdout}");
    #[cfg(unix)]
    assert_eq!(sb.run_demo(), "demo-1.1.0");
}

#[test]
fn update_is_noop_when_current() {
    let sb = Sandbox::with_demo_installed();
    let stdout = sb.update();

    assert!(
        stdout.contains("up to date"),
        "reports up to date: {stdout}"
    );
    assert!(!stdout.contains("Installing"), "no reinstall: {stdout}");
    assert_eq!(sb.version(), "1.0.0");
}

#[test]
fn update_falls_back_to_bucket_when_api_fails() {
    let sb = Sandbox::with_demo_installed();
    sb.fx.publish("1.1.0");
    sb.fx.set(
        "/api/repos/fx/demo/releases/latest",
        500,
        "boom".to_string(),
    );

    let stdout = sb.update();

    assert_eq!(sb.version(), "1.1.0", "upgraded from bucket data: {stdout}");
    #[cfg(unix)]
    assert_eq!(sb.run_demo(), "demo-1.1.0");
}
