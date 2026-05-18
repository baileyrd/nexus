# Editor Implementation Assessment
_Assessed: 2026-05-06_

## Overall: 8.5/10 — Feature-complete for single-user markdown, two deferred subsystems away from being exceptional

The editor is the most code-dense part of the shell — 8,800 LoC of Rust core and 34,000+ LoC of
CM6 TypeScript extensions across 34 files. The architecture is sound and the core loop (CM6 owns
live text, Rust holds a debounced snapshot) is a pragmatic choice that avoids IPC saturation without
sacrificing responsiveness.

---

## What's fully implemented and first-class

**Block tree + transaction system.** 22 block types, 7 atomic operation types, a non-linear undo
tree (branching history, not a linear stack), and 226 unit tests. Every operation is reversible;
`goto(target_idx)` walks the LCA of the undo tree to support jumping between branches. Lossless
markdown roundtrip across all block types including frontmatter with reserved-key ordering.

**Stable block IDs.** Per ADR 0017, blocks can carry a `<!-- ^<uuid> -->` trailing comment that
survives upstream edits. `resolve_block_link` (handler 13) uses this to navigate cross-document
block references without being invalidated by insertions above the target.

**13 IPC handlers, all fully wired.** `open`, `close`, `get_tree`, `save`, `apply_transaction`,
`undo`, `redo`, `list_open`, `sync_content`, `get_markdown`, `stamp_block`,
`execute_database_view`, `resolve_block_link`. No stubs in the handler set.

**CM6 extensions — comprehensive.** 16 extensions covering the full editing surface:

| Extension | Lines | Purpose |
|---|---|---|
| `marginSuggestions.ts` | 35,232 | AI hints in the right margin with trigger rules |
| `blockHandle.ts` | 32,415 | Drag handles, collapse/expand, block selection |
| `slashCommand.ts` | 20,393 | 30+ slash commands (all block types + AI actions) |
| `databaseViewDecorations.ts` | 19,431 | Table/Kanban/Calendar/Gallery renderers |
| `livePreviewDecorations.ts` | 19,302 | Markdown rendered in-place as decorations |
| `databaseViewWidget.ts` | 17,770 | Database query block widget |
| `linkSuggest.ts` | 17,886 | Wikilink autocomplete with path resolution |
| `transactionBridge.ts` | 14,037 | CM6 transactions → Rust operation vectors |
| `inlineToolbar.ts` | 10,672 | Floating format toolbar on text selection |
| `ghostCompletion.ts` | 10,447 | Streaming AI ghost text (Tab accept, Esc dismiss) |
| `multiCursorPromote.ts` | 6,074 | Multi-cursor on same block auto-splits lines |
| `blockLinkNav.ts` | 5,745 | Cmd+Click to follow `[[links]]` |
| `blockSelection.ts` | 4,871 | Notion-style multi-block selection |
| `fencedCodeRegistry.ts` | 6,427 | Syntax highlighting language detection |
| `inputRules.ts` | 2,925 | Smart punctuation, auto-close brackets |
| `marginSuggestTrigger.ts` | 7,620 | Trigger rules for margin suggestions |

**AI properly wired, not bolted on.** Ghost completion (350ms debounce, 64-token cap,
single-flight with cancel-on-edit), margin suggestions with trigger rules, Cmd+I overlay — all
three stream through `com.nexus.ai::stream_chat`. The editor doesn't implement AI; it consumes it
correctly.

**MDX component runtime.** Self-closing (`<Card />`) and block-form
(`<Alert type="warning">...</Alert>`) both work. Inner body content is parsed for markdown
formatting. Unknown components fall back gracefully. Host-approved rendering without `unsafe-eval`.
Built-in registry: Card, Callout, Alert, Badge.

**Debounced sync is pragmatic.** 800ms debounce on the CM6→Rust sync path. Fast enough that the
outline panel updates once per second during typing; slow enough that the IPC channel stays
uncongested. The `BlockPositionMap` from the original PRD was formally retired (commit 6f3b36d) in
favor of this model.

**Slash commands.** 30+ commands, categorized, keyboard-friendly. Every block type is reachable
from `/` — headings, lists, code blocks, callouts, tables, database views, embeds, dividers, and AI
actions.

---

## IPC handler inventory

```
com.nexus.editor:

✅  1. open(path)                     → EditorSnapshot
✅  2. close(path)                    → {}
✅  3. get_tree(path)                 → BlockTree
✅  4. save(path, markdown)           → {}
✅  5. apply_transaction(path, ops)   → { errors }
✅  6. undo(path)                     → {}
✅  7. redo(path)                     → {}
✅  8. list_open()                    → { sessions }
✅  9. sync_content(path, markdown)   → BlockTree
✅ 10. get_markdown(path)             → { markdown }
✅ 11. stamp_block(path, id, uuid?)   → {}
✅ 12. execute_database_view(...)     → { records, groups }  ⚠ query executor gap
✅ 13. resolve_block_link(path, id)   → { block, path, line }
```

---

## Feature inventory

### Fully shipped

