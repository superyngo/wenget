//! Archive extraction utilities

use anyhow::{Context, Result};
use bzip2::read::BzDecoder;
use flate2::read::GzDecoder;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use tar::Archive;
use xz2::read::XzDecoder;
use zip::ZipArchive;

/// Extract an archive file to a destination directory
/// For standalone executables, copies them directly to the destination
pub fn extract_archive(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    log::info!("Extracting: {}", archive_path.display());
    log::debug!("Destination: {}", dest_dir.display());

    // Create destination directory
    fs::create_dir_all(dest_dir)
        .with_context(|| format!("Failed to create directory: {}", dest_dir.display()))?;

    // Determine archive type by extension
    let filename = archive_path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Invalid file name")?;

    let extracted_files = if is_standalone_executable(filename) {
        // Handle standalone executable
        extract_standalone_executable(archive_path, dest_dir)?
    } else if filename.ends_with(".tar.gz") || filename.ends_with(".tgz") {
        extract_tar_gz(archive_path, dest_dir)?
    } else if filename.ends_with(".tar.xz") {
        extract_tar_xz(archive_path, dest_dir)?
    } else if filename.ends_with(".tar.bz2") || filename.ends_with(".tbz") {
        extract_tar_bz2(archive_path, dest_dir)?
    } else if filename.ends_with(".zip") {
        extract_zip(archive_path, dest_dir)?
    } else if filename.ends_with(".7z") {
        extract_7z(archive_path, dest_dir)?
    } else {
        anyhow::bail!("Unsupported archive format: {}", filename);
    };

    log::info!("Extracted {} file(s)", extracted_files.len());

    Ok(extracted_files)
}

/// Check if a file is a standalone executable (not an archive)
fn is_standalone_executable(filename: &str) -> bool {
    // Windows executables
    if cfg!(windows) && filename.ends_with(".exe") {
        return true;
    }

    // Unix/Linux/macOS binaries often have no extension or are AppImage
    if cfg!(unix) {
        if filename.ends_with(".AppImage") {
            return true;
        }
        // Check if it has no common archive extension
        let archive_extensions = [
            ".zip", ".tar", ".gz", ".xz", ".bz2", ".7z", ".rar", ".tbz", ".tgz",
        ];
        if !archive_extensions.iter().any(|ext| filename.contains(ext)) {
            // Could be a standalone binary
            return true;
        }
    }

    false
}

/// "Extract" a standalone executable by copying it to the destination directory
fn extract_standalone_executable(executable_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    let filename = executable_path
        .file_name()
        .context("Invalid executable filename")?;

    let dest_path = dest_dir.join(filename);

    // Copy the executable to the destination
    fs::copy(executable_path, &dest_path).with_context(|| {
        format!(
            "Failed to copy executable from {} to {}",
            executable_path.display(),
            dest_path.display()
        )
    })?;

    // Set executable permission on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest_path, perms)?;
    }

    let relative_path = filename.to_string_lossy().to_string();
    Ok(vec![relative_path])
}

/// Extract a .tar.gz file
fn extract_tar_gz(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;

    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    extract_tar_archive(&mut archive, dest_dir)
}

/// Extract a .tar.xz file
fn extract_tar_xz(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;

    let decoder = XzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    extract_tar_archive(&mut archive, dest_dir)
}

/// Extract a .tar.bz2 or .tbz file
fn extract_tar_bz2(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;

    let decoder = BzDecoder::new(file);
    let mut archive = Archive::new(decoder);

    extract_tar_archive(&mut archive, dest_dir)
}

