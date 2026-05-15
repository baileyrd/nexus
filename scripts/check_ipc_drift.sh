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

echo "[drift] regenerating IPC TS bindings (BL-076: lsp) …"
cargo test -p nexus-lsp --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (BL-081: dap) …"
cargo test -p nexus-dap --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (BL-144: acp) …"
cargo test -p nexus-acp --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: agent) …"
cargo test -p nexus-agent --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: comments) …"
cargo test -p nexus-comments --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: theme) …"
cargo test -p nexus-theme --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: skills) …"
cargo test -p nexus-skills --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: workflow) …"
cargo test -p nexus-workflow --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: terminal) …"
cargo test -p nexus-terminal --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (audit P1-3: database) …"
cargo test -p nexus-database --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (templates) …"
cargo test -p nexus-templates --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (formats) …"
cargo test -p nexus-formats --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (BL-117: audio) …"
cargo test -p nexus-audio --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (BL-133: notifications) …"
cargo test -p nexus-notifications --features ts-export --quiet --tests

echo "[drift] regenerating IPC TS bindings (BL-134: ai-runtime) …"
cargo test -p nexus-ai-runtime --features ts-export --quiet --tests

echo "[drift] regenerating Phase 4 pilot IPC JSON Schemas (WI-36) …"
cargo test -p nexus-bootstrap --test ipc_schema_emit --features ts-export --quiet

# BL-137 — every plugin must publish bus events only under its own
# `type_id` prefix (or to the kernel-shared topic allowlist). The
# kernel enforces this at runtime, but many publish sites are
# `let _ = ctx.publish(...)` so a namespace mismatch silently drops.
# The static scan catches the bug at CI time.
echo "[drift] enforcing IPC topic-prefix invariant (BL-137) …"
cargo test -p nexus-bootstrap --test ipc_topic_prefix_invariant --quiet

# BL-137 — regenerate the capability inventory at docs/generated/
# capabilities.md from `Capability::ALL` + `risk_level`. The git diff
# below catches a stale generated file.
echo "[drift] regenerating capability inventory (BL-137) …"
cargo test -p nexus-security --test capability_inventory_emit --quiet

echo "[drift] checking for uncommitted changes in generated trees …"
TARGETS=(
    "packages/nexus-extension-api/src/generated"
    "crates/nexus-bootstrap/schemas/ipc"
    "docs/generated"
)

# `git diff --exit-code` catches modifications to tracked files but
# silently misses untracked new ones — e.g. a freshly emitted schema
# for a new IPC type. Layer in `git ls-files --others` so a missing
# commit of a new generated file is caught here too.
DRIFTED=0
git diff --exit-code -- "${TARGETS[@]}" || DRIFTED=1
UNTRACKED="$(git ls-files --others --exclude-standard -- "${TARGETS[@]}")"
if [ -n "$UNTRACKED" ]; then
    echo
    echo "[drift] untracked generated files (please commit):"
    echo "$UNTRACKED" | sed 's/^/  /'
    DRIFTED=1
fi

if [ "$DRIFTED" -ne 0 ]; then
    cat >&2 <<EOF

[drift] ERROR: regenerating IPC bindings produced a diff in:
${TARGETS[*]}

This means the Rust source of truth changed without committing the
regenerated TypeScript + JSON Schema output. Fix one of:

  1. Run \`scripts/check_ipc_drift.sh\` locally and commit the result
     (including any untracked files listed above).
  2. If the diff is unexpected, check that your pilot IPC type still
     carries \`#[cfg_attr(feature = "ts-export", derive(TS, JsonSchema))]\`
     and that the \`#[ts(export, export_to = …)]\` path is correct.

See \`docs/architecture/ipc-schemas.md\` for the full generator story.
EOF
    exit 1
fi

echo "[drift] OK — generated trees match HEAD."
