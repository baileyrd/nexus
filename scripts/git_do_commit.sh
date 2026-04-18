#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(workflow): manual execution engine + run handler + CLI (PRD-16)

nexus-workflow::executor::run_workflow iterates [[steps]] in order,
dispatching each through an injected ActionDispatcher and recording
per-step outcomes (ok / failed / skipped). Default on_error = stop
halts the loop; on_error = 'continue' / 'log_warn' keeps going.
StepOutcomeStatus mirrors nexus-agent's StepStatus so UI code can
render both shapes the same way.

com.nexus.workflow gains handler run (id 5). Its async dispatch
resolves the workflow by name, builds a KernelActionDispatcher
that routes step_type='ipc'/'ipc_call' through PluginContext::
ipc_call (60s per step), and returns a WorkflowRun. Unknown step
types degrade to logged no-ops so authors can iterate without
executor churn.

Bootstrap wires the plugin's kernel context and registers the
handler. 'nexus workflow run <name>' drives it from the CLI with
a 600s timeout for multi-step chains. 5 new executor unit tests
cover success / default-stop / continue / empty-plan / named-step
semantics.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
