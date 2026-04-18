#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(ui): agent history browser panel (PRD-15)

AgentHistoryPanel renders a two-pane view over the history_list /
history_get handlers shipped last commit: left column lists every
persisted run (newest first, success/failure badges); right column
renders plan + per-step results with status chips and truncated
JSON response previews. Delete removes entries via history_delete.
Registered as content-type com.nexus.agent.history with palette
command 'Agent: History'.

Editor-shell pattern preserved: no direct nexus-agent linkage from
the panel — the TS wrappers in ipc/agent.ts speak only to the
plugin via ipc_call.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
