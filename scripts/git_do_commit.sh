#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(workflow): cron trigger engine (PRD-16)

New nexus-workflow::cron module ships a zero-dep 5-field cron parser
with next_after(datetime) -> DateTime<Utc>. Supports wildcards,
literals, comma lists, ranges, and step forms including A-B/N.
Honours POSIX dom/dow OR semantics (when both fields are restricted,
match if either matches). 13 unit tests covering basic daily,
every-N-minutes, weekday-only, impossible-date (Feb 30), and range
validation.

com.nexus.workflow plugin grows a scheduler: on wire_context, every
workflow with trigger.type = 'cron' and a schedule field gets a
dedicated tokio task that sleeps until next fire, calls back into
the plugin's own run handler via ipc_call, and loops. JoinHandles
are retained on the plugin and aborted on Drop so no zombies
survive shutdown. Parse / dispatch failures log-and-continue.

Moves PRD-16 Workflow System from scaffolded (orange) to
substantially complete (green). file_event / webhook / git-event
triggers + condition evaluator + variable interpolation remain
open for future slices.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
