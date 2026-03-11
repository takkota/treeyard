use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Create a minimal git repo with a docker-compose.yml containing port variables.
fn setup_git_repo(dir: &std::path::Path) {
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .expect("git init failed");

    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir)
        .output()
        .expect("git config failed");

    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir)
        .output()
        .expect("git config failed");

    fs::write(
        dir.join("docker-compose.yml"),
        indoc::indoc! {r#"
            services:
              web:
                image: nginx
                ports:
                  - "${WEB_PORT:-3000}:3000"
              api:
                image: node
                ports:
                  - "${API_PORT:-8080}:8080"

            networks:
              default:
                name: ${COMPOSE_PROJECT_NAME:-testproject}-network
        "#},
    )
    .expect("write docker-compose.yml failed");

    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(dir)
        .output()
        .expect("git add failed");

    std::process::Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(dir)
        .output()
        .expect("git commit failed");
}

/// Strip ANSI escape codes from a byte slice.
fn strip_ansi(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let re = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    re.replace_all(&s, "").to_string()
}

#[test]
fn test_no_subcommand_shows_help() {
    Command::cargo_bin("tyd")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn test_version_flag() {
    Command::cargo_bin("tyd")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("treeyard"));
}

#[test]
fn test_init_outside_git_repo() {
    let dir = TempDir::new().unwrap();
    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not inside a git repository"));
}

#[test]
fn test_init_without_compose_file() {
    let dir = TempDir::new().unwrap();

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("docker-compose.yml not found"));
}

#[test]
fn test_init_main_worktree() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd init");

    assert!(output.status.success(), "tyd init should succeed");

    let stderr = strip_ansi(&output.stderr);
    assert!(stderr.contains("Slot 0 assigned"), "should assign slot 0");
    assert!(
        stderr.contains("COMPOSE_PROJECT_NAME=testproject"),
        "should set project name"
    );
    assert!(stderr.contains("WEB_PORT=3000"), "should show WEB_PORT");
    assert!(stderr.contains("API_PORT=8080"), "should show API_PORT");

    // Check .env was created with correct values
    let env_content = fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(env_content.contains("COMPOSE_PROJECT_NAME=testproject"));
    assert!(env_content.contains("WEB_PORT=3000"));
    assert!(env_content.contains("API_PORT=8080"));

    // Check override file was generated
    let override_path = dir.path().join("docker-compose.override.yml");
    assert!(override_path.exists());
    let override_content = fs::read_to_string(&override_path).unwrap();
    assert!(override_content.contains("testproject-shared"));
}

#[test]
fn test_init_idempotent() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    let env_first = fs::read_to_string(dir.path().join(".env")).unwrap();

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    let env_second = fs::read_to_string(dir.path().join(".env")).unwrap();
    assert_eq!(env_first, env_second);
}

#[test]
fn test_status_after_init() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .arg("status")
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd status");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(stderr.contains("SLOT"), "should show table header");
    assert!(stderr.contains("WORKTREE"), "should show table header");
}

#[test]
fn test_env_not_initialized() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("env")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not initialized"));
}

#[test]
fn test_env_after_init() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("env")
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(
            predicate::str::contains("WEB_PORT=3000")
                .and(predicate::str::contains("API_PORT=8080")),
        );
}

#[test]
fn test_cleanup_after_init() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .arg("cleanup")
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd cleanup");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("Removed slot for"),
        "should confirm removal"
    );

    // After cleanup, env should fail
    Command::cargo_bin("tyd")
        .unwrap()
        .arg("env")
        .current_dir(dir.path())
        .assert()
        .failure();
}

#[test]
fn test_prune_no_stale() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .arg("prune")
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd prune");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("nothing to prune"),
        "should report clean registry"
    );
}

#[test]
fn test_init_with_custom_port_step() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    fs::write(dir.path().join(".env"), "WORKTREE_PORT_STEP=100\n").unwrap();

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd init");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(stderr.contains("Slot 0 assigned"));

    let env_content = fs::read_to_string(dir.path().join(".env")).unwrap();
    assert!(env_content.contains("WEB_PORT=3000"));
}

#[test]
fn test_init_rejects_port_step_zero() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    fs::write(dir.path().join(".env"), "WORKTREE_PORT_STEP=0\n").unwrap();

    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("WORKTREE_PORT_STEP=0 is invalid"));
}

#[test]
fn test_hooks_install_and_uninstall() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .args(["hooks", "install"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run hooks install");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("Installed post-checkout hook"),
        "should confirm install"
    );

    let hook_path = dir.path().join(".git/hooks/post-checkout");
    assert!(hook_path.exists());
    let hook_content = fs::read_to_string(&hook_path).unwrap();
    assert!(
        !hook_content.contains("--auto-prune"),
        "default install should not include --auto-prune"
    );

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .args(["hooks", "uninstall"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run hooks uninstall");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("Removed post-checkout hook"),
        "should confirm uninstall"
    );
}

#[test]
fn test_hooks_install_with_auto_prune() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    let output = Command::cargo_bin("tyd")
        .unwrap()
        .args(["hooks", "install", "--auto-prune"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run hooks install --auto-prune");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("treeyard init --auto-prune"),
        "should show auto-prune in message"
    );

    let hook_path = dir.path().join(".git/hooks/post-checkout");
    let hook_content = fs::read_to_string(&hook_path).unwrap();
    assert!(
        hook_content.contains("treeyard init --auto-prune"),
        "hook script should include --auto-prune flag"
    );
}

#[test]
fn test_init_auto_prune_removes_stale_entries() {
    let dir = TempDir::new().unwrap();
    setup_git_repo(dir.path());

    // First, init to register this worktree
    Command::cargo_bin("tyd")
        .unwrap()
        .arg("init")
        .current_dir(dir.path())
        .assert()
        .success();

    // Manually add a stale entry to the registry (path that doesn't exist)
    let registry_path = dir.path().join(".git/worktree-slots");
    let existing = fs::read_to_string(&registry_path).unwrap();
    fs::write(
        &registry_path,
        format!(
            "{}/nonexistent/worktree\t1\n{}",
            dir.path().display(),
            existing
        ),
    )
    .unwrap();

    // Verify stale entry is present
    let content = fs::read_to_string(&registry_path).unwrap();
    assert!(content.contains("/nonexistent/worktree"));

    // Run init with --auto-prune
    let output = Command::cargo_bin("tyd")
        .unwrap()
        .args(["init", "--auto-prune"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run tyd init --auto-prune");

    assert!(output.status.success());
    let stderr = strip_ansi(&output.stderr);
    assert!(
        stderr.contains("Pruned stale slot 1"),
        "should report pruned stale entry"
    );

    // Verify stale entry was removed from registry
    let content = fs::read_to_string(&registry_path).unwrap();
    assert!(
        !content.contains("/nonexistent/worktree"),
        "stale entry should be removed"
    );
}
