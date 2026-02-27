use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use crate::cli::Cli;
use crate::core::{
    compose_parser, docker, env_file, port, shared_services, slot_registry, worktree,
};

const DEFAULT_PORT_STEP: u16 = 10;

pub fn run(cli: &Cli) -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;
    let compose_path = cli
        .file
        .clone()
        .unwrap_or_else(|| wt.path.join("docker-compose.yml"));
    let info = compose_parser::parse(&compose_path)?;

    let project_prefix = info.project_prefix.clone().unwrap_or_else(|| {
        wt.main_worktree_root()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string())
    });

    let port_step = env_file::get_var(&wt.path.join(".env"), "WORKTREE_PORT_STEP")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT_STEP);

    let shared_svcs = shared_services::load_shared_services(&wt);

    let registry = slot_registry::SlotRegistry::load(&wt.git_common_dir)
        .context("failed to load slot registry")?;

    let current = &wt.path;

    eprintln!();
    eprint!("{} ", "Worktree Environments".bold());
    eprintln!(
        "{}",
        format!("(step: {port_step}, prefix: {project_prefix})").dimmed()
    );
    if !shared_svcs.is_empty() {
        eprintln!(
            "  {}",
            format!(
                "shared services: {} (main worktree only)",
                shared_svcs.join(", ")
            )
            .dimmed()
        );
    }
    eprintln!();

    // Header
    eprint!("  {:4} {:30}", "SLOT".bold(), "WORKTREE".bold());
    for pm in &info.port_mappings {
        eprint!(" {:12}", pm.env_var.bold());
    }
    eprintln!(" {}", "STATUS".bold());

    // Separator
    eprint!("  {:4} {:30}", "────", "──────────────────────────────");
    for _ in &info.port_mappings {
        eprint!(" {:12}", "────────────");
    }
    eprintln!(" ──────");

    // Rows
    for entry in &registry.entries {
        let is_stale = !entry.path.exists();

        let assignments =
            port::compute_ports(&info.port_mappings, entry.slot, port_step, &shared_svcs)
                .unwrap_or_default();

        let name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| entry.path.display().to_string());

        let is_current = entry.path == *current;

        eprint!("  {:<4} ", entry.slot);
        if is_current {
            eprint!("{:<28} {}", name, "◀".green());
        } else {
            eprint!("{:<30}", name);
        }

        for a in &assignments {
            eprint!(" {:<12}", a.computed_port);
        }

        if is_stale {
            eprint!(" {}", "stale entry (path not found)".yellow());
        } else {
            // Container status
            let project_name = if entry.slot == 0 {
                project_prefix.clone()
            } else {
                let dir_name = entry
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                format!("{project_prefix}-{dir_name}")
            };

            match docker::running_container_count(&project_name) {
                Some(count) if count > 0 => {
                    eprint!(" {}", format!("{count} running").green());
                }
                Some(_) => {
                    eprint!(" {}", "stopped".yellow());
                }
                None => {
                    eprint!(" {}", "-".yellow());
                }
            }
        }

        eprintln!();
    }

    eprintln!();
    Ok(())
}
