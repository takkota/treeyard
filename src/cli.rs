use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "treeyard",
    version,
    about = "Manage Docker Compose environments across git worktrees"
)]
pub struct Cli {
    /// Path to docker-compose.yml (default: auto-detect from git toplevel)
    #[arg(short, long, global = true)]
    pub file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize port assignments for this worktree
    Init,
    /// Show all worktrees and their port/container status
    Status,
    /// Print computed environment variables
    Env,
    /// Remove this worktree from the slot registry
    Cleanup,
    /// Remove stale (non-existent) worktree entries
    Prune,
    /// Manage git hooks
    Hooks {
        #[command(subcommand)]
        command: HooksCommand,
    },
}

#[derive(Subcommand)]
pub enum HooksCommand {
    /// Install post-checkout hook for auto-initialization
    Install,
    /// Remove the post-checkout hook
    Uninstall,
}
