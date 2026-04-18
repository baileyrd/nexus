#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(chat): stepwise approval UI in ChatPanel (PRD-15 §3.4)

PendingPlanCard grows a 'Step ->' button alongside 'Approve all'.
Each click invokes com.nexus.agent::execute_step (shipped previous
commit) for the next step only, then re-renders the card with the
cursor advanced and per-step badges (checkmark / denied / failed)
inline with the plan. 'Approve rest' after partial stepping loops
agentExecuteStep from the current cursor instead of re-running the
plan from step 0.

Cancel preserves any partial observation in the turn summary so
users see what ran before bailing out. Persisted turns strip the
transient stepCursor / stepResults before writing — the next launch
sees a clean cancellation summary, not a half-hydrated approval
dialog.

Microkernel + editor-shell invariants held: all execution stays in
com.nexus.agent; the panel just drives the loop via ipc_call.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
