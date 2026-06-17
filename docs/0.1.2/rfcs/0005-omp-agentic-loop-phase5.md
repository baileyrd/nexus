# RFC 0005 — Phase 5: enrich the agentic loop toward omp parity

- **Status:** Draft (plan)
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** handoff "Next direction #1 — omp agentic loop (Phase 5)"; reference blueprint `baileyrd/rusty_omp`
- **Touches (across the phased plan):** new leaf `nexus-hashline` crate; `crates/nexus-storage/` (an `edit` IPC handler + read-snapshot store); `crates/nexus-agent/` (`tool_registry.rs`, tool catalog, session tree); `crates/nexus-ai/` (multi-turn chat shape); `crates/nexus-bootstrap/` (`cap_matrix.toml`); `docs/0.1.2/ipc-handlers.md`
- **Related:** [RFC 0002 — bundled shell (`rush`)](0002-bundled-shell-rush.md), [RFC 0003 — `rusty_term`](0003-terminal-emulator-rusty-term.md)

---

## Summary

The handoff's recommended direction is an "omp agentic loop (Phase 5)" built on
`nexus-agent`, with reference behaviors in `baileyrd/rusty_omp`. Two findings
reshape what that means:

1. **`rusty_omp` is a blueprint, not code.** It is a docs-only
   reverse-engineering spec for porting [`can1357/oh-my-pi`](https://github.com/can1357/oh-my-pi)
   ("omp") to Rust — agent loop, 32-tool catalog, hashline editing, subagents,
   session tree, context management. It is the *reference*, not a crate to vendor.
2. **`nexus-agent` already has the autonomous loop.** A real multi-round driver
   (default 32 rounds, `session.rs:588`), approval gating (auto + interactive
   bus-bridge), transcript persistence + FTS search, 6 archetypes + custom TOML
   manifests, subagent delegation via the AI-runtime worker pool, LLM context
   compression + mechanical sanitization, and an agent-scoped memory log.

So **Phase 5 is *enriching* a solid loop, not building one.** This RFC does the
gap analysis against the omp blueprint and proposes a phased, small-PR plan whose
**first increment is hashline editing** — omp's signature feature, entirely
absent in Nexus today, and the cleanest fit for the microkernel rules.

## Background

### What `rusty_omp` documents

A per-subsystem blueprint (`docs/00`–`17` + a port roadmap `docs/90`) capturing
omp's architecture: a turn loop driven by `AgentSession`; a 32-tool catalog
(`read/write/edit/ast_edit/ast_grep/search/find/bash/eval/ssh/lsp/debug/task/
irc/todo/job/ask/browser/web_search/github/…/checkpoint/rewind/retain/recall/
reflect/resolve`); the **hashline** content-hash-anchored patch format with a
SnapshotStore + 3-way merge; subagents with workspace isolation (worktree →
fuse/reflink/apfs/projfs) and patch/branch merge; a session JSONL tree with
branch/rewind; context compaction/retry/handoff/snapcompact. Its own roadmap is
12 phases to a single wire-compatible Rust binary — a *separate product*. We
borrow its **behaviors and formats**, not its packaging goal.

### What `nexus-agent` already has (today)

| Capability | Evidence |
|---|---|
| Multi-round autonomous loop (default 32 rounds, ≤16 tool calls/round, working-set of 4 uncompressed rounds) | `session.rs:588`; `DEFAULT_MAX_ITERATIONS`, `DEFAULT_MAX_TOOL_CALLS_PER_ITERATION`, `WORKING_SET_ROUNDS` |
| Outcomes: `Complete / Aborted / Errored / MaxRounds / ApprovalTimeout` | `session.rs:167` |
| Approval gating: auto-approve + interactive bus-bridge (`round_proposed` event ↔ `round_decide` callback) | `handlers/shared.rs` `BusBridgePolicy`; handler `round_decide` (17) |
| 12 IPC-routed tools, capability-filtered, approval-flagged, access-logged | `tool_registry.rs` |
| Subagent delegation via the AI-runtime worker pool | `delegate` (24) → `com.nexus.ai.runtime::submit`/`wait_for` |
| 6 archetypes + custom TOML agent manifests | `archetypes.rs`; `custom_agent.rs` (`.forge/agents/<slug>/agent.toml`) |
| Context: LLM/deterministic/noop compression + 4 mechanical sanitize passes | `compression.rs`; `context_sanitize.rs` |
| Agent-scoped memory log + recall preamble | `memory.rs` (`.forge/agents/<id>/history.jsonl`) |
| Session persistence + FTS transcript search | `.forge/agent/sessions/<id>.json`; `transcript_search.rs` (handler 25) |

The 12 current tools: `read_file`, `write_file`, `delete_file`,
`replace_in_files`, `search_forge`, `list_backlinks`, `git_log`, `git_push`,
`terminal_run_saved`, `terminal_get_status`, `terminal_send_signal`,
`delegate_to_agent`. Tool dispatch is **IPC-only** (`KernelToolBridge` →
`ctx.ipc_call(target_plugin_id, command_id, …)`); the agent crate is kernel-free
and never spawns shells directly — terminal work (and `SandboxPolicy`) lives
behind `com.nexus.terminal`.

## Gap analysis — `nexus-agent` vs the omp blueprint

| Handoff item | Nexus state | Gap to close |
|---|---|---|
| Richer turn loop | ✅ multi-round, approval, compression, memory | Provider-native multi-turn chat shape (Phase 2c stub — currently a "current-user-prompt" formulation, `session.rs:556`); explicit tool error/retry policies; steering/follow-up/interrupt queue semantics |
| **Hashline 3-way-merge editing** | ❌ **absent** | The whole feature: patch grammar, 4-hex TAG hashing, SnapshotStore, 3-way merge, `edit` tool. omp's measured signature win |
| Full tool catalog | ⚠️ 12 of omp's 32 | `edit`, `ast_grep`/`ast_edit`, richer `read` (selectors/dirs/archives), `todo`, `ask`, `job`, `web_search` |
| Subagents | ✅ delegation via runtime pool | **Workspace isolation** (git-worktree → overlay/reflink) + patch/branch merge; this is where the OS-sandbox work plugs in |
| Sessions / breadcrumbs | ✅ atomic sessions + memory + FTS | Session **tree** (branch / rewind / checkpoint) + resumable sessions |

Two omp pillars are deliberately **out of scope** for Nexus Phase 5 because Nexus
already solves them its own way or they conflict with existing ADRs: the **TUI
ledger renderer** (Nexus has CLI/TUI/shell frontends) and **wire-compatibility
with omp's on-disk formats** (Nexus owns its forge layout). We borrow omp's
*ideas*, not its `~/.omp` formats.

## Phased plan (small, single-purpose PRs)

Ordered by value-per-risk and architectural self-containment. Each phase is
independently shippable and CI-green on its own.

| Phase | Scope | Crates / surface | Risk |
|---|---|---|---|
| **5.1 Hashline editing** (first) | Leaf `nexus-hashline` (parser + TAG hasher + applier + 3-way merge); `com.nexus.storage::edit` IPC handler + read-snapshot store; agent `edit` tool | new crate, `nexus-storage`, `nexus-agent`, `cap_matrix.toml`, ipc-handlers docs | Low — self-contained; `.BLK`/tree-sitter ops deferred |
| **5.2 Tool catalog breadth** | `ast_grep`/`ast_edit` (tree-sitter), richer `read` selectors, `todo`, `ask` | `nexus-agent` tools + owning service handlers | Med — tree-sitter footprint |
| **5.3 Subagent workspace isolation** | git-worktree isolation + patch/branch merge for delegated subagents; opt-in `SandboxPolicy` pass-through when spawning tool sessions | `nexus-agent`/`nexus-ai-runtime`, `nexus-git`, ties to OS-sandbox | Med — concurrency + merge semantics |
| **5.4 Session tree** | branch / rewind / checkpoint; resumable sessions | `nexus-agent` session model + handlers | Med — persistence shape change |
| **5.5 Loop hardening** | provider-native multi-turn chat (Phase 2c), tool error/retry policies | `nexus-ai`, `nexus-agent` | Med — provider-protocol surface |

## First increment in detail — Phase 5.1 hashline editing

**Why first:** highest measured value (omp reports edit success jumping
6.7%→68.3% on a weak model purely from the format), entirely absent today,
self-contained, and a textbook microkernel fit — a leaf crate + one new IPC
handler in the service that owns file-as-truth + one new agent tool. No kernel
changes, no ADR tension.

### Design

1. **`nexus-hashline` (new leaf crate).** Pure logic, no kernel dep — same
   "leaf-usable helper" posture as `ForgePathValidator` / `SandboxPolicy`:
   - **Parser** for the `[PATH#TAG]` section grammar and ops `SWAP A.=B:`,
     `DEL A.=B`, `INS.PRE/POST A:`, `INS.HEAD:`, `INS.TAIL:` (body rows `+TEXT`,
     `++`/`+-` escapes). Block ops (`SWAP.BLK`, `DEL.BLK`, `INS.BLK.POST`) parse
     but return a "tree-sitter unavailable" error until Phase 5.2.
   - **TAG hasher** — 4-uppercase-hex content hash of normalized text. Define
     Nexus's own normalization + hash (we are *not* wire-compatible with omp);
     record it as named consts. (See open questions.)
   - **Applier** + **3-way merge** (base = recorded snapshot, ours = intended
     patch result, theirs = current file) using the `similar` crate. Bounded
     `SnapshotStore` (≤30 paths × 4 versions × ≤4 MB, FIFO) as named consts.
   - Thoroughly unit-tested (parser round-trips, TAG match/mismatch, merge
     success + conflict-marker output).
2. **`com.nexus.storage::edit` IPC handler.** Storage owns file-as-truth, so the
   edit applier belongs there. It validates the patch TAG against the live
   file's hash, applies on match, 3-way-merges on mismatch, returns a diff
   preview + diagnostics. `read_file` gains snapshot recording so the store has a
   base to merge against. New capability gated as `fs.write`.
3. **Agent `edit` tool.** A 13th entry in `tool_registry.rs` targeting
   `com.nexus.storage::edit`, capability `fs.write`, `requires_approval = true`,
   with a JSON-schema input (the hashline patch text). The system prompt gains a
   short "prefer `edit` (hashline) over `write_file` for in-place changes" note.

### PR breakdown (CI-green increments)

- **PR A:** `nexus-hashline` leaf crate (parser + hasher + applier + 3-way merge
  + tests). No wiring yet — pure library, lands green in isolation.
- **PR B:** `com.nexus.storage::edit` handler + read-snapshot store. Follows the
  handoff IPC checklist: handler const + `IPC_HANDLERS` entry + dispatch arm;
  `cap_matrix.toml` entry; bump the count + row in `docs/0.1.2/ipc-handlers.md`;
  `cargo test -p nexus-bootstrap --test cap_matrix_complete` +
  `scripts/check_ipc_drift.sh` (the `EditArgs`/reply are IPC-boundary types →
  regenerate bindings).
- **PR C:** agent `edit` tool + system-prompt nudge + an end-to-end test
  (read → edit-with-stale-TAG → 3-way-merge path).

### Microkernel & workflow guardrails (apply to every Phase 5 PR)

- New backend capability ⇒ new IPC handler in the owning service crate, reachable
  from CLI/TUI/MCP/shell uniformly — never a frontend-direct call or a bespoke
  `#[tauri::command]`.
- Run `gitnexus_impact` before editing any existing symbol (e.g. `read_file`,
  the tool registry seed) and `gitnexus_detect_changes` before each commit, per
  `CLAUDE.md`. Report HIGH/CRITICAL blast radius before proceeding.
- Verify scoped to touched crates: `cargo test -p <crate>` + the pedantic clippy
  line from the handoff; shell `typecheck|lint|test` if the shell is touched.

## Non-goals

- Vendoring omp or `rusty_omp` source (the latter has none).
- Wire-compatibility with omp's `~/.omp` formats, session JSONL v3, or its exact
  TAG hashing — Nexus owns its forge layout.
- The omp TUI ledger renderer, 40+ providers, `snapcompact`, or `eval`/`browser`
  in this phase.

## Open questions

- **TAG hashing.** Define Nexus's normalization (line-ending policy) and which
  hash → 4 hex digits. We are not omp-wire-compatible, so we are free to choose a
  clean scheme (e.g. truncated SHA-256 of `\n`-normalized bytes) — but lock it
  behind tests before the `edit` tool ships, since the TAG is load-bearing for
  the 3-way-merge trigger.
- **SnapshotStore home.** Live in `nexus-storage` (alongside the index) keyed by
  session/agent, or in `nexus-hashline` as an in-memory bounded cache the storage
  handler owns? Leaning storage-owned, in-memory, per-process.
- **Tree-sitter scope (Phase 5.2).** Which grammars to pull first for `.BLK` ops
  and `ast_grep`/`ast_edit`; gate block ops on grammar availability with a clear
  fallback error (as omp does).
- **Sequencing vs. RFC 0002/0003.** Subagent isolation (5.3) is the natural place
  to flip on terminal sandboxing and, eventually, a bundled `rush` (RFC 0002) +
  OSC 133 introspection (RFC 0003) for fully Nexus-owned, agent-observable tool
  sessions.

## Progress & deferred follow-ups

Phase 5.1 (hashline editing) shipped in #303–#306; Phase 5.2 (tool-catalog
breadth) shipped in #307–#311. The agent tool catalog now covers
`read_file` / `read_lines` / `grep` / `find_symbol` / `ast_query` / `edit`
(hashline 3-way merge) / `todo` / `ask` / `delegate`, plus git and terminal.

