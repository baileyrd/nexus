# Notion-style block UX — shell implementation plan

Surface the already-shipped block-tree engine (PRD-08) through the UI
affordances that make a block-based editor *feel* block-based: slash
menu, block handles, block selection, block drag, per-block actions.
Today the shell renders markdown line-by-line in CodeMirror 6 even
though every paragraph, heading, list item, and code block is already
tracked as a `Block` in the `nexus-editor` core plugin on the Rust
side.

References:
- **Spec**: `docs/PRDs/08-editor-engine.md` — the Notion-like target
  state. §4 covers the extension table, §6 the slash command system.
- **Engine evidence**: `crates/nexus-editor/src/*` — `Block`,
  `BlockType`, transactions, undo tree, annotations, `EditorCorePlugin`.
  3.7k LoC shipped.
- **Current shell entry point**: `shell/src/plugins/nexus/editor/`
  — CM6 view + `types.ts:212` already has a stubbed
  `{ kind: 'slash_command'; command: string }` variant waiting to be
  wired.
- **Implementation status**: `docs/PRDs/IMPLEMENTATION_STATUS.md` row 08.

## Goal

Editing a markdown file feels like editing a Notion page:

- Type `/` to summon a command palette that inserts / transforms
  blocks.
- Hover a block's gutter to reveal a handle; click the handle for a
  per-block menu, drag it to reorder.
- `Cmd+A` on a line selects the whole block.
- Selected blocks can be dragged, duplicated, deleted, or transformed
  en-masse with a single keystroke.

All of this sits on top of the plain-text markdown file — the on-disk
representation never changes. Every block-level action reduces to one
or more existing editor transactions dispatched through the Rust
engine.

## What's already in place (don't rebuild)

- **Block tree core** in `nexus-editor`: `Block`, `BlockType`,
  insert/delete/merge/split transactions, undo tree, annotations.
- **Sync pipeline**: CM6 text → 800 ms debounced `editor_sync_content`
  → Rust reparse → block tree. Outline + word count + AI already read
  this.
- **Transaction IPC**: the Rust side exposes transaction handlers
  callable from the shell (annotations, block moves, etc.). Some of
  the UI phases below reuse them; a couple may need a new one, called
  out in-line.
- **CM6 extensions registry**: `shell/src/plugins/nexus/editor/cm/`
  is set up to compose new extensions alongside syntax highlighting,
  snippets, decorations.

## Scope split

### Phase 1 — slash command menu

Budget: 1–2 days. Highest immediate user-visible payoff.

- **Trigger**: a `/` typed at a paragraph start (or after whitespace)
  opens an overlay popover anchored at the cursor.
- **Overlay UI**: small rounded box with an input row + scrollable
  category list. Filters by substring + fuzzy match.
- **Command registry**:
  - Built-in commands for every `BlockType` transform: Text, Heading
    1/2/3, Bullet list, Numbered list, Todo, Quote, Callout, Code,
    Divider, Math, Table, Embed.
  - Plugin hook so other plugins can contribute commands (e.g., a
    bookmarks plugin contributes `/bookmark`). Reuse the existing
    `api.commands.register` + a new `slashCommand: true` flag; no new
    registry.
- **Action**: executing a command replaces the current paragraph
  block (or inserts a new block) via the editor's existing
  transaction API. No new Rust handler required.
- **Keyboard**: `↑`/`↓` nav, `Enter` commit, `Esc` dismiss. `Tab`
  accepts the highlighted command. Typing characters filters.
- **CM6 extension**: ships as `slashCommandExt` under
  `shell/src/plugins/nexus/editor/cm/` next to the existing
  extensions. Hooks `keydown` on `/` and on all filter keys.
- **Stub already present**: `types.ts:212` has
  `{ kind: 'slash_command'; command: string }` — keep it, wire the
  palette into that transaction kind.

### Phase 2 — block selection extension

Budget: half a day.

- **`Cmd+A`** (`Ctrl+A` on non-mac) with the cursor inside a block
  selects the *block*'s text range instead of the document. A second
  `Cmd+A` expands to the whole document (Notion behavior).
- **Visual**: selected blocks show a subtle accent background that
  extends full-width of the line, not just the text. Already compatible
  with the existing `decoration compartment` CM6 extension.
- **Multi-block selection**: dragging across multiple blocks (or
  `Shift+↑`/`Shift+↓` at block boundary) promotes the selection into
  "block mode" — highlights extend gutter-to-gutter, and the next
  command applies to all selected blocks.
- **State**: a small local CM6 state field tracks "block-mode
  selection"; no Rust involvement.

### Phase 3 — block handles + per-block menu

Budget: 1–2 days.

- **Handle**: a 6-dot grip glyph that fades in on row hover in the
  left gutter of each block. Follows the cursor row as it moves.
