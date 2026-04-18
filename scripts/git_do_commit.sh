#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "feat(skills): built-in skill library (PRD-13)

Four canonical .skill.md files (code-reviewer, daily-journal,
meeting-notes, commit-message) ship compiled into nexus-skills
via include_str!. New seed_builtins(dir) helper writes each file
that doesn't already exist at the target path; bootstrap calls it
against <forge>/.forge/skills/ before opening the registry so
fresh forges start with a useful default set while user edits are
never overwritten (uses create_new so a shadowing file wins).

Every built-in is parse-tested at build time via a unit test so
the shipped library stays well-formed. Seeding is idempotent —
the second run creates nothing and skips all four.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
