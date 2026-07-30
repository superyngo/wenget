//! Privilege detection module for wenget
//!
//! This module provides utilities to detect whether the current process
//! is running with elevated privileges (root on Unix, Administrator on Windows).

use std::sync::OnceLock;

/// Cached privilege detection result
static IS_ELEVATED: OnceLock<bool> = OnceLock::new();

/// Check if the current process is running with elevated privileges.
///
/// On Unix: Returns true if running as root (euid == 0)
/// On Windows: Returns true if running as Administrator
///
/// The result is cached using OnceLock for efficiency.
pub fn is_elevated() -> bool {
    *IS_ELEVATED.get_or_init(|| {
        #[cfg(unix)]
        {
            // Check if effective UID is 0 (root)
            unsafe { libc::geteuid() == 0 }
        }

        #[cfg(windows)]
        {
            is_elevated::is_elevated()
        }

        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_elevated_returns_bool() {
        // Just verify the function returns without panicking
        // and that calling it twice returns the same cached value
        let first = is_elevated();
        let second = is_elevated();
        assert_eq!(first, second, "is_elevated should return cached value");
    }

    #[test]
    fn test_is_elevated_consistency() {
        // Verify that multiple calls return the same result (caching works)
        let results: Vec<bool> = (0..10).map(|_| is_elevated()).collect();
        assert!(
            results.iter().all(|&r| r == results[0]),
            "is_elevated should return consistent results"
        );
    }
}
