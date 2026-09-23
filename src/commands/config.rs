//! Config command implementation

use anyhow::{Context, Result};
use colored::Colorize;
use std::env;
use std::process::Command;

use crate::core::{Config, Preferences};

/// Run the config command - opens config.toml in default editor
pub fn run(config: &Config) -> Result<()> {
    let config_path = config.paths().config_toml();

    // Generate default config file if it doesn't exist
    if !config_path.exists() {
        println!("{} Generating default config file...", "✓".green().bold());
        Preferences::generate_default_file(&config_path)?;
    }

    // Detect editor
    let editor = detect_editor();

    println!(
        "{} Opening config file: {}",
        "ℹ".cyan(),
        config_path.display().to_string().dimmed()
    );
    println!("  {} Using editor: {}", "ℹ".cyan(), editor.dimmed());

    // Open editor
    let (program, args) = split_editor_command(&editor);
    let status = Command::new(program)
        .args(args)
        .arg(&config_path)
        .status()
        .with_context(|| format!("Failed to launch editor: {}", editor))?;

    if !status.success() {
        anyhow::bail!("Editor exited with non-zero status: {}", status);
    }

    // Validate config after editing
    println!("{} Validating config...", "ℹ".cyan());
    match Preferences::load(&config_path) {
        Ok(prefs) => {
            if let Err(e) = prefs.validate() {
                println!(
                    "{} Invalid configuration: {}",
                    "⚠".yellow().bold(),
                    e.to_string().yellow()
                );
                println!(
                    "  {} The config will be ignored until errors are fixed",
                    "ℹ".cyan()
                );
            } else {
                println!("{} Configuration is valid", "✓".green().bold());
                println!(
                    "  {} Restart wenget or run commands to apply changes",
                    "ℹ".cyan()
                );
            }
        }
        Err(e) => {
            println!(
                "{} Failed to parse config: {}",
                "✗".red().bold(),
                e.to_string().red()
            );
            println!(
                "  {} The config will be ignored until errors are fixed",
                "ℹ".cyan()
            );
        }
    }

    Ok(())
}

/// Detect the appropriate editor to use
///
/// Priority:
/// 1. $VISUAL, then $EDITOR environment variable (may include arguments, e.g. `code --wait`)
/// 2. Platform-specific defaults:
///    - Unix: nano (fallback to vi if nano not found)
///    - Windows: notepad.exe
///    - macOS: nano (fallback to vim)
fn detect_editor() -> String {
    for var in ["VISUAL", "EDITOR"] {
        if let Ok(editor) = env::var(var) {
            if !editor.trim().is_empty() {
                return editor;
            }
        }
    }

    // Platform-specific defaults
    #[cfg(windows)]
    {
        "notepad.exe".to_string()
    }

    #[cfg(not(windows))]
    {
        // Try to find nano first, fallback to vi
        if which("nano") {
            "nano".to_string()
        } else if which("vim") {
            "vim".to_string()
        } else {
            "vi".to_string()
        }
    }
}

/// Split an editor command such as `code --wait` into program and arguments.
///
/// Splits on whitespace; editor paths containing spaces are not supported.
fn split_editor_command(editor: &str) -> (&str, Vec<&str>) {
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or(editor);
    (program, parts.collect())
}

/// Check if a command exists in PATH
#[cfg(not(windows))]
fn which(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_editor() {
        let editor = detect_editor();
        assert!(!editor.is_empty());

        #[cfg(windows)]
        {
            assert_eq!(editor, "notepad.exe");
        }
    }

    #[test]
    fn test_detect_editor_with_env() {
        env::remove_var("VISUAL");
        env::set_var("EDITOR", "custom-editor");
        let editor = detect_editor();
        assert_eq!(editor, "custom-editor");
        env::remove_var("EDITOR");
    }

    #[test]
    fn test_split_editor_command() {
        assert_eq!(split_editor_command("vim"), ("vim", vec![]));
        assert_eq!(
            split_editor_command("code --wait"),
            ("code", vec!["--wait"])
        );
        assert_eq!(
            split_editor_command("  vim -u NONE "),
            ("vim", vec!["-u", "NONE"])
        );
    }
}
