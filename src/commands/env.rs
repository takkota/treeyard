use anyhow::{Context, Result};

use crate::cli::Cli;
use crate::core::{compose_parser, env_file, port, shared_services, slot_registry, worktree};

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

    let registry = slot_registry::SlotRegistry::load(&wt.git_common_dir)
        .context("failed to load slot registry")?;

    let slot = registry
        .get_slot(&wt.path)
        .ok_or(crate::error::Error::NotInitialized)?;

    let port_step = env_file::get_var(&wt.path.join(".env"), "WORKTREE_PORT_STEP")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT_STEP);

    let shared_svcs = shared_services::load_shared_services(&wt);
    let assignments = port::compute_ports(&info.port_mappings, slot, port_step, &shared_svcs)?;

    let project_name = if slot == 0 {
        project_prefix
    } else {
        let dir_name = wt
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("wt{slot}"));
        format!("{project_prefix}-{dir_name}")
    };

    println!("COMPOSE_PROJECT_NAME={project_name}");
    for a in &assignments {
        println!("{}={}", a.env_var, a.computed_port);
    }

    Ok(())
}