/// Extract a .7z file
fn extract_7z(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    // `sevenz_rust::decompress_file` joins `entry.name()` onto the destination
    // with no sanitization, so a crafted archive can escape `dest_dir`.
    // Validate every entry name before letting the default extractor write it.
    let mut reader = File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;
    let archive = sevenz_rust::Archive::read(&mut reader, archive_path.metadata()?.len(), &[])
        .map_err(|e| anyhow::anyhow!("Failed to read 7z archive: {e}"))?;

    for entry in &archive.files {
        let name = entry.name();
        let path = Path::new(name);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Refusing to extract unsafe 7z entry: {name}");
        }
    }

    // Extract the 7z archive
    sevenz_rust::decompress_file(archive_path, dest_dir)
        .with_context(|| format!("Failed to extract 7z archive: {}", archive_path.display()))?;

    // Collect all extracted files
    let mut extracted_files = Vec::new();
    collect_files_recursively(dest_dir, dest_dir, &mut extracted_files)?;

    // Set executable permissions on Unix for files that should be executable
    #[cfg(unix)]
    {
        for file_path in &extracted_files {
            let full_path = dest_dir.join(file_path);
            if let Ok(metadata) = fs::metadata(&full_path) {
                if metadata.is_file() {
                    let filename = Path::new(file_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");

                    // Set executable permission for files that could be executables
                    if could_be_executable(filename, file_path)
                        && !is_excluded_file(filename, file_path)
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755);
                        fs::set_permissions(&full_path, perms)?;
                    }
                }
            }
        }
    }

    Ok(extracted_files)
}

/// Recursively collect all files in a directory (helper for 7z extraction)
fn collect_files_recursively(
    base_dir: &Path,
    current_dir: &Path,
    files: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(current_dir)
        .with_context(|| format!("Failed to read directory: {}", current_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_files_recursively(base_dir, &path, files)?;
        } else if path.is_file() {
            // Get relative path from base directory
            if let Ok(relative) = path.strip_prefix(base_dir) {
                files.push(relative.to_string_lossy().to_string());
            }
        }
    }

    Ok(())
}

/// Extract a tar archive (common logic for .tar.gz and .tar.xz)
fn extract_tar_archive<R: std::io::Read>(
    archive: &mut Archive<R>,
    dest_dir: &Path,
) -> Result<Vec<String>> {
    let mut extracted_files = Vec::new();

    for entry_result in archive
        .entries()
        .context("Failed to read archive entries")?
    {
        let mut entry = entry_result.context("Failed to read entry")?;

        let raw_path = entry.path().context("Failed to get entry path")?;
        // Normalize the entry path by dropping leading "./" (CurDir) components.
        // Tar archives commonly store entries as "./agd", which would otherwise
        // leak into the recorded relative path and the resulting symlink target
        // (e.g. /opt/wenget/apps/agd/./agd).
        let path: std::path::PathBuf = raw_path
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect();
        let path_str = path.to_string_lossy().to_string();

        // Skip directories (path normalization above strips trailing slashes,
        // so detect directories via the tar entry type instead).
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() || path.as_os_str().is_empty() {
            continue;
        }

        // Reject entries that would escape `dest_dir`. Absolute paths would
        // replace `dest_dir` entirely in `Path::join`, and `..` components
        // walk out of it (Zip Slip). `unpack_in` below also enforces this,
        // but failing loudly here gives the user an actionable error instead
        // of a silently skipped entry.
        if path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Refusing to extract entry with '..' in its path: {path_str}");
        }
        if path.is_absolute() {
            anyhow::bail!("Refusing to extract entry with an absolute path: {path_str}");
        }

        let dest_path = dest_dir.join(&path);

        // Log entry type for debugging
        log::debug!(
            "Extracting: {} (type: {:?}) to {}",
            path_str,
            entry_type,
            dest_path.display()
        );

        // `unpack_in` (not `unpack`) is what performs containment checking:
        // it strips root prefixes, refuses `..`, and validates that symlink
        // and hardlink targets stay inside `dest_dir`. It also creates any
        // missing parent directories itself.
        if !entry
            .unpack_in(dest_dir)
            .with_context(|| format!("Failed to extract: {path_str}"))?
        {
            anyhow::bail!("Refusing to extract unsafe archive entry: {path_str}");
        }

        // Set executable permission on Unix
        // Skip for symlinks (they inherit permissions from their target)
        #[cfg(unix)]
        {
            use tar::EntryType;
            if entry_type != EntryType::Symlink && is_executable(&mut entry)? {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&dest_path)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&dest_path, perms)?;
            }
        }

        extracted_files.push(path_str);
    }

    Ok(extracted_files)
}

/// Check if a tar entry is executable
#[cfg(unix)]
fn is_executable<R: std::io::Read>(entry: &mut tar::Entry<R>) -> Result<bool> {
    let mode = entry.header().mode()?;
    Ok(mode & 0o111 != 0)
}

