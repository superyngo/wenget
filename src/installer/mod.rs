//! Installer module for wenget

pub mod extractor;
pub mod input_detector;
pub mod local;
pub mod script;
pub mod staging;
pub mod symlink;

// Re-export commonly used items
pub use extractor::{
    extract_archive, find_executable, find_executable_candidates, normalize_command_name,
};
pub use script::{
    create_script_shim, detect_script_type, download_script, extract_script_name, install_script,
    read_local_script,
};
pub use staging::StagedInstall;

#[cfg(windows)]
pub mod shim;
#[cfg(windows)]
pub use shim::create_shim;

#[cfg(unix)]
pub use symlink::create_symlink;
