# IPC Handler Reference

> **As of:** 2026-05-22. Sourced verbatim from `crates/nexus-bootstrap/cap_matrix.toml` — every handler is required to appear there (`cap_matrix_complete` integration test). Per-handler descriptions and AUDIT flags are the matrix's own. The plugin-level counts in the table below are guarded by `scripts/check_ipc_docs_drift.sh`.

## Reading the tables

- **Caps** column: capabilities the caller must hold **in addition to** the unconditional `ipc.call` check. `—` means the handler is classified `unrestricted` in the matrix (any caller with `ipc.call` may dispatch). The rationale is in the handler's own row in `cap_matrix.toml`.
- **Policy** column: name of an args-aware cap closure in `crates/nexus-bootstrap/src/cap_policies.rs` that stacks on top of `Caps`. Only `ai_tools_policy` exists at v0.1.2.
- **Risk note**: `AUDIT:` = current `unrestricted` classification preserves pre-BL-138 behaviour but is flagged as a candidate for cap elevation. Full list at [`reference/audit-flags.md`](reference/audit-flags.md).

## Counts by plugin

| Plugin | Handlers |
|--------|---------:|
| `com.nexus.storage` | 75 |
| `com.nexus.git` | 38 |
| `com.nexus.terminal` | 29 |
| `com.nexus.ai` | 28 |
| `com.nexus.dap` | 21 |
| `com.nexus.agent` | 20 |
| `com.nexus.editor` | 15 |
| `com.nexus.lsp` | 14 |
| `com.nexus.workflow` | 12 |
| `com.nexus.mcp.host` | 12 |
| `com.nexus.theme` | 11 |
| `com.nexus.ai.runtime` | 12 |
| `com.nexus.skills` | 8 |
| `com.nexus.acp` | 8 |
| `com.nexus.security` | 9 |
| `com.nexus.comments` | 7 |
| `com.nexus.memory` | 21 |
| `com.nexus.database` | 6 |
| `com.nexus.notifications` | 5 |
| `com.nexus.templates` | 5 |
| `com.nexus.collab` | 4 |
| `com.nexus.audio` | 3 |
| `com.nexus.formats` | 2 |
| `com.nexus.linkpreview` | 1 |
| **Total** | **342** |

`.v<N>` aliases (per ADR 0021) are not listed separately — the matrix applier auto-mirrors a row's classification onto every alias.

> **Drift note.** The counts above are recomputed from `cap_matrix.toml`. The per-handler tables below may lag individual additions/renames — `cap_matrix.toml` is canonical for the exact handler set, and `cap_matrix_complete` enforces that every registered handler has a row. A future drift script (`scripts/check_ipc_docs_drift.sh`) should compare both directions; until then, treat this page as a guide and the matrix as the source of truth.

---

## com.nexus.storage (75)

### Read

| Command | Caps | Note |
|---------|------|------|
| `query_files` | — | file enumeration with filters |
| `read_file` | — | downstream `fs.read` if path resolves external |
| `read_lines` | — | line-range read (1-based, inclusive) for large files; returns the slice + total lines + the file's hashline tag |
| `read_frontmatter` | — | frontmatter extraction |
| `find_in_files` | — | ripgrep-style content search |
| `ast_query` | — | tree-sitter structural code search (S-expression query with `@capture`s) over one language |
| `search` | — | Tantivy FTS query |
| `file_exists` | — | existence check |
| `list_dir` | — | directory enumeration |
| `backlinks` / `backlinks_to_block` | — | wiki backlink query |
| `outgoing_links` / `unresolved_links` / `list_all_links` | — | link enumeration |
| `graph_stats` / `graph_neighbors` | — | knowledge graph |
| `query_tags` / `query_tasks` / `query_blocks` / `query_symbol` | — | index queries |
| `entity_get` / `entity_search` / `entity_relations` / `entity_find_duplicates` | — | entity graph |
| `list_draft_relations` | — | low-confidence entity relations (Dream Cycle inbox) |
| `vector_query` / `vectorstore_count` | — | RAG vector store reads |
| `config_read` / `settings_read` | — | forge config + settings |
| `canvas_read` / `canvas_nodes` / `canvas_edges` | — | canvas parse |
| `base_list` / `base_load` / `base_index` / `base_query` / `obsidian_base_query` | — | Bases read surface |

