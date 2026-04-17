#!/usr/bin/env bash
# Seed a directory into a fully-capable Nexus forge: init `.forge/`,
# drop the markdown + canvas content tree from fixtures/forge/, and
# copy the `.bases` demo databases from fixtures/bases/.
#
# Usage:
#   bash scripts/seed_fixtures.sh <forge-root>                  # seed content only
#   bash scripts/seed_fixtures.sh <forge-root> --init           # init + seed
#   bash scripts/seed_fixtures.sh <forge-root> --overwrite      # overwrite existing entries
#   bash scripts/seed_fixtures.sh <forge-root> --init --overwrite
#
# Re-running is idempotent: existing entries are skipped unless you
# pass --overwrite. `--init` is safe to re-run on an already-
# initialised forge (`forge init` no-ops when `.forge/` exists).
#
# After seeding, open the forge by either:
#   NEXUS_FORGE_DIR=<forge-root> cargo run -p nexus-app
#   cargo run -p nexus-tui -- <forge-root>

set -euo pipefail

die() {
    echo "error: $1" >&2
    exit 1
}

usage() {
    sed -n '3,17p' "$0" >&2
    exit 2
}

if [[ $# -lt 1 ]]; then
    usage
fi

FORGE_ROOT="$1"
shift || true

DO_INIT=false
OVERWRITE=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --init) DO_INIT=true ;;
        --overwrite) OVERWRITE=true ;;
        -h|--help) usage ;;
        *) die "unknown flag: $1" ;;
    esac
    shift
done

# Anchor on this script's location so the command works from any
# cwd. `scripts/seed_fixtures.sh` → parent is the repo root.
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
FORGE_FIXTURES="$REPO_ROOT/fixtures/forge"
BASES_FIXTURES="$REPO_ROOT/fixtures/bases"

[[ -d "$FORGE_FIXTURES" ]] || die "forge fixtures not found at $FORGE_FIXTURES"
[[ -d "$BASES_FIXTURES" ]] || die "bases fixtures not found at $BASES_FIXTURES"

mkdir -p "$FORGE_ROOT"
FORGE_ROOT=$(cd "$FORGE_ROOT" && pwd)  # canonicalise
echo "target forge: $FORGE_ROOT"

# ── Step 1: optionally run `forge init` ─────────────────────────────────────
if $DO_INIT; then
    if [[ -d "$FORGE_ROOT/.forge" ]]; then
        echo "  [init] .forge/ already exists — skipping"
    else
        echo "  [init] running nexus-cli forge init"
        (
            cd "$REPO_ROOT"
            cargo run --quiet -p nexus-cli -- forge init "$FORGE_ROOT"
        )
    fi
fi

# ── Step 2: copy a single file or directory from a source tree ─────────────
#
# Files: skip if present (unless --overwrite), otherwise copy.
# Directories: MERGE into the destination recursively, applying the
#   same file-level skip/overwrite logic to each child. This is
#   important because `forge init` pre-creates some empty skeleton
#   directories (notes/, attachments/) — a naïve "skip dir if it
#   exists" would silently drop the seed's daily-note files.
#
# $1 = source path
# $2 = destination path (created if parent missing)
# $3 = short name for status messages
copy_entry() {
    local src="$1" dst="$2" name="$3"
    if [[ -d "$src" ]]; then
        mkdir -p "$dst"
        local child
        shopt -s dotglob nullglob
        for child in "$src"/*; do
            local child_name
            child_name=$(basename "$child")
            copy_entry "$child" "$dst/$child_name" "$name/$child_name"
        done
        shopt -u dotglob nullglob
        return 0
    fi
    # Leaf file.
    if [[ -e "$dst" ]]; then
        if $OVERWRITE; then
            rm -rf "$dst"
        else
            echo "    skip (exists): $name"
            return 0
        fi
    fi
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
    echo "    wrote: $name"
}

# ── Step 3: content tree from fixtures/forge/ ──────────────────────────────
echo "  [content] seeding markdown + canvas tree"
# Walk every top-level entry under fixtures/forge/ and copy into the
# target. Preserves subfolder structure (notes/, projects/, etc.).
#
# Using `-depth 1` so we recurse into sub-dirs via the copy itself,
# not via the outer loop — keeps status output readable at the
# top-level granularity.
shopt -s dotglob nullglob
for src in "$FORGE_FIXTURES"/*; do
    name=$(basename "$src")
    # Guard against accidentally overwriting the .forge index dir.
    if [[ "$name" == ".forge" ]]; then
        continue
    fi
    copy_entry "$src" "$FORGE_ROOT/$name" "$name"
done
shopt -u dotglob nullglob

# ── Step 4: `.bases` demo databases into <forge>/fixtures/bases/ ───────────
echo "  [bases] seeding .bases fixtures"
TARGET_BASES="$FORGE_ROOT/fixtures/bases"
mkdir -p "$TARGET_BASES"
for src in "$BASES_FIXTURES"/*.bases; do
    [[ -d "$src" ]] || continue
    name=$(basename "$src")
    copy_entry "$src" "$TARGET_BASES/$name" "fixtures/bases/$name"
done

echo ""
echo "done. next:"
echo "  NEXUS_FORGE_DIR=\"$FORGE_ROOT\" cargo run -p nexus-app"
echo "  # or"
echo "  cargo run -p nexus-tui -- \"$FORGE_ROOT\""
