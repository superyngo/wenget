//! User interaction utilities for wenget
//!
//! This module provides common prompts for user confirmation and input.

use anyhow::Result;
use std::io::{self, Write};

/// Prompt the user for confirmation with a yes/no question.
///
/// Returns `true` if the user confirms (Y/y/yes or empty for default yes),
/// `false` otherwise.
///
/// # Arguments
/// * `message` - The prompt message to display (without the [Y/n] suffix)
///
/// # Example
/// ```ignore
/// if confirm("Proceed with installation?")? {
///     // User confirmed
/// }
/// ```
pub fn confirm(message: &str) -> Result<bool> {
    print!("{} [Y/n] ", message);
    io::stdout().flush()?;

    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    let response = response.trim().to_lowercase();

    Ok(response.is_empty() || response == "y" || response == "yes")
}

/// Prompt the user for confirmation with a no as default.
///
/// Returns `true` if the user explicitly confirms (Y/y/yes),
/// `false` otherwise (including empty input).
///
/// # Arguments
/// * `message` - The prompt message to display (without the [y/N] suffix)
pub fn confirm_no_default(message: &str) -> Result<bool> {
    print!("{} [y/N] ", message);
    io::stdout().flush()?;

    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    let response = response.trim().to_lowercase();

    Ok(response == "y" || response == "yes")
}

#[cfg(test)]
mod tests {
    // Note: These tests are for documentation purposes.
    // Testing stdin/stdout requires mock implementations.

    #[test]
    fn test_module_compiles() {}
}