- **Click**: opens a dropdown with the standard block operations:
  - Turn into → submenu of `BlockType` options (triggers the same
    transform as the slash menu).
  - Duplicate.
  - Delete.
  - Copy link to block (anchors on the block's persistent id —
    already assigned by the engine).
  - Color / highlight (Phase 5).
  - Comment (Phase 6, if we add comments).
  - Move up / Move down (keyboard shortcut also: `Alt+↑` / `Alt+↓`).
- **Drag**: dragging the handle initiates a block reorder. Ghost
  rendering at the new position, drop indicator line between
  neighbour blocks. Commit calls the block-move transaction on drop.
- **Multi-select**: dragging when multiple blocks are in block-mode
  selection moves them all.

### Phase 4 — per-block transforms without the menu

Budget: half a day.

- **Keyboard shortcuts** on the current block (no slash menu needed):
  - `#` → H1, `##` → H2, `###` → H3 (when at block start).
  - `-` + space → bullet list.
  - `1.` + space → numbered list.
  - `[]` + space → todo.
  - ```` ``` ```` → code block.
  - `>` + space → quote / callout prompt.
  - `---` on its own line → horizontal divider.
- Already achievable via existing markdown input rules; what's
  missing is the user-facing documentation and a "keyboard shortcuts"
  palette entry. Should consolidate under a CM6 `inputRulesExt`
  extension with a full audit.

### Phase 5 — inline annotations (Notion-style)

Budget: 2 days.

PRD-08 §2 describes annotations as layered ranges — bold, italic,
link, mention, color, highlight — over text. Most of the underlying
model is already in Rust. What's missing is the UI to toggle them:

- **Floating toolbar** appears on text selection above the selection
  rect. Shows: **B** (bold), **I** (italic), **code**, **link**, **color
  swatches**, **highlight swatches**. Mirrors Notion's inline toolbar.
- **Link**: opens a mini input for URL or wikilink autocomplete.
- **Mention (`@`)**: typeahead against people / files / blocks. Uses
  the same query path as wikilinks.
- **Color / highlight**: palette of 8–10 theme-coordinated swatches.
  The Rust annotation model already supports color tags; emits a span
  with `color`/`highlight` annotation on apply.

Keyboard: `Cmd+B` / `Cmd+I` / `Cmd+K` (link) / `Cmd+E` (code). Already
standard — wire them into the new annotation transactions.

### Phase 6 — polish + parity

Budget: variable; pick as desired.

- **Drag-to-embed**: drag a block into a `file` node in the Canvas
  view (depends on canvas shell plan Phase 5 — cross-plugin hook).
- **Block links** (`[[…#^block-id]]`): the engine already assigns
  persistent block ids. Add a "Copy block link" item in the handle
  menu and a navigator that resolves these on click.
- **Comment on block** (side margin comments): needs a comments
  subsystem; dependency for shipping.
- **Block AI actions**: right-click on block → AI submenu (explain /
  rewrite / expand / summarise) tied to `com.nexus.ai` streaming.
  Already have the streaming pipe from AI inline-complete; extend it
  to whole-block operations.
- **Multi-cursor from multi-block selection** — if all selected blocks
  are same type + single-line, promote to multi-cursor edit.

## Kernel-side add requests

Everything above is shell work except:

- **Block-move transaction** — if the engine doesn't already have a
  single "move block at id X to position Y" transaction, add it.
  Composing insert+delete works but generates two undo entries; a
  dedicated move transaction keeps undo coherent. Check
  `crates/nexus-editor/src/transaction.rs` first; add if absent.
- **Persistent block ids over markdown roundtrip** — engine assigns
  block ids today, but verify they survive a save + reopen cycle. If
  not, we either stamp ids as HTML comments in the markdown or
  maintain an out-of-band `{file}.blocks.json` sidecar under `.forge/`.
  Engineering choice; deferred until block-link feature lands.

## Phasing recap

- **Phase 1** (slash menu) — biggest perceived-quality jump; wireable
  without any Rust change.
- **Phase 2** (block selection) — prerequisite for block-mode
  operations. Tiny on its own.
- **Phase 3** (handle + menu + drag) — makes the editor visually
  Notion-like.
- **Phase 4** (input rules) — housekeeping; just documents behaviour
  users partially already get.
- **Phase 5** (inline annotations) — round-out edit controls.
- **Phase 6** (polish) — optional follow-ups.

Phases 1–3 together are the "feels like Notion" threshold; after that
we're in polish territory.

## Out of scope

- Server-side collaborative editing / CRDT mesh editing.
- Migrating away from plain-markdown storage. The whole value is
  keeping markdown on disk; block UX is a presentation layer.
- Nested pages / sub-pages as first-class blocks. Wikilinks and
  embeds already cover most of this; a deeper Notion-style nesting
  model is a separate architectural discussion.
- Per-block permissions.
- A visual database block inline (handled in the bases shell plan).
