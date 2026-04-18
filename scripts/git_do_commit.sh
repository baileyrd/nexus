#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): plan history persistence (PRD-15 memory gap)

Every run / run_plan completion now writes
{ plan_id, goal?, plan, observation, created_at } to
<forge>/.forge/agent/history/<plan_id>.json via a best-effort
save_history helper (failures logged, never fail the caller).

Three append-only handlers on com.nexus.agent:
  - history_list (5): enumerate archive entries
  - history_get (6): load one record by plan_id
  - history_delete (7): remove one entry

Plan ids are validated against [A-Za-z0-9_-]{1,96} before path
composition to block traversal via model-generated ids. Tauri
bridges + TS helpers (agentHistoryList / Get / Delete) wire
through to the UI. A history browser panel on top of these
handlers is the follow-up.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
