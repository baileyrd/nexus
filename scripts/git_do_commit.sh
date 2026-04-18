#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(ui): SkillsPanel + WorkflowsPanel (PRD-13 / PRD-16)

Two new two-pane browser panels: list on the left, metadata + body
on the right. Registered as content-types com.nexus.skills.browser
and com.nexus.workflow.browser with palette commands 'Skills: Browse'
and 'Workflows: Browse'. Each panel consumes its plugin only via
ipc_call through new Tauri bridges (nexus-app/src/skills.rs and
workflow.rs) + typed TS wrappers (app/src/ipc/skills.ts, workflow.ts).

Editor-shell pattern preserved: the Tauri command layer is a thin
adapter over KernelPluginContext::ipc_call with a 30s timeout; no
direct nexus-skills / nexus-workflow linkage from the desktop crate.
Panels are plain React components consuming the typed TS surface.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