### Write

All write handlers are classified `unrestricted` in the matrix — the downstream `fs.write` check in the storage plugin itself rejects external paths.

| Command | Caps | Note |
|---------|------|------|
| `write_file` / `write_vault_file` / `write_default_gitignore` / `note_append` | — | text writes |
| `edit` | — | apply a hashline patch (content-hash-anchored edits), then write through `write_file` |
| `create_file` / `create_dir` / `delete_file` / `delete_entry` / `rename_entry` | — | fs ops |
| `toggle_task` | — | inline task checkbox toggle |
| `replace_in_files` | — | find/replace across files |
| `import_forge` | — | external archive ingest |
| `rebuild_index` / `rebuild_search_index` | — | re-derive SQLite / Tantivy |
| `vector_insert` / `vector_delete_by_file` | — | RAG vector writes |
| `entity_upsert` / `entity_merge` / `entity_decay_relations` | — | entity graph writes |
| `config_reset` / `settings_write` | — | forge config + settings writes |
| `canvas_write` / `canvas_patch` | — | canvas writes |
| `base_create` / `base_property_*` / `base_record_*` / `base_view_*` | — | Bases write surface (15 verbs) |

---

## com.nexus.git (38)

| Command | Caps | Note |
|---------|------|------|
| `status` / `log` / `branches` / `blame` / `diff_file` / `diff_staged` / `file_status` / `file_statuses` / `lfs_status` / `list_tags` / `conflict_files` / `conflict_versions` / `stash_list` | — | read-only inspection |
| `stage_file` / `stage_all` / `stage_hunks` / `unstage_file` / `unstage_all` / `unstage_hunks` / `discard_hunks` | — | working-tree / index mutation inside forge root |
| `commit` | — | writes commit object |
| `create_branch` / `delete_branch` / `switch_branch` / `create_tag` / `delete_tag` | — | ref mutation |
| `push` / `push_tags` | — | **AUDIT** — outbound network, candidate for `net.http` |
| `merge` / `abort_merge` / `rebase` / `abort_rebase` / `cherry_pick` / `abort_cherry_pick` | — | history mutation |
| `stash_push` / `stash_pop` / `stash_drop` | — | stash mgmt |

---

## com.nexus.ai (28)

### Generation (gated by `ai.chat`)

| Command | Caps | Policy | Note |
|---------|------|--------|------|
| `stream_chat` | `ai.chat` | `ai_tools_policy` | tools=auto → +`ai.tools.write`; auto_with_mcp → +`ai.tools.mcp` |
| `stream_ask` / `ask` / `semantic_search` / `embed_text` / `generate` / `enrich_file` / `enrich_entity` / `infer_entity_relations` / `generate_docs` / `predict` | `ai.chat` | — | one provider round-trip per call |
| `propose_tool_calls` | `ai.chat` | `ai_tools_policy` | same policy as `stream_chat` |

### Index + sessions + config

| Command | Caps | Note |
|---------|------|------|
| `index_file` / `index_trigger` | `ai.index` | RAG index daemon |
| `session_load` / `session_list` | `ai.session.read` | — |
| `session_save` / `session_delete` | `ai.session.write` | — |
| `set_config` | `ai.config.write` | hot-swap provider creds — same risk surface as `process.spawn` |
| `activity_clear` | `ai.activity.write` | — |
| `status` / `config` / `index_status` / `vectorstore_count` / `activity_list` / `entity_recall` | — | read-only |
| `enrich_apply` | — | writes via storage |
| `resolve_credentials` | — | **AUDIT** — returns keyring material; candidate for in-tree-only marker |

