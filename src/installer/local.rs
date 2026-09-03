//! Local file installation logic

use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;

#[cfg(unix)]
use crate::installer::symlink::create_symlink;

use crate::core::manifest::PackageSource;
use crate::core::{InstalledPackage, WenPaths};
use crate::installer::{extract_archive, find_executable_candidates, normalize_command_name};

#[cfg(windows)]
use crate::installer::create_shim;

/// Install a local file (archive or binary)
pub fn install_local_file(
    paths: &WenPaths,
    file_path: &Path,
    custom_name: Option<&str>,
    original_source: Option<String>,
) -> Result<InstalledPackage> {
    // Determine package name from filename or custom name
    let filename = file_path
        .file_name()
        .and_then(|s| s.to_str())
        .context("Invalid file path")?;

    let name = if let Some(custom) = custom_name {
        custom.to_string()
    } else {
        // Extract name from filename (remove extension, versions, etc.)
        normalize_command_name(filename)
    };

    let staged = crate::installer::StagedInstall::begin(paths, &name)?;
    let app_dir = staged.target().to_path_buf();

    log::info!(
        "Installing local file {} to {}",
        file_path.display(),
        app_dir.display()
    );

    // Extract or copy file into the staging directory, then swap it into place.
    // extract_archive handles both archives and standalone executables
    let extracted_files = extract_archive(file_path, staged.path())?;

    // Find executable candidates
    let candidates = find_executable_candidates(&extracted_files, &name, Some(staged.path()));

    if candidates.is_empty() {
        anyhow::bail!(
            "Failed to find executable in {}. Extracted files:\n{}",
            file_path.display(),
            extracted_files.join("\n")
        );
    }

    // Select the best executable
    // (Simply take the first one as they are sorted by score)
    let selected = &candidates[0];
    log::info!(
        "Selected executable: {} ({})",
        selected.path,
        selected.reason
    );

    // Swap into place before resolving the executable path: everything below
    // (launcher target, recorded path) must refer to the app directory.
    let app_dir = staged.commit()?;

    let exe_relative = &selected.path;
    let exe_path = app_dir.join(exe_relative);

    if !exe_path.exists() {
        anyhow::bail!(
            "Executable not found at expected path: {}",
            exe_path.display()
        );
    }

    // Determine finalized command name
    let command_name = if let Some(custom) = custom_name {
        custom.to_string()
    } else {
        // Use the executable name but normalized
        let exe_filename = exe_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(&name);
        normalize_command_name(exe_filename)
    };

    println!("  Command will be available as: {}", command_name);

    // Create shim
    let bin_path = paths.bin_shim_path(&command_name);
    println!("  Creating launcher at {}...", bin_path.display());

    #[cfg(unix)]
    {
        create_symlink(&exe_path, &bin_path)?;
    }

    #[cfg(windows)]
    {
        create_shim(&exe_path, &bin_path, &command_name)?;
    }

    // Construct InstalledPackage info
    let source = if let Some(src) = original_source {
        if src.starts_with("http") {
            PackageSource::DirectRepo { url: src }
        } else {
            // For local files, strict PackageSource mapping is tricky as it's not a repo or bucket.
            // We reuse DirectRepo with a file URI or path for now to fit the schema
            // without breaking existing types.
            PackageSource::DirectRepo { url: src }
        }
    } else {
        PackageSource::DirectRepo {
            url: file_path.to_string_lossy().to_string(),
        }
    };

    Ok(InstalledPackage {
        meta_version: crate::core::manifest::CURRENT_META_VERSION,
        repo_name: name.clone(),
        variant: None,
        version: "local".to_string(), // We don't know the version from a file
        platform: "local".to_string(),
        installed_at: Utc::now(),
        install_path: app_dir.to_string_lossy().to_string(),
        executables: {
            let mut m = HashMap::new();
            m.insert(exe_relative.to_string(), command_name);
            m
        },
        source,
        description: format!("Local installation of {}", filename),
        command_names: vec![],
        command_name: None,
        asset_name: filename.to_string(),
        parent_package: None,
        download_url: None,
    })
}
