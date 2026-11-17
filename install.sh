#!/usr/bin/env bash
set -euo pipefail

echo "dasiiProxy v3 — Rust installer"
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/Cargo is required. Install it from https://rustup.rs and run this script again."
  exit 1
fi

cargo install --path . --force
echo "Installed: ${CARGO_HOME:-$HOME/.cargo}/bin/dasiproxy"
echo "Run: dasiproxy tui"