/// Extract a .zip file
fn extract_zip(archive_path: &Path, dest_dir: &Path) -> Result<Vec<String>> {
    let file = File::open(archive_path)
        .with_context(|| format!("Failed to open archive: {}", archive_path.display()))?;

    let mut archive = ZipArchive::new(file).context("Failed to read ZIP archive")?;

    let mut extracted_files = Vec::new();

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("Failed to read ZIP entry")?;

        let file_path = file
            .enclosed_name()
            .context("Invalid file path in ZIP")?
            .to_owned();

        let dest_path = dest_dir.join(&file_path);

        if file.is_dir() {
            fs::create_dir_all(&dest_path)?;
            continue;
        }

        // Create parent directory
        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Extract file
        let mut dest_file = File::create(&dest_path)
            .with_context(|| format!("Failed to create file: {}", dest_path.display()))?;

        std::io::copy(&mut file, &mut dest_file).context("Failed to extract file")?;

        // Set executable permission on Unix
        #[cfg(unix)]
        {
            if let Some(mode) = file.unix_mode() {
                if mode & 0o111 != 0 {
                    use std::os::unix::fs::PermissionsExt;
                    let mut perms = fs::metadata(&dest_path)?.permissions();
                    perms.set_mode(0o755);
                    fs::set_permissions(&dest_path, perms)?;
                }
            }
        }

        extracted_files.push(file_path.to_string_lossy().to_string());
    }

    Ok(extracted_files)
}

/// Candidate executable with priority score
#[derive(Debug, Clone)]
pub struct ExecutableCandidate {
    /// Relative path to the executable
    pub path: String,
    /// Priority score (higher is better)
    pub score: u32,
    /// Human-readable reason for this candidate
    pub reason: String,
}

/// Check if a filename should be excluded as a non-executable documentation/config file
fn is_excluded_file(filename: &str, file_path: &str) -> bool {
    let lower_name = filename.to_lowercase();
    let lower_path = file_path.to_lowercase();

    // Exclude documentation files by extension
    let doc_extensions = [
        ".md", ".txt", ".rst", ".html", ".htm", ".pdf", ".doc", ".docx", ".1", ".2", ".3", ".4",
        ".5", ".6", ".7", ".8", // man pages
    ];
    if doc_extensions.iter().any(|ext| lower_name.ends_with(ext)) {
        return true;
    }

    // Exclude license/readme files by name pattern
    let excluded_names = [
        "license",
        "licence",
        "copying",
        "unlicense",
        "notice",
        "readme",
        "changelog",
        "changes",
        "history",
        "authors",
        "contributors",
        "credits",
        "thanks",
        "todo",
        "news",
    ];
    if excluded_names.iter().any(|name| lower_name.contains(name)) {
        return true;
    }

    // Exclude config/data files by extension
    let config_extensions = [
        ".yml", ".yaml", ".toml", ".json", ".xml", ".ini", ".cfg", ".conf",
    ];
    if config_extensions
        .iter()
        .any(|ext| lower_name.ends_with(ext))
    {
        return true;
    }

    // Exclude shell completion files (usually in complete/ or completions/ directory)
    let completion_extensions = [".fish", ".bash", ".zsh", ".ps1"];
    let in_completion_dir = lower_path.contains("complete") || lower_path.contains("completion");
    if in_completion_dir
        && completion_extensions
            .iter()
            .any(|ext| lower_name.ends_with(ext))
    {
        return true;
    }

    // Exclude files starting with underscore in completion directories (e.g., _rg for zsh)
    if in_completion_dir && lower_name.starts_with('_') {
        return true;
    }

    false
}

/// Check if a file could be an executable based on its filename
fn could_be_executable(filename: &str, file_path: &str) -> bool {
    let lower_name = filename.to_lowercase();

    if cfg!(windows) {
        // On Windows, must have .exe extension
        lower_name.ends_with(".exe")
    } else {
        // On Unix: check if in bin/ directory OR has no extension in filename
        let in_bin_dir = file_path.contains("bin/");

        // Check if filename has an extension (only check the filename, not the path!)
        let has_extension = filename.contains('.') && !filename.starts_with('.');

        // Executable scripts
        let script_extensions = [".sh"];
        let is_script = script_extensions
            .iter()
            .any(|ext| lower_name.ends_with(ext));

        in_bin_dir || !has_extension || is_script
    }
}

