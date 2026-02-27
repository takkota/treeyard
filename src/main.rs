mod cli;
mod commands;
mod core;
mod error;

use clap::Parser;
use cli::{Cli, Commands, HooksCommand};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Init => commands::init::run(&cli)?,
        Commands::Status => commands::status::run(&cli)?,
        Commands::Env => commands::env::run(&cli)?,
        Commands::Cleanup => commands::cleanup::run()?,
        Commands::Prune => commands::prune::run()?,
        Commands::Hooks { command } => match command {
            HooksCommand::Install => commands::hooks::install()?,
            HooksCommand::Uninstall => commands::hooks::uninstall()?,
        },
    }

    Ok(())
}
