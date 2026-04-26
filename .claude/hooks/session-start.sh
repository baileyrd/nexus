#!/bin/bash
set -euo pipefail

# Only run in remote (Claude Code on the web) sessions.
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "$CLAUDE_PROJECT_DIR"

echo "[session-start] Pre-fetching Rust workspace dependencies..."
cargo fetch --locked 2>/dev/null || cargo fetch

echo "[session-start] Installing pnpm workspace dependencies..."
pnpm install --prefer-offline

echo "[session-start] Done."