---

## com.nexus.terminal (29)

| Command | Caps | Note |
|---------|------|------|
| `create_session` / `repl_start` | `process.spawn` | spawns arbitrary processes with guest-supplied shell/working_dir/env |
| `send_input` / `send_raw_input` | — | **AUDIT** — writes into already-spawned PTY; candidate for `process.spawn` |
| `run_saved` / `adhoc_promote` / `repl_eval` | — | **AUDIT** — same posture |
| `repl_stop` / `repl_list` | — | session lifecycle (already-spawned) |
| `list_sessions` / `get_session_info` / `rename_session` / `close_session` / `pump` / `read_output` / `read_raw_since` / `search_output` / `cross_session_search` / `wait_for_pattern` / `suggest` | — | read / session control |
| `saved_list` / `saved_create` / `saved_update` / `saved_delete` / `saved_reorder` | — | saved-command CRUD (KV) |
| `adhoc_list` / `adhoc_get` / `adhoc_delete` | — | ad-hoc history (KV) |
| `open_in_terminal` | — | host-level invocation |

---

## com.nexus.dap (21)

| Command | Caps | Note |
|---------|------|------|
| `launch` / `attach` | `process.spawn` | spawns debug adapter |
| `register_adapter` / `unregister_adapter` | `protocol.host.contribute` | invoker-only contribution lifecycle |
| `list_adapters` | — | read-only |
| `configuration_done` / `disconnect` / `terminate` | — | session control |
| `set_breakpoints` / `set_function_breakpoints` / `set_exception_breakpoints` | — | breakpoint control |
| `continue` / `next` / `step_in` / `step_out` / `pause` | — | execution control |
| `threads` / `stack_trace` / `scopes` / `variables` / `evaluate` | — | inspection |

---

## com.nexus.agent (20)

| Command | Caps | Note |
|---------|------|------|
| `session_run` / `round_decide` | `ai.chat` | drives the tool loop |
| `ask` / `ask_respond` | `ai.chat` | interactive prompt during the loop (publishes `ask_requested`, awaits a frontend reply) |
| `delegate` / `plan` | — | **AUDIT** — drives chat under the hood; candidate for `ai.chat` |
| `history_get` / `history_list` / `history_delete` / `search_transcripts` | — | transcript surface |
| `list_archetypes` / `list_custom` / `list_tools` | — | catalog |
| `memory_record` / `memory_query` / `memory_export` / `memory_prune` | — | KV-backed agent memory |
| `session_get` / `session_list` / `session_delete` | — | stored sessions |

---

## com.nexus.editor (15)

| Command | Caps | Note |
|---------|------|------|
| `open` / `close` / `list_open` | — | session lifecycle |
| `get_markdown` / `get_tree` | — | session reads |
| `apply_transaction` / `undo` / `redo` / `stamp_block` / `sync_content` | — | in-memory mutation |
| `save` | — | downstream `fs.write` via storage |
| `resolve_block_link` / `execute_database_view` | — | read-only embed support |
| `open_excerpts` / `refresh_excerpts` | — | BL-141 multibuffer view |

---

## com.nexus.lsp (14)

| Command | Caps | Note |
|---------|------|------|
| `register_server` / `unregister_server` | `protocol.host.contribute` | invoker-only contribution lifecycle |
| `list_servers` | — | read-only |
| `open_file` / `close_file` / `change_file` | — | text document tracking |
| `completions` / `hover` / `definition` / `references` / `code_actions` / `format` / `rename` / `execute_command` | — | LSP protocol verbs |

---

## com.nexus.workflow (12)

| Command | Caps | Note |
|---------|------|------|
| `list` / `get` / `validate` / `next_fire` / `run_history` / `templates_list` / `templates_get` | — | read / pure |
| `templates_init` | — | downstream `fs.write` |
| `set_digest_config` / `reload` | — | KV / fs traversal |
| `run` / `run_digest` | — | **AUDIT** — each step gated by target handler's caps; issue #77 |

