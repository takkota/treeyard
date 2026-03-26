use std::process::Command;

/// Ensure a Docker network exists, creating it if necessary.
/// Returns Ok(true) if created, Ok(false) if already existed.
pub fn ensure_network(name: &str) -> anyhow::Result<bool> {
    if !is_docker_available() {
        return Ok(false);
    }

    // Check if network exists
    let output = Command::new("docker")
        .args(["network", "inspect", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match output {
        Ok(status) if status.success() => Ok(false),
        _ => {
            // Create it
            let status = Command::new("docker")
                .args(["network", "create", name])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            Ok(status.success())
        }
    }
}

/// Get the number of running containers for a given compose project.
pub fn running_container_count(project_name: &str) -> Option<usize> {
    if !is_docker_available() {
        return None;
    }

    let output = Command::new("docker")
        .args([
            "compose",
            "-p",
            project_name,
            "ps",
            "--status",
            "running",
            "-q",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return Some(0);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.lines().filter(|l| !l.trim().is_empty()).count())
}

/// Run `docker compose down` in the given directory to stop and remove containers.
/// Returns Ok(true) if successful, Ok(false) if docker is unavailable.
pub fn compose_down(working_dir: &std::path::Path) -> anyhow::Result<bool> {
    if !is_docker_available() {
        return Ok(false);
    }

    let status = Command::new("docker")
        .args(["compose", "down"])
        .current_dir(working_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    Ok(status.success())
}

/// Run `docker compose down` for a given project name (no working directory needed).
/// This is used when the worktree directory no longer exists.
/// Returns Ok(true) if successful, Ok(false) if docker is unavailable.
/// Returns Err if docker is available but `compose down` fails.
pub fn compose_down_project(project_name: &str) -> anyhow::Result<bool> {
    if !is_docker_available() {
        return Ok(false);
    }

    let status = Command::new("docker")
        .args(["compose", "-p", project_name, "down"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if status.success() {
        Ok(true)
    } else {
        anyhow::bail!(
            "docker compose down failed for project '{project_name}' (exit code: {})",
            status.code().map_or("unknown".to_string(), |c| c.to_string())
        );
    }
}

fn is_docker_available() -> bool {
    Command::new("docker")
        .arg("version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
