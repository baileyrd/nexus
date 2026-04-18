#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(workflow): com.nexus.workflow plugin + nexus workflow CLI (PRD-16)

WorkflowCorePlugin wraps WorkflowRegistry behind 4 append-only
handlers (list/get/reload/validate). validate takes raw TOML text so
UIs can smoke-test edits without touching disk. Bootstrap opens the
registry at <forge>/.workflows.

nexus workflow list|show|reload|validate provides the CLI surface
over ipc_call — no direct nexus-workflow linkage. validate reads a
file, pipes it through the plugin, and exits non-zero on parse
failure so it doubles as a CI check.

Same microkernel posture as nexus-skills / nexus-agent: kernel-free
library, single plugin integration point, thin editor-shell
consumers.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
