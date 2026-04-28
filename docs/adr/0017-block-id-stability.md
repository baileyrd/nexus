# ADR 0017: Block-ID Stability via Lazy Inline Stamping

**Date:** 2026-04-28
**Status:** Proposed

## Context

`crates/nexus-editor/src/markdown/id.rs` generates block ids by hashing
`(file_path, visit_order, BlockType)`. Pre-order `visit_order` is assigned
during parse:

```rust
let id = deterministic_block_id(&self.options.file_path, self.visit_order, &ty);
self.visit_order += 1;
```

This is **stable for files that round-trip unchanged** but **unstable
under edits**: inserting a block at offset N renumbers `visit_order` for
every downstream block, so every downstream id changes on the next
reload.

Today, no shipping feature depends on cross-session block-id stability,
so the issue is latent. Three Phase-7 features in the implementation plan
break without it:

- **BL-048** (drag-to-embed into canvas) — a canvas node referencing a
  source block by id breaks the moment the source document gains a
  block above the reference.
- **BL-049** (block-links navigator, `[[file#^block-id]]`) — link rot on
  every insert upstream of the target.
- **BL-050** (side-margin comments subsystem) — comment threads are
  anchored to block ids; insert-above-thread re-anchors every comment.

The Notion-block-UX plan enumerated two candidate fixes; this ADR
adds a third (the recommendation) and decides between all three.

## Decision

**Lazy inline stamping via HTML-comment anchors.**

A block gets a stable id **only when something first needs to reference
it across sessions**. The id is written into the markdown source as an
HTML comment at the end of the block's source range, in the form
`<!-- ^uuid -->`. Parse layer reads the stamped id when present and
falls back to the existing positional hash when absent.

### Mechanics

1. **Parse-side change** (`crates/nexus-editor/src/markdown/parse.rs`):
   - When the parser encounters a trailing `<!-- ^<uuid> -->` HTML comment
     on a block, that uuid becomes the block's id and the comment is
     stripped from the rendered block content (kept in the AST as a
     `Block.stable_id` field).
   - Absent the comment, fall back to `deterministic_block_id(...)` as
     today. No behaviour change for existing files.
2. **Stamp-on-reference** — a new IPC handler
   `com.nexus.editor::stamp_block` accepts `(file_path, block_id)` and:
   - Resolves the block's current source range from the parsed AST.
   - Writes a `<!-- ^<uuid> -->` comment at the end of that range.
   - Returns the persisted uuid (the same one passed in).
   - Idempotent — running twice is a no-op.
3. **Caller responsibility** — features that need stable refs
   (BL-048 / BL-049 / BL-050) call `stamp_block` before writing the
   reference. Features that don't (everything that ships today, plus
   editor undo / canvas / bases) keep using the positional hash as-is.

### Format choice

The `^uuid` form mirrors Obsidian's block-anchor syntax, so a
`[[file#^uuid]]` link in BL-049 round-trips losslessly with Obsidian
forges. The HTML-comment wrapper hides the marker in rendered markdown
(GFM, CommonMark, mdBook all skip HTML comments).

## Alternatives considered

### A. Eager HTML-comment stamping (every block, on every save)

Stamp every parsed block with `<!-- ^uuid -->` whenever the file is
saved. This was the obvious option from the Notion-UX plan.

**Rejected.** Three reasons:

1. **Source pollution.** Every paragraph, list item, and heading carries
   an inline comment. Markdown is supposed to be human-editable; users
   reading the raw file see noise after every block.
2. **Diff churn.** A single insert at the top of a document re-stamps
   nothing structurally, but the eager stamper would be tempted to
   re-canonicalize positioning, producing meaningless line-level diffs.
3. **External-edit friction.** Users editing files in vim / VS Code /
   Obsidian see the markers and may delete them, silently breaking
   references. Lazy stamping leaves untouched blocks unmarked, so this
   only happens for blocks that have actually been referenced.

### B. Out-of-band sidecar (`.forge/blocks.json`)

