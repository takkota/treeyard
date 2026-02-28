use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use crate::core::{slot_registry, worktree};

pub fn run() -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;

    let mut registry = slot_registry::SlotRegistry::load_locked(&wt.git_common_dir)
        .context("failed to load slot registry")?;

    let pruned = registry.prune_stale();
    if !pruned.is_empty() {
        registry.save().context("failed to save slot registry")?;
        for entry in &pruned {
            eprintln!(
                "{} Pruned stale slot {}: {}",
                "[treeyard]".yellow(),
                entry.slot,
                entry.path.display()
            );
        }
        eprintln!(
            "{} Pruned {} stale entries",
            "[treeyard]".green(),
            pruned.len()
        );
    } else {
        eprintln!(
            "{} Registry is clean, nothing to prune",
            "[treeyard]".green(),
        );
    }

    Ok(())
}
