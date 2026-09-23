//! Detached helper-process launching
//!
//! Used on Windows to run a batch script after wenget exits (self-update
//! cleanup, self-uninstall).

use std::ffi::OsString;
use std::path::Path;

/// Build the `cmd` arguments that start `script` in the background
///
/// `start` treats its first quoted argument as the window title, and a script
/// path containing spaces is passed quoted, so an explicit empty title (`""`)
/// comes first. Arguments stay `OsString`, so non-UTF-8 paths need no `unwrap`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn start_script_args(script: &Path, window_flag: &str) -> Vec<OsString> {
    vec![
        "/C".into(),
        "start".into(),
        "".into(),
        window_flag.into(),
        script.as_os_str().to_owned(),
    ]
}

/// Launch `script` via `cmd /C start` without waiting for it
#[cfg(windows)]
pub fn spawn_script_detached(script: &Path, window_flag: &str) -> std::io::Result<()> {
    std::process::Command::new("cmd")
        .args(start_script_args(script, window_flag))
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_script_args_has_empty_title() {
        let args = start_script_args(Path::new("C:\\Program Files\\w\\x.cmd"), "/B");
        assert_eq!(args[2], OsString::from(""));
        assert_eq!(args[3], OsString::from("/B"));
        assert_eq!(args[4], OsString::from("C:\\Program Files\\w\\x.cmd"));
    }

    /// The real regression: a script in a directory with spaces must run
    #[cfg(windows)]
    #[test]
    fn test_spawn_script_detached_path_with_spaces() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dir = tmp.path().join("dir with spaces");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("ran.txt");
        let script = dir.join("mark me.cmd");
        std::fs::write(
            &script,
            format!("@echo off\r\necho ok> \"{}\"\r\n", marker.display()),
        )
        .unwrap();

        spawn_script_detached(&script, "/B").unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !marker.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(marker.exists(), "script in a path with spaces did not run");
    }
}
