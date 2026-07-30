//! CLI argument parsing for wenget

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wenget")]
#[command(author = env!("CARGO_PKG_AUTHORS"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A cross-platform package manager for GitHub binaries", long_about = None)]
#[command(propagate_version = true)]
#[command(help_template = "\
{name} {version} by {author}

{about-with-newline}
{usage-heading}
    {usage}

{all-args}{after-help}
")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose logging
    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage buckets (remote manifest sources)
    Bucket {
        #[command(subcommand)]
        command: BucketCommands,
    },

    /// Install packages or scripts from buckets ,GitHub repo, URLs, or local files
    #[command(visible_alias = "install")]
    #[command(visible_alias = "a")]
    Add {
        /// Package names, GitHub URLs, or script paths/URLs to add (supports wildcards *)
        names: Vec<String>,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Custom command name (overrides the default executable name)
        #[arg(short = 'c', long = "command")]
        script_name: Option<String>,

        /// Specify target platform (e.g., windows-x64, linux-x64, darwin-arm64)
        #[arg(short = 'p', long = "platform")]
        platform: Option<String>,

        /// Specify version to install (e.g., v1.0.0, 1.0.0)
        #[arg(short = 'v', long = "ver")]
        pkg_version: Option<String>,

        /// Specify variant to install (e.g., baseline, profile)
        #[arg(long = "variant")]
        variant: Option<String>,

        /// Don't append variant suffix to command name
        #[arg(long = "no-suffix")]
        no_suffix: bool,
    },

    /// List installed packages
    #[command(visible_alias = "ls")]
    List {
        /// Show all available packages from buckets (not just installed)
        #[arg(short = 'a', long = "all")]
        all: bool,
    },

    /// Show package information from buckets or GitHub repo
    #[command(visible_alias = "i")]
    Info {
        /// Package names or GitHub URLs to show (supports wildcards * for cache queries)
        names: Vec<String>,
    },

    /// Search for packages in buckets
    #[command(visible_alias = "s")]
    Search {
        /// Package names to search (supports wildcards *)
        names: Vec<String>,
    },

    /// Upgrade installed packages
    #[command(visible_alias = "up")]
    Update {
        /// Package names to upgrade, or "all" for all packages (supports wildcards *)
        names: Vec<String>,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Specify target platform (e.g., linux-x86_64-musl, aarch64-unknown-linux-musl)
        #[arg(short = 'p', long = "platform")]
        platform: Option<String>,
    },

    /// Delete (remove) installed packages
    #[command(visible_alias = "remove")]
    #[command(visible_alias = "rm")]
    #[command(visible_alias = "uninstall")]
    Del {
        /// Package names to delete (supports wildcards *)
        names: Vec<String>,

        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,

        /// Force deletion (allow deleting wenget itself)
        #[arg(short, long)]
        force: bool,

        /// Specify variant to delete (e.g., baseline, profile)
        #[arg(long = "variant")]
        variant: Option<String>,
    },

    /// Initialize wenget (create directories and set up PATH)
    Init {
        /// Skip confirmation prompts
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Repair corrupted configuration files
    Repair {
        /// Force rebuild all configuration files (not just corrupted ones)
        #[arg(short, long)]
        force: bool,
    },

    /// Edit configuration file with default editor
    #[command(visible_alias = "c")]
    Config,

    /// Rename an installed command
    #[command(visible_alias = "mv")]
    #[command(visible_alias = "rn")]
    Rename {
        /// Current command name or package key
        old_name: String,

        /// New command name (if omitted, will prompt interactively)
        new_name: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum BucketCommands {
    /// Add a bucket
    Add {
        /// Bucket name
        name: String,

        /// URL to the manifest.json file
        url: String,
    },

    /// Delete buckets
    Del {
        /// Bucket names to delete
        names: Vec<String>,
    },

    /// List all buckets
    List,

    /// Refresh cache from buckets
    Refresh,

    /// Create a bucket manifest from source files or direct URLs
    Create {
        /// Source file(s) containing GitHub repository URLs (comma-separated for multiple)
        #[arg(short = 'r', long = "repos-src", value_delimiter = ',')]
        repos_src: Vec<String>,

        /// Source file(s) containing script URLs/Gist URLs (comma-separated for multiple)
        #[arg(short = 's', long = "scripts-src", value_delimiter = ',')]
        scripts_src: Vec<String>,

        /// Direct URLs or local paths to add (comma-separated, supports GitHub URLs, Gist URLs, raw scripts, local files)
        #[arg(short = 'd', long = "direct", value_delimiter = ',')]
        direct: Vec<String>,

        /// Output file path (default: manifest.json)
        #[arg(short = 'o', long = "output")]
        output: Option<String>,

        /// GitHub personal access token (or use GITHUB_TOKEN env var)
        #[arg(short = 't', long = "token")]
        token: Option<String>,

        /// Update mode when output file exists
        #[arg(short = 'u', long = "update-mode", value_enum)]
        update_mode: Option<UpdateMode>,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum UpdateMode {
    /// Replace entire manifest file
    Overwrite,
    /// Keep existing entries not in current run, update/add new entries
    Incremental,
}

impl Cli {
    /// Parse CLI arguments
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_cli() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
