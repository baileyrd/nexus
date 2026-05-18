# AFFiNE Capability Assessment & Nexus Portability

**Status:** Research / non-binding
**Authored:** 2026-05-07
**Branch:** `claude/assess-affine-nexus-SqKJi`
**Subject:** [`toeverything/AFFiNE`](https://github.com/toeverything/AFFiNE) (community + BlockSuite + OctoBase)

This document is a read-only audit. It catalogs what AFFiNE does, then asks — for each capability — whether Nexus could absorb it, *should* absorb it, and what porting would look like under Nexus's architectural invariants. Nothing here is a commitment.

---

## 1. Executive summary

AFFiNE is a feature-dense, CRDT-native "all-in-one" knowledge OS: a unified document/whiteboard editor (BlockSuite), an AI co-pilot, a database/property/views system, a NestJS+Postgres+Redis sync server, and clients on web/desktop/mobile. Roughly a third of its surface area is genuinely useful inspiration for Nexus; another third overlaps with capabilities Nexus already has under a different shape; the final third collides with Nexus's "files-as-truth" and "microkernel + IPC" invariants and is **not portable without abandoning identity**.

**The biggest tension** is data model. AFFiNE treats a Yjs CRDT block tree as the source of truth and projects to disk lazily; Nexus treats markdown files as the source of truth and projects to a rebuildable index. Anything that *requires* CRDT-as-truth (real-time multi-user co-editing, snapshot/restore via Yjs deltas, native collaborative undo) is structurally awkward to port. Anything that operates above the data layer — UI affordances, AI features, edgeless canvas, database views, slide generation, mind-map rendering — is fair game.

**Top three porting candidates** (high value, low conflict):

1. **Edgeless / infinite-canvas surface** as a first-class Nexus shell plugin (BL-053 already proposes a "forge visual target" — this is the natural home).
2. **Database block with multiple views** (table/kanban/list/gallery) layered on top of Nexus's existing `nexus-database` crate and the markdown front-matter / dataview pattern from the Obsidian-base format (ADR 0019).
3. **AI surface conventions** — slide generation from outline, mind-map from doc, "ask the workspace" RAG chat, AI inline-assist toolbars — most of this maps cleanly onto the per-handler AI capability model in ADR 0022 and the agent tool registry in ADR 0023.

**Top three "do not port"**:

1. **OctoBase / y-octo as Nexus's storage backbone.** Replacing file-as-truth violates invariant #1 and ADR 0003.
2. **Real-time multi-user co-editing.** It requires either a sync server (NestJS+Postgres+Redis) or peer-to-peer Yjs awareness; both are out of scope for a local-first single-user microkernel. Revisit only if Nexus ever ships a sync service crate.
3. **Workspace-as-CRDT-document semantics.** Nexus's "workspace" is a forge directory; AFFiNE's is a Yjs root doc with sub-docs. The two models do not interoperate without a translator.

---

## 2. AFFiNE at a glance

### 2.1 Tech stack

| Layer | Technology |
|---|---|
| Frontend | TypeScript / React, Vite, Jotai (state) |
| Editor engine | **BlockSuite** — Web Components, framework-agnostic, canvas renderer for the edgeless surface |
| Data layer | **Yjs** CRDTs via **y-octo** (Rust port, binary-compatible with Yjs); **OctoBase** as the embeddable database |
| Desktop | Electron + napi-rs (Rust↔Node bindings) |
| Mobile | Native Swift (iOS) + Kotlin (Android), shared Rust core via napi/uniffi |
| Server | NestJS (Node) — GraphQL primary, REST for binary/streaming/SSE; **PostgreSQL + pgvector + Redis** |
| Auth | JWT + sessions + RBAC |
| Licensing | MIT (Community Edition); proprietary EE add-ons (SSO, admin, branding) |

### 2.2 Repository layout (top level)

```
AFFiNE/
├── blocksuite/      # the editor engine (also separately at toeverything/blocksuite)
├── packages/        # frontend packages (@affine/component, @toeverything/theme, …)
├── tools/           # build/dev tooling
├── tests/
└── docs/
```

BlockSuite itself decomposes into:

| Package | Responsibility |
|---|---|
| `@blocksuite/store` | Yjs-backed document state, time-travel, doc collection |
| `@blocksuite/inline` | Minimal inline rich-text primitive (a "rope" of leaves) |
| `@blocksuite/block-std` | Framework-agnostic block schema, events, selection, clipboard |
| `@blocksuite/blocks` | Default block implementations + widgets |
| `@blocksuite/presets` | Plug-and-play `PageEditor`, `EdgelessEditor`, `CopilotPanel`, `DocTitle` |
| `@blocksuite/affine-ext-loader` | Extension registration & lifecycle for AFFiNE-flavoured BlockSuite |

OctoBase / y-octo:

- `y-octo`: tiny Rust CRDT implementation, binary-compatible with Yjs.
- `OctoBase`: Rust embeddable database wrapping y-octo with storage plugins (sqlite, fs) and a P2P sync protocol.

### 2.3 User-facing capability inventory

(Compiled from affine.pro, the GitHub README, the AFFiNE 0.25 release notes, and DeepWiki.)

**Editor (PageEditor):**
- Block-based document with rich text, headings, lists, code blocks, callouts, toggles, embeds.
- Linked docs (drag-from-sidebar or `@`-mention), backlinks panel with surrounding context.
- Per-doc properties (page width, doc mode, custom front-matter-like attributes).
- Snapshot/transform API → markdown / HTML import-export.
- Command system (type-safe editing primitives, "React-hooks-style").

**Edgeless / whiteboard (EdgelessEditor):**
- Infinite canvas, 60 fps target, canvas-based rendering.
- Sticky notes, shapes, brush/pen, connectors, frames, mind-map nodes.
- Same block content embeddable on canvas (a doc block is a "first-class citizen" of the edgeless surface).
- High-resolution image and PDF embedding for review/markup.
- "Sticky-notes → database row" conversion.

**Databases:**
- Table, Kanban, List views (Gallery exists in beta).
- Property types: text, select, multi-select, date, number, checkbox, link, file, formula, etc.
- Filter, sort, group-by.
- Database properties auto-sync to a linked doc's "Doc Info" sidebar.

**AI (AFFiNE AI):**
- Inline writing assist (rewrite, expand, summarise, translate).
- "Chat with workspace" RAG over docs.
- One-click outline → slide deck generation.
- Doc → mind-map generation.
- Image generation embeds.
- Multimodal (image + text) input as of 0.25 (Feb 2026).
- AI tagging / auto-organise.

**Collaboration / sync:**
- Real-time multi-user editing on a single doc (Yjs awareness).
- Cloud sync (AFFiNE.pro hosted) or self-hosted server.
- Offline-first with merge-on-reconnect (CRDT).
- Sharing, permissions, workspace members, RBAC.

**Platform reach:**
- Web (PWA), macOS, Windows, Linux (Electron), iOS, Android, Chrome extension.
- Self-host via Docker (NestJS + Postgres + pgvector + Redis).

**Templates / starters:**
- Vision boards, lesson plans, planners, OKRs, etc.

**Plugin / extension story:**
- BlockSuite extension system: register custom blocks, inline embeds, widgets, providers.
- Distinction between Block / Gfx (graphics on edgeless) / Widget extensions.
- *No* shipped third-party plugin marketplace; extensions are a developer-facing API.

---

## 3. Nexus subsystems (as of 2026-05)

Recapped from `CLAUDE.md`, `docs/PRDs/IMPLEMENTATION_STATUS.md`, and `crates/`:

| Crate | Role | Status |
|---|---|---|
| `nexus-kernel` | Event bus, IPC dispatch, capability gating | ✅ |
| `nexus-types`, `nexus-plugin-api` | Leaf contract crates | ✅ |
| `nexus-storage` | File-as-truth + SQLite index + Tantivy + watcher | ✅ |
| `nexus-editor` | Markdown editor engine, block tree | 🟢 |
| `nexus-ai` | Provider trait, context, embeddings, tool registry | 🟢 |
| `nexus-agent` | Agent loop on AI tool registry (ADR 0023, 0024) | 🟢 |
| `nexus-database` | Database engine (PRD-10) | 🟢 |
| `nexus-skills` | User-defined skill packages | 🟢 |
| `nexus-mcp` | MCP server frontend | 🟢 |
| `nexus-templates`, `nexus-formats`, `nexus-git`, `nexus-comments`, `nexus-linkpreview`, `nexus-terminal`, `nexus-theme`, `nexus-workflow`, `nexus-kv`, `nexus-security`, `nexus-panic-log` | Service plugins | varies |
| `nexus-bootstrap` | Boot orchestrator, dep-invariant tests | ✅ |
| `nexus-cli`, `nexus-tui`, `nexus-mcp` | Frontends | varies |
| `shell/` (`nexus-shell`) | Plugin-first Tauri desktop shell, post-ADR 0011 | 🟢 |

**Key invariants** (from `CLAUDE.md`):
1. Markdown files on disk are authoritative; index is rebuildable.
2. Microkernel isolation — kernel depends only on leaves; subsystems depend on kernel.
3. Everything goes through `context.ipc_call(plugin_id, command, args)`.
4. Capabilities gate all operations.

**Relevant ADRs for this assessment:**
- ADR 0003 — Storage owns the file watcher.
- ADR 0011 — Plugin-first shell (single Tauri target).
- ADR 0015 — Iframe-sandbox plugin runtime.
- ADR 0017 — Block ID stability.
- ADR 0018 — Embedding backend.
- ADR 0019 — Obsidian "Bases" format compatibility.
- ADR 0020 — Popout window architecture.
- ADR 0022 — Per-handler AI capabilities.
- ADR 0023 — Unify agent on AI tool registry.

---

## 4. Portability matrix

For each AFFiNE capability: what it is, the closest Nexus surface, the architectural fit, an effort estimate, and a recommendation.

Effort scale: **S** = days, **M** = weeks, **L** = months, **XL** = quarter+. Verdict: ✅ **Adopt**, 🟡 **Adapt** (substantial change in shape), 🟠 **Defer** (worth doing, not now), ❌ **Reject** (incompatible).

### 4.1 Editor & content model

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Block-based document tree | `nexus-editor` (PRD-08 already specifies this; ADR 0017 covers ID stability) | Strong — Nexus is already block-tree-shaped | — | ✅ Already adopted in spirit |
| Yjs CRDT as source of truth | — | Direct conflict with invariant #1 | XL | ❌ Reject |
| Snapshot/transform API (markdown/HTML import/export) | `nexus-formats` + editor | Strong | S–M | ✅ Adopt the API shape (port/export adapters) |
| Inline rich-text primitive (`@blocksuite/inline`) | Editor inline layer | Useful design reference; Nexus already targets markdown | S | 🟡 Reference only |
| Command system (type-safe editing primitives) | Editor IPC handlers + ADR 0021 versioning | Strong | M | ✅ Adopt the *pattern* (composable, typed commands) |
| Linked docs / `@`-mentions / drag-link | Storage + editor | Strong; fits file-as-truth (links resolve to file paths) | M | ✅ Adopt |
| Backlinks panel with surrounding context | Storage knowledge graph + editor side panel | Strong; partially exists | S–M | ✅ Adopt (UX upgrade) |
| Per-doc properties (page width, doc mode, custom) | Front-matter (already supported via `nexus-formats`) | Strong | S | ✅ Adopt as front-matter conventions |
| Time-travel via CRDT history | — | Requires CRDT-as-truth; Nexus relies on git for history | L | ❌ Use `nexus-git` instead |

### 4.2 Edgeless / infinite canvas

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Infinite canvas surface (60 fps, canvas renderer) | New shell plugin under `shell/src/plugins/nexus/canvas/`; relates to BL-053 "forge visual target" | Strong — UI-only; persists to a `.canvas` file (JSON) à la Obsidian Canvas (ADR 0019 already targets Bases parity) | L | ✅ Adopt — high-value differentiator |
| Sticky notes / shapes / connectors / pen | Same plugin | Strong | M | ✅ Adopt |
| Embedding doc blocks on canvas | Editor+canvas plugin via IPC | Medium — needs a "doc-block-as-canvas-element" contract | M–L | 🟡 Adapt |
| Image / PDF embed with markup | Canvas plugin + `nexus-formats` for PDF rendering | Medium — PDF rendering is a non-trivial dep (pdfium / pdf.js) | M | 🟡 Adapt |
| Sticky-notes → database row conversion | Canvas + `nexus-database` IPC | Strong if both exist | S–M (after deps) | ✅ Adopt |
| Frames / mind-map nodes / connectors | Canvas plugin | Strong | M | ✅ Adopt |

### 4.3 Database & views

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Table / Kanban / List / Gallery views | `nexus-database` (PRD-10) + UI plugin | Strong; PRD-10 doesn't yet enumerate views | M | ✅ Adopt |
| Property types (text, select, date, formula, file, link…) | `nexus-database` schema | Strong | M | ✅ Adopt |
| Filter / sort / group-by | `nexus-database` | Strong | S–M | ✅ Adopt |
| Database properties → linked doc's Doc Info | Editor side-panel + database IPC | Strong; fits the property-sync pattern | M | ✅ Adopt |
| Database backed by Yjs sub-doc | — | Conflict — Nexus stores rows in SQLite + projects to markdown front-matter / Bases | — | ❌ Reject (use Bases format from ADR 0019 instead) |

### 4.4 AI

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Inline writing assist (rewrite/expand/summarise/translate) | `nexus-ai` inline-assist (PRD-12 §11) | Already designed | S | ✅ Already adopted |
| Chat-with-workspace RAG | `nexus-ai` embeddings (PRD-12 §9, ADR 0018) + agent | Already designed | M | ✅ Already adopted |
| Outline → slide deck (one click) | New AI tool registered via ADR 0023 + a renderer (reveal.js / marp) | Medium — output format matters | M | 🟡 Adopt as an AI tool + skill |
| Doc → mind-map | New AI tool emitting JSON consumable by the canvas plugin | Strong if canvas exists | M | 🟡 Adopt (depends on §4.2) |
| Image generation embeds | `nexus-ai` provider + storage attachments | Medium — capability negotiation per provider; ADR 0022 covers per-handler caps | M | 🟡 Adopt behind capability flag |
| Multimodal input (image+text) | `nexus-ai` provider trait extension | Already designed | S–M | ✅ Adopt |
| AI auto-tagging / auto-organise | `nexus-ai` + storage knowledge graph | Strong | M | ✅ Adopt as a skill |
| AFFiNE Copilot panel UX | Shell plugin pattern (`shell/src/plugins/nexus/ai/`) | Reference only | S | 🟡 Take UX inspiration |

### 4.5 Collaboration & sync

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Real-time multi-user co-editing (Yjs awareness) | — | Out of scope; Nexus is single-user local-first; would require a new `nexus-sync` crate + server | XL | ❌ Reject for now (revisit if a sync service is on the roadmap) |
| Offline-first with auto-merge | Inherent to file-as-truth + git | — | — | ✅ Already covered (different mechanism) |
| Cloud sync (AFFiNE.pro / self-host) | Out of scope | Conflict with local-first ethos unless explicitly opted in | XL | ❌ Reject (use git remote / iCloud / Syncthing instead) |
| RBAC, workspace members, sharing | — | Requires server + accounts; Nexus has no auth model | XL | ❌ Reject |
| Comments & threads on blocks | `nexus-comments` (already exists) | Strong | S | ✅ Already adopted |

### 4.6 Platform reach

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| Web (PWA) | Out of scope per ADR 0011 (Tauri desktop is the target) | Conflict | XL | ❌ Reject |
| macOS / Windows / Linux desktop | `shell/` (Tauri) | Already there | — | ✅ Already adopted |
| iOS / Android | Out of scope; PRD-17 cross-platform doesn't include mobile | Conflict | XL | 🟠 Defer |
| Chrome extension (clip-to-workspace) | Could be a community plugin or `nexus-extension-api`-based companion | Medium | M | 🟠 Defer |

### 4.7 Plugin / extension surface

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| BlockSuite extension system (Block / Gfx / Widget) | Nexus plugin tiers (core Rust + WASM community) | Different shape but analogous taxonomy | — | 🟡 Reference for naming/lifecycle |
| Custom block schemas | Editor block registry (PRD-08, ADR 0017) | Strong | S–M | ✅ Adopt the registration *pattern* |
| Inline-embed extensions | Editor inline layer | Strong | M | ✅ Adopt |
| Widget extensions (toolbars, popups, menus) | Shell plugin contributions (ribbon/activity bar — ADR 0014) | Strong | — | ✅ Already adopted |
| Plug-and-play presets (`PageEditor`, `EdgelessEditor`) | Shell plugins | Strong | — | ✅ Already adopted |

### 4.8 Server / infrastructure

| AFFiNE capability | Nexus home | Fit | Effort | Verdict |
|---|---|---|---|---|
| NestJS GraphQL+REST server | — | Out of scope | XL | ❌ Reject |
| Postgres + pgvector | — | Nexus uses SQLite + Tantivy + per-ADR 0018 embedding store | — | ❌ Reject |
| Redis (sessions / job queue) | — | Out of scope | — | ❌ Reject |
| JWT + sessions + RBAC | — | Out of scope | — | ❌ Reject |
| Docker self-host | — | Nexus ships as a desktop app | — | ❌ Reject |

---

## 5. Architectural friction points

### 5.1 CRDT-as-truth vs. file-as-truth

This is the central incompatibility. AFFiNE round-trips every keystroke through a Yjs document; markdown is an export format. Nexus does the inverse: markdown is the canonical form and the index is rebuildable. Concretely:

- **Block IDs:** AFFiNE generates them inside the CRDT (Yjs assigns op-IDs). Nexus assigns stable IDs per ADR 0017 and persists them in the markdown via HTML comments / front-matter.
- **History:** AFFiNE history is a Yjs update log. Nexus history is git.
- **Concurrent edits:** AFFiNE merges via CRDT. Nexus merges via the file watcher + git.
- **Undo across sessions:** AFFiNE is free; Nexus relies on git or a per-doc undo journal.

Practically, this means anything you port from BlockSuite needs a translator at the data boundary: **a BlockSuite-style block tree on top of a markdown projection, not on top of Yjs.** This is workable for editor UX (BlockSuite's block tree is itself fine) but rules out copying their persistence layer.

### 5.2 Web Components vs. React

BlockSuite uses Lit-based web components on purpose, so it can be embedded in any frontend. AFFiNE (the app) wraps it in React. Nexus's shell already uses a TypeScript framework; embedding BlockSuite directly is plausible but the ROI of *reusing* their components vs. taking their *design* and re-implementing on Nexus's stack is rarely positive — the value is in the schema, command system, and edgeless renderer, not the rendering glue.

### 5.3 Server-shaped features

Anything with "share", "team", "sync", "cloud", "workspace member", "permissions" assumes a server. Nexus is single-tenant local-first by ADR 0011 / invariant #1. These are categorically out unless and until Nexus introduces a sync service crate (no current ADR proposes this).

### 5.4 License / lineage

AFFiNE Community is MIT, BlockSuite is MPL-2.0, OctoBase is Apache-2.0, y-octo is Apache-2.0 / MIT. All compatible with a permissive Nexus, but **MPL-2.0 is file-level copyleft** — directly vendoring BlockSuite source files would impose MPL-2.0 on those files. Either keep them in their own crate/package and respect MPL, or re-implement clean-room. Cite-and-credit either way.

---

## 6. Recommended next steps (if any of this is pursued)

These are *suggestions* for follow-up, not a plan:

1. **Edgeless canvas plugin spike.** A `shell/src/plugins/nexus/canvas/` plugin that persists to a JSON `.canvas` file (Obsidian-compatible per ADR 0019). Render with a 2D canvas; embed Nexus blocks as canvas elements via IPC. Single highest-impact lift from AFFiNE.

2. **Database views layer.** Extend `nexus-database` (PRD-10) with view definitions (table / kanban / list / gallery) stored as YAML/Bases files; UI is a shell plugin. Pairs naturally with the canvas → "sticky notes to rows" feature.

3. **AI tool registry: slide and mind-map generators.** Two new tools registered per ADR 0023, returning structured outputs (slide deck JSON, mind-map JSON) consumed by canvas / a presenter plugin. Cheap, high visibility.

4. **Editor command system audit.** AFFiNE's command-as-React-hook idea is good. Compare against the IPC handler model (ADR 0021) and decide whether the editor needs a finer-grained intra-handler composition layer.

5. **Side-panel "Doc Info + Backlinks" UX.** Mostly a UX consolidation task on top of capabilities Nexus already has.

6. **Defer everything sync-shaped.** Track separately if sync is ever in scope; do not let AFFiNE's sync UX leak into Nexus design before then.

---

## 7. Summary table

| Bucket | Count | Notes |
|---|---|---|
| ✅ Already adopted (in spirit) | 11 | Block tree, comments, inline assist, RAG, desktop, etc. |
| ✅ Adopt | 9 | Snapshot API, command pattern, linked docs, db views, mind-map output, multimodal, etc. |
| 🟡 Adapt | 8 | Canvas, PDF embed, slide gen, image gen, custom blocks, AI panel UX, inline primitive, doc-on-canvas |
| 🟠 Defer | 2 | Mobile, Chrome extension |
| ❌ Reject | 13 | Yjs-as-truth, real-time co-edit, server stack, RBAC, web PWA, Postgres, Redis, JWT, cloud sync, Docker self-host, time-travel via CRDT, db-as-Yjs-subdoc, workspace members |

---

## 8. Sources

- [toeverything/AFFiNE on GitHub](https://github.com/toeverything/AFFiNE)
- [toeverything/blocksuite on GitHub](https://github.com/toeverything/blocksuite)
- [toeverything/OctoBase on GitHub](https://github.com/toeverything/OctoBase)
- [y-crdt/y-octo on GitHub](https://github.com/y-crdt/y-octo)
- [BlockSuite Architecture (docs.affine.pro)](https://docs.affine.pro/blocksuite-wip/architecture)
- [BlockSuite Block Model](https://docs.affine.pro/blocksuite-wip/store/block-model)
- [BlockSuite Framework Overview](https://blocksuite.io/guide/overview)
- [`@blocksuite/affine-ext-loader` on npm](https://www.npmjs.com/package/@blocksuite/affine-ext-loader)
- [AFFiNE — What's New](https://affine.pro/what-is-new)
- [AFFiNE Server and API (DeepWiki)](https://deepwiki.com/toeverything/AFFiNE/3.1-server-and-api)
- [AFFiNE Self-host docs](https://docs.affine.pro/self-host-affine/)
- [AFFiNE Blocks with Databases](https://docs.affine.pro/core-concepts/elements-of-affine/blocks-with-databases)
- [AFFiNE Docs concepts](https://docs.affine.pro/core-concepts/elements-of-affine/docs)
- [OctoBase site](https://octobase.dev/)
- [Nexus `CLAUDE.md` (project root)](../../CLAUDE.md)
- [Nexus PRD index](../PRDs/IMPLEMENTATION_STATUS.md)
- [ADR 0003 Storage owns file watcher](../adr/0003-storage-owns-file-watcher.md)
- [ADR 0011 Adopt plugin-first shell](../adr/0011-adopt-plugin-first-shell.md)
- [ADR 0017 Block ID stability](../adr/0017-block-id-stability.md)
- [ADR 0018 Embedding backend](../adr/0018-embedding-backend.md)
- [ADR 0019 Obsidian Base format](../adr/0019-obsidian-base-format.md)
- [ADR 0022 Per-handler AI capabilities](../adr/0022-per-handler-ai-capabilities.md)
- [ADR 0023 Unify agent on AI tool registry](../adr/0023-unify-agent-on-ai-tool-registry.md)
