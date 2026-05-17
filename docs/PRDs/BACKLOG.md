# Nexus Feature Backlog

> **Single source of truth for unfinished work.** This file is the index every other planning doc points to.
>
> - **Per-PRD status** lives in [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).
> - **Completed items** are archived verbatim in [backlog/](backlog/).
> - **Full descriptions of OI-\*** items live in [../roadmap/OPEN-ITEMS.md](../roadmap/OPEN-ITEMS.md); this file cross-lists by ID.
> - **Formal-release work** (auto-updater, telemetry, marketplace, beta→GA) is deferred to [../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md](../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md); the WI-IDs are indexed below for completeness.
> - **Doc gaps and product gaps surfaced by the 2026-05-12 traceability audit** live in [../roadmap/DOC-GAPS.md](../roadmap/DOC-GAPS.md) as DG-01..DG-46. Product-gap entries (DG-32..DG-46) are cross-listed in this file under "Doc-audit-surfaced product gaps". Doc-bug entries (DG-01..DG-31) live in DOC-GAPS only since they're documentation edits, not feature work.
> - **Research-surfaced ideas** from external-project assessments under [../research/](../research/) are indexed under "Research-surfaced ideas (2026-05, unscoped)".
> - **Exploratory / unscoped design docs** (AI directions, ambient copilot, memory layer, settings extraction inventory) are linked under "Future directions" — they do not have committed timelines.
>
> Section headings with no listed items are preserved as structural placeholders — consult the archive for what landed under each, and add new follow-ups directly below the heading.

---

## Backlog item index

Each BL-* item lives in its own file under [`backlog/`](backlog/). The table below tracks status; see the linked file for the full body, closure notes, and any follow-ups filed under the same ID.

