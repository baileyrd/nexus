#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(agent): Writer/Coder/Researcher archetypes (PRD-15 §3.3)

New nexus-agent::archetypes module ships three pre-baked LlmAgent
configurations with domain-tuned planner system prompts. Each is a
thin constant + constructor — driver, plan schema, and executor loop
are unchanged — so archetypes compose with skill-matched prompts
through build_archetype(name, driver, extra_prompt).

com.nexus.agent::plan and ::run accept an optional 'archetype' arg
(writer|coder|researcher|general, case-insensitive, unknown falls
back to default). Skills layer on top as the 'extra_prompt' so both
selectors compose.

nexus agent plan|run --archetype <name> exposes it from the CLI.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