/// Check if a file has executable permission on Unix
#[cfg(unix)]
fn has_executable_permission(file_path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(metadata) = fs::metadata(file_path) {
        let mode = metadata.permissions().mode();
        mode & 0o111 != 0
    } else {
        false
    }
}

#[cfg(not(unix))]
#[allow(dead_code)]
fn has_executable_permission(_file_path: &Path) -> bool {
    // On Windows, we rely on .exe extension, not permissions
    true
}

/// Detect if a file is a native executable by reading its magic bytes.
/// Returns the executable type label if detected, or None.
fn detect_executable_type(file_path: &Path) -> Option<&'static str> {
    let mut buf = [0u8; 4];
    let mut file = File::open(file_path).ok()?;
    let bytes_read = file.read(&mut buf).ok()?;
    if bytes_read < 2 {
        return None;
    }

    // ELF (Linux): \x7fELF
    if bytes_read >= 4 && buf[0] == 0x7f && buf[1] == b'E' && buf[2] == b'L' && buf[3] == b'F' {
        return Some("ELF");
    }

    // PE (Windows): MZ
    if buf[0] == b'M' && buf[1] == b'Z' {
        return Some("PE");
    }

    // Mach-O (macOS)
    if bytes_read >= 4 {
        let magic = u32::from_be_bytes(buf);
        match magic {
            0xFEED_FACE | 0xFEED_FACF => return Some("Mach-O"),
            0xCEFA_EDFE | 0xCFFA_EDFE => return Some("Mach-O"),
            0xCAFE_BABE => return Some("Mach-O fat"),
            _ => {}
        }
    }

    None
}

/// Detect if a file is a script by checking for a shebang line.
/// Returns the interpreter label if detected, or None.
fn detect_script_type(file_path: &Path) -> Option<&'static str> {
    let mut buf = [0u8; 128];
    let mut file = File::open(file_path).ok()?;
    let bytes_read = file.read(&mut buf).ok()?;
    if bytes_read < 2 {
        return None;
    }

    // Check shebang #!
    if buf[0] == b'#' && buf[1] == b'!' {
        let line = std::str::from_utf8(&buf[..bytes_read])
            .ok()?
            .lines()
            .next()?;
        let lower = line.to_lowercase();
        if lower.contains("python") {
            return Some("Python script");
        } else if lower.contains("bash") || lower.contains("/sh") {
            return Some("Shell script");
        } else if lower.contains("node") {
            return Some("Node.js script");
        } else if lower.contains("ruby") {
            return Some("Ruby script");
        } else if lower.contains("perl") {
            return Some("Perl script");
        }
        return Some("script with shebang");
    }

    None
}

