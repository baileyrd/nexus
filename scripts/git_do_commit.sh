#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(chat): live agent plan progress in ChatPanel (PRD-15)

ChatPanel subscribes to onAgentRunStart / StepStart / StepDone /
RunDone and writes a running checklist into the pending turn's
content field as each step lands. Steps render as '▶ [n] desc'
while executing and flip to '✓' / '✗' / '·' badges when done.
The awaited agent_run / agent_run_plan resolution overwrites
content with the full formatted observation when the plan ends,
so the handoff is seamless.

Transient agentProgress field on Turn is dropped from persistence
alongside the existing stepCursor / stepResults hygiene — restarts
see only the final summary, never a stale in-flight checklist.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
