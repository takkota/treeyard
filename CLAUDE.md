# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run all unit tests
cargo test <test_name>         # Run a single test by name
cargo fmt --check              # Check formatting
cargo clippy -- -D warnings    # Lint (CI treats warnings as errors)
make install                   # Build release and install to ~/.cargo/bin or ~/.local/bin
```

CI runs `fmt --check`, `clippy -D warnings`, and `cargo test` on both Ubuntu and macOS.

## Architecture

**treeyard (`tyd`)** is a CLI tool that resolves Docker Compose port collisions across multiple git worktrees. It assigns each worktree a slot number and offsets host ports by `base_port + (slot × step)`.

### Module Layout

- **`src/cli.rs`** — clap derive structs for CLI parsing
- **`src/error.rs`** — domain error enum via thiserror
- **`src/main.rs`** — subcommand dispatch to `commands::*`
- **`src/commands/`** — one file per subcommand (`init`, `status`, `env`, `cleanup`, `prune`, `hooks`)
- **`src/core/`** — shared logic:
  - `compose_parser` — `serde_yaml_ng` based parser with `${VAR:-DEFAULT}` pre-processing (placeholders before YAML parse, restore after); detects port patterns, hardcoded ports, project prefix, services, networks, volumes, container_name/hostname, container refs, and YAML anchors/merge keys
  - `slot_registry` — TSV file at `.git/worktree-slots` with `fs2` file locking for concurrent safety
  - `port` — port offset arithmetic with overflow validation
  - `env_file` — `.env` read/write and batch URL port replacement (single-pass regex to avoid cascading substitutions)
  - `override_gen` — generates `docker-compose.override.yml` with shared network config and shared service profiles
  - `shared_services` — reads `WORKTREE_SHARED_SERVICES` from main worktree's `.env`, manages `COMPOSE_PROFILES`
  - `docker` — thin wrapper around `docker` CLI commands
  - `worktree` — git worktree detection via `git rev-parse`

### Central Flow (`tyd init`)

1. Detect worktree info via git CLI → `WorktreeInfo`
2. Parse `docker-compose.yml` → `ComposeInfo` (serde_yaml_ng with `${...}` pre-processing)
3. Assign slot number via locked `SlotRegistry`
4. Compute port assignments and write to `.env`
5. Batch-rewrite `://localhost:<old>` URLs in `.env`
6. Generate `docker-compose.override.yml` for shared Docker network
7. Ensure shared Docker network exists

### Key Design Decisions

- **YAML parsing with pre-processing**: `compose_parser` uses `serde_yaml_ng` to parse the YAML structure. Since `${VAR:-default}` is not valid YAML, the parser pre-processes the content by replacing `${...}` patterns with safe placeholders before parsing, then restores them when examining string values. Regex is still used for extracting port variables and project prefixes from the restored string values. YAML anchors (`&name`) and merge keys (`<<: *name`) are resolved via `apply_merge()`.
- **YAML 1.2 compliance**: `serde_yaml_ng` follows YAML 1.2, which does not have YAML 1.1's sexagesimal number interpretation. Unquoted port mappings like `5432:5432` are correctly treated as strings.
- **File locking**: `SlotRegistry` uses `fs2` for cross-process exclusive locking to prevent TOCTOU races when multiple worktrees init simultaneously.
- **Two binary targets**: Both `treeyard` and `tyd` are built from the same `src/main.rs`.
- **MSRV**: Requires Rust 1.80+ (uses `std::sync::LazyLock`).

## Testing

All tests are unit tests co-located in source files (`#[cfg(test)]` blocks). No integration tests exist yet. Tests use `tempfile` for temporary directories and `indoc` for clean multi-line string literals.

## Language

README and user-facing messages are in Japanese.
