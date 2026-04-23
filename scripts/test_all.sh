#!/bin/bash
set -o pipefail
# Resolve repo root relative to this script so the runner works in CI,
# WSL, and Git Bash without a hardcoded path.
cd "$(dirname "$0")/.." || exit 1
# Prefer the local toolchain when present (interactive WSL sessions);
# in CI $PATH already has cargo from the setup step.
if [ -d "$HOME/.cargo/bin" ]; then
  export PATH="$HOME/.cargo/bin:$PATH"
fi
cargo test --workspace 2>&1 | tail -80