Keep the markdown source clean; persist `path → block_position → uuid`
in a JSON file under `.forge/`.

**Rejected.** External edits desynchronize the sidecar from the
markdown. A user opens the forge in Obsidian (or any non-Nexus editor),
inserts a block, and saves — the sidecar's `block_position` keys now
point to the wrong blocks. Reconciliation requires content-fingerprint
matching across sessions, which is the bulk of the implementation work
of the eager stamping option but with worse failure modes (the sidecar
silently lies; eager stamping at least visibly mismatches).

### C. Content-only hash (drop `visit_order`)

Hash on `(file_path, content_hash, BlockType)` and accept that duplicate
blocks share an id.

**Rejected.** Markdown lists, repeated headings, and intentionally
duplicate paragraphs all collide. The id ceases to identify a block
position; it identifies a content equivalence class. This breaks
features that need to address a *specific occurrence* (canvas-embed,
side-margin comments).

## Consequences

### Positive

- **Zero migration cost.** Existing files don't change. The positional
  hash continues to drive every shipping feature; lazy stamping kicks in
  only for new cross-session references.
- **Source stays clean.** Untouched blocks carry no marker. Only blocks
  that were intentionally referenced gain a `<!-- ^uuid -->` line.
- **Obsidian-compatible.** `^uuid` syntax matches Obsidian's block
  anchors; round-tripping a Nexus-stamped file through Obsidian
  preserves the anchor.
- **External-edit tolerant.** A user editing the file in vim and
  preserving the HTML comments preserves the references. A user who
  deletes a comment breaks one reference, locally — no silent drift
  across the rest of the file.
- **Backwards-compat for the parse layer.** `BlockType` and `visit_order`
  remain inputs to the fallback hash; nothing about the existing
  `deterministic_block_id` signature has to change.

### Negative

- **Two id sources.** Some blocks have a stamped uuid; others have a
  positional-hash uuid. The `Block.stable_id` field can be `None` until
  stamped. Code that wants the "always-stable" guarantee has to call
  `stamp_block` first, even on first reference.
- **Source pollution for referenced blocks.** Frequently-referenced
  documents will accumulate markers; users who care will see them in raw
  markdown. Mitigated by: only the *referenced* blocks are marked, not
  every block.
- **Stamp ordering.** If two clients race to stamp the same block,
  whichever write lands second wins. Acceptable because both writers
  produce the same uuid (the caller passes it in) — the race is benign.

### Neutral

- **No change to today's positional hash.** It remains in use for the
  canvas / editor / bases / undo paths that don't need cross-session
  stability.
- The IPC handler `com.nexus.editor::stamp_block` is the new surface;
  it gets a handler id and a capability check (`fs.write` on the host
  file path).

## Implementation sketch

1. Extend `Block` (in `crates/nexus-editor/src/block.rs`) with
   `stable_id: Option<BlockId>`.
2. In `parse.rs`, recognize a trailing `<!-- ^<uuid> -->` comment per
   block, parse it into `stable_id`, and strip it from the rendered
   content range.
3. Resolve `block.id()` as `stable_id.unwrap_or_else(|| deterministic_block_id(...))`.
4. Add the `stamp_block` IPC handler in
   `crates/nexus-editor/src/core_plugin.rs` that:
   - Reads the file via `fs.read`.
   - Re-parses to find the target block's source-range end.
   - Writes the file back with the comment inserted (capability-gated
     `fs.write`).
5. Add tests for: stamped block survives reordering; un-stamped block
   continues to use positional hash; round-trip through external editor
   that preserves the comment leaves the id stable.

## References

- `crates/nexus-editor/src/markdown/id.rs` — `deterministic_block_id`.
- `crates/nexus-editor/src/markdown/parse.rs:95` — `visit_order`
  assignment.
- `docs/notion-block-ux-plan.md` — original enumeration of options.
- ADR 0003 — storage owns the file watcher (relevant to stamp-driven
  rewrites not racing with the watcher).
- BL-048, BL-049, BL-050 in `docs/PRDs/BACKLOG.md`.
