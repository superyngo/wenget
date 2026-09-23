//! Source providers for wenget

pub mod github;

// Re-export commonly used items
pub use github::{GitHubProvider, GitHubRepo};
