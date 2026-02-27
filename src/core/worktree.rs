use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
pub struct WorktreeInfo {
    /// Absolute canonical path to this worktree's root
    pub path: PathBuf,
    /// The resolved git common dir (shared across all worktrees)
    pub git_common_dir: PathBuf,
    /// Whether this is the main worktree
    pub is_main: bool,
}

impl WorktreeInfo {
    pub fn detect() -> Result<Self> {
        let toplevel = git_cmd(&["rev-parse", "--show-toplevel"])?;
        let toplevel = PathBuf::from(&toplevel)
            .canonicalize()
            .with_context(|| format!("failed to canonicalize toplevel: {toplevel}"))?;

        let git_dir_raw = git_cmd(&["rev-parse", "--git-dir"])?;
        let git_dir = if PathBuf::from(&git_dir_raw).is_absolute() {
            PathBuf::from(&git_dir_raw)
                .canonicalize()
                .with_context(|| format!("failed to canonicalize git-dir: {git_dir_raw}"))?
        } else {
            toplevel
                .join(&git_dir_raw)
                .canonicalize()
                .with_context(|| format!("failed to canonicalize git-dir: {git_dir_raw}"))?
        };

        let common_dir_raw = git_cmd(&["rev-parse", "--git-common-dir"])?;
        // git-common-dir may be relative to cwd; resolve it from toplevel
        let common_dir = if PathBuf::from(&common_dir_raw).is_absolute() {
            PathBuf::from(&common_dir_raw)
                .canonicalize()
                .with_context(|| {
                    format!("failed to canonicalize git-common-dir: {common_dir_raw}")
                })?
        } else {
            toplevel
                .join(&common_dir_raw)
                .canonicalize()
                .with_context(|| {
                    format!("failed to canonicalize git-common-dir: {common_dir_raw}")
                })?
        };

        // Main worktree: git-dir and git-common-dir point to the same directory
        // Linked worktree: git-dir points to .git/worktrees/<name>, common-dir points to .git
        let is_main = git_dir == common_dir;

        Ok(WorktreeInfo {
            path: toplevel,
            git_common_dir: common_dir,
            is_main,
        })
    }

    /// Root of the main worktree (parent of the .git directory)
    pub fn main_worktree_root(&self) -> PathBuf {
        if self.is_main {
            self.path.clone()
        } else {
            // common_dir is .git (inside the main worktree root)
            // For linked worktrees, common_dir is resolved through .git/worktrees/*
            // but we already canonicalized it, so it points to the actual .git dir
            self.git_common_dir
                .parent()
                .unwrap_or(&self.git_common_dir)
                .to_path_buf()
        }
    }
}

fn git_cmd(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("failed to execute: git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") {
            return Err(crate::error::Error::NotGitRepo.into());
        }
        return Err(crate::error::Error::GitCommand(format!(
            "git {} failed: {}",
            args.join(" "),
            stderr.trim()
        ))
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
