#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): streaming plan-execution events (PRD-15)

run_plan_internal now drives steps one-at-a-time via execute_step_at
and publishes four kernel-bus topics around each dispatch:
com.nexus.agent.{run_start,step_start,step_done,run_done}. The UI
no longer has to wait for the whole observation to land before
showing progress.

nexus-app::start_agent_event_forwarder mirrors the AI forwarder:
subscribes to the CustomPrefix com.nexus.agent., translates each
event into a Tauri event (agent:run_start / step_start / step_done /
run_done), and emits the plugin's JSON payload verbatim. TS helpers
onAgentStepStart / onAgentStepDone / onAgentRunStart / onAgentRunDone
let panels subscribe without touching the raw listen() API.

Library unchanged — all orchestration stays in the plugin, so the
microkernel boundary is preserved.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
