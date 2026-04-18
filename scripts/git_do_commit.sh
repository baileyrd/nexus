#!/bin/bash
export PATH=/home/baileyrd/.cargo/bin:/usr/local/bin:/usr/bin:/bin
cd /mnt/c/Users/baile/dev/Nexus || exit 1

git add -A
git commit -m "docs: archive shipped BL-005 + BL-006 to BACKLOG_COMPLETED

BL-005 (In-Memory Knowledge Graph) and BL-006 (Block-Level Content
Chunking for RAG) have been delivered — the petgraph graph ships
in nexus-storage and the chunker in nexus-ai carries
heading_context per chunk. Moved both entries verbatim from
BACKLOG.md to BACKLOG_COMPLETED.md with a one-line delivery note
and evidence pointers.

Remaining open items in BACKLOG.md: BL-001 (daily notes), BL-002
(typed property columns), BL-003 (scoping operators post-filter),
BL-004 (3-tier wikilink resolution), BL-007 (CRDT-over-git —
deferred large).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"

echo "---push---"
git push origin main
echo "---done---"
git log --oneline -3
