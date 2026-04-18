#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(skills): parameter substitution + render handler + CLI (PRD-13)

nexus-skills::substitute::render walks a skill body and replaces
{{ name }} tokens for declared parameters, falling back to the
frontmatter default and erroring on required-missing / enum-mismatch.
Undeclared tokens pass through so Jinja-style prompt templates keep
working.

Exposed via com.nexus.skills::render (handler id 6) accepting
{ id, values? }, and 'nexus skill render <id> [--param k=v ...]'
as the CLI surface. Library remains kernel-free; the plugin is the
sole microkernel integration point.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
