#!/usr/bin/env bash
# B1 / B2 (2026-05-21 gaps audit) drift guard for the human-readable
# IPC-handler docs. Complements `scripts/check_ipc_drift.sh`, which
# regenerates the machine-emitted bindings; this script enforces that
# two hand-maintained docs stay in sync with the matrix:
#
#   1. `docs/0.1.2/ipc-handlers.md` — the per-plugin handler-count
#      table and the per-plugin section headers (e.g. `## com.nexus.storage (72)`)
#      must match `crates/nexus-bootstrap/cap_matrix.toml`'s actual
#      per-plugin handler counts.
#   2. `docs/0.1.2/reference/audit-flags.md` — every `unrestricted`
#      handler in the matrix that the security author flagged as a
#      promotion candidate (today encoded by a preceding `# AUDIT:`
#      comment or by the explicit `internal = true` marker on
#      `com.nexus.ai::resolve_credentials`) must appear in the doc's
#      live-severity table; conversely, no handler that has since been
#      promoted to `caps = […]` may still be listed there as live.
#
# Exit codes: 0 = in sync, 1 = drift, 2 = invocation error.
#
# Run locally before opening a PR. CI invokes it as a smoke check.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

MATRIX="crates/nexus-bootstrap/cap_matrix.toml"
HANDLERS_DOC="docs/0.1.2/ipc-handlers.md"
AUDIT_FLAGS_DOC="docs/0.1.2/reference/audit-flags.md"

for f in "$MATRIX" "$HANDLERS_DOC" "$AUDIT_FLAGS_DOC"; do
    [ -f "$f" ] || { echo "[docs-drift] missing $f" >&2; exit 2; }
done

DRIFTED=0

# ── B1: per-plugin handler counts ─────────────────────────────────
# Build "<plugin_id> <count>" sorted pairs from the matrix vs the
# doc, then `diff` them.

matrix_counts="$(
    awk '/^\[\[handler\]\]/{flag=1; next}
         flag && /^plugin = /{
             gsub(/^plugin = "|"$/, "");
             print;
             flag=0
         }' "$MATRIX" | sort | uniq -c | awk '{print $2, $1}' | sort
)"

# Doc table rows look like:  | `com.nexus.storage` | 72 |
doc_counts="$(
    grep -E '^\| `com\.nexus\.[^`]+` \|[[:space:]]+[0-9]+ \|' "$HANDLERS_DOC" \
        | sed -E 's/^\| `([^`]+)` \|[[:space:]]+([0-9]+) \|.*$/\1 \2/' \
        | sort
)"

if ! diff <(printf '%s\n' "$matrix_counts") <(printf '%s\n' "$doc_counts") > /tmp/docs-drift-counts.diff; then
    echo "[docs-drift] B1 FAIL — ipc-handlers.md counts table out of sync with $MATRIX"
    echo "(left = matrix, right = doc)"
    diff <(printf '%s\n' "$matrix_counts") <(printf '%s\n' "$doc_counts") | sed 's/^/  /'
    DRIFTED=1
fi

# Per-plugin section headers like `## com.nexus.storage (72)` must
# also match. Walk every matrix plugin and grep for the header.
while IFS=' ' read -r plugin count; do
    [ -z "$plugin" ] && continue
    expected="## ${plugin} (${count})"
    if ! grep -qF "$expected" "$HANDLERS_DOC"; then
        actual="$(grep -E "^## ${plugin//./\\.} \([0-9]+\)" "$HANDLERS_DOC" || echo '(missing)')"
        echo "[docs-drift] B1 FAIL — section header mismatch for ${plugin}"
        echo "  expected: ${expected}"
        echo "  doc:      ${actual}"
        DRIFTED=1
    fi
done <<< "$matrix_counts"

# ── B2: audit-flags.md vs matrix `unrestricted` + `# AUDIT:` ──────
# Collect the set of (plugin, command) pairs currently classified as
# unrestricted AND either preceded by a `# AUDIT:` comment OR carrying
# `internal = true`. These are the live candidates the doc must list.

