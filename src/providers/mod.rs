//! Source providers for wenget

pub mod base;
pub mod github;

// Re-export commonly used items
pub use base::SourceProvider;
pub use github::{GitHubProvider, GitHubRepo};
