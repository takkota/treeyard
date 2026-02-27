#!/bin/bash
set -euo pipefail

REPO="takkota/worktree-compose"

# Detect install root: use ~/.cargo if it's in PATH, otherwise ~/.local
if echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.cargo/bin"; then
  ROOT="$HOME/.cargo"
else
  ROOT="$HOME/.local"
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Installing worktree-compose to $ROOT/bin ..."
gh repo clone "$REPO" "$TMPDIR/worktree-compose" -- --depth 1 --quiet
cargo install --path "$TMPDIR/worktree-compose" --root "$ROOT"
echo "Installed to $ROOT/bin/worktree-compose"