Backlog items spun out of the shipped work (not blockers):

- **`ask` frontend wiring.** The `ask` backend (#311) publishes
  `com.nexus.agent.ask_requested` and awaits `ask_respond`, but no frontend
  renders the prompt yet — so `ask` currently always times out. The shell, CLI,
  and TUI already consume `round_proposed` / `round_decide` for interactive
  approval; wiring `ask_requested` / `ask_respond` the same way (a question
  panel that posts the answers) makes `ask` usable. Frontend-scoped PR(s).
- **Per-tool dispatch timeout.** `ask` can only wait `DEFAULT_ASK_TIMEOUT_SECS`
  (50 s), kept under the 60 s `KernelToolBridge` `DEFAULT_TOOL_TIMEOUT` ceiling.
  A genuinely interactive prompt wants longer; that needs a per-tool dispatch
  timeout (the `AgentToolSpec` already carries `estimated_duration_ms` as a
  starting point) rather than the single shared bridge timeout.
- **Subagent isolation — orchestration (RFC 0006 Step 2).** Step 1 (git-worktree
  primitives in `nexus-git`: `worktree_list` / `worktree_create` /
  `worktree_remove`) shipped in #313. Step 2 is the chosen isolation model —
  **Option A, process-level**: spin a child agent runtime on a worktree forge
  root, run it OS-sandboxed, and merge its delta back into the parent forge. This
  is the architecturally significant build (child-process orchestration, headless
  run + result plumbing, worktree merge / conflict surfacing) and wants a design
  proposal of its own before coding — deferred to backlog rather than started now.
