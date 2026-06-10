#!/usr/bin/env bash
# Seed a local test forge (default: ~/notes) with the fixtures.
# Safe to re-run; passes --init so `forge init` runs once, and
# skips any files that already exist. Add --overwrite to replace.
#
# Usage:
#   bash scripts/seed_notes.sh                # seeds $HOME/notes
#   NOTES_DIR=~/elsewhere bash scripts/seed_notes.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="${NOTES_DIR:-$HOME/notes}"
bash "$SCRIPT_DIR/seed_fixtures.sh" "$TARGET" --init "$@" 2>&1 | tail -50
