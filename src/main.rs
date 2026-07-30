//! wenget - A cross-platform package manager for GitHub binaries

mod bucket;
mod cache;
mod cli;
mod commands;
mod core;
mod downloader;
mod installer;
mod package_resolver;
mod providers;
mod utils;

use clap::CommandFactory;
use cli::{BucketCommands, Cli, Commands};
use colored::Colorize;

fn main() {
    // Initialize logger
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();

    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Set verbose logging if requested
    if cli.verbose {
        log::set_max_level(log::LevelFilter::Debug);
    }

    // Handle no command (show help and exit 0)
    let Some(command) = cli.command else {
        let _ = Cli::command().print_help();
        println!(); // Add newline after help
        return;
    };

    // Run the appropriate command
    let result = match command {
        Commands::Init { yes } => commands::run_init(yes),

        Commands::Bucket { command } => {
            let bucket_cmd = match command {
                BucketCommands::Add { name, url } => {
                    commands::bucket::BucketCommand::Add { name, url }
                }
                BucketCommands::Del { names } => commands::bucket::BucketCommand::Del { names },
                BucketCommands::List => commands::bucket::BucketCommand::List,
                BucketCommands::Refresh => commands::bucket::BucketCommand::Refresh,
                BucketCommands::Create {
                    repos_src,
                    scripts_src,
                    direct,
                    output,
                    token,
                    update_mode,
                } => commands::bucket::BucketCommand::Create {
                    repos_src,
                    scripts_src,
                    direct,
                    output,
                    token,
                    update_mode,
                },
            };
            commands::run_bucket(bucket_cmd)
        }

        Commands::Add {
            names,
            yes,
            script_name,
            platform,
            pkg_version,
            variant,
            no_suffix,
        } => commands::run_add(
            names,
            yes,
            script_name,
            platform,
            pkg_version,
            variant,
            no_suffix,
            false,
        ),

        Commands::List { all } => commands::run_list(all),

        Commands::Info { names } => commands::run_info(names),

        Commands::Search { names } => commands::run_search(names),

        Commands::Update {
            names,
            yes,
            platform,
        } => commands::run_update(names, yes, platform),

        Commands::Del {
            names,
            yes,
            force,
            variant,
        } => commands::run_delete(names, yes, force, variant),

        Commands::Repair { force } => commands::run_repair(force),

        Commands::Config => (|| {
            let config = core::Config::new()?;
            commands::run_config(&config)
        })(),

        Commands::Rename { old_name, new_name } => (|| {
            let config = core::Config::new()?;
            commands::run_rename(old_name, new_name, &config)
        })(),
    };

    // Handle errors
    if let Err(e) = result {
        eprintln!("{} {}", "Error:".red().bold(), e);
        std::process::exit(1);
    }
}
