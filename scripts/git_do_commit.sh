#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(cli): nexus mcp servers|tools|call over com.nexus.mcp.host (PRD-14)

Refactors the single-verb 'nexus mcp' command into a subcommand
family: 'serve' (existing stdio server), 'servers' (list configured
external MCP servers), 'tools <server>' (enumerate tools exposed by
one server), 'call <server> <tool> --arguments '{...}'' (invoke a
tool). All three host commands route through com.nexus.mcp.host via
ipc_call — no direct nexus-mcp linkage from the CLI crate.

Updates PRD-14 from partial (yellow) to substantially complete
(green): server + host + host-orchestrator plugin + host CLI all
ship. Gaps remaining: no WebSocket/HTTP transport, no auto-
discovery by the AI plugin.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