| Feature | Notes |
|---|---|
| Syntax highlighting | CM6 markdown rules + fenced-code language detection |
| Wikilinks | `[[path]]` parse, click-through, auto-suggest |
| Frontmatter | YAML parse/serialize, reserved key ordering |
| Code blocks | Language hints, line numbers toggle, syntax coloring |
| Tables | Markdown table parsing, edit-in-place cells |
| Embeds | Image, video, audio, bookmark, embed-note |
| Image support | Drag/drop, paste, slash command, relative-path resolution |
| Multi-cursor | Native CM6 + line-auto-split on same block |
| Drag-to-reorder blocks | Block handle + drag bridge to outline panel |
| Callouts | Icon + color + alert-type variants |
| Slash commands | 30+ commands |
| Annotations | 11 types (bold, italic, links, colors, mentions, math) |
| Undo/redo | Non-linear tree, branch on edit-after-undo |
| Transaction system | 7 operation types, atomic apply/reverse |
| Live preview | CM6 decorations render markdown in-place |
| Stable block IDs | `<!-- ^<uuid> -->` comments, persisted across edits |
| Database view renderers | Table/Kanban/Calendar/Gallery, filters, sorts |
| MDX components | Self-closing + block-form, built-in registry |

### Stubbed or partial

| Feature | Status |
|---|---|
| Database query execution | Renderers complete; `[[{db:query}]]` blocks parse but don't execute |
| Vim keybindings | Declared in PRD §9; not wired |
| Emacs mode | Declared in PRD §9; not wired |
| Block auto-stamping | Stable IDs require explicit `stamp_block` call; no auto-assign on first reference |
| Collaborative editing | CRDT design documented; not wired to live editing |
| Undo history persistence | Session-local only; lost on tab close |

---

## Where it falls short

### 1. Database query execution is missing

The database view renderers exist and look complete — filter engine (14 operators), sort engine
(multi-level null-last), Table/Kanban/Calendar/Gallery view layouts. But `[[{db:query}]]` inline
blocks parse without executing. There's no query dispatcher wired to the view renderer. Users see
the block; it doesn't run. This is the single largest functional gap.

### 2. Vim and Emacs keybindings declared but not wired

PRD-08 §9 specifies both. The CM6 keybinding system can support them. Neither exists in the
codebase. For a keyboard-first knowledge tool targeting developers this is a meaningful miss.

### 3. Undo history is session-local only

Close the editor tab and the undo tree is gone. On reopen the file loads from disk and history
starts fresh. For long documents with complex edit histories this is a real limitation. Persisting
a ring buffer of recent sessions to `.forge/` would close it.

### 4. Block stamping is manual

Stable IDs don't get assigned by default. A block needs explicit `stamp_block` before it can be
cross-referenced. The automatic-stamping-on-first-reference flow doesn't exist yet.

### 5. Collaborative editing is spec-only

The CRDT design is documented but not wired. Explicitly deferred — the block model was built with
collaboration in mind (stable IDs, annotation ranges) but the live sync loop doesn't exist.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| Block data model | 10/10 | 22 types, full annotation system, stable IDs |
| Transaction/undo system | 10/10 | Non-linear tree, 7 op types, 226 tests |
| Markdown roundtrip | 9/10 | Lossless for all block types; HTML stripped by design |
| CM6 extensions | 9/10 | 16 extensions, comprehensive coverage |
| Slash commands | 10/10 | 30+ commands, all block types represented |
| AI integration | 9/10 | Ghost text, margin hints, Cmd+I all wired and streaming |
| Database views | 6/10 | Renderers complete; query executor missing |
| Keybindings | 4/10 | Vim/Emacs declared, not implemented |
| Undo persistence | 4/10 | Session-local only |
| Collaborative editing | 2/10 | Spec only |

---

## The honest summary

The editor is production-ready for its primary use case — a single-user markdown-first knowledge
environment. Block model, transaction system, wikilinks, frontmatter, slash commands, and AI
integration are all genuinely first-class. The gaps are real but scoped: the database query
executor is weeks of work, vim keybindings are days, undo persistence is a focused weekend.
Collaborative editing is the only gap that requires new infrastructure rather than wiring.

For the vast majority of forge workflows — writing, linking, organizing — the editor is ready to
ship today.

---

## Key source files

```
crates/nexus-editor/src/
├── block.rs              (610)  — Block, BlockType (22 variants), annotations, properties
├── annotation.rs         (389)  — Annotation types, range adjustment, merge logic
├── tree.rs               (790)  — BlockTree structure, navigation helpers
├── transaction.rs        (883)  — 7 operation types, apply + reverse
├── undo_tree.rs          (369)  — Non-linear undo tree, LCA navigation
├── core_plugin.rs      (1,943)  — 13 IPC handlers, session management (max 100 sessions)
└── markdown/
    ├── parse.rs          (973)  — Comrak AST walker, block extraction
    ├── serialize.rs      (682)  — Block tree → markdown roundtrip
    ├── inline.rs         (535)  — Annotation serialization
    └── id.rs             (177)  — Stable ID comment parsing

shell/src/plugins/nexus/editor/
├── index.ts           (48,282)  — Plugin manifest + command registration
├── EditorView.tsx     (33,640)  — React component, tab layout, session lifecycle
├── editorStore.ts     (16,582)  — Zustand state
├── sessionManager.ts  (11,953)  — Session lifecycle
└── cm/                          — 16 CM6 extensions, ~251 KB total
    ├── blockHandle.ts   (32,415)
    ├── marginSuggestions.ts (35,232)
    ├── slashCommand.ts  (20,393)
    ├── ghostCompletion.ts (10,447)
    ├── transactionBridge.ts (14,037)
    └── … 11 more
```
