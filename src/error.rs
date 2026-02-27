use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not inside a git repository")]
    NotGitRepo,

    #[error("docker-compose.yml not found at {0}")]
    ComposeFileNotFound(PathBuf),

    #[error("failed to parse slot registry at {path}: {reason}")]
    SlotRegistryCorrupt { path: PathBuf, reason: String },

    #[error("this worktree is not initialized (run 'worktree-compose init' first)")]
    NotInitialized,

    #[error("failed to execute git command: {0}")]
    GitCommand(String),

    #[error(
        "port {computed} for {env_var} would exceed 65535 (base={base}, slot={slot}, step={step})"
    )]
    PortOverflow {
        env_var: String,
        base: u16,
        computed: u32,
        slot: u32,
        step: u16,
    },
}
