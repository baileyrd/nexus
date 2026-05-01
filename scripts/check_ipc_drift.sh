#!/usr/bin/env bash
# Phase 4 WI-36 — drift check for regenerable IPC schemas.
#
# Contract: a PR that mutates a pilot IPC type without regenerating its
# TypeScript binding AND its JSON Schema fails this check. CI invokes
# this script after `cargo test`; local devs run it before opening a PR.
#
# Regenerates:
#   * `packages/nexus-extension-api/src/generated/ipc/*.ts`  (via ts-rs)
#   * `crates/nexus-bootstrap/schemas/ipc/*.json`            (via schemars)
#
# Fails if `git diff --exit-code` reports any change in either tree.
#
# The script also extends to the Phase 1 WI-20 ts-rs exports in
# `packages/nexus-extension-api/src/generated/` so one invocation covers
# both generators.

set -euo pipefail

# Repo root — this script must run from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

echo "[drift] regenerating Phase 1 plugin-api TS bindings (WI-20) …"
cargo test -p nexus-plugin-api --features ts-export --quiet

echo "[drift] regenerating Phase 4 pilot IPC TS bindings (WI-36: storage + ai) …"
cargo test -p nexus-storage --features ts-export --quiet --tests
cargo test -p nexus-ai --features ts-export --quiet --tests

# audit-2026-05-01 P1-3 (#113): subsystems brought online after the
# original WI-36 pilot. Each new entry below also needs a feature
# enable in `crates/nexus-bootstrap/Cargo.toml::ts-export`.
echo "[drift] regenerating IPC TS bindings (audit P1-3: linkpreview) …"
cargo test -p nexus-linkpreview --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: git) …"
cargo test -p nexus-git --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: mcp) …"
cargo test -p nexus-mcp --features ts-export --quiet --tests

echo "[drift] regenerating Phase 4 pilot IPC JSON Schemas (WI-36) …"
cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export --quiet

echo "[drift] checking for uncommitted changes in generated trees …"
TARGETS=(
    "packages/nexus-extension-api/src/generated"
    "crates/nexus-bootstrap/schemas/ipc"
)

if ! git diff --exit-code -- "${TARGETS[@]}"; then
    cat >&2 <<EOF

[drift] ERROR: regenerating IPC bindings produced a diff in:
${TARGETS[*]}

This means the Rust source of truth changed without committing the
regenerated TypeScript + JSON Schema output. Fix one of:

  1. Run \`scripts/check_ipc_drift.sh\` locally and commit the result.
  2. If the diff is unexpected, check that your pilot IPC type still
     carries \`#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]\`
     and that the \`#[ts(export, export_to = …)]\` path is correct.

See \`docs/ipc-schemas.md\` for the full generator story.
EOF
    exit 1
fi

echo "[drift] OK — generated trees match HEAD."
