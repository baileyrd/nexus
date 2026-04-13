# Nexus Feature Backlog

> Features identified in the [Growth Plan](Nexus_Growth_Plan.md) that are not fully covered by existing PRDs 01–17. Items are categorized by coverage gap and listed in suggested implementation order.

---

## New Features (not addressed in any PRD)

### BL-001: Daily Notes

**Source**: Growth Plan Phase 5, Task 5.1
**Effort**: Small (0.5 day)
**Crate**: `nexus-cli`

Template-based daily note creation via `nexus content daily [--date YYYY-MM-DD]`. Creates `notes/daily/YYYY-MM-DD.md` with YAML frontmatter (date, tags) and section stubs (Tasks, Notes). Opens existing note if already present.

### BL-002: Typed Property Index

**Source**: Growth Plan Phase 0, Task 0.1
**Effort**: Small (schema migration)
**Crate**: `nexus-storage`

Add `value_num REAL`, `value_date INTEGER`, `value_bool BOOLEAN` columns to the `properties` table alongside the existing `value TEXT`. Enables type-aware property queries (numeric ranges, date filtering, boolean checks) without JSON parsing at query time. Requires a schema migration (v2) and updates to property insert/query logic.

---

## Partially New Features (concept exists in PRDs but design is unspecified)

### BL-003: Search Scoping Operators

**Source**: Growth Plan Phase 2, Task 2.5
**Effort**: Medium (1 day)
**Crate**: `nexus-storage`
**Related PRD**: PRD 03 (mentions faceted search, shows `path:notes/*` example — but no other operators or implementation strategy)

Implement query-time scope prefixes for Tantivy search:
- `tag:rust` — post-filter via SQLite tags table
- `path:notes/` — post-filter on file path prefix
- `prop:status:done` — post-filter via SQLite properties table
- `type:heading` — Tantivy field filter on block type

Approach: parse scope prefixes from query string, run plain-text portion through Tantivy, post-filter results with SQLite, merge scores.

### BL-004: Obsidian-Style 3-Tier Link Resolution

**Source**: Growth Plan Phase 1, Task 1.3
**Effort**: Medium (1 day)
**Crate**: `nexus-storage`
**Related PRDs**: PRD 03 (link resolution cache), PRD 06 (wikilink resolution — no specifics)

Define a concrete resolution cascade for wikilinks:
1. Exact path match: `[[folder/note]]` resolves to `folder/note.md`
2. Filename-only match: `[[note]]` resolves to the first file whose stem is `note`
3. Case-insensitive fallback

Resolution must run bidirectionally: when a new file is created, check all unresolved links for matches; when a file is deleted, mark links pointing to it as unresolved. Integrates with reconcile pass.

### BL-005: In-Memory Knowledge Graph (petgraph)

**Source**: Growth Plan Phase 1, Tasks 1.1–1.6
**Effort**: Large (5–7 days)
**Crate**: `nexus-storage`
**Related PRD**: PRD 10 (mentions knowledge graphs as future concept — no implementation design)

Build a live `petgraph::StableGraph<NodeData, EdgeData, Directed>` that:
- Rebuilds from SQLite links table on startup
- Updates incrementally on file write/delete
- Supports backlink queries, outgoing link queries, unresolved link detection
- Provides BFS neighbor traversal up to N hops
- Tracks "phantom" nodes (link targets that don't exist as files)
- Publishes `GraphRebuilt` and `BacklinksChanged` events via the kernel EventBus

Includes CLI commands (`nexus graph status`, `nexus content backlinks <path>`) and a TUI backlinks panel.

### BL-006: Block-Level Content Chunking for RAG

**Source**: Growth Plan Phase 3, Task 3.4
**Effort**: Small (0.5 day)
**Crate**: `nexus-ai`
**Related PRD**: PRD 12 (one line: "split into chunks respecting provider limits" — no design)

Use the parser's existing block-level output as natural chunk boundaries:
- Each parsed block (heading, paragraph, code block) becomes one chunk
- Oversized blocks (>2000 chars) split on sentence boundaries
- Each chunk includes the nearest parent heading as context prefix: `"## Section Name\n\n{block content}"`
- Chunk struct carries `file_path`, `block_id`, `block_type`, `content`, `heading_context`

### BL-007: CRDT-over-Git Transport

**Source**: PRD 11, Section 4.4 (Level 3)
**Effort**: Large (2–3 weeks)
**Crate**: `nexus-git`, new `nexus-crdt`
**Related PRD**: PRD 11 (specified but deferred — requires collaborative editing layer)

Serialize Nexus CRDT state (rich text buffer) as JSON in `.nexus/crdt-state.json`, tracked in git. On push, CRDT state is included in commits. On pull with merge conflict in the CRDT file, apply CRDT merge semantics (operation-based or state-based) for automatic convergence. Fallback to content conflict if CRDT merge fails. Enables multi-user async collaboration via git push/pull without manual conflict resolution. Prerequisite: a CRDT-based editor engine (PRD 08) or collaborative editing layer.
