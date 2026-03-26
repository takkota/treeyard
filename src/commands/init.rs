use std::path::Path;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use crate::cli::Cli;
use crate::core::{
    compose_parser, docker, env_file, override_gen, port, shared_services, slot_registry, worktree,
};

const DEFAULT_PORT_STEP: u16 = 10;

pub fn run(cli: &Cli, auto_prune: bool) -> Result<()> {
    let wt = worktree::WorktreeInfo::detect()?;

    // Resolve compose file
    let compose_path = cli
        .file
        .clone()
        .unwrap_or_else(|| wt.path.join("docker-compose.yml"));
    let info = compose_parser::parse(&compose_path)?;

    // Warn about hardcoded ports
    for hp in &info.hardcoded_ports {
        let svc_upper = hp.service.replace('-', "_").to_uppercase();
        eprintln!(
            "{} Service '{}' has hardcoded port {} — cannot auto-offset.",
            "[treeyard]".yellow(),
            hp.service,
            hp.host_port
        );
        eprintln!(
            "{}   Tip: change to ${{{}_PORT:-{}}}:{} in docker-compose.yml",
            "[treeyard]".yellow(),
            svc_upper,
            hp.host_port,
            hp.container_port
        );
    }

    // Warn about fixed-name volumes (will be overridden to project-prefixed names)
    for vol in &info.fixed_name_volumes {
        eprintln!(
            "{} Volume '{}' has fixed name '{}' — overriding to ${{COMPOSE_PROJECT_NAME}}_{}",
            "[treeyard]".yellow(),
            vol.key,
            vol.name,
            vol.key
        );
    }

    // Inform about container_name overrides
    for fc in &info.fixed_container_names {
        eprintln!(
            "{} Service '{}' has fixed container_name '{}' — overriding to ${{COMPOSE_PROJECT_NAME}}-{}",
            "[treeyard]".yellow(),
            fc.service,
            fc.name,
            fc.service
        );
    }

    // Inform about hostname overrides
    for fh in &info.fixed_hostnames {
        eprintln!(
            "{} Service '{}' has fixed hostname '{}' — overriding to ${{COMPOSE_PROJECT_NAME}}-{}",
            "[treeyard]".yellow(),
            fh.service,
            fh.hostname,
            fh.service
        );
    }

    // Inform about container ref rewrites
    for cr in &info.container_refs {
        // Check if the referenced name maps to a known service
        let target = info
            .fixed_container_names
            .iter()
            .find(|fc| fc.name == cr.referenced_name);
        if let Some(fc) = target {
            eprintln!(
                "{} Service '{}' has {}: \"container:{}\" — rewriting to \"service:{}\"",
                "[treeyard]".yellow(),
                cr.service,
                cr.directive,
                cr.referenced_name,
                fc.service
            );
        } else {
            eprintln!(
                "{} Service '{}' has {}: \"container:{}\" — referenced container not found in this compose file, cannot auto-rewrite",
                "[treeyard]".yellow(),
                cr.service,
                cr.directive,
                cr.referenced_name
            );
        }
    }

    // Print warnings for potentially problematic directives
    for w in &info.warnings {
        eprintln!(
            "{} Service '{}' uses {} — {}",
            "[treeyard]".yellow(),
            w.service,
            w.directive,
            w.message
        );
    }

    if info.port_mappings.is_empty() {
        eprintln!(
            "{} No port variables detected in docker-compose.yml.",
            "[treeyard]".yellow()
        );
        eprintln!(
            "{}   Use ${{VAR:-DEFAULT}}:CONTAINER format for auto-detection.",
            "[treeyard]".yellow()
        );
    }

    // Determine project prefix (always based on the main worktree to keep
    // the shared network name consistent across all worktrees)
    let project_prefix = info.project_prefix.clone().unwrap_or_else(|| {
        let prefix_source = wt.main_worktree_root();
        let name = prefix_source
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string());
        eprintln!(
            "{} Could not detect project prefix, using directory name: {}",
            "[treeyard]".yellow(),
            name
        );
        name
    });

    // Ensure .env exists before reading from it
    let created = env_file::ensure_env_file(&wt.path)?;
    if created {
        let source = if wt.path.join(".env.example").exists() {
            ".env.example"
        } else {
            "empty"
        };
        eprintln!("{} Created .env from {}", "[treeyard]".blue(), source);
    }

    let main_root = wt.main_worktree_root();

    // Auto-initialize main worktree if needed
    if !wt.is_main {
        let main_override = main_root.join("docker-compose.override.yml");
        if !main_override.exists() {
            if let Err(e) = auto_init_main(cli, &wt, &project_prefix) {
                eprintln!(
                    "{} Could not auto-initialize main worktree: {}",
                    "[treeyard]".yellow(),
                    e
                );
                eprintln!(
                    "{}   Run 'treeyard init' in the main worktree manually if needed.",
                    "[treeyard]".yellow(),
                );
            }
        }
    }

    // Load slot registry and assign slot
    let mut registry = slot_registry::SlotRegistry::load_locked(&wt.git_common_dir)
        .context("failed to load slot registry")?;

    // Auto-prune stale entries before assigning a slot
    if auto_prune {
        let pruned = registry.prune_stale();
        for entry in &pruned {
            // Skip stopping containers for slot 0 (main worktree) to avoid
            // accidentally taking down the main project on a corrupted registry.
            if entry.slot == 0 {
                eprintln!(
                    "{} Pruned stale slot {}: {}",
                    "[treeyard]".yellow(),
                    entry.slot,
                    entry.path.display()
                );
                continue;
            }

            // Stop containers for the stale worktree using its project name
            let stale_project = format!("{project_prefix}-wt{}", entry.slot);
            match docker::compose_down_project(&stale_project) {
                Ok(true) => {
                    eprintln!(
                        "{} Stopped containers for stale project: {}",
                        "[treeyard]".yellow(),
                        stale_project
                    );
                }
                Ok(false) => {}
                Err(e) => {
                    eprintln!(
                        "{} Failed to stop containers for {}: {}",
                        "[treeyard]".yellow(),
                        stale_project,
                        e
                    );
                }
            }
            eprintln!(
                "{} Pruned stale slot {}: {}",
                "[treeyard]".yellow(),
                entry.slot,
                entry.path.display()
            );
        }
    }

    let slot = registry.assign_slot(&wt.path, &main_root);
    registry.save().context("failed to save slot registry")?;

    // Read port step from .env or use default
    let port_step = env_file::get_var(&wt.path.join(".env"), "WORKTREE_PORT_STEP")
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(DEFAULT_PORT_STEP);

    if port_step == 0 {
        anyhow::bail!(
            "WORKTREE_PORT_STEP=0 is invalid (all worktrees would use the same ports). \
             Remove it from .env or set a positive value (default: {DEFAULT_PORT_STEP})."
        );
    }

    // Load shared services
    let shared_svcs = shared_services::load_shared_services(&wt);

    // Compute ports
    let assignments = port::compute_ports(&info.port_mappings, slot, port_step, &shared_svcs)?;

    // Compute project name
    let project_name = if slot == 0 {
        project_prefix.clone()
    } else {
        format!("{project_prefix}-wt{slot}")
    };

    let wt_type = if wt.is_main { "main" } else { "linked" };
    let wt_name = wt
        .path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    eprintln!(
        "{} Initializing {} worktree: {}",
        "[treeyard]".blue(),
        wt_type,
        wt_name
    );

    let env_path = wt.path.join(".env");

    // Collect old port values for URL rewriting
    let old_ports: Vec<(String, u16)> = assignments
        .iter()
        .map(|a| {
            let old = env_file::get_var(&env_path, &a.env_var)
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(a.base_port);
            (a.env_var.clone(), old)
        })
        .collect();

    // Update COMPOSE_PROJECT_NAME
    env_file::set_var(&env_path, "COMPOSE_PROJECT_NAME", &project_name)?;

    // Update port variables
    for a in &assignments {
        env_file::set_var(&env_path, &a.env_var, &a.computed_port.to_string())?;
    }

    // Auto-update derived URL variables (batch to avoid cascading replacements)
    let port_pairs: Vec<(u16, u16)> = assignments
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            let old = old_ports[i].1;
            if old != a.computed_port {
                Some((old, a.computed_port))
            } else {
                None
            }
        })
        .collect();
    if !port_pairs.is_empty() {
        env_file::replace_ports_in_urls_batch(&env_path, &port_pairs)?;
    }

    // Update COMPOSE_PROFILES for shared services
    if !shared_svcs.is_empty() {
        shared_services::update_profiles(&wt.path, slot)?;
    }

    // Print summary
    eprintln!();
    eprintln!("{} Slot {} assigned", "[treeyard]".green(), slot.bold());
    eprintln!("   COMPOSE_PROJECT_NAME={}", project_name.cyan());
    for a in &assignments {
        if a.is_shared {
            eprintln!(
                "   {}={} {}",
                a.env_var,
                a.computed_port,
                "(shared)".dimmed()
            );
        } else if a.computed_port == a.base_port {
            eprintln!("   {}={}", a.env_var, a.computed_port);
        } else {
            eprintln!(
                "   {}={} {}",
                a.env_var,
                a.computed_port.cyan(),
                format!("(base: {})", a.base_port).dimmed()
            );
        }
    }
    if !shared_svcs.is_empty() {
        eprintln!(
            "   {}",
            format!("shared services: {}", shared_svcs.join(", ")).dimmed()
        );
    }

    // Generate override
    let override_content = override_gen::generate(&override_gen::OverrideConfig {
        project_prefix: &project_prefix,
        services: &info.services,
        shared_services_list: &shared_svcs,
        fixed_name_volumes: &info.fixed_name_volumes,
        fixed_container_names: &info.fixed_container_names,
        fixed_hostnames: &info.fixed_hostnames,
        container_refs: &info.container_refs,
        version: env!("CARGO_PKG_VERSION"),
        is_linked: !wt.is_main,
        depends_on: &info.depends_on,
    });
    override_gen::write_override(&wt.path, &override_content)?;

    let shared_network = format!("{project_prefix}-shared");
    if let Ok(created) = docker::ensure_network(&shared_network) {
        if created {
            eprintln!(
                "{} Created shared network: {}",
                "[treeyard]".blue(),
                shared_network
            );
        }
    }

    eprintln!(
        "{} Generated docker-compose.override.yml (shared network: {})",
        "[treeyard]".blue(),
        shared_network
    );

    eprintln!();
    eprintln!(
        "{} Done! Run {} to start services.",
        "[treeyard]".green(),
        "docker compose up -d".bold()
    );

    Ok(())
}

