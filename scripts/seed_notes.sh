#!/bin/bash
# Seed ~/notes (the user's real test forge) with the fixtures.
# Safe to re-run; passes --init so `forge init` runs once, and
# skips any files that already exist. Add --overwrite to replace.
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
TARGET="$HOME/notes"
bash /mnt/c/Users/baile/dev/Nexus/scripts/seed_fixtures.sh "$TARGET" --init "$@" 2>&1 | tail -50
