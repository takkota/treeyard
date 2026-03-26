use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use crate::core::{docker, override_gen, slot_registry, worktree};

pub fn run() -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;

    // Stop and remove containers before releasing the slot
    match docker::compose_down(&wt.path) {
        Ok(true) => {
            eprintln!("{} Stopped containers", "[treeyard]".green());
        }
        Ok(false) => {
            eprintln!(
                "{} Docker not available, skipping compose down",
                "[treeyard]".yellow()
            );
        }
        Err(e) => {
            eprintln!("{} Failed to stop containers: {}", "[treeyard]".yellow(), e);
        }
    }

    let mut registry = slot_registry::SlotRegistry::load_locked(&wt.git_common_dir)
        .context("failed to load slot registry")?;

    registry.remove(&wt.path);
    registry.save().context("failed to save slot registry")?;

    override_gen::remove_override(&wt.path)?;

    let name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    eprintln!("{} Removed slot for: {}", "[treeyard]".green(), name);

    Ok(())
}
