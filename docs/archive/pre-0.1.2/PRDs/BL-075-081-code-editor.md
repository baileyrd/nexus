# BL-075–081 — Code Editor Capability
_Captured: 2026-05-06_

## The question

What would it take to bring the Nexus editor up to a code editor level — meaning
a genuine alternative to VS Code, Zed, or Helix for everyday coding tasks?

## TL;DR

Nexus is ~60% of the way to a capable code editor by infrastructure and ~15% by
user-facing features. CM6 is the right foundation. The terminal is integrated.
Git exists. The plugin system handles extensions. The AI layer is ahead of most
code editors. **The gap is Language Server Protocol.** Without LSP, code editing
feels like a syntax-highlighted textarea. With LSP, it becomes a first-class IDE.

Minimum viable code editor: **~6 weeks** (dual-mode routing + nexus-lsp + CM6
LSP client). Full VS Code feature parity on core features: **~12–14 weeks**.

---

## The foundational constraint: the block tree model

The current editor routes all files through a `BlockTree` — every document is
parsed into typed blocks (headings, code blocks, lists, etc.). This model is
excellent for markdown-first knowledge bases and wrong for code files.

A 500-line Rust file is not a document with headings and bullet points. Forcing it
through the block tree would mean treating the entire file as a single `CodeBlock`,
which gives you nothing — no structure, no semantics, no LSP integration points.

**The solution is a dual-mode editor (BL-075):**

- **Document mode** (current): Block tree + CM6 + `nexus-editor` IPC sync. Used
  for `.md` files, notes, wikis. Unchanged.
- **Code mode** (new): Raw CM6 directly on file content, no block tree, backed by
  `nexus-lsp`. Used for `.rs`, `.ts`, `.py`, `.go`, etc.

CM6 supports both modes — it's the same editing engine. The shell routes by file
type: markdown → document mode, code file → code mode. The Rust backend difference
is that code mode skips `nexus-editor` entirely and routes through `nexus-lsp`.

---

## The core infrastructure gap: Language Server Protocol (BL-076 + BL-077)

Everything that makes a code editor feel like a code editor flows from LSP:

| LSP capability | What it enables |
|---|---|
| `textDocument/completion` | Context-aware symbol/method completions |
| `textDocument/hover` | Hover docs, type signatures |
| `textDocument/definition` | Go-to-definition (Cmd+Click) |
| `textDocument/references` | Find all references |
| `textDocument/rename` | Rename symbol across files |
| `textDocument/diagnostics` | Real-time error + warning squiggles |
| `textDocument/codeAction` | Quick-fix suggestions (import missing, etc.) |
| `textDocument/formatting` | Format document / format selection |
| `textDocument/signatureHelp` | Function signature pop-up while typing args |
| `textDocument/documentSymbol` | Symbol list for breadcrumbs + outline |
| `workspace/symbol` | Search all symbols across a project |

### nexus-lsp architecture (BL-076)

Mirrors `nexus-mcp` (spawns external processes, bridges JSON-RPC to kernel bus).

**Server config:** `.forge/lsp.toml`
```toml
[[servers]]
name = "rust-analyzer"
command = "rust-analyzer"
file_types = ["rs"]

[[servers]]
name = "typescript-language-server"
command = "typescript-language-server"
args = ["--stdio"]
file_types = ["ts", "tsx", "js", "jsx"]
```

**IPC surface** (`com.nexus.lsp`, ~12 handlers):
```
open_file(path)             — textDocument/didOpen
close_file(path)            — textDocument/didClose
change_file(path, content)  — textDocument/didChange (every edit, no debounce)
completions(path, position) — textDocument/completion → CompletionList
hover(path, position)       — textDocument/hover → Hover
definition(path, position)  — textDocument/definition → Location[]
references(path, position)  — textDocument/references → Location[]
rename(path, position, new) — textDocument/rename → WorkspaceEdit
code_actions(path, range)   — textDocument/codeAction → CodeAction[]
format(path)                — textDocument/formatting → TextEdit[]
list_servers()              — enumerate configured + connected servers
```

**Push events:**
```
com.nexus.lsp.diagnostics.<path>  — published on every server push
```