---

## com.nexus.theme (11)

| Command | Caps | Note |
|---------|------|------|
| `get_available_themes` / `get_available_snippets` / `get_theme_config` / `compute_variables` | — | read / pure |
| `apply_theme` / `apply_config` / `set_mode` / `toggle_snippet` / `reorder_snippets` / `set_plugin_overrides` | — | forge-local settings mutation |
| `reload` | — | rescans theme files |

---

## com.nexus.mcp.host (12)

| Command | Caps | Note |
|---------|------|------|
| `connect` | `process.spawn` | spawns MCP server over stdio |
| `register_server` / `unregister_server` | `protocol.host.contribute` | invoker-only contribution lifecycle |
| `list_servers` / `list_tools` / `list_prompts` / `list_resources` / `list_dynamic_tools` | — | read-only |
| `disconnect` | — | tears down a connected server |
| `call_tool` | — | invokes a tool on a connected server (server holds its own caps) |
| `register_tool` / `unregister_tool` | — | dynamic-tool registry write |

---

## com.nexus.ai.runtime (12)

| Command | Caps | Note |
|---------|------|------|
| `submit` | `ai.runtime.submit` | enqueue a task |
| `cancel` | `ai.runtime.control` | signals CancelGate |
| `pause` / `resume` | `ai.runtime.control` | unsupported for Session tasks at v0.1.2; cap reserved |
| `get` / `list` / `events` / `pool_stats` / `wait_for` | `ai.runtime.observe` | read-only observation |
| `register_trigger` / `unregister_trigger` | `ai.runtime.control` | BL-134 Phase 7 — ambient trigger management |
| `list_triggers` | `ai.runtime.observe` | BL-134 Phase 7 — read-only trigger listing |

---

## com.nexus.skills (8)

| Command | Caps | Note |
|---------|------|------|
| `list` / `get` / `list_by_context` / `triggered_by` | — | catalog |
| `render` / `compose` | — | pure string templating |
| `invoke` | — | downstream gating via storage/AI verbs |
| `reload` | — | rescans `.skill.md` files |

---

## com.nexus.security (9)

| Command | Caps | Note |
|---------|------|------|
| `get_secret` | — | namespace-prefixed lookup; cross-plugin reads blocked |
| `set_secret` / `delete_secret` | `security.write` | P1-01 — keyring writes |
| `list_secret_names` | — | names only (no values) |
| `query_audit_log` | `security.audit.read` | V12 (2026-06-10) — log discloses cross-plugin telemetry; previously unrestricted |
| `metrics_snapshot` | — | read-only observability |
| `clear_audit_log` | `security.audit.write` | P1-01 — destroys audit history |
| `sandbox_policy` | — | read-only introspection of the active OS-sandbox config (`sandbox.toml`) |
| `download` | `net.http` | **async** — brokered, allowlisted download into a writable root (`{ url, dest, cwd? }`) |

---

## com.nexus.comments (7)

All forge-local thread store mutations; downstream `fs.write` gated.

`list`, `create_thread`, `add_reply`, `edit_comment`, `delete_comment`, `delete_thread`, `set_resolved`.

---

## com.nexus.acp (8)

| Command | Caps | Note |
|---------|------|------|
| `initialize` | `process.spawn` | spawns ACP agent process |
| `register_server` / `unregister_server` | `protocol.host.contribute` | invoker-only contribution lifecycle |
| `list_agents` | — | read-only |
| `propose` / `accept` / `reject` / `disconnect` | — | session-scoped |

---

## com.nexus.database (6)

All `unrestricted` — pure compute and serialization. Downstream `fs.write` via storage handles persistence.

`formula_eval`, `compute_rollup`, `resolve_relation`, `apply_view`, `csv_import`, `csv_export`.

