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
# In memory-constrained environments (7 GB GitHub runner) the tauri
# crate graph OOM-kills the agent. Set NEXUS_SKIP_TAURI_CRATES=1 to
# exclude nexus-app (the only crate pulling webkit2gtk/soup3). Local
# runs keep the full workspace.
if [ "${NEXUS_SKIP_TAURI_CRATES:-0}" = "1" ]; then
  cargo test --workspace --exclude nexus-app 2>&1 | tail -80
else
  cargo test --workspace 2>&1 | tail -80
fi