| ID | Status | Date | Title |
|---|---|---|---|
| [BL-148](backlog/BL-148.md) | closed | 2026-05-16 | Launcher modal for `ssh://` URI entry (BL-140 follow-up) |
| [BL-147](backlog/BL-147.md) | closed | 2026-05-16 | Bootstrap storage helpers — `IpcInvoker` trait surface (BL-140 follow-up) |
| [BL-146](backlog/BL-146.md) | closed | 2026-05-16 | Subscription replay on remote-forge reconnect (BL-140 follow-up) |
| [BL-143](backlog/BL-143.md) | Phases 1.1+1.2+1.3 closed | 2026-05-16 | Live collaboration network transport (relay + client bridge + presence shipped; CLI / reconnect / shell UI remain open) |
| [BL-142](backlog/BL-142.md) | Phases 1+2a+2b closed | 2026-05-16 | REPL / in-buffer evaluation (Phase 2b CM6 wiring needs live-Tauri visual verification; Phase 3 settings polish optional) |
| [BL-141](backlog/BL-141.md) | Phases 1+2A+3 closed | 2026-05-16 | Multibuffer / multi-excerpt view (diagnostics-panel consumer + Approach B remain open) |
| [BL-140](backlog/BL-140.md) | closed | 2026-05-16 | SSH remote forge |
| [BL-145](backlog/BL-145.md) | closed | 2026-05-15 | `nexus-acp` server — inbound ACP surface for external clients (Hermes Feature 7) |
| [BL-144](backlog/BL-144.md) | closed | 2026-05-15 | `nexus-acp` host — outbound ACP adapter integration (BL-113 Phase 4) |
| [BL-139](backlog/BL-139.md) | closed | 2026-05-16 | Per-keystroke edit prediction |
| [BL-138](backlog/BL-138.md) | closed | 2026-05-15 | Default-deny capability registration |
| [BL-137](backlog/BL-137.md) | closed | 2026-05-14 | Architectural review (2026-05-14) follow-ups |
| [BL-136](backlog/BL-136.md) | closed | 2026-05-15 | Notification Center — persistent inbox + shell panel |
| [BL-135](backlog/BL-135.md) | closed | 2026-05-15 | Notification router refactor |
| [BL-134](backlog/BL-134.md) | closed | 2026-05-15 | `nexus-ai-runtime` — unified AI/agent event loop |
| [BL-133](backlog/BL-133.md) | closed | 2026-05-14 | Multi-channel notification output (Thoth port) |
| [BL-132](backlog/BL-132.md) | closed | 2026-05-14 | Runtime approval gates in the agent loop (Thoth port) |
| [BL-131](backlog/BL-131.md) | closed | 2026-05-14 | Pre-invocation message sanitisation in the agent loop (Thoth port) |
| [BL-130](backlog/BL-130.md) | closed | 2026-05-14 | Prompt injection detection (Thoth port) |
| [BL-129](backlog/BL-129.md) | closed | 2026-05-15 | Dream Cycle — knowledge refinement engine (Thoth port) |
| [BL-128](backlog/BL-128.md) | closed | 2026-05-14 | Personal entity knowledge graph (Thoth port) |
| [BL-127](backlog/BL-127.md) | closed | 2026-05-14 | Phase A |
| [BL-126](backlog/BL-126.md) | closed | 2026-05-14 | Drop redundant size-cap serialize + tighten session-mutex span (Phase 4 of TYPING-LATENCY-PLAN) |
| [BL-125](backlog/BL-125.md) | closed | 2026-05-14 | Viewport-scoped live-preview decorations (Phase 3 of TYPING-LATENCY-PLAN) |
| [BL-124](backlog/BL-124.md) | closed | 2026-05-14 | `useFrameSnapshot` adoption in `EditorView` (Phase 2 of TYPING-LATENCY-PLAN) |
| [BL-123](backlog/BL-123.md) | closed | 2026-05-14 | Slim `apply_transaction` response for text-only ops (Phase 1 of TYPING-LATENCY-PLAN) |
| [BL-122](backlog/BL-122.md) | closed | 2026-05-14 | Typing-latency measurement scaffold (Phase 0 of TYPING-LATENCY-PLAN) |
| [BL-121](backlog/BL-121.md) | closed | 2026-05-14 | Session-transcript FTS5 search (Hermes Feature 5) |
| [BL-120](backlog/BL-120.md) | closed | 2026-05-14 | Context compression in the agent session loop (Hermes Feature 4) |
| [BL-119](backlog/BL-119.md) | closed | 2026-05-14 | `SessionConfig` + iteration budget (Hermes Feature 1) |
| [BL-118](backlog/BL-118.md) | closed | 2026-05-14 | Web Speech API shell integration (Anything-LLM port) |
| [BL-117](backlog/BL-117.md) | closed | 2026-05-14 | `nexus-audio` STT + TTS crate (Anything-LLM port) |
| [BL-116](backlog/BL-116.md) | closed | 2026-05-14 | `ai.generate_docs` symbol-aware doc generator (GitNexus port) |
| [BL-115](backlog/BL-115.md) | closed | 2026-05-14 | MCP tools for code intel (GitNexus port) |
| [BL-114](backlog/BL-114.md) | closed | 2026-05-14 | Code-symbol index foundation (GitNexus port) |
| [BL-113](backlog/BL-113.md) | closed | 2026-05-15 | Protocol-host contribution model for LSP / DAP / MCP / ACP |
| [BL-112](backlog/BL-112.md) | closed | 2026-05-13 | Frontend perf benchmark harness — first cut |
| [BL-111](backlog/BL-111.md) | closed | 2026-05-13 | Defer Mermaid + heavy diagram libs until first use |
| [BL-110](backlog/BL-110.md) | closed | 2026-05-13 | Per-frame snapshot idiom for multi-store renders |
| [BL-109](backlog/BL-109.md) | closed | 2026-05-13 | Virtualize the files tree |
| [BL-108](backlog/BL-108.md) | closed | 2026-05-06 | Snippet row — mode and scope metadata in Snippets tab |
| [BL-107](backlog/BL-107.md) | closed | 2026-05-06 | Density tier CSS activation via `[data-density]` |
| [BL-106](backlog/BL-106.md) | closed | 2026-05-06 | Theme builder — light/dark side-by-side preview |
| [BL-105](backlog/BL-105.md) | closed | 2026-05-06 | Theme builder — WCAG contrast checker |
| [BL-104](backlog/BL-104.md) | closed | 2026-05-06 | Theme picker — swatch cache invalidation on hot-reload |
| [BL-103](backlog/BL-103.md) | closed | 2026-05-06 | Security fuzz targets |
| [BL-102](backlog/BL-102.md) | closed | 2026-05-06 | TLS pinning infrastructure for AI provider HTTP clients |
| [BL-101](backlog/BL-101.md) | closed | 2026-05-06 | `granted_caps.json` encryption at rest |
| [BL-100](backlog/BL-100.md) | closed | 2026-05-06 | Audit log CLI subcommands |
| [BL-099](backlog/BL-099.md) | closed | 2026-05-06 | Plugin manifest signing verification |
| [BL-098](backlog/BL-098.md) | closed | 2026-05-06 | `com.nexus.security` IPC handlers — credential vault via IPC |
| [BL-097](backlog/BL-097.md) | closed | 2026-05-06 | IPC schema versioning rollout |
| [BL-096](backlog/BL-096.md) | closed | 2026-05-06 | Capability revocation at runtime |
| [BL-095](backlog/BL-095.md) | closed | 2026-05-06 | Plugin lifecycle hook timeouts |
| [BL-094](backlog/BL-094.md) | closed | 2026-05-06 | Audit event persistence |
| [BL-093](backlog/BL-093.md) | closed | 2026-05-06 | Kernel metrics and observability exports |
| [BL-092](backlog/BL-092.md) | closed | 2026-05-06 | Kernel IPC latency benchmarks |
| [BL-091](backlog/BL-091.md) | closed | 2026-05-06 | Git-LFS read-path support and status surface |
| [BL-090](backlog/BL-090.md) | closed | 2026-05-06 | SSH passphrase caching via nexus-security |
| [BL-089](backlog/BL-089.md) | closed | 2026-05-06 | Git tags — create, list, delete, push |
| [BL-088](backlog/BL-088.md) | closed | 2026-05-06 | Non-interactive rebase + cherry-pick |
| [BL-087](backlog/BL-087.md) | closed | 2026-05-06 | Stash support |
| [BL-086](backlog/BL-086.md) | closed | 2026-05-06 | Auto-commit scheduler activation |
| [BL-085](backlog/BL-085.md) | closed | 2026-05-06 | Hunk-level staging |
| [BL-084](backlog/BL-084.md) | closed | 2026-05-06 | Shell git conflict panel |
| [BL-083](backlog/BL-083.md) | closed | 2026-05-06 | Forge-to-forge import and migration tool |
| [BL-082](backlog/BL-082.md) | closed | 2026-05-06 | Symlink handling in watcher and reconcile |
| [BL-081](backlog/BL-081.md) | closed | 2026-05-15 | DAP debugger integration |
| [BL-080](backlog/BL-080.md) | closed | 2026-05-06 | File tree / project explorer |
| [BL-079](backlog/BL-079.md) | closed | 2026-05-07 | Git gutter + diff viewer |
| [BL-078](backlog/BL-078.md) | closed | 2026-05-07 | Multi-file search and replace |
| [BL-077](backlog/BL-077.md) | closed | 2026-05-07 | CM6 LSP client extension |
| [BL-076](backlog/BL-076.md) | closed | 2026-05-07 | `nexus-lsp` — Language Server Protocol core plugin |
| [BL-075](backlog/BL-075.md) | closed | 2026-05-07 | Dual-mode editor — code files vs. document files |
| [BL-074](backlog/BL-074.md) | closed | 2026-05-14 | Collaborative editing — CRDT layer |
| [BL-073](backlog/BL-073.md) | closed | 2026-05-06 | Block auto-stamping on first reference |
| [BL-072](backlog/BL-072.md) | closed | 2026-05-06 | Undo history persistence across sessions |
| [BL-071](backlog/BL-071.md) | closed | 2026-05-06 | Emacs keybindings mode |
| [BL-070](backlog/BL-070.md) | closed | 2026-05-06 | Vim keybindings mode |
| [BL-069](backlog/BL-069.md) | closed | 2026-05-07 | Database query executor — Kanban / Calendar / Gallery layouts + type-aware cells |
| [BL-068](backlog/BL-068.md) | closed | 2026-05-14 | Theme Builder — visual token editor with live preview |
| [BL-067](backlog/BL-067.md) | closed | 2026-05-14 | Shell View Builder — visual layout composer for plugin panels |
| [BL-066](backlog/BL-066.md) | closed | 2026-05-06 | Terminal sidebar hover buttons |
| [BL-065](backlog/BL-065.md) | closed | 2026-05-07 | Windows pre-command support (cmd.exe / PowerShell) |
| [BL-064](backlog/BL-064.md) | closed | 2026-05-07 | Terminal AI suggestion LLM bridge |
| [BL-063](backlog/BL-063.md) | closed | 2026-05-07 | Terminal FTS5 scrollback index |
| [BL-062](backlog/BL-062.md) | closed | 2026-05-07 | Terminal session LRU eviction policy |
| [BL-061](backlog/BL-061.md) | closed | 2026-05-07 | Terminal memory backpressure — enforce kill policy |
| [BL-060](backlog/BL-060.md) | closed | 2026-05-07 | Ad-hoc command history — IPC exposure and shell UI |
| [BL-059](backlog/BL-059.md) | closed | 2026-05-06 | "Open in external terminal" escape hatch |
| [BL-058](backlog/BL-058.md) | closed | 2026-05-06 | Terminal URL chip extraction |
| [BL-057](backlog/BL-057.md) | closed | 2026-05-07 | Terminal activity timeline integration |
| [BL-056](backlog/BL-056.md) | closed | 2026-05-07 | Terminal workflow step type |
| [BL-055](backlog/BL-055.md) | closed | 2026-05-07 | Terminal commands in agent tool registry |
| [BL-054](backlog/BL-054.md) | closed | 2026-05-14 | Nexus OS Mode — Agentic OS methodology layer |
| [BL-053](backlog/BL-053.md) | closed | 2026-05-14 | Forge visual target — close the gap to the design mockup |
| [BL-052](backlog/BL-052.md) | closed | 2026-05-07 | Universal activity timeline |
| [BL-051](backlog/BL-051.md) | closed | 2026-04-30 | NB multi-cursor from multi-block selection |
| [BL-050](backlog/BL-050.md) | closed | 2026-04-30 | NB side-margin comments subsystem |
| [BL-049](backlog/BL-049.md) | closed | 2026-04-30 | NB block-links navigator (`[[…#^block-id]]`) |
| [BL-048](backlog/BL-048.md) | closed | 2026-04-30 | NB drag-to-embed into canvas |
| [BL-047](backlog/BL-047.md) | closed | 2026-04-29 | MEM scheduled digests |
| [BL-046](backlog/BL-046.md) | closed | 2026-04-30 | MEM code-aware capture |
| [BL-045](backlog/BL-045.md) | closed | 2026-04-29 | MEM auto-enrichment on save |
| [BL-044](backlog/BL-044.md) | closed | 2026-04-29 | MEM recall hotkey |
| [BL-043](backlog/BL-043.md) | closed | 2026-04-28 | MEM quick-capture hotkey |
| [BL-042](backlog/BL-042.md) | closed | 2026-04-30 | AI-DIR Nexus-as-MCP-server |
| [BL-041](backlog/BL-041.md) | closed | 2026-04-29 | AI-DIR background indexing daemon |
| [BL-040](backlog/BL-040.md) | closed | 2026-04-29 | AI-DIR semantic search |
| [BL-039](backlog/BL-039.md) | closed | 2026-04-29 | AI-DIR auto-link suggestions |
| [BL-038](backlog/BL-038.md) | closed | 2026-04-29 | AMB citations |
| [BL-037](backlog/BL-037.md) | closed | 2026-04-30 | AMB activity timeline |
| [BL-036](backlog/BL-036.md) | closed | 2026-04-30 | AMB margin suggestions + inline correction |
| [BL-035](backlog/BL-035.md) | closed | 2026-04-29 | Right-click AI actions + block AI actions (shared registry) |
| [BL-034](backlog/BL-034.md) | closed | 2026-04-28 | AMB ghost suggestions (CM6 inline-completion decoration) |
| [BL-033](backlog/BL-033.md) | closed | 2026-04-28 | AMB context chips + model switcher |
| [BL-032](backlog/BL-032.md) | closed | 2026-04-28 | Cmd+I command-anywhere AI overlay |
| [BL-031](backlog/BL-031.md) | closed | 2026-04-28 | Bases cell / row clipboard |
| [BL-030](backlog/BL-030.md) | closed | 2026-04-28 | Bases per-surface undo / redo |
| [BL-029](backlog/BL-029.md) | closed | 2026-04-30 | Multi-window / detachable panels |
| [BL-028](backlog/BL-028.md) | closed | 2026-04-29 | Workflow trigger expansion + control flow |
| [BL-027](backlog/BL-027.md) | closed | 2026-04-29 | Multi-agent orchestration / delegation |
| [BL-026](backlog/BL-026.md) | closed | 2026-04-28 | MCP Resource Enumeration |
| [BL-025](backlog/BL-025.md) | closed | 2026-04-28 | MCP authentication |
| [BL-024](backlog/BL-024.md) | closed | 2026-04-28 | MCP Reconnection + Connection Pool |
| [BL-023](backlog/BL-023.md) | closed | 2026-04-28 | MCP WebSocket + HTTP+SSE transports |
| [BL-022](backlog/BL-022.md) | closed | 2026-04-29 | Skill in-app editor UI |
| [BL-021](backlog/BL-021.md) | closed | 2026-04-28 | Skill `depends_on` composition resolver |
| [BL-020](backlog/BL-020.md) | closed | 2026-04-28 | Skill REGISTRY.json Persistence |
| [BL-019](backlog/BL-019.md) | closed | 2026-04-29 | AI local embeddings backend |
| [BL-018](backlog/BL-018.md) | closed | 2026-04-28 | AI Token Budget Enforcement |
| [BL-017](backlog/BL-017.md) | closed | 2026-04-28 | AI PII / Secret Egress Filter |
| [BL-016](backlog/BL-016.md) | closed | 2026-04-28 | AI tool registration for LLM function-calling |
| [BL-015](backlog/BL-015.md) | closed | 2026-04-28 | Soft-deleted bases — trash view UI |
| [BL-014](backlog/BL-014.md) | closed | 2026-04-28 | nexus db CLI Subcommand Group |
| [BL-013](backlog/BL-013.md) | closed | 2026-04-28 | Terminal event subscription over plugin IPC |
| [BL-012](backlog/BL-012.md) | closed | 2026-04-30 | Database query blocks in the editor (`[[{db:query}]]`) |
| [BL-011](backlog/BL-011.md) | closed | 2026-04-28 | `nexus ai complete` CLI |
| [BL-010](backlog/BL-010.md) | closed | 2026-04-28 | `nexus ai chat` interactive REPL |
| [BL-009](backlog/BL-009.md) | closed | 2026-04-28 | Whole-File `.mermaid` Viewer |
| [BL-008](backlog/BL-008.md) | closed | 2026-04-28 | Mermaid Diagram Plugin |
| [BL-007](backlog/BL-007.md) | closed | 2026-05-09 | CRDT-over-Git Transport |
| [BL-006](backlog/BL-006.md) | closed | 2026-04-18 | Block-Level Content Chunking for RAG |
| [BL-005](backlog/BL-005.md) | closed | 2026-04-18 | In-Memory Knowledge Graph (petgraph) |
| [BL-004](backlog/BL-004.md) | closed | 2026-04-18 | Obsidian-Style 3-Tier Link Resolution |
| [BL-003](backlog/BL-003.md) | closed | 2026-04-18 | Search Scoping Operators |
| [BL-002](backlog/BL-002.md) | closed | 2026-04-18 | Typed Property Index |
| [BL-001](backlog/BL-001.md) | closed | 2026-04-18 | Daily Notes |


