//! Input detection for add command

use crate::installer::script::is_script_input;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum InputType {
    /// GitHub package name or generic identifier (including GitHub repo URLs)
    PackageName,
    /// Local or remote script file
    Script,
    /// Local archive or binary file
    LocalFile,
    /// Direct URL to archive or binary (NOT GitHub repo URLs)
    DirectUrl,
}

/// Check if a URL is a GitHub repository URL (not a direct download URL)
fn is_github_repo_url(url: &str) -> bool {
    // GitHub repo URLs: https://github.com/owner/repo[/...]
    // NOT direct downloads: .../releases/download/..., .../raw/...
    // Source archives (.../archive/...) hold no binaries, so they stay repo URLs
    // and install the repo's release binary instead.
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.host_str(), Some("github.com" | "www.github.com")) {
        return false;
    }

    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();
    if segments.len() < 2 {
        return false;
    }

    let rest = &segments[2..];
    let is_download =
        rest.starts_with(&["releases", "download"]) || rest.first().is_some_and(|s| *s == "raw");
    !is_download
}

pub fn detect_input_type(input: &str) -> InputType {
    // Check if it's a script first (existing logic)
    if is_script_input(input) {
        return InputType::Script;
    }

    // Check if it's a URL
    if input.starts_with("http://") || input.starts_with("https://") {
        // Distinguish between GitHub repo URLs and direct download URLs
        if is_github_repo_url(input) {
            // GitHub repo URLs should be treated as package names
            return InputType::PackageName;
        } else {
            // Direct download URLs
            return InputType::DirectUrl;
        }
    }

    // Check if it looks like a local file path
    let path = Path::new(input);
    if path.exists()  // File actually exists
        || path.is_absolute() // Is absolute path
        || input.starts_with("./") || input.starts_with(".\\") // Explicit relative path
        || input.starts_with("../") || input.starts_with("..\\")
    {
        return InputType::LocalFile;
    }

    // Otherwise, assume it's a package name
    InputType::PackageName
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_repo_urls_are_package_names() {
        for url in [
            "https://github.com/BurntSushi/ripgrep",
            "https://github.com/BurntSushi/ripgrep/",
            "https://www.github.com/BurntSushi/ripgrep.git",
            "https://github.com/BurntSushi/ripgrep/tree/master",
            "https://github.com/BurntSushi/ripgrep/archive/refs/tags/14.0.0.tar.gz",
        ] {
            assert_eq!(detect_input_type(url), InputType::PackageName, "{url}");
        }
    }

    #[test]
    fn test_non_repo_urls_are_direct_urls() {
        for url in [
            "https://github.com/o/r/releases/download/v1/tool.tar.gz",
            "https://github.com/o/r/raw/main/tool.tar.gz",
            "https://github.com/",
            "https://example.com/downloads/github.com-release.tar.gz",
            "https://gitlab.com/mirrors/github.com/tool.tar.gz",
            "https://server.net/archive.tar.gz?ref=github.com",
            "https://notgithub.com/o/r",
        ] {
            assert_eq!(detect_input_type(url), InputType::DirectUrl, "{url}");
        }
    }
}