/// Find all possible executables and rank them by priority
/// `extract_dir` is the directory where files were extracted to (used for permission checks)
pub fn find_executable_candidates(
    extracted_files: &[String],
    package_name: &str,
    extract_dir: Option<&Path>,
) -> Vec<ExecutableCandidate> {
    let mut candidates = Vec::new();

    log::debug!(
        "find_executable_candidates: package_name={}, extract_dir={:?}, files={}",
        package_name,
        extract_dir,
        extracted_files.len()
    );

    for file in extracted_files {
        let path = Path::new(file);

        // Get just the filename (not the full path)
        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // Skip excluded files (docs, licenses, configs, etc.)
        if is_excluded_file(filename, file) {
            log::trace!("Skipping {} - excluded file", file);
            continue;
        }

        // Check if this could be an executable
        if !could_be_executable(filename, file) {
            log::trace!(
                "Skipping {} - not executable candidate (filename: {})",
                file,
                filename
            );
            continue;
        }

        // Skip test/debug/benchmark executables (check filename only, not full path)
        let lower_filename = filename.to_lowercase();
        if lower_filename.contains("test")
            || lower_filename.contains("debug")
            || lower_filename.contains("bench")
            || lower_filename.contains("example")
        {
            log::trace!("Skipping {} - test/debug/bench/example in filename", file);
            continue;
        }

        log::trace!("Evaluating candidate: {} (filename: {})", file, filename);

        let name_without_ext = filename.trim_end_matches(".exe");
        let mut score = 0u32;
        let mut reasons = Vec::new();

        // Check executable permission (Unix only)
        #[cfg(unix)]
        let has_exec_perm = if let Some(dir) = extract_dir {
            let full_path = dir.join(file);
            has_executable_permission(&full_path)
        } else {
            false
        };
        #[cfg(not(unix))]
        let has_exec_perm = false;

        // Rule 0: Has executable permission (Unix) - strong signal
        #[cfg(unix)]
        if has_exec_perm {
            score += 35;
            reasons.push("has exec permission");
        }
        // Suppress unused warning on non-Unix
        #[cfg(not(unix))]
        let _ = extract_dir;

        // Rule 0b: Content-based detection via magic bytes (strongest signal)
        if let Some(dir) = extract_dir {
            let full_path = dir.join(file);
            if let Some(exe_type) = detect_executable_type(&full_path) {
                score += 60;
                reasons.push(match exe_type {
                    "ELF" => "ELF binary",
                    "PE" => "PE binary",
                    "Mach-O" | "Mach-O fat" => "Mach-O binary",
                    _ => "native binary",
                });
            } else if let Some(script_type) = detect_script_type(&full_path) {
                score += 30;
                reasons.push(match script_type {
                    "Shell script" => "shell script (shebang)",
                    "Python script" => "python script (shebang)",
                    "Node.js script" => "node.js script (shebang)",
                    "Ruby script" => "ruby script (shebang)",
                    "Perl script" => "perl script (shebang)",
                    _ => "script (shebang)",
                });
            }
        }

        // Rule 1: Exact match with package name (highest priority)
        if name_without_ext == package_name {
            score += 100;
            reasons.push("exact name match");
        }
        // Rule 2: Partial match or package name contains file name
        else if name_without_ext.contains(package_name) || package_name.contains(name_without_ext)
        {
            score += 50;
            reasons.push("partial name match");
        }
        // Rule 3: Common abbreviation patterns (e.g., ripgrep -> rg)
        else if is_likely_abbreviation(package_name, name_without_ext) {
            score += 40;
            reasons.push("likely abbreviation");
        }

        // Rule 4: Located in bin/ directory - strong signal that file is an executable
        if file.contains("bin/") {
            score += 40;
            reasons.push("in bin/ directory");
        }

        // Rule 5: Located in target/release/ (Rust projects)
        if file.contains("target/release/") {
            score += 25;
            reasons.push("in target/release/");
        }

        // Rule 6: Shallow directory depth (prefer files closer to root)
        let depth = file.matches('/').count() + file.matches('\\').count();
        if depth <= 1 {
            score += 20;
        } else if depth <= 2 {
            score += 10;
        }

        // Rule 7: Simple filename (fewer special characters)
        if !name_without_ext.contains('-') && !name_without_ext.contains('_') {
            score += 5;
            reasons.push("simple name");
        }

        // Only add if score is above threshold (or has exec permission on Unix)
        // Note: has_exec_perm is always false on non-Unix, so this works cross-platform
        let should_add = score > 0 || has_exec_perm;

        if should_add {
            let reason = if reasons.is_empty() {
                "potential executable".to_string()
            } else {
                reasons.join(", ")
            };

            candidates.push(ExecutableCandidate {
                path: file.clone(),
                score,
                reason,
            });
        }
    }

    // Sort by score (highest first)
    candidates.sort_by(|a, b| b.score.cmp(&a.score));

    candidates
}

/// Check if name2 is likely an abbreviation of name1
fn is_likely_abbreviation(full_name: &str, abbrev: &str) -> bool {
    // Simple heuristic: check if abbrev matches first letters of words in full_name
    if abbrev.len() < 2 || abbrev.len() > full_name.len() {
        return false;
    }

    // Extract first letters of each word/segment
    let segments: Vec<&str> = full_name.split(&['-', '_'][..]).collect();
    if segments.len() > 1 {
        let first_letters: String = segments.iter().filter_map(|s| s.chars().next()).collect();

        if first_letters.to_lowercase() == abbrev.to_lowercase() {
            return true;
        }
    }

    // Check if abbrev is first N chars of full_name
    full_name.to_lowercase().starts_with(&abbrev.to_lowercase())
}

/// Find the main executable in extracted files
/// Returns the best candidate if found
pub fn find_executable(extracted_files: &[String], package_name: &str) -> Option<String> {
    let candidates = find_executable_candidates(extracted_files, package_name, None);
    candidates.first().map(|c| c.path.clone())
}

