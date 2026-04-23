#!/usr/bin/env bash
# Phase 0 commit + tag script for the shell-migration decision.
#
# Run this from the repo root on a machine where:
#   (a) git commits work (no stale .git/index.lock, proper committer config), and
#   (b) your checkout has its usual line endings (LF; the sandbox where this
#       repo was audited presented CRLF which would otherwise poison a broad
#       `git add -A`).
#
# This script stages ONLY the intentional edits from Phase 0 — it does not
# touch the 884 CRLF-drift files the audit environment had in its working
# tree.
#
# Produces:
#   1. One commit with all Phase 0 edits (banners, ADR accept, freeze notice,
#      integration review artifacts, parity checklist, comparison matrix).
#   2. An annotated tag `v0.1.0-legacy-shell` at that commit (so the legacy
#      shell is recoverable).
#
# Review the `git status` output before running. If you see unrelated changes
# you want to keep out of this commit, stash or reset them first.

set -euo pipefail

cd "$(dirname "$0")/.."

echo "== Sanity checks =="
git rev-parse --abbrev-ref HEAD
git log -1 --format="%h %s"
echo

# Paths that are part of the Phase 0 landing. All must exist.
FILES=(
  # Phase 0 edits
  "README.md"
  "CONTRIBUTING.md"
  "app/README.md"
  "crates/nexus-app/src/lib.rs"
  # Decision record
  "docs/adr/0011-adopt-plugin-first-shell.md"
  # Companion docs
  "docs/INTEGRATION-REVIEW.md"
  "docs/INTEGRATION-ARCHITECTURE.html"
  "docs/Nexus-Integration-Architecture.docx"
  "docs/SHELL-COMPARISON.md"
  "docs/Shell-Capability-Comparison.xlsx"
  "docs/PARITY-CHECKLIST.md"
  "docs/Parity-Checklist.xlsx"
  # Helper scripts
  "scripts/phase-0-commit.sh"
  "scripts/phase-0-commit.ps1"
)

echo "== Verifying files exist =="
for f in "${FILES[@]}"; do
  if [[ ! -f "$f" ]]; then
    echo "MISSING: $f" >&2
    exit 1
  fi
done
echo "All $(wc -w <<<"${FILES[@]}") files present."
echo

echo "== Staging =="
for f in "${FILES[@]}"; do
  git add -- "$f"
done
git status --short -- "${FILES[@]}"
echo

echo "== Committing =="
git commit -m "$(cat <<'EOF'
Phase 0: freeze legacy shell, adopt plugin-first shell (ADR 0011)

Accept ADR 0011 (plugin-first shell as the single desktop target).
Add DEPRECATED banners to the legacy shell (app/ + crates/nexus-app).
Add CONTRIBUTING.md with the freeze policy. Land the integration
review, ADR, architecture diagram, Word companion, per-command
comparison matrix, and Phase 2 parity checklist (23 work items).

Tag v0.1.0-legacy-shell at this commit preserves the pre-freeze state.

Per CONTRIBUTING.md, new desktop capabilities land as service-crate
IPC handlers + a plugin in shell/src/plugins/nexus/, not as new
#[tauri::command] handlers in crates/nexus-app.

See docs/INTEGRATION-REVIEW.md and docs/PARITY-CHECKLIST.md.
EOF
)"
echo

echo "== Tagging =="
git tag -a v0.1.0-legacy-shell -m "$(cat <<'EOF'
Pre-freeze snapshot of the legacy Tauri desktop shell (app/ + crates/nexus-app).

Per ADR 0011 (docs/adr/0011-adopt-plugin-first-shell.md, Accepted
2026-04-23), the plugin-first shell at shell/ + shell/src-tauri
(crate nexus-shell) is the single desktop target going forward. This
tag preserves the legacy tree at its last unfrozen state.

Recovery: `git checkout v0.1.0-legacy-shell -- app crates/nexus-app`
to retrieve specific legacy files if needed during Phase 2 migration.
EOF
)"
echo

echo "== Done =="
echo "Commit: $(git log -1 --format='%h %s')"
echo "Tag:    $(git describe --tags --exact-match HEAD)"
echo
echo "To push:  git push origin main && git push origin v0.1.0-legacy-shell"
