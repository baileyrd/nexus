#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): per-step execution API (PRD-15 §3.4)

PlanExecutor gains execute_step_at(plan, index) -> StepResult for
single-step execution outside the policy loop. Exposed as
com.nexus.agent::execute_step (handler id 4) + agent_execute_step
Tauri command + agentExecuteStep TS helper, so UIs can drive a
per-step approval flow without wiring a custom StepPolicy through
Tauri events: iterate the plan, pause for approval between steps,
call execute_step when the user clicks Approve.

Library stays kernel-free; the new handler reuses the same
KernelToolBridge as run_plan. 2 new unit tests cover the happy path
and out-of-bounds index reporting.

Stepwise UI in ChatPanel on top of this primitive is the follow-up.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
