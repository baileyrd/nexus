#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(workflow): nexus-workflow library scaffold (PRD-16)

New kernel-free crate projecting .workflow.toml (§4) into a typed
model: Workflow / WorkflowMeta / Input / Trigger / Condition / Step /
ErrorHandling. Type-dispatched tables (Trigger, Condition, Step) keep
per-kind fields in a flatten-extra map so cron schedules, fs-watch
globs, action parameters, and combinator children all round-trip
without forcing a monolithic enum.

parse_workflow_text / parse_workflow_file validate required fields
after decode. WorkflowRegistry walks <forge>/.workflows/ recursively,
keyed by workflow.name; partial parse failures surface after the good
subset is inserted so callers can log + continue.

Moves PRD-16 from spec-only (white) to scaffolded (orange). Trigger
engine, condition evaluator, action executor, core plugin, and CLI
are follow-ups. Library stays kernel-free to match nexus-skills /
nexus-agent posture.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