/// Normalize a command name by removing platform-specific suffixes
///
/// Strategy: Check if filename contains platform keywords. If yes, remove everything
/// from the first `-` or `_`. Finally, always remove `.exe` extension.
///
/// Examples:
///   "cate-windows-x86_64.exe" -> "cate"
///   "bat-v0.24-x86_64.exe" -> "bat"
///   "git-lfs.exe" -> "git-lfs"
///   "ripgrep.exe" -> "ripgrep"
///   "tool-linux-aarch64" -> "tool"
pub fn normalize_command_name(name: &str) -> String {
    // Platform keywords to detect platform-specific suffixes
    let platform_keywords = [
        "windows", "linux", "darwin", "macos", "freebsd", "netbsd", "openbsd", "x86_64", "aarch64",
        "arm64", "armv7", "i686", "x64", "x86", "pc", "unknown", "gnu", "musl", "msvc",
    ];

    // Check if filename contains any platform keywords (case-insensitive)
    let lower_name = name.to_lowercase();
    let has_platform_suffix = platform_keywords.iter().any(|kw| lower_name.contains(kw));

    let result = if has_platform_suffix {
        // Find first `-` or `_` and remove everything from there
        if let Some(pos) = name.find(['-', '_']) {
            &name[..pos]
        } else {
            name
        }
    } else {
        // No platform keywords, keep original name
        name
    };

    // Always remove .exe extension at the end
    result.trim_end_matches(".exe").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tar_gz_strips_curdir_prefix() {
        use std::io::Write;
        use tempfile::TempDir;

        // Build a .tar.gz whose entries are prefixed with "./" (as produced by
        // `tar -czf` from inside a directory), matching the real agd archive.
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("pkg.tar.gz");
        {
            let file = File::create(&archive_path).unwrap();
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);

            let data = b"binary";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "./agd", &data[..])
                .unwrap();

            let mut sub = tar::Header::new_gnu();
            sub.set_size(data.len() as u64);
            sub.set_mode(0o644);
            sub.set_cksum();
            builder
                .append_data(&mut sub, "./Resources/cli-templates.toml", &data[..])
                .unwrap();

            builder.into_inner().unwrap().finish().unwrap().flush().ok();
        }

        let dest = dir.path().join("out");
        let files = extract_archive(&archive_path, &dest).unwrap();

        // Recorded relative paths must not contain a "./" component.
        assert!(
            files.iter().any(|f| f == "agd"),
            "expected 'agd', got {files:?}"
        );
        assert!(
            files
                .iter()
                .all(|f| !f.starts_with("./") && !f.contains("/./")),
            "paths leaked a curdir component: {files:?}"
        );
        assert!(dest.join("agd").is_file());
        assert!(dest.join("Resources/cli-templates.toml").is_file());
    }

    /// Build a .tar.gz containing a single entry with `name` written straight
    /// into the header. `Builder::append_data` rejects traversal names, so the
    /// GNU name field is filled in by hand to mimic a hostile archive.
    #[cfg(test)]
    fn build_tar_gz_with_raw_entry_name(archive_path: &Path, name: &[u8]) {
        use std::io::Write;

        let file = File::create(archive_path).unwrap();
        let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(enc);

        let data = b"payload";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o755);
        header.set_entry_type(tar::EntryType::Regular);
        {
            let gnu = header.as_gnu_mut().unwrap();
            gnu.name[..name.len()].copy_from_slice(name);
        }
        header.set_cksum();
        builder.append(&header, &data[..]).unwrap();
        builder.into_inner().unwrap().finish().unwrap().flush().ok();
    }

    #[test]
    fn test_extract_tar_rejects_parent_dir_traversal() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("evil.tar.gz");
        build_tar_gz_with_raw_entry_name(&archive_path, b"../../escaped.txt");

        let dest = dir.path().join("apps").join("victim");
        let err = extract_archive(&archive_path, &dest)
            .expect_err("a '..' entry must be rejected, not extracted");
        assert!(
            err.to_string().contains("'..'"),
            "unexpected error: {err:#}"
        );

        // Nothing may be written outside the destination directory.
        assert!(
            !dir.path().join("escaped.txt").exists(),
            "entry escaped to {}",
            dir.path().join("escaped.txt").display()
        );
    }

    #[test]
    fn test_extract_tar_rejects_absolute_entry_path() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let outside = dir.path().join("absolute.txt");
        let archive_path = dir.path().join("evil-abs.tar.gz");
        // An absolute entry path would replace the destination in Path::join.
        let mut name = outside.to_string_lossy().into_owned().into_bytes();
        name.truncate(99);
        build_tar_gz_with_raw_entry_name(&archive_path, &name);

        let dest = dir.path().join("apps").join("victim");
        let err = extract_archive(&archive_path, &dest)
            .expect_err("an absolute entry must be rejected, not extracted");
        assert!(
            err.to_string().contains("absolute path"),
            "unexpected error: {err:#}"
        );
        assert!(!outside.exists(), "entry escaped to {}", outside.display());
    }

    #[test]
    fn test_find_executable() {
        let files = vec![
            "ripgrep-15.1.0/README.md".to_string(),
            "ripgrep-15.1.0/bin/rg.exe".to_string(),
            "ripgrep-15.1.0/doc/guide.md".to_string(),
        ];

        let exe = find_executable(&files, "rg");
        assert_eq!(exe, Some("ripgrep-15.1.0/bin/rg.exe".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_find_executable_ripgrep_linux() {
        // This is the actual file list from ripgrep Linux release
        let files = vec![
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/doc/CHANGELOG.md".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/doc/FAQ.md".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/doc/GUIDE.md".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/doc/rg.1".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/UNLICENSE".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/README.md".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/LICENSE-MIT".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/COPYING".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/complete/_rg.ps1".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/complete/_rg".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/complete/rg.fish".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/complete/rg.bash".to_string(),
            "ripgrep-15.1.0-aarch64-unknown-linux-gnu/rg".to_string(),
        ];

        let exe = find_executable(&files, "ripgrep");
        // Should find 'rg' even though package name is 'ripgrep'
        // rg is an abbreviation of ripgrep (r + g from rip-grep)
        assert_eq!(
            exe,
            Some("ripgrep-15.1.0-aarch64-unknown-linux-gnu/rg".to_string())
        );
    }

    #[test]
    #[cfg(windows)]
    fn test_find_executable_ripgrep_windows() {
        // Windows version with .exe extension
        let files = vec![
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/doc/CHANGELOG.md".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/doc/FAQ.md".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/UNLICENSE".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/README.md".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/LICENSE-MIT".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/COPYING".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/complete/_rg.ps1".to_string(),
            "ripgrep-15.1.0-x86_64-pc-windows-msvc/rg.exe".to_string(),
        ];

        let exe = find_executable(&files, "ripgrep");
        // Should find 'rg.exe' even though package name is 'ripgrep'
        assert_eq!(
            exe,
            Some("ripgrep-15.1.0-x86_64-pc-windows-msvc/rg.exe".to_string())
        );
    }

    #[test]
    fn test_is_excluded_file() {
        // Documentation files
        assert!(is_excluded_file("README.md", "foo/README.md"));
        assert!(is_excluded_file("CHANGELOG.md", "doc/CHANGELOG.md"));
        assert!(is_excluded_file("rg.1", "doc/rg.1")); // man page

        // License files
        assert!(is_excluded_file("LICENSE", "foo/LICENSE"));
        assert!(is_excluded_file("LICENSE-MIT", "foo/LICENSE-MIT"));
        assert!(is_excluded_file("COPYING", "foo/COPYING"));
        assert!(is_excluded_file("UNLICENSE", "foo/UNLICENSE"));

        // Completion files in completion directory
        assert!(is_excluded_file("rg.fish", "complete/rg.fish"));
        assert!(is_excluded_file("rg.bash", "completions/rg.bash"));
        assert!(is_excluded_file("_rg", "complete/_rg")); // zsh completion
        assert!(is_excluded_file("_rg.ps1", "complete/_rg.ps1"));

        // NOT excluded: regular executables
        assert!(!is_excluded_file("rg", "foo/rg"));
        assert!(!is_excluded_file("ripgrep", "foo/ripgrep"));
        assert!(!is_excluded_file("tool.sh", "bin/tool.sh"));
    }

    #[test]
    fn test_could_be_executable() {
        // Windows
        #[cfg(windows)]
        {
            assert!(could_be_executable("tool.exe", "foo/tool.exe"));
            assert!(!could_be_executable("tool", "foo/tool"));
        }

        // Unix
        #[cfg(unix)]
        {
            // No extension = could be executable
            assert!(could_be_executable("rg", "foo/rg"));
            assert!(could_be_executable("ripgrep", "foo/ripgrep"));

            // In bin/ = could be executable
            assert!(could_be_executable("tool", "bin/tool"));

            // Script = could be executable
            assert!(could_be_executable("run.sh", "foo/run.sh"));

            // Has extension = not executable (unless script)
            assert!(!could_be_executable("rg.fish", "foo/rg.fish"));
            assert!(!could_be_executable("config.toml", "foo/config.toml"));
        }
    }

    #[test]
    fn test_normalize_command_name() {
        // Files with platform suffixes - should remove from first - or _
        assert_eq!(normalize_command_name("cate-windows-x86_64.exe"), "cate");
        assert_eq!(normalize_command_name("bat-v0.24-x86_64.exe"), "bat");
        assert_eq!(normalize_command_name("tool-linux-aarch64"), "tool");
        assert_eq!(normalize_command_name("app-darwin-x86_64.exe"), "app");
        assert_eq!(
            normalize_command_name("ripgrep-13.0.0-x86_64-pc-windows-msvc.exe"),
            "ripgrep"
        );
        assert_eq!(normalize_command_name("fd_v8.7.0_x86_64.exe"), "fd");

        // Files without platform suffixes - should keep name but remove .exe
        assert_eq!(normalize_command_name("ripgrep.exe"), "ripgrep");
        assert_eq!(normalize_command_name("git-lfs.exe"), "git-lfs");
        assert_eq!(normalize_command_name("gh-cli.exe"), "gh-cli");
        assert_eq!(normalize_command_name("node-sass.exe"), "node-sass");

        // Unix executables without .exe
        assert_eq!(normalize_command_name("ripgrep"), "ripgrep");
        assert_eq!(normalize_command_name("git-lfs"), "git-lfs");

        // Edge cases
        assert_eq!(normalize_command_name("tool.exe"), "tool");
        assert_eq!(normalize_command_name("tool"), "tool");
    }

    #[test]
    fn test_detect_executable_type_elf() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test_elf");
        // ELF magic: \x7fELF followed by some bytes
        fs::write(&path, b"\x7fELF\x02\x01\x01\x00").unwrap();
        assert_eq!(detect_executable_type(&path), Some("ELF"));
    }

    #[test]
    fn test_detect_executable_type_pe() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test_pe.exe");
        // PE magic: MZ header
        fs::write(&path, b"MZ\x90\x00\x03\x00\x00\x00").unwrap();
        assert_eq!(detect_executable_type(&path), Some("PE"));
    }

    #[test]
    fn test_detect_executable_type_macho() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();

        // Mach-O 64-bit
        let path = dir.path().join("test_macho64");
        fs::write(&path, b"\xfe\xed\xfa\xcf\x00\x00\x00\x00").unwrap();
        assert_eq!(detect_executable_type(&path), Some("Mach-O"));

        // Mach-O fat binary
        let path2 = dir.path().join("test_macho_fat");
        fs::write(&path2, b"\xca\xfe\xba\xbe\x00\x00\x00\x02").unwrap();
        assert_eq!(detect_executable_type(&path2), Some("Mach-O fat"));
    }

    #[test]
    fn test_detect_executable_type_none() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test_text");
        fs::write(&path, b"Hello, world!\n").unwrap();
        assert_eq!(detect_executable_type(&path), None);
    }

    #[test]
    fn test_detect_script_type() {
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();

        // Shell script
        let sh = dir.path().join("test.sh");
        fs::write(&sh, b"#!/bin/bash\necho hello").unwrap();
        assert_eq!(detect_script_type(&sh), Some("Shell script"));

        // Python script
        let py = dir.path().join("test_py");
        fs::write(&py, b"#!/usr/bin/env python3\nprint('hi')").unwrap();
        assert_eq!(detect_script_type(&py), Some("Python script"));

        // Node script
        let node = dir.path().join("test_node");
        fs::write(&node, b"#!/usr/bin/env node\nconsole.log('hi')").unwrap();
        assert_eq!(detect_script_type(&node), Some("Node.js script"));

        // Not a script
        let txt = dir.path().join("test.txt");
        fs::write(&txt, b"Just some text").unwrap();
        assert_eq!(detect_script_type(&txt), None);
    }
}
