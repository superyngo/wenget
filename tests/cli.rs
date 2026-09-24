//! End-to-end tests for `init`, `bucket`, `list`, and `info` over the real `wenget` binary
//!
//! A one-route local HTTP server hosts the bucket manifest, so no network access is needed.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

/// Serve `body` for every request; returns the manifest URL
fn serve(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/manifest.json", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                line.clear();
            }
            let mut out = &stream;
            let _ = write!(
                out,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    url
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

fn manifest() -> String {
    serde_json::json!({
        "packages": [{
            "name": "fixturetool",
            "description": "a fixture package for cli tests",
            "repo": "https://github.com/fx/fixturetool",
            "version": "1.2.3",
            "platforms": {
                platform_key(): [{ "url": "https://example.invalid/a.tar.gz", "size": 1, "asset_name": "a.tar.gz" }]
            }
        }],
        "scripts": []
    })
    .to_string()
}

struct Sandbox {
    dir: TempDir,
}

impl Sandbox {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        Self { dir }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().join("root")
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn wenget(&self) -> Command {
        let mut cmd = Command::cargo_bin("wenget").unwrap();
        cmd.env("WENGET_ROOT", self.root())
            .env("HOME", self.home())
            .env("NO_COLOR", "1")
            // Unreachable API keeps `info`'s latest-version lookup offline
            .env("WENGET_GITHUB_API", "http://127.0.0.1:9")
            .env_remove("GITHUB_TOKEN")
            .write_stdin("");
        cmd
    }

    fn stdout(&self, args: &[&str]) -> String {
        let out = self.wenget().args(args).assert().success();
        String::from_utf8_lossy(&out.get_output().stdout).into_owned()
    }
}

#[test]
fn bucket_add_list_del_round_trip() {
    let sb = Sandbox::new();
    let url = serve(manifest());

    assert!(sb
        .stdout(&["bucket", "add", "fx", &url])
        .contains("Bucket 'fx' added"));
    assert!(sb
        .stdout(&["bucket", "add", "fx", &url])
        .contains("already exists"));

    let list = sb.stdout(&["bucket", "list"]);
    assert!(list.contains("fx") && list.contains(&url), "{list}");
    assert!(list.contains("Total: 1 bucket(s)"), "{list}");

    let del = sb.stdout(&["bucket", "del", "fx", "nope"]);
    assert!(del.contains("1 bucket(s) deleted"), "{del}");
    assert!(del.contains("1 bucket(s) not found"), "{del}");
    assert!(del.contains("Total buckets: 0"), "{del}");
}

#[test]
fn list_all_and_info_read_bucket_packages() {
    let sb = Sandbox::new();
    let url = serve(manifest());
    sb.wenget()
        .args(["bucket", "add", "fx", &url])
        .assert()
        .success();

    let all = sb.stdout(&["list", "--all"]);
    assert!(all.contains("fixturetool"), "{all}");

    let info = sb.stdout(&["info", "fixturetool"]);
    assert!(info.contains("a fixture package for cli tests"), "{info}");
    assert!(info.contains("https://github.com/fx/fixturetool"), "{info}");

    let missing = sb.wenget().args(["info", "nosuchpkg"]).assert().success();
    let stderr = String::from_utf8_lossy(&missing.get_output().stderr);
    assert!(
        stderr.contains("nosuchpkg") && stderr.contains("Not found"),
        "{stderr}"
    );

    let installed = sb.stdout(&["list"]);
    assert!(!installed.contains("fixturetool"), "{installed}");
}

#[test]
fn init_is_idempotent_and_leaves_shell_rc_alone_under_wenget_root() {
    let sb = Sandbox::new();
    // A pre-existing `wenget` bucket makes init skip the network bucket fetch
    let url = serve(manifest());
    sb.wenget()
        .args(["bucket", "add", "wenget", &url])
        .assert()
        .success();

    for _ in 0..2 {
        sb.wenget().args(["init", "-y"]).assert().success();
    }
    for dir in ["apps", "bin", "cache"] {
        assert!(sb.root().join(dir).is_dir(), "{dir} created");
    }
    let bin = fs::read_dir(sb.root().join("bin")).unwrap();
    assert!(
        bin.flatten()
            .any(|e| e.path().file_stem().and_then(|s| s.to_str()) == Some("wenget")),
        "wenget launcher created"
    );
    assert_eq!(
        fs::read_dir(sb.home()).unwrap().count(),
        0,
        "HOME untouched"
    );

    let buckets = sb.stdout(&["bucket", "list"]);
    assert!(buckets.contains("Total: 1 bucket(s)"), "{buckets}");
}
