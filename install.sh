#!/bin/bash
set -euo pipefail

REPO="takkota/treeyard"

# Detect install root: use ~/.cargo if it's in PATH, otherwise ~/.local
if echo "$PATH" | tr ':' '\n' | grep -q "$HOME/.cargo/bin"; then
  ROOT="$HOME/.cargo"
else
  ROOT="$HOME/.local"
fi

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Installing treeyard to $ROOT/bin ..."
gh repo clone "$REPO" "$TMPDIR/treeyard" -- --depth 1 --quiet
cargo install --path "$TMPDIR/treeyard" --root "$ROOT"
echo "Installed to $ROOT/bin/treeyard"
