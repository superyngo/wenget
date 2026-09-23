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
            windows_token_is_elevated()
        }

        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    })
}

/// Whether the current process token is elevated (UAC "Run as administrator")
#[cfg(windows)]
fn windows_token_is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: plain Win32 calls with valid out-pointers; the token handle is
    // closed on every path after it was opened.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
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