live_audit="$(
    awk '
        function emit() {
            if (have_block && unrestricted && (block_audit || internal) && plugin && command) {
                print plugin "::" command
            }
        }
        /^# AUDIT:/  { saw_audit = 1; next }
        /^$/         { saw_audit = 0; next }
        /^\[\[handler\]\]/ {
            emit();
            have_block = 1
            plugin = ""; command = ""; unrestricted = 0; internal = 0
            block_audit = saw_audit
            saw_audit = 0
            next
        }
        /^plugin = "/      { gsub(/^plugin = "|"$/, ""); plugin = $0; next }
        /^command = "/     { gsub(/^command = "|"$/, ""); command = $0; next }
        /^unrestricted = / { unrestricted = 1; next }
        /^internal = true/ { internal = 1; next }
        END { emit() }
    ' "$MATRIX" | sort -u
)"

doc_audit="$(
    grep -E '^\| `com\.nexus\.[^`]+::[^`]+`' "$AUDIT_FLAGS_DOC" \
        | sed -E 's/^\| `([^`]+)`.*$/\1/' \
        | sort -u
)"

# The doc has both a live table and a "Closed since…" table. The
# CLOSED entries are explicitly historical and must NOT appear in
# the live `unrestricted` set in the matrix (they should have
# `caps = […]` now). Split the doc at the "Closed since" heading and
# check each half against the matrix.

live_doc_table="$(
    awk '/^## Severity table/{flag=1; next}
         /^## Closed since/{flag=0}
         flag' "$AUDIT_FLAGS_DOC" \
        | grep -E '^\| `com\.nexus\.[^`]+::[^`]+`' \
        | sed -E 's/^\| `([^`]+)`.*$/\1/' \
        | sort -u
)"

closed_doc_table="$(
    awk '/^## Closed since/{flag=1; next}
         /^## /{flag=0}
         flag' "$AUDIT_FLAGS_DOC" \
        | grep -E '^\| `com\.nexus\.[^`]+::[^`]+`' \
        | sed -E 's/^\| `([^`]+)`.*$/\1/' \
        | sort -u
)"

# Live in matrix but not in doc-live → missing row.
missing_live="$(comm -23 <(printf '%s\n' "$live_audit") <(printf '%s\n' "$live_doc_table"))"
# In doc-live but not in matrix-live → stale row (already promoted).
stale_live="$(comm -13 <(printf '%s\n' "$live_audit") <(printf '%s\n' "$live_doc_table"))"
# In doc-closed but matrix says still live → doc moved too eagerly.
prematurely_closed="$(comm -12 <(printf '%s\n' "$live_audit") <(printf '%s\n' "$closed_doc_table"))"

if [ -n "$missing_live" ]; then
    echo "[docs-drift] B2 FAIL — audit-flags.md is missing live unrestricted handlers:"
    printf '%s\n' "$missing_live" | sed 's/^/  /'
    DRIFTED=1
fi
if [ -n "$stale_live" ]; then
    echo "[docs-drift] B2 FAIL — audit-flags.md lists handlers that are no longer unrestricted in the matrix:"
    printf '%s\n' "$stale_live" | sed 's/^/  /'
    echo "  Either move them to the 'Closed since' table or restore the row."
    DRIFTED=1
fi
if [ -n "$prematurely_closed" ]; then
    echo "[docs-drift] B2 FAIL — audit-flags.md 'Closed since' lists handlers that are still unrestricted in the matrix:"
    printf '%s\n' "$prematurely_closed" | sed 's/^/  /'
    DRIFTED=1
fi

if [ "$DRIFTED" -ne 0 ]; then
    cat >&2 <<EOF

[docs-drift] ERROR: hand-maintained IPC docs are out of sync with $MATRIX.
See the per-failure detail above. Fix one of:

  1. Update the doc to match the matrix (the matrix is the source of truth).
  2. If a handler was just gated, move its row from the live table in
     audit-flags.md to the 'Closed since' table.
  3. If you genuinely meant to change the matrix without updating the
     docs, that's a policy break — open an ADR.

See docs/0.1.2/audits/gaps-inconsistencies-2026-05-21.md for the audit
that surfaced this requirement (B1 / B2).
EOF
    exit 1
fi

echo "[docs-drift] OK — ipc-handlers.md + audit-flags.md match $MATRIX."
