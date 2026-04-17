#!/usr/bin/env bash
# Seed a forge with the .bases fixtures from fixtures/bases/.
# Usage: bash scripts/seed_fixtures.sh <forge-root> [--overwrite]
# See fixtures/bases/README.md for what the fixtures exercise.

set -euo pipefail

usage() {
    echo "usage: $(basename "$0") <forge-root> [--overwrite]" >&2
    echo "" >&2
    echo "Copies every .bases directory from fixtures/bases/ into" >&2
    echo "<forge-root>/fixtures/. Skips existing entries unless" >&2
    echo "--overwrite is passed." >&2
    exit 2
}

if [[ $# -lt 1 ]]; then
    usage
fi

FORGE_ROOT="$1"
OVERWRITE="${2:-}"

if [[ ! -d "$FORGE_ROOT" ]]; then
    echo "error: forge root does not exist: $FORGE_ROOT" >&2
    exit 1
fi

# Find the repo root from this script's location so the command works
# from any cwd. `scripts/seed_fixtures.sh` → `scripts/..` is the root.
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd)
FIXTURE_DIR="$REPO_ROOT/fixtures/bases"

if [[ ! -d "$FIXTURE_DIR" ]]; then
    echo "error: fixtures not found at $FIXTURE_DIR" >&2
    exit 1
fi

TARGET="$FORGE_ROOT/fixtures"
mkdir -p "$TARGET"

echo "seeding fixtures into: $TARGET"
for src in "$FIXTURE_DIR"/*.bases; do
    [[ -d "$src" ]] || continue
    name=$(basename "$src")
    dst="$TARGET/$name"
    if [[ -e "$dst" && "$OVERWRITE" != "--overwrite" ]]; then
        echo "  skip (exists): $name  [re-run with --overwrite to replace]"
        continue
    fi
    if [[ -e "$dst" ]]; then
        rm -rf "$dst"
    fi
    cp -r "$src" "$dst"
    echo "  wrote: $name"
done

echo ""
echo "done. open the forge and navigate to fixtures/ to click through them."
