#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add crates/nexus-workflow/src/core_plugin.rs \
        crates/nexus-bootstrap/tests/workflow_ipc.rs \
        docs/PRDs/IMPLEMENTATION_STATUS.md

git commit -m "feat(workflow): file_event trigger engine (PRD-16)

Subscribe each trigger.type=file_event workflow to
com.nexus.storage.file_* on the kernel bus and dispatch
com.nexus.workflow::run when an event matches.

Trigger fields:
  watch_dir = \"notes/\"       path-prefix filter
  pattern   = \"\\\\.md\$\"       regex-lite against path
  events    = [\"created\",     subset of created|modified|deleted
               \"modified\",     (defaults to all three when omitted)
               \"deleted\"]

On match, flattens the storage event payload into
  variables.trigger.path
  variables.trigger.event_type
  variables.trigger.content_hash
so steps compose with the existing \${trigger.*} interpolation —
e.g. a step can ai_chat on the changed file by templating
\${trigger.path} into its args.

Architecture: pure plugin-side addition — uses the kernel's existing
bus subscription API + IPC dispatcher, no kernel changes, no new
handler ids, no plugin manifest fields. Composes with the cron
scheduler (both share the same scheduler_handles mutex so Drop
aborts every background task). The storage plugin is the sole
source of file events; the workflow plugin is a pure consumer.
Microkernel + editor-shell invariants held.

Malformed specs (invalid regex, unknown event type, non-string
events) log-and-skip per-workflow without poisoning siblings.

Coverage (6 new tests):
- core_plugin.rs: file_event_spec_parses_all_fields,
  file_event_spec_defaults_to_all_events_when_omitted,
  file_event_spec_rejects_invalid_regex_and_unknown_event,
  file_event_spec_matches_path_combines_dir_and_pattern,
  event_type_mapping_covers_all_storage_file_events
- workflow_ipc.rs: file_event_trigger_fires_workflow_when_watched_path_changes
  — drops a file at notes/observed.md via direct disk write, waits
  up to 8s for storage watcher → bus → workflow → step chain, and
  asserts the marker file written by the step appears with the
  expected content.

Closes the file_event portion of PRD-16; webhook / git_event /
mcp_event triggers, parallel steps, retry / backoff still open.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
