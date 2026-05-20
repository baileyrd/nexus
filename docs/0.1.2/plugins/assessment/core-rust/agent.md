# com.nexus.agent

- **Path:** `crates/nexus-agent/`
- **Tier:** Core Rust
- **Bootstrap order:** 10

## Architecture

- Entry point: `crates/nexus-agent/src/core_plugin.rs` — `AgentCorePlugin`. Bootstrap registration: `crates/nexus-bootstrap/src/plugins/agent.rs` (also calls `nexus_agent::seed_default_tools()` to populate the process-global agent-tool registry). Lifecycle: `on_init` opens the BL-121 FTS5 transcript-search index; `on_start` / `on_stop` are no-ops.
- Key modules:
  - `session.rs` — `AgentSession`, `run_session*` family, the iteration / tool-call loop, `SessionPolicy` for per-step approval gates.
  - `llm.rs` — `LlmAgent` driving `com.nexus.ai::propose_tool_calls`; `ChatDriver` adapter.
  - `archetypes.rs` — built-in archetype IDs + system prompts (`writer`, `coder`, `researcher`, `librarian`, `coach`, `auditor`).
  - `custom_agent.rs` — `.agent.md` manifest parser + `scan_forge` over `<forge>/.forge/agents/`.
  - `memory.rs` — append-only `history.jsonl` per agent under `<forge>/.forge/agents/<agent_id>/`.
  - `transcript_search.rs` — FTS5 index at `<forge>/.forge/agent/transcripts.sqlite`, rebuilt from every agent's `history.jsonl` on `on_init` when empty.
  - `tool_registry.rs` — process-global agent tool catalogue with capability checks.
  - `compression.rs`, `context_sanitize.rs` (BL-131), `auto_notify.rs` (BL-133 subscriber on `com.nexus.agent.session_completed`), `handlers/` (one file per handler family).
- Persistence:
  - `<forge>/.forge/agents/<agent_id>/history.jsonl` — append-only memory log (per `memory.rs:12-13`, constant `AGENTS_DIR = ".forge/agents"`).
  - `<forge>/.forge/agent/sessions/<session_id>.json` — per-session transcripts (per `memory.rs:3-4`).
  - `<forge>/.forge/agent/transcripts.sqlite` — FTS5 index (`transcript_search.rs:37`).
  - `<forge>/.forge/agents/<agent_id>/.agent.md` — optional custom-agent manifests parsed by `custom_agent.rs`.
- Settings owned: none of its own. Reads `ai.toml` indirectly through `com.nexus.ai` for the underlying provider config. `agents.toml` is not used.
- External dependencies: `rusqlite` (bundled SQLite for FTS5), `regex-lite` (BL-131 sanitiser passes).

## Surface

18 IPC handlers (full table at `crates/nexus-agent/src/core_plugin.rs:133`):

`plan`, `history_list`, `history_get`, `history_delete`, `list_archetypes`, `session_run`, `session_list`, `session_get`, `session_delete`, `round_decide`, `list_tools`, `list_custom`, `memory_record`, `memory_query`, `memory_prune`, `memory_export`, `delegate`, `search_transcripts`.

Bus topics consumed: `com.nexus.agent.round_proposed`, `com.nexus.agent.session_completed`. Topics published: same family, via the session loop.

## Necessity

- **Verdict:** Optional
- **Required for basic capabilities?** No — agents are an opt-in surface for autonomous work. Browsing, editing, searching, and committing all complete without ever dispatching `session_run`.
- **Depended on by:** `com.nexus.ai.runtime` (the `submit` handler dispatches into `com.nexus.agent::session_run`), shell-nexus `agent` panel, MCP server (re-exposes agent operations as MCP tools), and the shell command palette's "Run agent" affordances.
- **Depends on:** `com.nexus.ai` (planner + tool-call proposer), `com.nexus.storage` (transcript persistence routes through file I/O the storage plugin owns), kernel + plugins crates only.
- **What breaks if removed:** no archetype runs, no plan / session_run, no transcript search, no custom-agent loading. The ai-runtime plugin's `submit` calls would fail with an IPC-target-missing error. The basic workflow is unaffected.

## Notes

- BL-121 FTS5 index rebuild is "soft" — failure to open the database leaves the plugin in a state where `search_transcripts` returns an empty result instead of crashing.
- `seed_default_tools()` is called from bootstrap, not the plugin's lifecycle hooks — the catalogue is process-global, not forge-scoped.
- BL-131 context sanitisation passes are pure and unit-tested in `context_sanitize.rs`; they fire just before each `ChatDriver` invocation in the session loop.
- The `Agent` trait + `Plan` / `Step` types in `lib.rs` remain as the library-first scaffold from PRD-15; today the only in-tree producer is `LlmAgent` driving `propose_tool_calls`.
