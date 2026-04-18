#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

# Stage session summaries and all new scripts
git add SESSION_SUMMARIES.md
git add app/SESSION_SUMMARIES.md
git add scripts/

git status --short
echo "---log---"
git log --oneline -5
