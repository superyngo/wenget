//! Utility modules for wenget

pub mod http;
pub mod prompt;

// Re-export commonly used items
pub use http::HttpClient;
pub use prompt::confirm;