/// Auto-initialize the main worktree when a linked worktree runs `tyd init`
/// and main has not been initialized yet. This generates
/// `docker-compose.override.yml` for main and sets up shared service profiles
/// if needed, so the user never has to explicitly init the main worktree.
fn auto_init_main(cli: &Cli, wt: &worktree::WorktreeInfo, project_prefix: &str) -> Result<()> {
    let main_root = wt.main_worktree_root();

    // Resolve compose file for the main worktree
    let main_compose_path = resolve_main_compose_path(cli, &main_root, &wt.path);
    let main_info = compose_parser::parse(&main_compose_path).with_context(|| {
        format!(
            "failed to parse main worktree compose file: {}",
            main_compose_path.display()
        )
    })?;

    // Load shared services (reads from main's .env / .env.example)
    let shared_svcs = shared_services::load_shared_services(wt);

    // If shared services are configured, ensure main's .env has the profile
    if !shared_svcs.is_empty() {
        env_file::ensure_env_file(&main_root)?;
        shared_services::update_profiles(&main_root, 0)?;
    }

    // Generate main's docker-compose.override.yml
    let override_content = override_gen::generate(&override_gen::OverrideConfig {
        project_prefix,
        services: &main_info.services,
        shared_services_list: &shared_svcs,
        fixed_name_volumes: &main_info.fixed_name_volumes,
        fixed_container_names: &main_info.fixed_container_names,
        fixed_hostnames: &main_info.fixed_hostnames,
        container_refs: &main_info.container_refs,
        version: env!("CARGO_PKG_VERSION"),
        is_linked: false,
        depends_on: &main_info.depends_on,
    });
    override_gen::write_override(&main_root, &override_content)?;

    eprintln!(
        "{} Auto-initialized main worktree (generated docker-compose.override.yml in {})",
        "[treeyard]".blue(),
        main_root.display()
    );

    Ok(())
}

/// Resolve the compose file path for the main worktree.
///
/// If the user specified `--file`, we try to find the equivalent file in main:
/// - If the path is inside the current worktree, compute the relative path and
///   resolve it from the main worktree root.
/// - Otherwise, fall back to the default `docker-compose.yml` in main.
fn resolve_main_compose_path(
    cli: &Cli,
    main_root: &Path,
    current_wt_root: &Path,
) -> std::path::PathBuf {
    if let Some(ref file) = cli.file {
        // Try to compute relative path from current worktree
        if let Ok(rel) = file.strip_prefix(current_wt_root) {
            let candidate = main_root.join(rel);
            if candidate.exists() {
                return candidate;
            }
        }
        // If --file is a relative path, resolve from main root
        if file.is_relative() {
            let candidate = main_root.join(file);
            if candidate.exists() {
                return candidate;
            }
        }
        eprintln!(
            "{} --file '{}' not found in main worktree, using docker-compose.yml",
            "[treeyard]".yellow(),
            file.display()
        );
    }
    // Default
    main_root.join("docker-compose.yml")
}
