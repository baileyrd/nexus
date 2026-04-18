#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(chat): archetype selector in Chat panel (PRD-15 §3.3)

Chat panel now surfaces the writer/coder/researcher/general archetype
as a dropdown in the toolbar, enabled only while the Agent chip is on.
Selection threads through agent_plan / agent_run (now carrying an
optional archetype arg) and the Tauri bridge in nexus-app/src/agent.rs.

Editor-shell architecture preserved: the React panel only talks to
com.nexus.agent via ipc_call; all archetype logic lives in the plugin.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
