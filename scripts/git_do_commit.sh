#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): render skill bodies before layering into planner prompt

The skill-aware system prompt assembly in com.nexus.agent now calls
com.nexus.skills::render for each matched skill instead of reading the
raw body, so frontmatter parameter defaults get substituted before the
planner sees the prompt. Falls back to the pre-rendered body when
render errors (e.g. required param with no default), preserving the
previous behaviour for skills that can't be rendered with empty values.

Microkernel boundary held: agent still only speaks to skills via
ipc_call; no direct nexus-skills linkage added.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
