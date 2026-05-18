# GitNexus Capability Assessment

**Source:** https://github.com/abhigyanpatwari/GitNexus  
**Assessed against:** Nexus microkernel workspace (`baileyrd/nexus`)  
**Date:** 2026-05-08

---

## 1. What GitNexus Is

GitNexus is an open-source TypeScript monorepo that transforms codebases into **structural knowledge graphs**. Its core thesis is that AI coding agents work blindly without pre-computed architectural context — they modify functions without knowing what depends on them. GitNexus indexes repositories at rest, builds a typed graph of symbols and their relationships, and exposes that graph through 13 MCP tools, an HTTP API, and a browser UI.

It is **not** a git-workflow tool in the way `nexus-git` is. The "Git" in the name refers to its ability to map `git diff` hunks to indexed symbols before a commit, not to providing staging/commit/branch operations.

---

## 2. GitNexus Capabilities (Inventory)

### 2.1 Ingestion Pipeline

A **12-phase typed DAG** (`pipeline.ts`) that produces a `KnowledgeGraph` in memory, then persists it to LadybugDB:

```
scan → structure → [markdown, cobol] → parse → [routes, tools, orm]
     → crossFile → scopeResolution → mro → communities → processes
```

| Phase | What it produces |
|-------|-----------------|
| `scan` | File paths and sizes |
| `structure` | File/Folder nodes, `CONTAINS` edges, path index |
| `markdown` | Section nodes, cross-link edges |
| `cobol` | COBOL symbol nodes (regex-based, no Tree-sitter) |
| `parse` | Symbol nodes, `IMPORTS`/`CALLS`/`EXTENDS` edges; routes, tools, ORM queries detected |
| `routes` | Route nodes + `HANDLES_ROUTE` edges (web framework route extraction) |
| `tools` | Tool/RPC nodes + `HANDLES_TOOL` edges |
| `orm` | `QUERIES` edges between methods and DB queries |
| `crossFile` | Cross-file return-type propagation in topological order |
| `scopeResolution` | Registry-primary call resolution (Python, C# today; replaces legacy DAG per language) |
| `mro` | `METHOD_OVERRIDES` + `METHOD_IMPLEMENTS` edges via per-language MRO strategy |
| `communities` | Community nodes + `MEMBER_OF` edges via **Leiden algorithm** clustering |
| `processes` | Process nodes + `STEP_IN_PROCESS` edges (execution flow from entry points) |

Phases that are slow or unneeded for pure search can be skipped via `skipGraphPhases`.

### 2.2 Multi-Language AST Parsing (Tree-sitter)

**14–16 languages** share a unified capture-tag vocabulary (`@definition.class`, `@call.name`, `@import.source`, `@heritage.extends`, etc.) so downstream extraction has no language branches.

Languages: TypeScript, JavaScript, Python, Java, Go, Rust, C#, Swift, Ruby, Kotlin, PHP, C, C++, Dart, Cobol (regex), and more.

Each language plugs in a `LanguageProvider`:

| Field | Purpose |
|-------|---------|
| `treeSitterQueries` | S-expression queries → unified capture tags |
| `importSemantics` | `named` / `wildcard-leaf` / `wildcard-transitive` / `namespace` |
| `importResolver` | Language-specific path → file resolution |
| `exportChecker` | Public/exported symbol detection |
| `typeConfig` | Type annotation extraction |
| `mroStrategy` | `first-wins` / `c3` / `ruby-mixin` / `none` |

### 2.3 Call Graph Resolution (Two Paths)

**Legacy 6-stage DAG** (all unmigrated languages):

```
extract-call → classify-form → infer-receiver → select-dispatch → resolve-target → emit-edge
```

Resolves with a 3-tier confidence model: same-file (0.95) → import-scoped (0.9) → global (0.5).

**Scope-resolution pipeline** (Python, C# — RFC #909): A registry-primary approach replacing the legacy DAG per language. Adds `ScopeResolver` per language; no shared-code changes needed. Both paths emit identical edges to downstream consumers ("same-graph guarantee").

### 2.4 Graph Schema (LadybugDB)

**31 node types:** File, Folder, Function, Class, Interface, Method, Constructor, CodeElement, Struct, Enum, Macro, Typedef, Union, Namespace, Trait, Impl, TypeAlias, Const, Static, Property, Record, Delegate, Annotation, Template, Module, Community, Process, Route, Tool, Section, Embedding.

**17 relationship types:** `CONTAINS`, `DEFINES`, `CALLS`, `IMPORTS`, `EXTENDS`, `IMPLEMENTS`, `HAS_METHOD`, `HAS_PROPERTY`, `ACCESSES`, `METHOD_OVERRIDES`, `METHOD_IMPLEMENTS`, `MEMBER_OF`, `STEP_IN_PROCESS`, `HANDLES_ROUTE`, `FETCHES`, `HANDLES_TOOL`, `ENTRY_POINT_OF`.

Stored in **LadybugDB** (custom SQLite-backed graph store with separate node tables per type and a unified `CodeRelation` table).

### 2.5 Hybrid Search

- **BM25** full-text index over symbol names, file paths, docstrings
- **Semantic vector embeddings** — Snowflake `arctic-embed-xs` (384-dimensional), stored incrementally by content SHA1; skipped if >50k nodes
- **Reciprocal Rank Fusion (RRF, K=60)** merges BM25 and vector results
- Exposes via `query` MCP tool with natural-language or keyword input

### 2.6 Impact Analysis

`impact` MCP tool: given a symbol, traverses the `CALLS` graph outward, grouping affected symbols by traversal depth (direct → indirect → transitive) and attaching a risk summary. Confidence-weighted.

`detect_changes` MCP tool: maps uncommitted `git diff` hunks to indexed symbols and traces which execution-flow processes are impacted — enabling pre-commit blast-radius preview.

### 2.7 Git Staleness Detection

`git-staleness.ts` module: compares the `lastCommit` stored in `.gitnexus/meta.json` against the current HEAD using `git rev-list --count`. Surfaces warnings when the index is behind HEAD, and handles sibling-clone drift (two checkouts of the same remote at different commits) via longest-prefix and remote-URL matching in a global `~/.gitnexus/registry.json`.

### 2.8 MCP Server (13 Tools)

All tools are available to AI agents (Claude Code, Cursor, Windsurf, etc.) via stdio MCP:

| Tool | Purpose |
|------|---------|
| `list_repos` | Enumerate indexed repositories |
| `query` | Hybrid BM25 + vector search over the graph |
| `cypher` | Ad hoc Cypher queries against the graph schema |
| `context` | 360° view of one symbol (callers, callees, processes, file location) |
| `impact` | Blast radius + risk summary for a symbol change |
| `detect_changes` | Map uncommitted git diffs to affected symbols and processes |
| `rename` | Graph-guided multi-file rename with confidence tagging and dry-run |
| `route_map` | API route → handler → consumer mappings |
| `tool_map` | MCP/RPC tool definitions and handlers |
| `api_impact` | Pre-change impact for API route handlers (combines route, shape, impact) |
| `shape_check` | Detect response-shape ↔ consumer property-access mismatches |
| `group_list` | List configured repo groups |
| `group_sync` | Rebuild group Contract Registry and cross-links |

Group-mode variants of `query`, `context`, and `impact` fan out across a registered repo group and merge results via RRF.

### 2.9 Agent Skills (4)

Pre-installed `.claude/skills/` files guide AI agents through:
1. Exploring unfamiliar codebases
2. Debugging through call chains
3. Impact analysis before making changes
4. Safe refactoring with dependency mapping

### 2.10 Wiki / Documentation Generation

`src/core/wiki/` generates **LLM-powered documentation** from the knowledge graph:
- `generator.ts` — orchestrates doc generation from graph queries
- `llm-client.ts` — interfaces with an LLM for prose generation
- `prompts.ts` — prompt templates per documentation type
- `graph-queries.ts` — graph traversals to gather symbol context
- `html-viewer.ts` — renders generated docs for the web UI

### 2.11 Multi-Repo Group Analysis

A **Contract Registry** tracks HTTP contracts across microservice repos:
- `group_sync` extracts provider/consumer contracts and cross-links them
- `api_impact` fans out blast-radius analysis across service boundaries
- `gitnexus://group/{name}/contracts` resource exposes the contract map

### 2.12 Web UI

`gitnexus-web/` (React + Vite): interactive graph explorer with node/edge visualization and an AI chat panel. Runs entirely in-browser (WASM-backed Tree-sitter) for repos ≤ ~5k files, or connects to a local HTTP server for larger repos.

---

## 3. What Nexus Already Has (Overlap)

| GitNexus capability | Nexus equivalent | Notes |
|--------------------|-----------------|-------|
| Git operations (status, diff, blame, log, staging, commits, branches, remotes, tags, stash, merge, rebase, cherry-pick) | `nexus-git` — 36 IPC handlers backed by libgit2 | Nexus is **more complete** here |
| Auto-commit with debounce | `nexus-git/auto_commit.rs` | Full feature parity |
| FTS / keyword search | `nexus-storage` — Tantivy + BM25 | Covers markdown/notes, not code symbols |
| AI tool integration | `nexus-ai` — `git_log` tool | Git history accessible to LLMs |
| Workflow triggers on git events | `nexus-workflow` — `git_event` trigger type | Subscribes to `com.nexus.git.*` events |
| MCP server | `nexus-mcp` | Exists; exposes forge operations, not code structure |

---

## 4. Gaps: What Nexus Lacks

These are GitNexus capabilities that have **no equivalent** in Nexus today:

| Capability | GitNexus | Nexus gap |
|-----------|----------|-----------|
| **Code structural knowledge graph** | 31 node types, 17 relationship types built from AST | Nexus's knowledge graph covers markdown links and backlinks, not code symbols |
| **Multi-language AST parsing** | Tree-sitter, 14–16 languages | No code parsing exists in Nexus |
| **Call graph / import resolution** | 3-tier confidence model, 6-stage DAG | No function-call or import tracking |
| **Method Resolution Order (MRO)** | Per-language strategies (C3, first-wins, ruby-mixin) | Not applicable today |
| **Community detection** | Leiden algorithm clustering of symbols | No functional clustering |
| **Process / execution flow tracing** | Entry-point to call-chain mapping | Not present |
| **Semantic vector embeddings** | arctic-embed-xs 384D, incremental by SHA1 | Tantivy only; no dense vectors |
| **Hybrid search (BM25 + vectors + RRF)** | Full hybrid for code symbols | Tantivy BM25 only; no vector fusion |
| **Impact analysis (blast radius)** | Depth-grouped traversal + risk scoring | No code-change blast radius |
| **Git-diff → symbol mapping** | `detect_changes` maps hunks to code symbols | Nexus tracks diffs but not to symbols |
| **API route extraction** | Route/handler/middleware graph | Not present |
| **Response shape checking** | Consumer property-access vs. API contract | Not present |
| **Multi-repo group / Contract Registry** | Cross-service contract bridge | Single forge only |
| **LLM-powered wiki generation** | From knowledge graph → structured docs | No code-doc generation |
| **Interactive graph visualisation** | React + Vite web UI | TUI only; no graph renderer |

---

## 5. Portability Assessment

Portability into Nexus is evaluated against four constraints:

1. **Language boundary** — GitNexus is TypeScript; Nexus is Rust. Tree-sitter has first-class Rust bindings (`tree-sitter` crate). SQLite is already present via `nexus-storage`. ONNX runtime (for arctic-embed-xs) has Rust bindings (`ort` crate). All core dependencies are bridgeable.
2. **Microkernel isolation** — New capabilities must live in a `nexus-<service>` crate and expose IPC handlers; no direct frontend deps.
3. **IPC over direct calls** — Every new tool must be reachable via `context.ipc_call(plugin_id, command, args)`.
4. **Capability gating** — Any capability that reads forge files or runs computationally must be gated.

### 5.1 High Portability — Strong Fit

These capabilities are architecturally well-aligned and the Rust ecosystem has the needed primitives.

#### A. Code Structural Knowledge Graph (`nexus-codegraph` crate)

**What it does:** Builds a graph of code symbols (files, functions, classes, methods) and their relationships (calls, imports, inherits) from a codebase.

**Portability rationale:**
- `tree-sitter` Rust crate is mature and widely used. Nexus already depends on it indirectly (via `tree-sitter-highlight` in the editor service).
- SQLite is already the storage layer in `nexus-storage`; the same database can host code-graph tables alongside the markdown index using a separate schema prefix, or as a distinct `.forge/code.db`.
- The 12-phase DAG pattern maps naturally onto Rust async tasks or a `tokio` task graph.
- Exposes as a `CorePlugin` registered by `nexus-bootstrap`, with IPC handlers for graph queries.

**Recommended scope (MVP):**
- Phases: `scan → structure → parse (TS/JS, Rust, Python) → crossFile → mro`
- Node types: File, Function, Class, Method, Interface, Module
- Relation types: CALLS, IMPORTS, EXTENDS, IMPLEMENTS, HAS_METHOD
- Storage: SQLite tables mirroring LadybugDB's pattern
- IPC handlers: `codegraph.query_symbol`, `codegraph.callers`, `codegraph.callees`, `codegraph.file_symbols`

**Deferred:** community detection (Leiden), process tracing, route/ORM extraction, COBOL.

#### B. Git-Diff → Symbol Mapping (extend `nexus-git`)

**What it does:** Takes a set of changed hunks from a `git diff` and maps them back to the code symbols (functions, classes) that overlap with those line ranges.

**Portability rationale:**
- `nexus-git` already exposes `diff_file` and `diff_staged` with hunk-level granularity (`HunkDiff` with `old_start`/`old_count`/`new_start`/`new_count`).
- With the code graph in place, this is a join: `hunk.new_start` ∈ `symbol.start_line..symbol.end_line` for the file.
- No new dependencies. Adds one IPC handler: `git.detect_changes` → returns a list of affected symbols per changed file.

**Recommended scope:** Single new handler in `nexus-git/core_plugin.rs` that queries `nexus-codegraph` for symbols overlapping each diff hunk, returning `Vec<AffectedSymbol>` with name, kind, file path, and line range.

#### C. Impact Analysis (extend `nexus-codegraph`)

**What it does:** Given a symbol, traverses the `CALLS` graph outward to a configurable depth and groups affected symbols by distance, yielding a blast-radius report.

**Portability rationale:**
- Pure graph traversal over the `CALLS`/`IMPORTS` tables already in `nexus-codegraph`.
- BFS or DFS with depth limit; depth-grouped result maps cleanly to `serde_json::Value`.
- Adds one IPC handler: `codegraph.impact` → `Vec<ImpactDepth>` (depth, symbols, risk score).

**Recommended scope:** `codegraph.impact(symbol_id, max_depth: u8)` returning depth-grouped affected symbols. Risk scoring can be a simple heuristic (affected count × depth weight) to start.

#### D. Code-Aware Search Enhancement (extend `nexus-storage`)

**What it does:** Extends FTS to cover code symbols, and adds optional semantic vector search via embeddings.

**Portability rationale:**
- Tantivy (already in `nexus-storage`) can index code symbols alongside markdown. Adding a `SymbolDocument` schema to the existing Tantivy index is low-friction.
- Vector embeddings via the `ort` crate (ONNX Runtime) are well-supported in Rust. arctic-embed-xs is a public ONNX model. Embedding generation can be opt-in (large repos, slow).
- RRF fusion is a pure algorithmic layer (a few lines of Rust) applied before returning search results.

**Recommended scope (phase 1):** BM25 over code symbols using existing Tantivy. Vector embeddings and RRF as a follow-on phase behind a capability flag.

#### E. MCP Tools for Code Structure (extend `nexus-mcp`)

**What it does:** Exposes code graph intelligence — `context`, `impact`, `detect_changes` — to AI agents via the MCP protocol.

**Portability rationale:**
- `nexus-mcp` already wires tool calls through `context.ipc_call`. Adding new tools is pattern-match on a string + forward to `nexus-codegraph` IPC handlers.
- No new dependencies. Three new MCP tool registrations follow the exact same pattern as existing tools.

**Recommended scope:** `nexus_context` (callers/callees for a symbol), `nexus_impact` (blast radius), `nexus_detect_changes` (diff → symbols). All route through IPC to `nexus-codegraph`.

### 5.2 Medium Portability — Worthwhile but More Effort

#### F. Hybrid Search with Vector Embeddings

Tantivy BM25 is already present. The gap is dense-vector storage and similarity search. SQLite's `sqlite-vss` or `sqlite-vec` extension (both available as Rust crates) would add approximate nearest-neighbour search. RRF merging is trivial. Main cost: shipping an ONNX model binary and the `ort` runtime (~60 MB). Could be gated behind an opt-in capability flag.

#### G. LLM-Powered Documentation Generation (extend `nexus-ai` or new `nexus-wiki` crate)

**What it does:** Queries the code graph for context about a symbol or module, constructs a prompt, and calls an LLM to produce structured documentation.

**Portability rationale:**
- `nexus-ai` already manages LLM interactions. Adding a "generate docs for this symbol" workflow is a new IPC handler + prompt template.
- Graph queries for context (callers, callees, file, module) come from `nexus-codegraph`.
- Output could be written back as a markdown file in the forge (preserving file-as-truth).

**Recommended scope:** `nexus-ai` new handler `ai.generate_docs(symbol_id)` that pulls graph context, constructs a prompt, calls the LLM, and returns or writes a markdown doc.

#### H. Community Detection (within `nexus-codegraph`)

**What it does:** Applies the Leiden community detection algorithm to the `CALLS`/`IMPORTS` graph to discover functionally cohesive clusters.

**Portability rationale:**
- The Leiden algorithm has Rust implementations (`leiden-rs`, `graspologic` bindings). The graph structure is already in SQLite.
- Main cost: an additional indexing phase that can be skipped for performance.
- Output is `Community` nodes + `MEMBER_OF` edges, useful for organizing large codebases.

**Recommended scope:** Optional phase in `nexus-codegraph` triggered by a `codegraph.build_communities` IPC call or after initial graph construction if the forge has > N symbols.

### 5.3 Low Portability — Poor Fit or Deferred

| Capability | Why low fit |
|-----------|------------|
| **Web UI graph visualizer** | Nexus targets a TUI + Tauri shell. The graph explorer is a full React app. TUI graph rendering is a separate, smaller problem. |
| **Multi-repo group / Contract Registry** | Nexus is single-forge today. Multi-repo support would require ADR-level changes to the forge model before this is applicable. |
| **API route extraction + shape checking** | Useful for server-side web frameworks (Express, FastAPI, etc.). Nexus users are primarily note-takers and knowledge workers, not necessarily web devs. Low ROI without evidence of demand. |
| **COBOL support** | Niche. COBOL has no Tree-sitter grammar; GitNexus uses regex. Not a priority. |
| **Docker / Kubernetes deployment** | Nexus ships as a CLI binary and a Tauri shell; it doesn't run as a server farm. |
| **LadybugDB migration** | LadybugDB is a custom TypeScript graph store. Nexus already has SQLite + Tantivy. No need to port LadybugDB specifically. |
| **Scope-resolution pipeline (RFC #909)** | The full two-path (legacy DAG + scope-resolution) coexistence system is a significant engineering effort. The simplified single-path approach with Rust is more appropriate for Nexus's initial code graph. |

---

## 6. Proposed Implementation Order

Given Nexus's current state, this sequence minimises cross-crate dependencies and delivers user-visible value earliest:

```
Phase 1 — Foundation
  nexus-codegraph crate
    ├── Tree-sitter parsing (Rust, TypeScript, Python to start)
    ├── SQLite schema for code symbols + relationships
    ├── CALLS / IMPORTS / EXTENDS graph construction
    └── IPC handlers: file_symbols, callers, callees

Phase 2 — Intelligence
  nexus-git extension
    └── detect_changes: diff hunk → overlapping symbols
  nexus-codegraph extension
    └── impact: BFS blast-radius from a symbol

Phase 3 — Discoverability
  nexus-storage extension
    └── Tantivy index for code symbols (BM25)
  nexus-mcp extension
    └── nexus_context, nexus_impact, nexus_detect_changes tools

Phase 4 — Advanced (opt-in)
  nexus-codegraph extension
    └── community detection (Leiden)
  nexus-storage extension
    └── Vector embeddings + RRF hybrid search
  nexus-ai / nexus-wiki
    └── LLM-powered documentation generation
```

---

## 7. Key Architectural Notes for Integration

1. **Tree-sitter in Nexus** — The `nexus-editor` crate may already link Tree-sitter grammars; `nexus-codegraph` should share grammar loading to avoid duplicate native libraries. Coordinate via `nexus-types` or a shared `nexus-tree-sitter` library crate.

2. **Storage isolation** — The code graph is a derived index (like the Tantivy index), not the source of truth. It must be fully rebuildable from source files. Store it in `.forge/code.db` separate from `.forge/index.db` to make this explicit and allow independent schema evolution.

3. **Forge scope** — GitNexus indexes any repository. Nexus's forge is the user's note-taking directory. The code graph should index code files within or adjacent to the forge (e.g., linked repositories), not be limited to markdown. The `nexus-codegraph` plugin could accept a target path distinct from `NEXUS_FORGE_PATH`.

4. **IPC handler count** — `nexus-git` has 36 handlers. `nexus-codegraph` Phase 1 needs ~8 (file_symbols, callers, callees, query_symbol, build_index, index_status, impact, detect_changes). This is well within the kernel's routing capacity.

5. **Dependency invariant** — `nexus-codegraph` must not depend on `nexus-git`. If `detect_changes` needs both diff data and symbol data, the CLI/TUI/MCP frontend makes two IPC calls and joins the results, or a new `nexus-analysis` crate depends on both service crates without either depending on the other.

6. **Staleness** — Mirror GitNexus's `lastCommit` pattern: store the HEAD SHA at index time in `code.db` metadata. Expose a `codegraph.staleness` IPC call that compares stored SHA to current HEAD (via `nexus-git`) and emits a warning event if the index is behind.
