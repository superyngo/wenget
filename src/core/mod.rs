//! Core modules for wenget

pub mod checksum;
pub mod config;
pub mod manifest;
pub mod paths;
pub mod platform;
pub mod preferences;
pub mod privilege;
pub mod registry;
pub mod repair;
pub mod store;

// Re-export commonly used items
pub use config::Config;
#[allow(unused_imports)]
pub use manifest::{
    InstalledPackage, InstalledSet, Package, PlatformBinary, ScriptItem, ScriptPlatform, ScriptType,
};
pub use paths::WenPaths;
#[allow(unused_imports)]
pub use platform::{Arch, BinaryAsset, BinarySelector, Compiler, FileExtension, Os, Platform};
pub use preferences::Preferences;
pub use privilege::is_elevated;
#[allow(unused_imports)]
pub use registry::{add_to_system_path, remove_from_system_path};
#[allow(unused_imports)]
pub use store::InstalledStore;