---

## com.nexus.notifications (5)

| Command | Caps | Note |
|---------|------|------|
| `inbox_list` / `inbox_stats` | `notifications.inbox.read` | — |
| `inbox_mark_read` / `inbox_dismiss` | `notifications.inbox.write` | — |
| `send` | — | downstream `ui.notify` gates toast transport |

---

## com.nexus.templates (5)

All `unrestricted`. `list`, `get`, `render`, `apply` (downstream `fs.write`), `reload`.

---

## com.nexus.memory (21)

Native memory engine (`nexus-memory`). SQLite-persisted memories with FTS5 search and SPO entity facts. Handlers operate on the plugin's own `.forge/memory/memory.db` and are `unrestricted`, **except** `sync` (gated by `net.http` for outbound hub calls) and `wiki_compile` (gated by `ai.chat` for the synthesis round-trip). The async `recall`/`vector_sync`/`wiki_*` reach AI + storage through the plugin's *own* capability-gated context, not the caller's.

| Command | Caps | Note |
|---------|------|------|
| `add` | — | store a memory (incl. optional `subject`/`predicate`/`object` fact); returns it |
| `get` | — | fetch one by id |
| `list` | — | recent memories, newest first; optional `category`/`memory_type`/`status`/`tag` filters |
| `search` | — | FTS5 full-text search |
| `update` | — | patch mutable fields (content/category/tags/status/SPO) |
| `delete` | — | remove a memory |
| `stats` | — | store count + category/type/source breakdowns |
| `facts` | — | recall SPO entity facts; optional `subject`/`predicate`/`object` filters |
| `entities` | — | distinct entities (fact subjects + objects) with fact counts |
| `export` | — | full dump of every memory, oldest first (for portability / re-import) |
| `tags` | — | distinct tags with the number of memories carrying each |
| `vitality_report` | — | active memories ranked by computed ACT-R-style vitality (frequency + recency) |
| `recall` | — | **async** — hybrid FTS + vector recall fused via Reciprocal Rank Fusion; falls back to FTS-only when no embedder/vectors |
| `vector_sync` | — | **async** — backfill embeddings for stored memories into the `memory` vector namespace |
| `sync` | `net.http` | **async** — push/pull the store to a caller-configured `nexus-memory-hub` (LWW, keyset cursors) |
| `wiki_compile` | `ai.chat` | **async** — synthesize a `wiki/<slug>.md` page from memories on a topic (AI generate → storage write) |
| `wiki_read` | — | **async** — read a wiki page's Markdown by topic/slug |
| `wiki_list` | — | **async** — list the wiki pages |
| `auto_capture` | — | **async** — capture a turn as a memory; optional LLM `decompose` into atomic child facts |
| `get_capture` | — | a capture's lineage (parent + decomposed children) by `capture_id` |
| `consolidate` | — | dedupe: supersede exact normalized-content duplicates, keeping the freshest (`dry_run?`, `category?`) |

---

## com.nexus.collab (4)

| Command | Caps | Note |
|---------|------|------|
| `publish_presence` | — | stamps local peer identity from `[collab]` config; short-circuits when collab unconfigured |
| `start_relay` | — | **AUDIT** — binds 0.0.0.0; candidate for future `network.bind` cap |
| `stop_relay` / `relay_status` | — | tears down / reads local relay state |

---

## com.nexus.audio (3)

| Command | Caps | Note |
|---------|------|------|
| `transcribe` | `audio.record` | privacy-sensitive — captures room audio |
| `synthesize` | `audio.synthesize` | — |
| `status` | — | read-only |

---

## com.nexus.formats (2)

`import_notion`, `export_notion` — pure parse / serialize; fs ops route through storage.

---

## com.nexus.linkpreview (1)

| Command | Caps | Note |
|---------|------|------|
| `fetch` | — | **AUDIT** — outbound HTTP to arbitrary URLs; candidate for `net.http` |
