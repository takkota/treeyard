use std::path::Path;

use crate::core::worktree::WorktreeInfo;

const SHARED_PROFILE: &str = "_wt_main_only";

/// Read WORKTREE_SHARED_SERVICES from the main worktree's .env (or .env.example).
/// This ensures all worktrees share a consistent view of which services are shared.
pub fn load_shared_services(wt: &WorktreeInfo) -> Vec<String> {
    let main_root = wt.main_worktree_root();
    let local_path = main_root.join(".env.local");
    let env_path = main_root.join(".env");
    let example_path = main_root.join(".env.example");

    let val = read_shared_var(&local_path)
        .or_else(|| read_shared_var(&env_path))
        .or_else(|| read_shared_var(&example_path))
        .unwrap_or_default();

    if val.is_empty() {
        return Vec::new();
    }

    val.split([',', ' '])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn read_shared_var(path: &Path) -> Option<String> {
    crate::core::env_file::get_var(path, "WORKTREE_SHARED_SERVICES")
}

/// Update COMPOSE_PROFILES in .env to control shared service activation.
/// - slot 0 (main): ensure SHARED_PROFILE is present
/// - slot > 0 (linked): remove SHARED_PROFILE
pub fn update_profiles(toplevel: &Path, slot: u32) -> anyhow::Result<()> {
    let env_path = toplevel.join(".env");
    let current = crate::core::env_file::get_var(&env_path, "COMPOSE_PROFILES").unwrap_or_default();

    let new_profiles = if slot == 0 {
        // Main: ensure shared profile is active
        if current.is_empty() {
            SHARED_PROFILE.to_string()
        } else if current.contains(SHARED_PROFILE) {
            current
        } else {
            format!("{current},{SHARED_PROFILE}")
        }
    } else {
        // Linked: remove shared profile
        current
            .split(',')
            .filter(|p| p.trim() != SHARED_PROFILE)
            .collect::<Vec<_>>()
            .join(",")
    };

    crate::core::env_file::set_var(&env_path, "COMPOSE_PROFILES", &new_profiles)
}

pub fn shared_profile_name() -> &'static str {
    SHARED_PROFILE
}

/// Check if a service is in the shared services list
pub fn is_shared(service: &str, shared_services: &[String]) -> bool {
    shared_services.iter().any(|s| s == service)
}
