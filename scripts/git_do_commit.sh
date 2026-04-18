#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): MCP tool auto-discovery in planner prompt (PRD-14 x PRD-15)

system_prompt_with_skills now queries com.nexus.mcp.host at plan
time and appends a compact advertisement of enabled servers and
their top-8 tool names to the planner system prompt, alongside
instructions for invoking them as com.nexus.mcp.host::call_tool
steps. 3s timeout per call; any host failure leaves the prompt
untouched so forges without MCP keep working.

Closes the remaining gap between PRD-14 host orchestrator and the
PRD-15 agent — the planner can now route goals like 'fetch the
latest issues' to a github MCP server without the user having to
hard-code the tool id.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