## New Features (not addressed in any PRD)

---

### Follow-up: kernel-side caller-identity threading for register_adapter verb hardening (from BL-113)

**Source**: BL-113 capability-surface resolution (ADR 0027 §Open Question #3). Filed 2026-05-15.
**Effort**: Medium. Touches the kernel IPC dispatcher + every handler signature.
**Crates**: `nexus-kernel`, every service crate that registers a handler.

ADR 0027 resolves the capability question by treating contribution wiring as a declarative manifest pipeline (no verb-level gate). The trust model is **"plugins author manifests; bootstrap calls IPC."** Today there's no kernel-side enforcement preventing a plugin with `ipc.call` from invoking `com.nexus.dap::register_adapter` (or LSP / MCP equivalents) directly — bypassing the contribution pipeline. That can't escalate spawn privileges (those are checked at `launch` / `attach` / `connect`), but it does corrupt `contributed_by` provenance and skip marketplace install records.

Hardening: thread the calling plugin's id through `IpcDispatch::call(...)` into the handler. Handlers that want it (`register_adapter`, `register_server`) can then refuse the call when the caller isn't the trusted bootstrap context. Touches every handler signature in tree — defer until the corruption becomes a real problem or until another concern (audit logs, per-caller rate limits) wants the same plumbing.

---

### Follow-up: shell-side launch-config form renderer (from BL-113 first-party DAP plugin)

**Source**: BL-113 deferral. Filed 2026-05-15.
**Effort**: Medium. ~200–300 LOC across `shell/src/plugins/nexus/debugger/`.
**Crates**: shell (`nexus.debugger` plugin).

The first-party `first-party.dap.python` plugin (merged to `main` via PR #163) plumbs the contributed `launch_config_schema` to the shell via the host's opaque `metadata` field on `list_adapters`. What's still missing is the actual form-rendering UX:

1. A `<LaunchPicker>` dropdown in `DebuggerPanel.tsx` listing adapters from `list_adapters`, badged by `metadata.display_name`.
2. A `<LaunchConfigForm>` component that reads `metadata.launch_config_schema` (relative path), resolves it against the plugin directory, fetches via Tauri fs, and renders a typed form. Minimum coverage: top-level `type: object` with `string` / `boolean` / `array<string>` property kinds (debugpy's launch spec uses only these). Defaults from the schema seed initial values.
3. Submit calls `useDebuggerStore.startSession(api, { adapter, ...formValues })`.

Gating: the panel has no launch UI today, so this is a net-new feature track rather than a polish pass. The infrastructure to feed it (manifest contribution → host metadata → wire surface) is in place.

---

### Follow-up: per-session locks for concurrent multi-relpath mutation

**Source**: BL-126 deferral. Filed 2026-05-14.
**Effort**: Medium. ~22 handler signature changes.
**Crates**: `nexus-editor`.

BL-126's DoD asked for a "multi-relpath concurrency test [that] exercises two sessions being mutated simultaneously" — currently the `Mutex<HashMap<String, Session>>` map-level lock serialises mutations even when they target different files. To make concurrent edits actually overlap, switch the map to `Mutex<HashMap<String, Arc<Mutex<Session>>>>` so handlers acquire the outer lock briefly to clone the per-session Arc, drop the outer lock, then acquire the inner session lock. Snapshot-clone-outside-the-lock becomes feasible after that change (the inner lock guards a single session, so the tree clone can move outside the outer lock's scope, with the session-Arc clone replacing it). Tracing spans should then show two `apply_transaction` enters overlapping when the two sessions are independent.

---

### Follow-up: WDIO-Tauri runner (from BL-127)

**Source**: BL-127 deferral. Filed 2026-05-14. Original gate also blocks BL-112's runtime scenarios.
**Effort**: Medium — stand up a WebDriver-based runner that boots the Tauri shell, scripts pointer / keyboard events, and reads `performance.measure` entries back through the bridge.

BL-127 Phase A's editor-engine measurements capture the CM6 → StateField / ViewPlugin → DOM commit path on a happy-dom layout engine. What's still missing is the **runtime** end-to-end measurement that includes:

- Tauri IPC serialisation (kernel ↔ webview)
- Real GPU paint (happy-dom has no layout / paint pipeline)
- React commit through `EditorView`'s prop pipeline

`shell/e2e/` already has WDIO scaffolding for the E2E tests; the typing-perf runner would extend that surface with timing-aware scenarios. Once the runner exists, the existing `VITE_NEXUS_PERF_TYPING=1` hook drops the per-keystroke `performance.measure` entries into the buffer; the WDIO scenario reads them back and produces the same `MicrobenchResult` shape the BL-122 harness already writes. Targets stay: p95 keystroke → paint < 16 ms (small/medium), < 33 ms (large).

---

### Follow-up: shell subscriber for Dream Cycle relation proposals (from BL-129)

**Source**: BL-129 verification 2026-05-15.
**Effort**: Small. Shell-side plugin only; backend already publishes the event.
**Crates**: new `shell/src/plugins/nexus/dreamCycle/`; no Rust changes.

`com.nexus.ai::infer_entity_relations` already writes draft relations at `confidence: 0.5` and publishes `com.nexus.dream_cycle.proposals` on the kernel bus when any proposals land. The BL-129 DoD's "shell notification surfaces 'N new relation proposals from Dream Cycle' with an approve/skip action" needs a shell plugin that subscribes to that event and:

1. Counts the proposals in the payload, surfaces a toast through `api.notifications.show` (or routes through the BL-133 `nexus.notifications` plugin) with the `N new relation proposals` text.
2. Provides an inbox / panel listing the draft relations (entity pair, proposed kind, confidence) with per-row approve / skip actions.
3. Approve bumps `confidence` to a configurable confirmed value (default `1.0`) via `entity_upsert`; skip removes the draft via the same handler with the relation omitted from the payload.

Mirror the BL-133 shell-subscriber shape (`shell/src/plugins/nexus/notifications/index.ts`) — small plugin, default-on, registered in `shell/src/plugins/catalog.ts`.

---

### Follow-up: kernel-handler-level requires_approval flag (from BL-132)

**Source**: BL-132 deferral. Filed 2026-05-14.
**Effort**: Medium. Touches every plugin-handler registration.

DG-34 / BL-132 ship the flag at the agent-tool level (`AgentToolSpec.requires_approval`). The BL DoD originally proposed putting the flag on IPC handlers themselves so non-agent callers (CLI, MCP, shell) also see the prompt. That requires either threading the flag through `register_handler` everywhere or adding a registry side table — both touch every service plugin. Defer until a real non-agent caller surfaces a need.

---

### Follow-up: TUI modal for agent approval (from BL-132)

**Source**: BL-132 deferral. Filed 2026-05-14.
**Effort**: Small (after TUI agent surface lands).

`nexus-tui` has no `agent run` surface today, so the TUI half of BL-132's frontend triad is gated on the underlying agent UI landing first. When that surface ships, a `tui::dialog`-style modal subscribing to `com.nexus.agent.round_proposed` inherits the bus-event shape DG-34 already established.

---

### Follow-up: Tauri `notification` plugin integration (from BL-133)

**Source**: BL-133 deferral. Filed 2026-05-14. Updated 2026-05-14 after the in-app subscriber landed.
**Effort**: Small. Shell-side Tauri integration only.

The in-app toast subscriber landed; the next refinement is hooking the bus event into the OS-level notification plugin so notifications fire even when the Nexus window doesn't have focus. Add `tauri-plugin-notification` to `shell/src-tauri/Cargo.toml`, expose a `notify_desktop(title, message)` bridge command, and have `nexus.notifications` call it alongside (or instead of) `api.notifications.show`. The bus contract stays the same; only the renderer changes.

---

### Follow-up: background agent auto-notify (from BL-133)

**Source**: BL-133 deferral. Filed 2026-05-14. Updated 2026-05-14 after CLI auto-notify + workflow notify-step landed.
**Effort**: Medium. Needs a new bus event from `nexus-agent` + a subscriber in `nexus-bootstrap`.

CLI agent auto-notify (`nexus agent run --notify-after-secs`) covers human-driven sessions. Workflow `notify` step lets a workflow author surface specific moments. What's still missing is the **automatic background path**: a workflow- or schedule-triggered agent session that exceeds the threshold should notify without the author having to add an explicit `notify` step. Needs:

1. A new `com.nexus.agent.session_completed { session_id, duration_ms, outcome, ... }` event published from `handle_session_run` after the run finishes.
2. A `nexus-bootstrap` subscriber that watches the event and dispatches `notifications::send` when `duration_ms > threshold`. Threshold lives in `[agent] auto_notify_threshold_s` (default 30s).

The subscriber can be conditional — only fires when at least one notification channel is configured, so a forge with `[notifications]` absent doesn't pay for the subscription.

---

### Follow-up: shell settings panel for notifications (from BL-133)

**Source**: BL-133 deferral. Filed 2026-05-14.
**Effort**: Small. Shell-side UI.

A dedicated panel under Settings → Notifications would let users:
- Toggle each channel on/off.
- Enter the Discord webhook URL / Telegram credentials / SMTP server (stored via the existing `nexus-security` keyring IPC).
- "Send test" button per channel that dispatches a hello-world notification.

Today config lives in `.forge/config.toml` only; the panel would mediate the same keys behind a UI.

---

## Doc-audit-surfaced product gaps (2026-05-12)

Features spec'd in a PRD or ADR but missing from code, found by the 2026-05-12 doc-traceability audit. Full descriptions in [../roadmap/DOC-GAPS.md](../roadmap/DOC-GAPS.md) under DG-32 through DG-46. Doc-only bugs (DG-01..DG-31) live in DOC-GAPS but are not cross-listed here since they're not BL-shaped work.

- [x] **DG-32: PRD-15 §4 ToolRegistry** — closed 2026-05-12. Typed `AgentToolRegistry` + `Capability` enum + `AgentToolSpec` shipped in `crates/nexus-agent/src/tool_registry.rs`; seeded with the eight in-tree tools at bootstrap. New `com.nexus.agent::list_tools` (handler id 18) + `nexus tool list [--capability ID]…` CLI close the discoverability DoD. See [../roadmap/DOC-GAPS.md#dg-32](../roadmap/DOC-GAPS.md).
- [x] **DG-33: PRD-15 §5 Memory** — closed 2026-05-12. Filesystem-backed agent memory shipped at `crates/nexus-agent/src/memory.rs` with the eight `MemoryEntry` variants from the PRD, append-only `.forge/agents/<id>/history.jsonl`, decision-preserving prune, and four IPC handlers (`memory_record`, `memory_query`, `memory_prune`, `memory_export`). **Auto-recording follow-up shipped 2026-05-14**: `events_from_session(&session, now_ms)` + `serialize_entries_jsonl` pure helpers + `record_session_memory(ctx, &session)` fire from `handle_session_run` so every completed session appends its tool calls / errors / compactions to the agent's `history.jsonl`. **Prompt-time recall follow-up shipped 2026-05-14**: `format_memory_preamble(entries, decision_cap, recent_cap)` pure renderer + async `compose_memory_preamble(ctx, agent_id)` + `system_prompt_with_skills(ctx, goal, agent_id)` splice the most recall-worthy entries into the planner's system prompt at session start. Decisions are pinned (PRD-15 §5); recent non-decisions are capped independently so the budget stays bounded. Dated snapshots remain a deferred follow-up. See [../roadmap/DOC-GAPS.md#dg-33](../roadmap/DOC-GAPS.md).
- [x] **DG-34: PRD-15 §7 interactive approval round-trip** — closed 2026-05-12. Risk classification (`round_requires_approval`) layered on top of the existing ADR 0024 Phase 2b bus-bridge: `BusBridgePolicy` now auto-approves rounds where every proposed tool is registered with `requires_approval=false`, prompts otherwise. `round_proposed` payload carries `requires_approval` + `registered` per tool call so the shell can render per-call risk badges. New `strict_approval` arg restores the prompt-every-round behaviour. See [../roadmap/DOC-GAPS.md#dg-34](../roadmap/DOC-GAPS.md).
- [x] **DG-35: PRD-15 §8 six built-in agent classes** — closed 2026-05-12. `auditor` (read-heavy reviewer), `librarian` (knowledge organisation), and `coach` (guidance-over-execution) archetypes added in `crates/nexus-agent/src/archetypes.rs` with stable ids `com.nexus.agent.{auditor,librarian,coach}`. `list_archetypes` IPC now returns all six. See [../roadmap/DOC-GAPS.md#dg-35](../roadmap/DOC-GAPS.md).
- [x] **DG-36: PRD-15 §9 `.agent.toml`** — closed 2026-05-12. Parser + loader + scanner shipped in `crates/nexus-agent/src/custom_agent.rs`; new `com.nexus.agent::list_custom` IPC handler (id 19) + `nexus agent list-custom` CLI surface the manifests. **Routing follow-up shipped 2026-05-14**: `nexus agent plan/run --archetype <slug>` now accepts custom-manifest slugs — new `resolve_archetype_for_run(ctx, name)` reads `<forge>/.forge/agents/<slug>/agent.toml` and layers `[system_prompt]` over the manifest's `[agent].archetype` baseline; custom agents run with a `com.nexus.agent.custom.<slug>` id. **Tool allow/deny enforcement shipped 2026-05-14**: new `ManifestToolPolicy` + `ManifestPolicyGate<P>` decorator over `SessionPolicy` — manifest denials surface via `RoundDecision::Partial` so the session loop's existing failure path (denied call → `is_error: true` tool-result turn → model recovers) handles them. DG-36 is fully closed. See [../roadmap/DOC-GAPS.md#dg-36](../roadmap/DOC-GAPS.md).
- [x] **DG-37: PRD-15 §10 agent-to-agent comms** — closed 2026-05-12. New `com.nexus.agent::delegate` (handler id 24) runs a sub-session via the existing session machinery; new `delegate_to_agent` registered in the agent tool registry so a planner LLM sees A2A as a first-class tool call. Parallel / pipeline remain caller composition patterns (no resurrected orchestrator). See [../roadmap/DOC-GAPS.md#dg-37](../roadmap/DOC-GAPS.md).
- [x] **DG-38: PRD-17 cross-platform reframe-or-build** — closed 2026-05-12 with **Option A** (reframe as desktop-only). PRD-17 retitled "Desktop Strategy"; per-section "Deferred (DG-38)" callouts at §3/§5/§6/§15/§16 and a partial-deferral note at §4; `00-index.md` + `IMPLEMENTATION_STATUS.md` reflect the new scope. Web/mobile content preserved as design rationale; future multi-platform work would split into per-platform BL entries. See [../roadmap/DOC-GAPS.md#dg-38](../roadmap/DOC-GAPS.md).
- [x] **DG-39: PRD-14 §10 dynamic MCP tool registration** — closed 2026-05-12. New `crates/nexus-mcp/src/dynamic_tools.rs` registry plus `register_tool` / `unregister_tool` / `list_dynamic_tools` IPC handlers on `com.nexus.mcp.host`; `NexusMcpServer::list_tools` + `call_tool` merge static + dynamic surfaces. `notifications/tools/list_changed` broadcast + dedicated capability deferred as documented follow-ups. See [../roadmap/DOC-GAPS.md#dg-39](../roadmap/DOC-GAPS.md).
- [x] **DG-40: PRD-14 §12.2 MCP audit logging** — closed 2026-05-12 via `nexus_kernel::audit::log_mcp_tool_call` + `log_mcp_resource_read` wired through `crates/nexus-mcp/src/server.rs`. See [../roadmap/DOC-GAPS.md#dg-40](../roadmap/DOC-GAPS.md).
- [x] **DG-41: PRD-10 §7 relations + rollup** — closed 2026-05-12. New `crates/nexus-database/src/relations.rs` ships `resolve_relation` + `compute_rollup` with the existing `RollupAggregation` enum (Count / CountUnique / Sum / Average / Min / Max / Percent*). Two new IPC handlers (ids 5 + 6) on `com.nexus.database`. See [../roadmap/DOC-GAPS.md#dg-41](../roadmap/DOC-GAPS.md).
- [x] **DG-42: PRD-10 §8 SQL compilation** — closed 2026-05-12. The SQL compiler already shipped in `crates/nexus-storage/src/bases/query.rs` (16 FilterOps, sorts, pagination, parameter binding) and is exposed via `com.nexus.storage::base_query` (handler id 26). The 2026-05-12 audit conflated it with `apply_view`'s in-memory pipeline — both ship and target different use cases. See [../roadmap/DOC-GAPS.md#dg-42](../roadmap/DOC-GAPS.md).
- [x] **DG-43: PRD-06 §9 frontmatter versioning + migration** — closed 2026-05-12. `Frontmatter.version` typed field + new `nexus-formats::migration` module (FormatVersion parser, MigrationRegistry, scan_versions walker) + `nexus migrate scan|registered` CLI. Registry is empty in this build — no breaking change has shipped yet — but the infrastructure is in place. See [../roadmap/DOC-GAPS.md#dg-43](../roadmap/DOC-GAPS.md).
- [x] **DG-44: PRD-04 §10 dynamic .so/.dll loading** — rejected 2026-05-12; PRD-04 §10 gained a "Superseded by ADR 0011 + ADR 0016" callout.
- [x] **DG-45: ADR 0013 macOS menu-bar plugin** — re-phased 2026-05-12 to formal-release WI-45 alongside Mac packaging/notarization (WI-41). ADR 0013 addendum documents the slip; decision stands. See [../roadmap/DOC-GAPS.md#dg-45](../roadmap/DOC-GAPS.md).
- [ ] **DG-46: ADR 0006 first in-tree consumer** — convention-only ADR; no in-tree user yet because community WASM plugins haven't shipped.

---

## Post-migration carryover gaps (2026-04-24)

Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement. The single still-open OI lives in [../roadmap/OPEN-ITEMS.md](../roadmap/OPEN-ITEMS.md); the 21 resolved-OI audit trail was archived 2026-05-12 to [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md). BL-shaped follow-ups land in [backlog/](backlog/).

### Open

- [ ] **OI-05: Rust dep duplication** — Blocked on upstream. 34 crates with duplicated versions all trace through `wasmtime 42` (toml/sha2/digest/rand_core/reqwest/rustix/nix/hashbrown) or `portable-pty → filedescriptor` (`thiserror 1`). Revisit after the next wasmtime major release.

### Resolved (preserved here for cross-reference; full notes in [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md))

- [x] OI-01 — Settings modal + `registerSettingsTab` API _(2026-04-24)_
- [x] OI-02 — Split-size persistence (editor splits gained drag handles + `setSplitSizes` mutator) _(2026-04-24)_
- [x] OI-03 — Workspace-wide clippy `-D warnings` sweep _(2026-04-24)_
- [x] OI-04 — Kernel-contract promotion TODOs (`SlotId` and `list_archetypes` IPC) _(2026-04-24)_
- [x] OI-06 — ESLint 8 → 9 + typescript-eslint 7 → 8 + xterm → `@xterm/*` scoped _(2026-04-24)_
- [x] OI-07 — Capability grants/denials/path-traversal routed through `audit::*` _(2026-04-24)_
- [x] OI-08 — "Running Extensions" Settings tab (live plugin state + errors + Disable) _(2026-04-26)_
- [x] OI-09 — `pluginsStatusStore` aggregates plugin lifecycle events into a per-plugin `{ state, lastError }` map _(2026-04-26)_
- [x] OI-10 — `KeybindingRegistry.getConflicts()` + `plugins:keybindings-conflict` event with signature-dedup; per-row `!` badge + summary banner in Settings → Keybindings _(2026-04-27)_
- [x] OI-11 — `CommandRegistry.execute` races handlers against a configurable cancel deadline (`shell.command.timeoutCancelMs`, default 5s) with a soft warn at `shell.command.timeoutWarnMs` (default 250ms); emits `command:cancelled` and throws `CommandCancelledError` so the palette can dismiss in-flight state _(2026-04-27)_
- [x] OI-12 — Auto-promotion was already gone on the kernel side; this pass tightened the `confine_path` / `read_file` doc comments to spell out the contract, documented the script-plugin `PlatformFsAPI` path-semantics in `@nexus/extension-api`, and added two kernel tests that pin the loud `PermissionDenied` + traversal-message AC for absolute reads / writes _(2026-04-27)_
- [x] OI-13 — Deleted dead `nexus_kernel::PluginRegistry` + `Kernel::plugins()` (zero callers; `PluginLoader::loaded` is authoritative) _(2026-04-26)_
- [x] OI-16 — `ExtensionHost.deactivateAllForShutdown(perPluginCapMs)` runs every active plugin's `deactivate()` in parallel with a per-plugin soft cap; wired from a `beforeunload` listener in `main.tsx` so flush-on-stop hooks get one last shot before the WebView tears down _(2026-04-27)_
- [x] OI-17 — Deprecation policy lands as a three-way handshake — `@deprecated` JSDoc on the symbol + an entry in `packages/nexus-extension-api/DEPRECATED.md` + an `importNames` row in `shell/eslint.config.js`'s `no-restricted-imports` block. CI gate works without enabling type-aware lint (kept defer-decision intact); empty list today, table headers + protocol ready for the first deprecation _(2026-04-27)_
- [x] OI-20 — Terminal copy/paste — `attachCustomKeyEventHandler` claims `Ctrl+Shift+C/V` (Linux/Windows) and `Cmd+C/V` (macOS) without disturbing plain `Ctrl+C` SIGINT, right-click pastes from clipboard, paste honours bracketed-paste mode (`\e[200~ … \e[201~`) when xterm signals it. Uses `navigator.clipboard.{read,write}Text` from user-gesture handlers; denial logs a follow-up note pointing at `@tauri-apps/plugin-clipboard-manager` _(2026-04-27)_
- [x] OI-14 — `api.workspace.forgeRoot()` + `api.editor.active()/onChange()` exposed via `@nexus/extension-api` _(2026-04-26)_
- [x] OI-19 — Deferred createRoot/unmount in `TerminalPaneView` + `EmptyView`; React 18 commit-phase warnings on drawer collapse + StrictMode double-mount cleared _(2026-04-27)_
- [x] OI-22 — `com.nexus.git` passive-mode crash: `HANDLER_STATUS` now returns JSON null instead of `Err(ExecutionFailed)` so the IPC layer no longer wraps it as `PluginCrashedDuringCall`; shell handles null silently _(2026-05-01)_
- [x] OI-15 — Manifest signature / provenance — `ed25519-dalek` verification of `plugin.json.sig` against a trusted public-key list; `VerificationStatus` enum + `verify_plugin_signature` in `shell/src-tauri/src/lib.rs`; untrusted/invalid plugins filtered at scan time; "verified" / "unsigned" pill in Settings → Plugins. `TRUSTED_PUBLIC_KEYS` is empty pre-marketplace; populate when the marketplace CA exists _(2026-05-01)_
- [x] OI-18 — Snippet trigger collision detection — `SnippetRegistry` with `getConflicts()`, `plugins:snippets-conflict` event, Settings → Snippets tab with conflict banner + per-row badge; `editor.registerSnippet` API + `contributes.snippets` manifest path landed alongside _(2026-05-01)_

---

## Formal release scope (deferred)

Tracked in full in [../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md](../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md). Out of scope for personal-tool use; surface here so the IDs are findable.

- [ ] **WI-41: Tauri auto-updater + code-signing + release channel.** ~5–7 eng-days plus 1–3 weeks calendar for signing-cert procurement.
- [ ] **WI-42: Crash reporting & telemetry.** ~5 eng-days, opt-in via Settings.
- [ ] **WI-44: Minimal marketplace.** ~5 eng-days; index schema + shell UI + CLI install + tarball publishing. Paired with **OI-15** (manifest signing) and **F-8.1.1 / F-8.1.2** (iframe sandbox + boundary-bound `pluginId`) before opening to untrusted plugins.
- [ ] **WI-46: Beta → GA logistics.** Triage rubric, test-group recruitment, ship criteria. ~3 eng-days plus 2-week calendar.

---

## Future directions (scoped 2026-04-28)

Previously: design-only docs without committed timelines. **Scoped into the implementation plan on 2026-04-28** — each FD piece now has a BL-* ID (see "Future-direction items minted into the backlog" above) and the docs themselves remain authoritative for design rationale.

- **AI integration directions** — see [../roadmap/AI-INTEGRATION-DIRECTIONS.md](../roadmap/AI-INTEGRATION-DIRECTIONS.md). Mapping: "inline rewrite/summarize" → BL-034 (engine) + BL-035 (action surface); "auto-link suggestions" → BL-039; "semantic search" → BL-040; "per-surface chat" → merged into BL-010 (reshape note); "skills as prompts" → composed via BL-021 / BL-022; "agent loops" → merged into BL-027 (same surface); "MCP exposure" (Nexus-as-server) → BL-042; "background indexing" → BL-041. Direction "tool-calling" was already BL-016.
- **Ambient copilot UX patterns** — see [../roadmap/AI-AMBIENT-COPILOT-PLAN.md](../roadmap/AI-AMBIENT-COPILOT-PLAN.md). Mapping: Cmd+I overlay → BL-032; context chips + model switcher → BL-033; ghost suggestions → BL-034; right-click AI actions → BL-035 (shared with NB block AI actions); margin suggestions + inline correction → BL-036; activity timeline → BL-037; citations → BL-038; capture → AI → folded into BL-043 (memory quick-capture).
- **AI memory layer** — see [../roadmap/AI-MEMORY-LAYER-PLAN.md](../roadmap/AI-MEMORY-LAYER-PLAN.md). Mapping: quick-capture → BL-043; auto-enrichment on save → BL-045; recall hotkey → BL-044; implicit chat context → merged into BL-010 (reshape note); code-aware capture → BL-046; scheduled digests → BL-047.
- **Notion-style block UX out-of-scope follow-ups** — see [../archive/notion-block-ux-plan.md](../archive/notion-block-ux-plan.md) (archived 2026-05-12 — all 6 phases shipped). Mapping: drag-to-embed into canvas → BL-048; block-links navigator → BL-049 (gated on block-id stability ADR); side-margin comments → BL-050; block AI actions → merged into BL-035; multi-cursor from multi-block → BL-051.

---

## Research-surfaced ideas (2026-05)

Four external-project assessments under [`../research/`](../research/) each carry an explicit Adopt / Adapt / Skip column. Items that have been promoted into the backlog carry their BL-NN cross-ref; items left here are held discoverable until they're picked up. Skip items stay in the research doc only.

- **GitNexus capability porting** — see [../research/gitnexus-capability-assessment.md](../research/gitnexus-capability-assessment.md). Seven scoped ports recommended; three promoted as a coordinated track (2026-05-13):
    - Cross-repo code intel index → **[BL-114](backlog/BL-114.md)**
    - Three new MCP tools (`nexus_context` / `nexus_impact` / `nexus_detect_changes`) → **[BL-115](backlog/BL-115.md)**
    - `ai.generate_docs(symbol_id)` doc generator → **[BL-116](backlog/BL-116.md)**
  Remaining four (diff→symbol detection, `codegraph.impact(depth)` handler, BM25 over code symbols, optional clustering pass) are downstream of BL-114 and stay held here until BL-114 lands.
- **AFFiNE portability** — see [../research/affine-portability-assessment.md](../research/affine-portability-assessment.md). 9 Adopt items (snapshot API, command pattern, linked-docs UX, database views, mind-map output, multimodal embed pipeline, custom block schemas pattern, inline-embed extensions pattern, widget extensions — last two already adopted in spirit) plus 8 Adapt items (canvas, PDF embed, slide gen, image gen, custom blocks, AI panel UX, inline primitive, doc-on-canvas). 11 items judged already-adopted-in-spirit. **Held as reference material — not promoted to BL-NN entries.** The 2026-05-13 review judged the Adopt list as diffuse pattern-borrowing without clean PR shape, and most universally-compelling Adapt items (canvas, etc.) overlap shipped work (BL-049 / BL-051 / BL-067 / BL-068). Mine specific items when a feature need surfaces. See §6 of the assessment.
- **Anything-LLM portability** — see [../research/anything-llm-assessment.md](../research/anything-llm-assessment.md). Two Adapt items promoted (2026-05-13):
    - `nexus-audio` crate with Whisper STT + TTS provider-trait backends → **[BL-117](backlog/BL-117.md)**
    - Web Speech API native STT/TTS in the Tauri webview → **[BL-118](backlog/BL-118.md)**
  Per-user scoped API token issuance held here — conflicts with the personal-tool / single-user stance. Skip items (multi-user, PostHog telemetry, cloud deploys, scheduled-jobs engine, "desktop app") are settled as out-of-scope per existing PRDs/ADRs.
- **Hermes Agent native-Rust port** — see [../research/hermes-agent-implementation-plan.md](../research/hermes-agent-implementation-plan.md). Seven features with merge order; three promoted (2026-05-13), three already shipped, one covered by another BL:
    - Feature 1 — `SessionConfig` + iteration budget (S, merge-first) → **[BL-119](backlog/BL-119.md)**
    - Feature 2 — memory persistence on `com.nexus.agent` → **already shipped as DG-33** (see [backlog/](backlog/))
    - Feature 3 — skills system + RAG retrieval at session start → **already shipped per PRD-13 + DG-32**
    - Feature 4 — context compression in the session loop (L) → **[BL-120](backlog/BL-120.md)**
    - Feature 5 — session-transcript FTS5 search (M) → **[BL-121](backlog/BL-121.md)**
    - Feature 6 — multi-agent delegation → **already shipped as DG-37**
    - Feature 7 — ACP protocol adapter crate → **covered by BL-113 Phase 4** (shipped — see [backlog/](backlog/))
- **Thoth capability assessment** — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Thoth is a local-first Python AI assistant (NiceGUI + LangGraph + Ollama + FAISS + NetworkX) with broad tool coverage and a personal entity knowledge graph. Six high-priority ports promoted (2026-05-14); eight medium-priority items held here. Voice (STT/TTS) already covered by BL-117 / BL-118 from the Anything-LLM assessment; context compression overlap noted with BL-120 but BL-125 targets the cheaper pre-invocation sanitisation pass rather than LLM-based summarisation.
    - Typed personal entity graph (people/places/events/concepts, 40+ relation types, FAISS recall) → **[BL-128](backlog/BL-128.md)**
    - Dream Cycle refinement engine (nightly dedup, decay, enrich, infer) → **BL-129** (shipped — see [backlog/](backlog/))
    - Prompt injection detection (role-override patterns, invisible Unicode, HTML directives) → **[BL-130](backlog/BL-130.md)**
    - Pre-invocation message sanitisation (dedup tool results, strip base64 URIs, 85% trim) → **[BL-131](backlog/BL-131.md)**
    - Runtime approval gates in the agent loop (pause-and-ask before destructive steps) → **[BL-132](backlog/BL-132.md)**
    - Multi-channel notification output (OS, Telegram, Discord, SMTP) → **[BL-133](backlog/BL-133.md)**
  Medium-priority items held here until capacity opens: vision (`nexus-vision` plugin — screen/file image analysis via local Ollama vision model); built-in web search (Tavily + DuckDuckGo handlers in `nexus-ai`); browser automation (Playwright via `nexus-browser` plugin); chart generation (10 Plotly-equivalent chart types in `nexus-formats` or `nexus-ai`); health/habit tracker (`nexus-tracker` plugin or Bases extension); Docker sandbox for terminal sessions (shadow workspace + patch-apply in `nexus-terminal`); custom tool builder UX (wizard shell plugin wrapping `nexus plugin scaffold`). Skip items (Gmail, Calendar, image/video gen, X/Twitter, Arxiv/Wolfram/Weather — all reachable via MCP): these are community plugin territory.
- **Zed capability assessment** — see [../research/zed-capability-assessment.md](../research/zed-capability-assessment.md). Zed is a GPU-accelerated multiplayer code editor with strong AI-native features, a WASM extension system, and a DAP debugger. Nexus's terminal, git, and AI subsystems are richer; Zed leads on real-time networked collaboration, per-keystroke edit prediction, remote development, and a few editor-paradigm primitives (multibuffers, REPL). Assessment filed 2026-05-14; five P1/P2 capabilities promoted:
    - Per-keystroke ghost-text edit prediction (continuous CM6 completion loop, Ollama-compatible) → **[BL-139](backlog/BL-139.md)**
    - Live collaboration network transport — Zed-style WebSocket/WebRTC peer sync beyond the current in-process ops bus → **[BL-143](backlog/BL-143.md)**
    - SSH remote forge (headless Nexus server; local shell, remote `NEXUS_FORGE_PATH`) → **[BL-140](backlog/BL-140.md)**
    - Multibuffer / multi-excerpt view (editable excerpts from ≥2 files in one pane) → **[BL-141](backlog/BL-141.md)**
    - REPL / in-buffer evaluation (Jupyter-style cell execution via `nexus-terminal`) → **[BL-142](backlog/BL-142.md)**
  Vim mode (**BL-070** — shipped 2026-05-06), DAP debugger (**BL-081** — shipped 2026-05-15 via PR #163), and ACP external-agent hosting (**BL-113** Phase 4 — shipped as BL-144 + BL-145 on 2026-05-15; see [backlog/](backlog/)) are already tracked. P3 items held here until capacity opens: lightweight `tasks.toml` shorthand in `nexus-workflow` for single-command tasks; central graphical settings editor shell plugin; multi-line regex verification in `nexus-storage` find/replace. Skip items: GPUI rendering (Tauri/WebView is sufficient for the notes use case), 800-language extension ecosystem (markdown-first scope).

---

## Settings extraction queue

Inventory of named-constant / hardcoded settings candidates lives in [../../shell/HARDCODED_SETTINGS_AUDIT.md](../../shell/HARDCODED_SETTINGS_AUDIT.md). Pickable in any order; each is a 1–2 hour change.

- [x] **Zoom settings schema** _(shipped)_ — `ui.zoomStep` / `ui.zoomMin` / `ui.zoomMax` / `ui.zoomDefault` registered in `shell/src/plugins/core/zoom/index.ts` with bounds, step, and reset target read through `api.configuration.getValue` + `onChange`.
- [x] **Notification durations schema** _(shipped)_ — `ui.notificationDurationMs` (notificationService), `ui.fileCreationNotificationMs` (fileExplorer), `ui.commandSaveNotificationMs` + `ui.commandCopiedNotificationMs` (terminal `index.ts` schema; SavedCommandsView reads via `useConfigValue`), `ui.copiedNotificationMs` (`nexus.ai`'s `index.ts`; ChatView reads via `useConfigValue`).
- [x] **Search / palette result limits** _(shipped)_ — `search.maxResultsLimit` (schema in `shell/src/plugins/nexus/search/index.ts`, read in `searchRuntime.ts`); `commandPalette.maxResultsLimit` (schema in `shell/src/plugins/core/commandPalette/index.ts`, read by `match.ts`).
- [x] **Long-running operation timeout consolidation** _(shipped)_ — `LONG_RUNNING_OP_TIMEOUT_MS` defined once in `shell/src/plugins/nexus/constants.ts` and consumed by `nexus/agent/index.ts` (`RUN_TIMEOUT_MS`) and `nexus/workflow/index.ts` (`RUN_TIMEOUT_MS`); `SERVICE_CONNECT_TIMEOUT_MS` similarly consumed by `nexus/mcp/index.ts`.
- [x] **Buffer / event caps** _(shipped)_ — `PROCESS_EVENTS_CAP` named in `processesStore.ts`; `UNDO_HISTORY_CAP` lives in `shell/src/plugins/nexus/constants.ts` and is shared by `bases/basesStore.ts` + `canvas/canvasStore.ts` so the user-perceptible undo depth is consistent across surfaces.

---

## UI architecture review (2026-04-16) — editor-shell pattern

### Code gaps

### PRD gap — no owner for plugin-contributed tab surfaces

## Editor-shell capability gaps (2026-04-16) — vs VS Code / Obsidian / IntelliJ

### Spec'd in a PRD, not yet implemented

### Half-specced: manifest keys exist, but no UI/wiring spec in PRD-07

### Not in any PRD — new spec work needed

## Architecture audit (2026-04-16) — follow-ups

Findings surfaced by the microkernel + editor-shell audit that weren't already tracked above.

## Microkernel hardening — 2026-04-16 audit findings

Findings from `docs/archive/planning/MICROKERNEL-AUDIT.md` not yet tracked. Ordered by audit priority. The three 🔴 items and F-9.2.1 are blockers before any public plugin marketplace.

### 🔴 Red — blockers for untrusted plugin distribution

_None outstanding._ F-2.1.1 closed 2026-04-22 — see archive.

### 🟠 Orange — address before marketplace or next minor release

### 🟡 Yellow — quality / correctness improvements

## Suspected issues — not fully investigated

Threads from `docs/archive/planning/MICROKERNEL-AUDIT.md §Suspected Issues` that warrant a targeted code walk.

- [ ] **Hot-reload timing on macOS and Windows.** `notify-debouncer-mini` behaviour differs across platforms; F-4.3.1 covers one class of issue. A targeted cross-platform reliability pass on the hot-reload path would be worthwhile before shipping community plugin hot-reload as a feature. **Deferred** — requires running the shell on macOS and Windows hardware to reproduce and measure; this repo's test host is Linux/WSL only. Track for a dedicated cross-platform QA pass once a macOS/Windows CI runner or test machine is available.
_Three 2026-05-13 spikes (catalog-plugin Enable / src-colocated test gap / srcdoc sandbox boot errors) closed 2026-05-13 — see [backlog/](backlog/). Suite went from 1260 → 1331 (+71 previously-uncounted tests + 1 documented skip on a stale workspace-Leaf spec)._

## UI audit (2026-04-16) — follow-ups

Findings from `docs/archive/planning/UI-AUDIT.md` not yet tracked above. IDs reference the audit. The 🔴 items plus F-9.1.1 are blockers before any untrusted-plugin distribution.

### 🔴 Red — cannot ship to untrusted users without these

_F-8.1.1 (sub-tasks 1–5: iframe scaffold + sandbox flags, postMessage protocol, `NexusPluginContext` proxy, per-plugin manifest `sandboxed` flag, CSP + tests), **F-8.1.1-fo1** (precompiled `bootstrapSandboxedPlugin` runtime bundle + hello-world migration), and **F-8.1.2** (boundary-bound `pluginId` — orchestrator builds a per-plugin `PluginAPI` from the handshake-set id; `assertValidPluginId` rejects empty / colon-bearing ids) shipped 2026-04-28 — see [backlog/](backlog/). All red-tier UI items now closed; remaining gating for community marketplace launch is **WI-44** (marketplace UI / index / signing) and **OI-15** (manifest signing) at the orange tier._

> F-9.1.1 (validate `api_version` at load time) is the UI twin of the microkernel 🟠 item of the same ID already tracked above — no duplicate entry.

### 🟠 Orange — substantive design gaps, schedule before next external release

- [ ] **Memory budget / accounting for script plugins (UI F-8.3.1).** WASM plugins have `memory_mb = 8` in their manifest; script plugins have no equivalent and allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure OOMs the whole shell. **Now unblocked** — F-8.1.1 shipped 2026-04-28 (per-plugin iframe boundary in `shell/src/host/sandbox/SandboxOrchestrator.ts`). `performance.measureUserAgentSpecificMemory()` is per-frame, so the orchestrator can poll each iframe and attribute usage by `data-sandbox-plugin`. Today still unimplemented; a misbehaving script plugin's RSS is indistinguishable from the shell's. Track as a sandboxed-plugin watchdog enhancement.

### 🟡 Yellow — rough edges to fix opportunistically

### Suspected issues — UI audit §6 spike candidates

Threads from `docs/archive/planning/UI-AUDIT.md §6` not yet confirmed. Each is a 1–2 day targeted code walk or runtime probe.

- [x] **SI-1 — Blob-URL same-origin inheritance.** **Closed 2026-04-28** as a duplicate of F-8.1.1. The blob-URL same-origin inheritance behaviour is confirmed (MDN spec — a `blob:` URL inherits the origin of its creator), but it no longer matters for sandboxed plugins: `manifest.sandboxed === true` routes the plugin through `SandboxOrchestrator`, which mounts a null-origin iframe (`sandbox="allow-scripts"`, no `allow-same-origin`). Inside that iframe the host's blob URL is reachable for the bundle import but the iframe runs at `event.origin === "null"` so it can't read `window.parent.document` / `document.cookie` / Tauri's IPC bridge. Legacy non-sandboxed plugins still inherit the shell's origin — that's the "first-party only" trust posture documented in `DEPRECATED.md`.
- [ ] **SI-6 — `PluginManager` Mutex contention.** **Deferred — requires a dedicated load-test harness that doesn't exist yet.** Measuring requires 20+ chatty plugins and wall-clock profiling while a human drives the UI, which this environment cannot replicate. Hypothesis: per-plugin dispatch already uses `try_lock` + reentrancy guard + per-plugin backend mutex, so the `PluginManager` top-level mutex is only held during scan/load/unload/reload — not during steady-state dispatch. If the hypothesis holds this is cosmetic; if not, the fix is likely `RwLock<HashMap<id, …>>` inside the loader with per-plugin reader locks. Track as an explicit Phase-3 stability task once the load-test tooling exists.

## Audit findings (2026-04-28)

> Cross-PRD docs audit ([DOCS_AUDIT_2026-04-28.md](../audits/DOCS_AUDIT_2026-04-28.md)) — items spec'd in a PRD that are not yet built and were not previously assigned a backlog ID. Each cites the PRD section, target crate, and estimated effort. Effort scale: small ≈ ½–2 days, medium ≈ 3–10 days, large ≈ 2+ weeks.

---

## Future-direction items minted into the backlog (2026-04-28)

> The four future-direction tracks were brought into the implementation plan on 2026-04-28. The IDs below carry their FD doc as design rationale; the original entries in the "Future directions" section now point here. Effort scale: S ≈ ½–2 days, M ≈ 3–10 days, L ≈ 2+ weeks.

### Verification notes (no BL ID — informational)

- **ADR-0009 keyring hard-fail enforcement** — Verified 2026-04-30 and resolved as **OI-21** the same day: `SecurityCorePlugin::on_init` now runs an injected `KeyringProbe` (default `CredentialVault::new().available()`) and returns `PluginError::LifecycleError` with the platform hint when the OS keyring is unavailable. Bootstrap propagates the lifecycle error so frontends exit non-zero. See [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md) §OI-21.
- **PRD-04a MockPluginContext / MockEventBus** — referenced in template tests as TODO but not yet exposed from `nexus-plugin-api`. Low priority; community plugin authors are not yet writing many tests, and the issue surfaces only when someone tries.

## Design notes — 2026-04-28

- **Global cross-surface undo is a non-goal.** Considered alongside BL-030. Per-surface undo is the idiom in VS Code / Obsidian / IntelliJ; a unified Cmd+Z spanning editor + canvas + bases + file ops creates ambiguous "what does this undo right now" behaviour and would require every mutating IPC handler to register an inverse op against the file-as-truth + IPC-only invariants. The right primitive for cross-surface time-travel in this architecture is git-based history (point-in-time restore via the existing commit graph) rather than a unified action stack. New BL items for undo should be scoped to a single surface.

### Phase-0 ADRs (gating the implementation plan)

Two design decisions sat on the critical path of the multi-phase rollout. Both Phase-0 ADRs were drafted, reviewed, and accepted on 2026-04-28; the rest of the plan now executes against their answers.

- **[ADR-0017: Block-ID stability via lazy inline stamping](../adr/0017-block-id-stability.md)** _(Accepted 2026-04-28)_ — chooses HTML-comment stamping inside markdown, materialised on-demand the first time a block is referenced cross-session. Unblocks BL-048 (drag-to-embed), BL-049 (block-links navigator), BL-050 (side-margin comments).

- **[ADR-0018: Local embedding backend — fastembed-rs](../adr/0018-embedding-backend.md)** _(Accepted 2026-04-28)_ — chooses fastembed-rs over candle and sqlite-vec's bundled gguf path on the 5-axis comparison (model quality, RAM, cold-start, cross-platform binary cost, license). Unblocks BL-019 plus the nine downstream consumers (BL-038 / BL-039 / BL-040 / BL-041 / BL-044 / BL-045 / BL-047 and the BL-010 / BL-011 / BL-034 retrieval variants).

---

## Implementation plan (2026-04-28)

> Phased rollout for every non-deferred BL item including the future-direction items minted as BL-032..BL-051 above. Cross-references all live in those entries; this section is the schedule.

### Agent-load assumptions

- **One agent ≈ 1–3 days of focused work, single tractable PR.** Items rated >medium must split into multiple agent-sized chunks (splits are listed per-item below).
- **2 concurrent foreground agents + 1 background long-runner.** The fg slots are sized so the human review queue stays drainable; the bg slot is reserved for multi-week work (F-8.1.1 in particular).
- **Agents that overlap files waste work in merges**, so file-conflict groups must serialize within their group.
- Retune assumptions: 1 fg + 0 bg roughly doubles the timeline; 3 fg + 1 bg lets BL-022 / BL-029 / BL-037 land earlier and compresses Phases 3–6 by ~3 weeks.

### File-conflict groups (serialize within group)

| Group | Items |
|---|---|
| Bases plugin | BL-015 → BL-030 → BL-031 |
| nexus-cli AI subcommands | BL-010 → BL-011 |
| nexus-mcp client | BL-023 → BL-025 |
| nexus-mcp server | BL-042 (distinct from client group above) |
| Skills | BL-021 → BL-022 |
| nexus-ai (Cargo + provider mods) | BL-016, BL-019 — keep one full PR apart |
| Shell host / sandbox | F-8.1.1 → F-8.1.2 |
| AI overlay surface | BL-032 → BL-033 → BL-034 |
| Memory inbox surface | BL-043 → BL-046 |

### Hard dependency chain

| Prereq | Unblocks |
|---|---|
| BL-016 tool-calling | BL-010, BL-011, BL-027, BL-035, BL-036, BL-044 |
| BL-019 embeddings | BL-038, BL-039, BL-040, BL-041, BL-044, BL-045, BL-047, plus BL-010/11/34 retrieval variants |
| BL-013 stream convention | future plugin streaming work |
| BL-015 trash view | BL-030 (reuses row-restore code path) |
| BL-030 undo stack | BL-031 (paste = one undo step) |
| BL-032 Cmd+I overlay | BL-010 / BL-011 / BL-033 / BL-044 (shared UX) |
| BL-041 indexing daemon | BL-045 (auto-enrichment reads the index) |
| F-8.1.1 iframe sandbox | F-8.1.2, marketplace |
| Block-id stability ADR | BL-048, BL-049, BL-050 |

### Phased rollout

| Phase | Wks | Agent A (fg) | Agent B (fg) | Agent C (bg) | Phase exit criteria |
|---|---|---|---|---|---|
| **0 — Quick wins + ADRs** | 1.5 | settings ×5 + BL-009 + BL-015 | (idle / pulls Phase-1 prep) | block-id ADR + embedding-backend ADR | both ADRs signed and recorded under "Decisions"; trash view live in bases; foundations clear for Phase 1 |
| **1 — Foundations** | 6 | **BL-016** (split ×3) | **BL-013** stream convention + **BL-032** Cmd+I overlay | **F-8.1.1** kickoff (split ×5; per-plugin migration posture — see below) | BL-016 merged → unblocks AI surfaces; BL-032 lands → unblocks BL-010/11; F-8.1.1 sandbox scaffold reachable |
| **2 — Bases + AI CLI/UI** | 4 | BL-030 → BL-031 → **BL-043** quick-capture hotkey | BL-010 + BL-034 ghost suggestions (paired engine) → BL-011 | F-8.1.1 cont. | bases polish complete; shared chat + completion engine live in CLI and editor; global capture hotkey live |
| **3 — Skills + MCP client + small AMB** | 5 | BL-021 (split ×4) → BL-022 | BL-023 → BL-025; BL-033 chips/switcher slots in | F-8.1.1 wraps; **F-8.1.2** | skills composition lands; MCP client gains WS/SSE + auth |
| **4 — Heavy AI core** | 8 | **BL-019** (split ×4) | **BL-027** agent loops (split ×5) | BL-035 right-click + block-AI actions | BL-019 unblocks all retrieval consumers; BL-027 unlocks orchestrated agents |
| **5 — Retrieval consumers** | 5 | BL-040 semantic search → BL-039 auto-links → BL-038 citations | BL-041 indexing daemon → BL-045 auto-enrichment → BL-044 recall | BL-047 scheduled digests | the BL-019 dependency tail drains |
| **6 — Heavyweights + multi-window** | 8 | BL-028 workflow umbrella (split ≥6) | BL-029 multi-window → BL-037 timeline → BL-050 side-margin comments | BL-042 Nexus-as-MCP-server | multi-window opens, panes follow; workflow gains every spec'd trigger |
| **7 — Editor + Notion polish** | 6 | BL-012 DB query blocks (split ×5) | BL-049 block-links → BL-051 multi-cursor → BL-048 drag-to-embed | BL-046 code-aware capture; BL-036 margin / inline correction | tail polish; backlog drained to deferred-only items |

Cumulative: ~44 weeks raw, ~50–55 with PR-review buffer at the assumed 2 fg + 1 bg slot budget.

### Sub-task splits (items >medium)

| BL | Split |
|---|---|
| BL-016 | (1) `ToolRegistry` + `ToolExecutor` core, (2) Anthropic + OpenAI tool-call wire format, (3) Ollama tool-call format + dispatch loop |
| BL-019 | (1) backend impl (per ADR), (2) `EmbeddingModel` trait + cache, (3) RAG wire-up, (4) batch indexer hook for BL-041 |
| BL-021 | (1) parse `depends_on`, (2) topo + cycle detection, (3) prompt-fragment merge order, (4) conflict-warning UX |
| BL-027 | (1) `AgentOrchestrator` skeleton, (2) `delegate`, (3) `parallel`, (4) `pipeline`, (5) shared scratch state + replay hooks |
| BL-028 | one agent per primitive: webhook trigger → git_event → mcp_event → parallel scheduler → retry/backoff → AI step types → templates |
| BL-012 | (1) executor over `apply_view`, (2) CM6 widget, (3) decoration plumbing, (4) undo integration, (5) filter/sort UX |
| F-8.1.1 | (1) iframe scaffold + sandbox flags, (2) postMessage protocol, (3) `NexusPluginContext` proxy, (4) per-plugin migration via `manifest.toml` `sandbox: "iframe" \| "legacy"` flag, (5) CSP + tests. Per-plugin migration posture (decided 2026-04-28) — community plugins keep working during the multi-week build window; cost is +1–2 wks vs hard cutover. |

### Risks tracked

1. **Phase-2 lock-in.** BL-010 / BL-011 / BL-034 share an engine. If BL-032 (Cmd+I) shifts after Phase-1, three tracks rework.
2. **BL-019 is the single biggest schedule bet.** Nine tracks queue behind it; a backend mistake costs weeks. The Phase-0 ADR is non-negotiable.
3. **BL-029 promotion** means earlier multi-window, which means earlier per-window plumbing problems for plugin lifecycle. Worth a lightweight design pass before Phase-6 begins.
4. **F-8.1.1** runs 1–2 eng-months in the background. If it slips into Phase-4, BL-035 (right-click in iframe-sandboxed plugins) gets harder to test.
5. **BL-022 absorbs MEM "code-aware capture" UI patterns** in Phase 3 — make sure the skill-editor surface is pluggable enough to host them rather than blocking on a separate capture UI.

### Phase-0 entry / exit checklist

- [x] Block-id stability ADR drafted, reviewed, recorded under "Decisions".
- [x] Embedding-backend ADR drafted with the 5-axis comparison (quality / RAM / cold-start / binary cost / license), recorded under "Decisions".
- [x] BL-009 mermaid whole-file viewer merged.
- [x] BL-015 bases trash view merged.
- [x] Settings extraction queue (5 items) — all shipped; see "Settings extraction queue" section above for per-item file references.
- [x] No outstanding regressions in `cargo test --workspace` / `pnpm --filter nexus-shell test` / `scripts/check_ipc_drift.sh` _(verified 2026-04-30 on `claude/review-backlog-AOGDH`: 75 result blocks all `0 failed`; 681/681 shell tests; drift `OK — generated trees match HEAD`)_.

(BL-043 quick-capture hotkey moved to Phase 2 — Tauri global-hotkey plumbing is a 1–2 day task disguised as "small" and would eat into ADR review.)