**Process lifecycle:** same pool/reconnect pattern as `nexus-mcp::ConnectionPool`.
Servers restart on crash, shut down on forge close.

### CM6 LSP client extension (BL-077)

`codemirror-languageserver` (MIT, well-maintained) provides a CM6 extension that
handles completion widget, diagnostic squiggles, hover tooltip, and go-to-definition
for any LSP server over a WebSocket or custom transport.

The Tauri bridge proxies CM6 LSP requests through `com.nexus.lsp` IPC instead of a
WebSocket — a thin adapter. The extension is activated only in code mode.

**Key note on change notifications:** LSP needs `textDocument/didChange` on every
edit, not after an 800ms debounce. Code mode's CM6 `updateListener` fires
`com.nexus.lsp::change_file` on every transaction commit directly.

---

## Secondary gaps (all independent of LSP)

### Multi-file search and replace (BL-078)

`com.nexus.storage` already has Tantivy FTS and file enumeration. What's missing:
- Search panel shell plugin (results grouped by file, previewed with context)
- Replace-in-file batching via `com.nexus.storage::write_file`
- CM6 decorations for match highlighting across open tabs
- Regex + case-sensitive + whole-word options

Effort: ~1 week. No new backend infrastructure.

### Git gutter + diff viewer (BL-079)

`nexus-git` already provides `git diff` output. Missing:
- Git gutter CM6 extension (added/modified/deleted line indicators in the margin)
- Diff view panel using CM6's `MergeView` extension (side-by-side or inline)
- Staging individual hunks from within the editor (`git add -p` equivalent via
  `com.nexus.git` IPC)
- Inline `git blame` (line author + timestamp on hover)

Effort: ~1.5 weeks. Backend exists; pure shell work.

### File tree / project explorer (BL-080)

`com.nexus.storage` enumerates all forge files. A project-explorer shell plugin
showing a directory tree with file-type icons, expand/collapse, and right-click
context menu (new file, rename, delete, copy path) is pure shell work.

Effort: ~3 days. No new backend infrastructure.

### DAP debugger integration (BL-081)

DAP is the debugger equivalent of LSP. Supporting it requires:
- `nexus-dap` core plugin (same architecture as `nexus-lsp`, different protocol)
- Debug panel shell plugin (Variables, Call Stack, Watch, Breakpoints panels)
- Breakpoint gutter decorations in CM6
- Debug toolbar (Continue, Step Over, Step Into, Step Out, Restart, Stop)

This is a major addition — the UI surface alone is a 3–4 week project. Right
direction long-term but not a prerequisite for basic LSP functionality.

---

## LSP-dependent follow-ups (small, no separate BL entries yet)

These become trivial once BL-076 + BL-077 land:

- **Breadcrumb navigation** — `textDocument/documentSymbol` feeds the current
  function/class context bar above the editor
- **Diagnostics (Problems) panel** — aggregates `com.nexus.lsp.diagnostics.*`
  events across all open files
- **Code folding** — CM6 `foldGutter()` + LSP `textDocument/foldingRange`
- **Workspace symbol search** — `workspace/symbol` powers Cmd+T quick-open by
  symbol name across the entire forge

---

## Sequencing

```
BL-075  Dual-mode editor routing       3 days   → unblocks everything
   ↓
BL-076  nexus-lsp core plugin          3 weeks  → load-bearing
   ↓
BL-077  CM6 LSP client extension       1 week   → user-facing LSP features
   ↓
BL-070  Vim keybindings               3–5 days  (independent, can run parallel)
BL-079  Git gutter + diff viewer      1.5 weeks (independent)
BL-078  Multi-file search/replace     1 week    (independent)
BL-080  File tree explorer            3 days    (independent)
   ↓
BL-081  DAP debugger                  4–6 weeks (deferred, needs LSP first)
```

Independent items (BL-070, BL-078, BL-079, BL-080) can ship in any order
alongside the LSP track. They don't block and don't get blocked.

**Minimum viable code editor milestone:** BL-075 + BL-076 + BL-077 (~6 weeks).
Completions, diagnostics, hover, go-to-definition, format-on-save. The features
users notice most.

**Full-featured code editor milestone:** add BL-070, BL-078, BL-079, BL-080,
and the LSP follow-ups (~12–14 weeks total from start).
