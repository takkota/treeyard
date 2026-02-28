use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use crate::core::worktree;

const HOOK_MARKER: &str = "# treeyard-managed-hook";

const HOOK_SCRIPT: &str = r#"#!/usr/bin/env bash
# treeyard-managed-hook
# Auto-initialize treeyard on new worktree creation.
# Installed by: treeyard hooks install

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0

# Only run if docker-compose.yml exists
[[ -f "${REPO_ROOT}/docker-compose.yml" ]] || exit 0

# Require treeyard in PATH
command -v treeyard &>/dev/null || exit 0

# Skip if this worktree already has a slot
CURRENT_PATH="$(cd "$REPO_ROOT" && pwd -P)"
GIT_DIR="$(cd "$REPO_ROOT" && cd "$(git rev-parse --git-dir)" && pwd)"
COMMON_DIR="$(cd "$REPO_ROOT" && cd "$(git rev-parse --git-common-dir)" && pwd)"
if [[ "$GIT_DIR" != "$COMMON_DIR" ]]; then
  REGISTRY="${COMMON_DIR}/worktree-slots"
else
  REGISTRY="${GIT_DIR}/worktree-slots"
fi
if [[ -f "$REGISTRY" ]] && grep -qF "$CURRENT_PATH" "$REGISTRY"; then
  exit 0
fi

echo ""
echo "[treeyard] New worktree detected, initializing..."
treeyard init
echo ""
"#;

pub fn install() -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;
    let hooks_dir = resolve_hooks_dir(&wt)?;
    let hook_file = hooks_dir.join("post-checkout");

    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("failed to create hooks dir: {}", hooks_dir.display()))?;

    // Check for existing non-treeyard hook
    if hook_file.exists() {
        let content = fs::read_to_string(&hook_file)?;
        if !content.contains(HOOK_MARKER) {
            anyhow::bail!(
                "Existing post-checkout hook found: {}\nRemove it first, or merge manually.",
                hook_file.display()
            );
        }
    }

    fs::write(&hook_file, HOOK_SCRIPT)
        .with_context(|| format!("failed to write hook: {}", hook_file.display()))?;

    // Make executable
    let mut perms = fs::metadata(&hook_file)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&hook_file, perms)?;

    eprintln!(
        "{} Installed post-checkout hook: {}",
        "[treeyard]".green(),
        hook_file.display()
    );
    eprintln!(
        "{} New worktrees will auto-run {}",
        "[treeyard]".blue(),
        "treeyard init".bold()
    );

    Ok(())
}

pub fn uninstall() -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;
    let hooks_dir = resolve_hooks_dir(&wt)?;
    let hook_file = hooks_dir.join("post-checkout");

    if !hook_file.exists() {
        eprintln!(
            "{} No post-checkout hook found at {}",
            "[treeyard]".yellow(),
            hook_file.display()
        );
        return Ok(());
    }

    let content = fs::read_to_string(&hook_file)?;
    if !content.contains(HOOK_MARKER) {
        anyhow::bail!(
            "Hook at {} was not installed by treeyard. Refusing to remove.",
            hook_file.display()
        );
    }

    fs::remove_file(&hook_file)
        .with_context(|| format!("failed to remove hook: {}", hook_file.display()))?;

    eprintln!(
        "{} Removed post-checkout hook: {}",
        "[treeyard]".green(),
        hook_file.display()
    );

    Ok(())
}

fn resolve_hooks_dir(wt: &worktree::WorktreeInfo) -> Result<PathBuf> {
    // Check core.hooksPath
    let output = Command::new("git")
        .args(["config", "--get", "core.hooksPath"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let custom_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !custom_path.is_empty() {
                let path = PathBuf::from(&custom_path);
                return if path.is_absolute() {
                    Ok(path)
                } else {
                    Ok(wt.path.join(path))
                };
            }
        }
    }

    Ok(wt.git_common_dir.join("hooks"))
}
