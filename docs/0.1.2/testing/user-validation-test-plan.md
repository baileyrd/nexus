# Nexus User Validation Test Plan

> **As of:** 2026-07-03. Sourced from `docs/0.1.2/application-capabilities.md`, `docs/0.1.2/crates/nexus-cli.md`, `docs/0.1.2/capabilities.md`, `docs/0.1.2/settings/`, the live MCP tool list in `crates/nexus-mcp/src/server.rs`, and direct exploration of `crates/nexus-tui/src/` and `shell/src/plugins/`. An interactive, checkable version of this plan (progress saved locally, no server) is available as a companion Claude Artifact; this file is the version-controlled source of truth.

**Scope:** every capability of Nexus 0.1.2, walked from a completely fresh install through every domain, across all four frontends (CLI, desktop shell, TUI, MCP server) plus the optional remote-forge path.

**How to use this document:** work top to bottom. Section 0 is one-time setup. Section 1 is your very first boot. From there, sections are grouped by capability domain, each with a **Setup** note (if any), numbered test cases (**Steps** → **Expected**), and a **Shell UI** / **TUI** / **MCP** callout where that frontend has its own surface for the same capability. Check items off as you go. Anything marked **[KNOWN GAP]** is a documented limitation, not a bug — confirm the *documented* behavior, don't file it as a defect.

---

## 0. Prerequisites & one-time setup

- [ ] **0.1 Build.** From repo root: `cargo build --workspace` (Rust) and `pnpm install && pnpm --filter nexus-shell tauri:dev` availability check (desktop shell; needs `webkit2gtk-4.1`/`libsoup-3.0` system libs on Linux). Confirm both complete without error.
- [ ] **0.2 Pick a scratch forge location.** Everything below assumes `$FORGE` is an empty directory you own, e.g. `export FORGE=~/nexus-uvt-forge`. Never point this at a real vault you care about.
- [ ] **0.3 AI provider (needed for section 6 AI, 7 Agents, and the semantic/hybrid search + Dream Cycle enrich/infer cases).** Fastest path: `export ANTHROPIC_API_KEY=...` or `OPENAI_API_KEY=...` before launching Nexus — auto-detected, no file to edit. To pin explicitly, or to test config editing itself, write `$FORGE/.forge/ai.toml`:
  ```toml
  provider = "anthropic"
  model = "claude-sonnet-4-6"
  api_key = "${ANTHROPIC_API_KEY}"
  ```
  Local/offline alternative for **embeddings only** (enough for semantic/hybrid search, not chat): `export NEXUS_LOCAL_EMBEDDINGS=1` (binary must be built with `--features local-embeddings`) or run Ollama and set `ollama_base_url` in `ai.toml`. Verify anytime with `nexus ai status`.
- [ ] **0.4 Notifications (section 18).** No config needed for the desktop-toast channel. For Discord/Telegram/email, write `$FORGE/.forge/notifications.toml` with a real webhook/bot-token/SMTP block (there is no mock channel — see section 18 for the template).
- [ ] **0.5 MCP external server (section 17.2).** No bundled test server ships in-repo. Have `npx` available and use the reference filesystem server (`@modelcontextprotocol/server-filesystem`), or point at any MCP server you have.
- [ ] **0.6 Understand the trust model.** The CLI, TUI, and desktop shell all run at full first-party trust (`Capability::ALL`) — you will **not** see capability-grant prompts testing them directly. Grant prompts only appear when you install a **community plugin** (section 22.3) — that's the one place capability gating is actually visible to a user.

---

## 1. Fresh start — first boot

- [ ] **1.1 Init a forge.** `nexus --forge-path $FORGE forge init --template os`. Expect: `Forge initialised at '$FORGE'` + an initial index count. Confirm `$FORGE/.forge/` now exists (`index.db`, `search/`, `app.toml`, `.kernel/`, `.lock`) and the `os` template's folder scaffold is present.
- [ ] **1.2 Status & doctor.** `nexus --forge-path $FORGE forge status`, then `nexus --forge-path $FORGE forge doctor`. Expect clean output, no drift, no errors.
- [ ] **1.3 First note.** `nexus --forge-path $FORGE content create notes/hello.md --content "# Hello\n\nMy first note. See [[Second Note]]."`. Expect a success message with the written path.
- [ ] **1.4 Read it back.** `nexus --forge-path $FORGE content read notes/hello.md`. Confirm content matches.
- [ ] **1.5 First search.** `nexus --forge-path $FORGE content search "first note"`. Expect `notes/hello.md` ranked with a score.
- [ ] **1.7 Open the desktop shell for the first time.** `nexus desktop` (or launch the Tauri app directly) pointed at `$FORGE`. Expect the **launcher** welcome screen (no forge open yet) → "Open Folder…" → pick `$FORGE` → the full shell chrome loads (activity bar, file tree showing `notes/hello.md`, empty editor).
- [ ] **1.8 Open the TUI for the first time.** `NEXUS_FORGE_PATH=$FORGE nexus tui`. **[KNOWN GAP]** `nexus tui` does *not* forward `--forge-path`/a positional arg — you must use `NEXUS_FORGE_PATH` (env) or the standalone `nexus-tui $FORGE` binary, otherwise it silently opens `~/.nexus/default`. Expect the file tree (left) + viewer (right) with `notes/hello.md` visible; press `q` to confirm clean exit (terminal restored, no leftover raw-mode artifacts).

---

## 2. Notes & content

*Domain: markdown/GFM/MDX, wikilinks, block IDs, frontmatter, tags, tasks, trash, daily notes, quick switcher.*

- [ ] **2.1 Wikilink resolution.** Create `notes/second.md` titled "Second Note". Reopen `notes/hello.md`; confirm the `[[Second Note]]` link now resolves (3-tier: exact → basename → case-insensitive stem) instead of showing as a phantom/broken link.
- [ ] **2.2 Backlinks.** `nexus content backlinks notes/second.md`. Expect `notes/hello.md` listed. **Shell UI:** open `second.md`, check the **Note Context** right-panel "Backlinks" section shows the same.
- [ ] **2.3 Block IDs & block-level backlinks.** In the shell editor, hover/inspect a block's stable id (or via MCP: `nexus_comment_create_thread` internally stamps one — see 14.2). Confirm block ids survive a re-save/re-parse of the file.
- [ ] **2.4 Tags.** Add `#project` to a note body or `tags: [project]` frontmatter. `nexus content` has no direct tag-add verb — edit via `content update` or the editor. Then `nexus tags list --name project`. Expect the note listed. **Shell UI:** search `tag:project` in the Search sidebar.
- [ ] **2.5 Frontmatter.** `nexus --forge-path $FORGE content create notes/fm.md --content "---\ndate: 2026-01-01\ntags: [demo]\n---\n# FM Test\n"`. **Shell UI:** open it, use **File Properties** right-panel to edit the `date` field as a typed form control; confirm the YAML on disk updates.
- [ ] **2.6 Tasks — create & query.** Add `- [ ] Buy milk` and `- [x] Done thing` to a note. `nexus content tasks` (pending only, default), `nexus content tasks --all`. Expect both, filtered correctly.
- [ ] **2.7 Task toggle.** `nexus content task-toggle <id>` (id from 2.6's output). Confirm the checkbox flips in the source file. **Shell UI:** open **Task Dashboard**, click a checkbox there instead; confirm the source note's `- [ ]`/`- [x]` updates too (C7 finding — due-date/priority tokens like `@due(...)` group correctly in the dashboard).
- [ ] **2.8 Daily notes — idempotent open.** `nexus content daily`. Run it twice; confirm the second run does **not** create a duplicate file. **Shell UI:** `Ctrl+Shift+J` twice — same idempotency; use the calendar pane's Prev/Next Day nav to jump dates (C6 finding).
- [ ] **2.9 Delete → trash → restore.** `nexus content delete notes/fm.md` (no `--permanent`). Confirm the file is gone from `content list` but recoverable — check `$FORGE/.trash/` exists and is excluded from search/index/watcher. Use `nexus trash restore <id>` if available, else the shell's trash UI, to bring it back.
- [ ] **2.10 Permanent delete.** Create a throwaway note, `nexus content delete <path> --permanent`. Confirm no trash entry is created and it's unrecoverable.
- [ ] **2.11 Find/replace across files.** Exercise `com.nexus.storage::replace_in_files` via the shell's **Search in Files** panel (`Ctrl+Shift+F`): search a keyword across the forge, do a workspace-wide replace, confirm multiple files update.
- [ ] **2.12 Quick switcher.** Shell: `Ctrl+P`, type a partial filename of an existing note → Enter opens it. Type a name that doesn't exist → Enter creates it. Confirm per-forge "recent files" ordering (C5 finding).
- [ ] **2.13 Command palette.** Shell: `Ctrl+Shift+P`, search "Export as HTML", run it against the open note. Confirm every contributed command across every plugin is reachable from here (spot-check 3-4 unrelated commands, e.g. "Toggle bookmark for active note", "New from template…").
- [ ] **2.14 Link preview.** Paste a URL with an Open-Graph page into a note in the shell editor. Confirm a preview card (title/description/image) renders inline within ~5s, or gracefully does nothing if fetch times out/exceeds 512KB.
- [ ] **2.15 Export a note.** `nexus content export notes/hello.md -o /tmp/hello.md`. Confirm output matches.
- [ ] **2.16 `.aiignore`.** Create `$FORGE/.aiignore` excluding `notes/second.md` (or add `ai: exclude` frontmatter to it). Re-run `ai embed` (section 6) and confirm that note is skipped from embedding/RAG.

---

## 3. Search

*Domain: FTS (Tantivy), code-symbol index, find-in-files, semantic/hybrid, tag/task/block queries.*

- [ ] **3.1 Lexical FTS with scoping operators.** `nexus content search "hello"`; then try `path:notes/`, `tag:project`, `type:task` style operators in the shell's Search sidebar query box. Confirm each scopes results correctly.
- [ ] **3.2 Reindex.** `nexus forge reindex`. Confirm it completes and subsequent searches still work.
- [ ] **3.3 Semantic search (CLI).** With an AI embedding provider configured (0.3): `nexus content search "note about greetings" --semantic`. Confirm ranked hits with a score column and an excerpt (via `chunk_text`), distinct from the lexical result set.
- [ ] **3.4 Hybrid search (CLI).** `nexus content search "hello" --hybrid`. Confirm results carry an excerpt (`excerpt` field) and that `--semantic --hybrid` together is **rejected** by the CLI (mutually exclusive flags).
- [ ] **3.5 Semantic search (shell).** Command palette → "Search by Meaning" (`semanticSearch` plugin — default-off, enable via Plugins manager first). Run a natural-language query; confirm results ranked by meaning, not keyword overlap.
- [ ] **3.6 Code symbol index.** Add a `.rs`/`.ts`/`.py`/`.go` code block to a note (BL-114 indexes fenced code, tree-sitter-parsed). Confirm the symbol is queryable (used internally by `nexus_context`/`nexus_impact` MCP tools — see 17.1).
- [ ] **3.7 MCP search parity.** Via an MCP client (17.1): call `nexus_search` (lexical) and `nexus_semantic_search` (embedding, `hybrid: true|false`). Confirm `nexus_semantic_search` returns raw ranked matches (not a synthesized answer) unlike `nexus_ask`.

---

## 4. Knowledge graph

*Domain: entities, typed relations, Dream Cycle (extract → dedup → decay → enrich → infer).*

- [ ] **4.1 Entity CRUD.** `nexus graph entity list`; create one via the shell or `entity_upsert` IPC if no direct CLI create verb exists — confirm `nexus graph entity show <id>` returns it.
- [ ] **4.2 Entity search & duplicates.** `nexus graph entity search <query>`, `nexus graph entity duplicates`. Create two near-duplicate entities on purpose; confirm the duplicates command flags the pair.
- [ ] **4.3 Relations & neighbors.** `nexus graph entity related <id>`, `nexus graph neighbors notes/hello.md -d 2`. Confirm relation direction (outgoing/incoming/both) is respected.
- [ ] **4.4 Unresolved links.** `nexus graph status` and `nexus graph unresolved`. Add a `[[Nonexistent Note]]` wikilink somewhere; confirm it now appears in `unresolved`.
- [ ] **4.5 Global graph (shell).** Activity bar → **Global Graph**. Confirm forge-wide clustering renders and the search overlay highlights a node by name.
- [ ] **4.6 Local graph (shell).** Open a well-linked note → **Graph** right-panel section. Confirm it's centered on the active note with correct edges.
- [ ] **4.7 Dream Cycle — dedup/decay (no AI needed).** `nexus graph dream-cycle run --phase dedup`, then `--phase decay --decay-factor 0.9 --decay-floor 0.1`. Confirm these run without an AI provider configured and report counts.
- [ ] **4.8 Dream Cycle — extract (new entities, requires AI, opt-in).** Set `extract_enabled = true` under `[dream_cycle]` in `app.toml` (or shell settings). Add a note mentioning a clearly-new named entity. Run `nexus graph dream-cycle run --phase extract` (or a full cycle). Confirm a new low-confidence entity stub is created (C44 finding) and does **not** touch pre-existing entities.
- [ ] **4.9 Dream Cycle — enrich/infer (requires AI).** `nexus graph dream-cycle run --phase enrich`, `--phase infer`. Confirm new low-confidence *draft relations* appear on affected entities, and that `--dry-run` reports without writing.
- [ ] **4.10 Dream Cycle scheduler (shell/MCP-server sessions).** Leave the shell open with `dreamCycle` enabled; confirm a background cycle eventually fires and produces the toast "N new relation proposals / entities extracted from Dream Cycle" (C46 finding — the scheduler now spawns in both shell and `nexus mcp serve` sessions, not just TUI/CLI one-shots). Open the **Dream Cycle Inbox** panel and confirm proposals are listed (approve/skip UI may not be wired yet — viewing only is the testable surface).
- [ ] **4.11 Review threshold / merge threshold.** Re-run `dream-cycle run --phase dedup --merge-threshold 0.95 --review-threshold 0.7` with different values; confirm the count of auto-merged vs. flagged-for-review pairs changes accordingly.

---

## 5. Memory (long-term recall engine)

*Domain: capture, decompose, recall (hybrid-vector RRF), SPO facts, entity graph, ACT-R vitality, consolidate, wiki synthesis.*

- [ ] **5.1 Quick capture (shell, global hotkey).** Press `Ctrl+Alt+N` from *anywhere* (even unfocused, OS-level hotkey). Type a fact into the overlay, save. Confirm it lands in the memory store.
- [ ] **5.2 Search / recall.** Command palette → "Memory: Search" and "Memory: Recall". Confirm ranked hits; via MCP, `nexus_memory_search` / `nexus_memory_recall` should return equivalent results.
- [ ] **5.3 Facts & entities.** "Memory: Facts", "Memory: Entities", "Memory: Tags" palette commands (or `nexus_memory_facts`/`nexus_memory_entities`/`nexus_memory_tags` via MCP). Confirm SPO-shaped fact rows and an entity list are populated after a few captures.
- [ ] **5.4 Update / edit a memory.** Via MCP `nexus_memory_update` (or shell edit UX from C35): edit a captured memory's body; confirm the change persists and is reflected in subsequent search.
- [ ] **5.5 Forget / delete.** Delete a memory via the shell's forget UX or `nexus_memory_delete`. Confirm it's gone from search — and confirm the deletion **propagates as a sync tombstone** rather than a hard-delete that could resurrect on a later `sync` (C36 finding): run `nexus_memory_sync` (or the sync command) and confirm the deleted item does not come back.
- [ ] **5.6 Capture opt-out (config).** Toggle the passive-capture config block off (C37 finding — governs ambient bus-event capture). Perform an action that would normally be auto-captured; confirm nothing new appears in memory while the toggle is off; re-enable and confirm capture resumes.
- [ ] **5.7 Vitality / decay.** "Memory: Vitality" — confirm an ACT-R-style vitality/decay score is shown per memory (older/less-accessed memories score lower).
- [ ] **5.8 Consolidate.** "Memory: Consolidate (dedupe)" after creating two near-duplicate captures. Confirm they merge into one.
- [ ] **5.9 Wiki compile.** "Memory: Compile Wiki Page" on a topic with several related memories. Confirm an LLM-synthesized wiki page is produced (requires AI provider).
- [ ] **5.10 Export / import.** `nexus_memory_export` (MCP) or shell export command; confirm a portable export file is produced, then re-import into a fresh forge and confirm parity.
- [ ] **5.11 Cross-instance sync (optional, needs `nexus-memory-hub`).** If you stand up the standalone `nexus-memory-hub` binary, run `nexus_memory_sync`/`nexus_memory_vector_sync` against it and confirm two instances converge.

---

## 6. AI

*Domain: provider abstraction, streaming chat, RAG, embeddings, inline edit prediction, docs generation, runtime scheduler.*

**Setup:** complete 0.3 first — every case below needs a configured provider (embeddings-only suffices for 6.5/6.6).

- [ ] **6.1 Status & config.** `nexus ai status` (confirm provider/model detected), `nexus ai config` (view/set — gated by `ai.config.write`, a HIGH-risk cap the CLI already holds).
- [ ] **6.2 Ask (RAG with citations).** `nexus ai ask "What did I write in my first note?"`. Confirm an answer citing `notes/hello.md` with `[1]`-style citation markers.
- [ ] **6.3 Embed.** `nexus ai embed` (whole forge) and `nexus ai embed --file notes/hello.md` (single file). Confirm it completes and subsequent semantic search (3.3) returns fresh results.
- [ ] **6.4 Chat REPL.** `nexus ai chat`. Send a message, confirm streamed tokens print live. Try `/model`, `/context notes/hello.md`, `/save /tmp/chat.md`, `/clear`, then `/quit`. Confirm Ctrl+C separately exits with code 130.
- [ ] **6.5 Complete (headless single-shot).** `nexus ai complete notes/hello.md --line 3 --col 0`. Confirm a completion suggestion is returned without entering the REPL.
- [ ] **6.6 Semantic search parity.** Covered in 3.3/3.5/3.7 — confirm all three surfaces (CLI flag, shell command, MCP tool) return consistent results for the same query.
- [ ] **6.7 Shell AI chat panel.** Enable `nexus.ai` (default-off) + configure via **Settings → AI** (`aiSettings` — works even with the chat panel disabled). Open with `Ctrl+Alt+A`/`Ctrl+I`; ask a question, confirm streamed response; try "Ask AI about current context…" and "Clear Chat".
- [ ] **6.8 Inline edit prediction.** In the shell editor, place the cursor mid-note and press `Mod+Shift+Space` ("AI: Complete at cursor", FIM completion). Confirm an inline ghost-text suggestion appears, accept/reject it.
- [ ] **6.9 Enrich (ambient + forced).** Enable the `enrich` plugin (default-off). Run "Force enrich current file" on a note; confirm tags/summary/frontmatter fields populate. Wait/observe ambient enrichment on a newly created note if configured to run automatically.
- [ ] **6.10 Privacy / injection policy.** If exposed in `ai.toml`/shell AI settings, toggle `PrivacyPolicy` (`RedactPii`) and `InjectionPolicy` (`OnDemand`); send a chat message containing an obvious PII pattern (e.g. an email address) and confirm redaction behavior changes accordingly.
- [ ] **6.11 TLS pinning (opt-in, BL-102).** Set `tls_pinning_enabled = true` in `ai.toml` (or `NEXUS_TLS_PINNING=1`). Confirm AI calls still succeed against the real provider endpoint (pin matches); this is a smoke check, not a MITM test.
- [ ] **6.12 AI runtime scheduler.** Kick off a long-running AI task (e.g. a big `ai embed` on a large forge) and, via MCP or a future CLI verb, exercise `submit`/`get`/`list`/`pool_stats`/`cancel` on `com.nexus.ai.runtime` if reachable from your test surface. Confirm a submitted task is listed and cancelable mid-flight.
- [ ] **6.13 Docs generation.** If exposed, run `generate_docs` (BL-116) against a code file/module; confirm generated documentation text is produced.

---

## 7. Agents

*Domain: archetypes, plan/execute, stepwise approval, tools, transcripts, session forking.*

- [ ] **7.1 Plan (no execution).** `nexus agent plan "Summarize my forge notes" --archetype researcher`. Confirm a numbered `Plan` with steps is printed, nothing executed.
- [ ] **7.2 Run (non-interactive, auto-approve).** `nexus agent run "Create a note called agent-test.md with a haiku about forges"`. Confirm the transcript shows rounds, tool calls (✓/✗/·), and the note actually gets created.
- [ ] **7.3 Run (interactive approval).** `nexus agent run "Delete notes/hello.md" --interactive`. Confirm you're prompted `y/N` on stderr before the destructive tool call executes; reject it and confirm the file survives.
- [ ] **7.4 Custom archetypes.** `nexus agent list-custom`. If none exist, author a minimal `.forge/agents/<slug>/agent.toml` and confirm it now appears.
- [ ] **7.5 Session tree — list/show.** `nexus agent sessions` (confirm `↳` fork markers on any forked session), `nexus agent show <id>` (full transcript).
- [ ] **7.6 Resume / branch / rewind.** `nexus agent resume <id> "follow-up message"`, `nexus agent branch <id> <round> "alt path"`, `nexus agent rewind <id> <round> [msg]`. Confirm each produces a new session forked at the right point, non-destructively (`rewind` should redo without deleting the original branch).
- [ ] **7.7 Checkpoints.** `nexus agent checkpoint <id> <round> "milestone-1"`, `nexus agent checkpoints` (list), `nexus agent checkpoint-rm milestone-1`. Confirm CRUD works.
- [ ] **7.8 Tool catalogue.** `nexus tool list`, `nexus tool list --capability fs.write`. Confirm the capability filter narrows the list.
- [ ] **7.9 Notification on long run.** `nexus agent run "<something slow>" --notify-after-secs 5`. Confirm a desktop notification fires if the run exceeds 5s (needs 0.4's desktop channel, which needs no config).
- [ ] **7.10 Shell agent panel.** Enable `agent` (default-off). Activity bar → Agent, give it a task; confirm plan + tool-call transcript executes live in-panel.
- [ ] **7.11 Shell session tree UI.** Enable `sessions` ("Session Tree", default-off). Run an agent session, open Session Tree, branch from a checkpoint; confirm a new session node forks visually from that point.
- [ ] **7.12 TUI agent panel.** In the TUI, press `g` (with focus not on Viewer) to open the Agent panel, submit a goal. Confirm the approval **modal** intercepts `y`/`n`/Enter/Esc for any destructive round, and auto-rejects after 30 min idle if left pending.
- [ ] **7.13 MCP agent tools.** Via an MCP client: `nexus_agent_run` (confirm it always auto-approves — no interactive channel exists over MCP) and `nexus_agent_sessions` (list). Confirm the extended IPC timeout lets a multi-round session complete without a client-side timeout.

---

## 8. Skills

- [ ] **8.1 List & seeded built-ins.** `nexus skill list`. Confirm the built-in seed (code-reviewer, daily-journal, meeting-notes, commit-message) is present.
- [ ] **8.2 Show / render.** `nexus skill show code-reviewer`, `nexus skill render code-reviewer --param file=notes/hello.md` (or whatever params it declares). Confirm parameter substitution works.
- [ ] **8.3 Context-aware listing.** `nexus skill context <ctx>`, `nexus skill triggered "some trigger text"`. Confirm context-sensitive filtering.
- [ ] **8.4 Reload.** Add a new `.skill.md` under `$FORGE/skills/` by hand, `nexus skill reload`, confirm it now appears in `skill list`.
- [ ] **8.5 Shell skills panel.** Enable `skills` (default-off). Browse the list, invoke one from the command palette against the active note; confirm it runs against the current context.
- [ ] **8.6 MCP skill tools.** `nexus_list_skills`, `nexus_render_skill` via an MCP client. Confirm parity with the CLI output.

---

## 9. Workflows & automation

- [ ] **9.1 List / show / validate.** `nexus workflow list`, `nexus workflow show <name>`, `nexus workflow validate <file>`. Author a minimal `.workflow.toml` (cron or manual trigger, one `ipc_call` action step) first if none exist.
- [ ] **9.2 Run on demand.** `nexus workflow run <name>` (or the top-level alias `nexus run <name>`). Confirm every step executes in order and `run_history` records it.
- [ ] **9.3 Cron trigger.** Author a workflow with a short-interval cron trigger. **[KNOWN GAP — historical]** Prior to the C76 headless-daemon fix, cron/file/git/mcp triggers only armed inside the desktop shell; confirm they now also arm under: (a) `nexus daemon` (new headless verb — build_cli_runtime inside a live tokio runtime, blocking until signal), (b) the desktop shell, and (c) the TUI (armed as of commit `4113b04`). Leave each running past the interval and confirm the trigger actually fires in all three.
- [ ] **9.4 File-event trigger.** Author a `file_event`-triggered workflow watching a path; touch/edit that file; confirm the action step fires.
- [ ] **9.5 Templates within workflows.** `nexus workflow template list|show|init`. Confirm a workflow can be scaffolded from a template.
- [ ] **9.6 Digest pipeline.** If a digest-producing workflow exists, trigger it (cron or manual) and confirm a report/digest is assembled and (if routed to notifications) delivered.
- [ ] **9.7 Capability-aggregation audit awareness.** While `workflow run` executes a multi-step workflow touching several plugins, check `nexus logs tail` / audit log for the tracing warning that lists implied caller capabilities (issue #77 "laundering surface" — each step is still capability-checked individually; there is no workflow-level cap ceiling). This is documented behavior to be aware of, not a defect to fix.
- [ ] **9.8 Shell workflow panel.** Enable `workflow` (default-off). Author/run a workflow from the panel UI; confirm results surface step-by-step.
- [ ] **9.9 MCP workflow tools.** `nexus_workflow_list`, `nexus_workflow_run` via an MCP client. Confirm the extended timeout and that per-step capability gating still applies exactly as it does from the CLI.

---

## 10. Structured content — Bases & Canvas

- [ ] **10.1 Create a base.** `nexus bases create people.bases --schema '{"name":"Title","role":"Select"}'` (or shell "New base…"). Confirm the `.bases` file is created with the declared property types.
- [ ] **10.2 Add records / query.** `nexus bases add-record people.bases --data '{"name":"Ada"}'`, `nexus bases query people.bases`. Confirm CRUD round-trips.
- [ ] **10.3 Views (shell).** Open the base in the shell; switch between Table, Kanban (drag a card between columns), Calendar (month grid), Gallery. Confirm each view renders and Kanban drag actually mutates the underlying `Select` property.
- [ ] **10.4 Filters/sort/group.** Apply a filter operator + multi-level sort + grouping in the Table view. Confirm the 14 filter operators behave (spot-check 3-4: equals, contains, is-empty, date-before).
- [ ] **10.5 Formulas & rollups.** Add a formula property referencing another field; add a rollup aggregating a related base's records. Confirm both compute correctly and update live on edit.
- [ ] **10.6 CSV import/export.** `nexus bases import people.bases --csv people.csv`, `nexus bases export people.bases -o out.csv`. Confirm round-trip fidelity.
- [ ] **10.7 Canvas.** Shell: "Canvas: New" or open a `.canvas` file. Add a few nodes + edges, snap-to-grid, undo/redo (`Ctrl+Z`/`Ctrl+Shift+Z`), auto-layout ("Tidy"), export as PNG (confirm max-8192px edge cap doesn't corrupt a large canvas). Embed a terminal node or file preview and confirm the 32KB/64KB content caps are respected (very large embedded file gracefully truncates, doesn't crash).

---

## 11. Editor deep-dive

*(Notes CRUD is covered in section 2 — this section is about the editing surface itself.)*

- [ ] **11.1 Block tree & transactions.** In the shell editor, make several edits (insert/delete/merge/split paragraphs), then `Ctrl+Z`/`Ctrl+Shift+Z` repeatedly. Confirm the undo tree is coherent (not just linear undo — branching redo after an undo+new-edit should still be reachable per the block-tree model, if exposed).
- [ ] **11.2 Live/source/reading modes.** Toggle between the three note view modes for the same file. Confirm rendering differs appropriately (raw markdown vs. rendered) and edits in one mode are reflected when switching to another.
- [ ] **11.3 MDX components.** Insert `<Callout>`, `<Alert>`, `<Badge>`, `<Card />` into a note body. Confirm they render as styled components in live/reading mode, not raw JSX text.
- [ ] **11.4 Outline.** Open a note with several heading levels; right-panel **Outline** section; click an entry, confirm cursor/scroll jumps there.
- [ ] **11.5 Pane mode.** Command palette → "Enter Pane Mode" / "Exit Pane Mode". Confirm sidebars hide/reappear around the focused pane.
- [ ] **11.6 Multibuffer / open excerpts.** Trigger "Open All Diagnostics in Multibuffer" (11.7) or an explicit multi-file excerpt view if separately exposed; confirm a combined read-only view assembles excerpts from multiple files correctly.
- [ ] **11.7 LSP actions in-editor.** With an LSP server configured for a code block's language: hover, `F2` rename a symbol, `Ctrl+.` code actions, `Shift+F12` find references, format-on-save. Confirm each round-trips through `com.nexus.lsp`.
- [ ] **11.8 Block-link resolution.** Reference a specific block via a block-ref link syntax; confirm it resolves to that exact block, not just the file.
- [ ] **11.9 Embedded base view.** Embed a `.bases` query inline in a note (`execute_database_view`); confirm it renders a live mini-table.
- [ ] **11.10 CRDT collaboration.** With `[collab] enabled` and two shell clients open on the same forge/file, edit the same block concurrently from both. Confirm the `crdtConflict` toast appears and a pick-a-side resolution is offered (or, per docs, that only the toast is wired yet — confirm actual behavior against that expectation).
- [ ] **11.11 External editor round-trip (TUI).** In the TUI, press `e` on an open file. Confirm it suspends the TUI, opens `$VISUAL`→`$EDITOR`→`vi`, and on save+quit, resumes the TUI with reloaded content from storage.

---

## 12. Terminal & process manager

- [ ] **12.1 CLI PTY run/shell.** `nexus term run "echo hi"`, `nexus term run "sleep 999" --timeout 3` (confirm exit code 124 on timeout), `nexus term shell` (confirm Ctrl+C is handled gracefully via the installed `ctrlc` handler, not a hard crash).
- [ ] **12.2 Saved commands (proc).** `nexus proc add "run tests" --command "cargo test" --shell bash`, `nexus proc list`, `nexus proc show <id>`, `nexus proc reorder`, `nexus proc history`, `nexus proc delete <id>`.
- [ ] **12.3 Shell terminal panel.** Activity bar → Terminal. Open multiple tabs, confirm each is scoped to the forge root cwd. Run a long-running command, confirm ANSI colors/bold/italic render correctly.
- [ ] **12.4 AI suggestions on failure.** In the shell terminal, run a failing `cargo` command (bad syntax) or a not-found npm/ssh/port scenario. Confirm the corresponding built-in suggestion rule fires (5 rules exist: cargo-fail, npm-not-found, command-not-found, ssh-permission-denied, port-in-use).
- [ ] **12.5 Memory monitoring.** Run a memory-heavy command in a terminal session; if RSS monitoring/limits UI is exposed, confirm the rolling history updates and a configured `MemoryLimits` soft/hard cap triggers a warning.
- [ ] **12.6 REPL sessions.** If exposed (BL-142): `repl_start`, send an eval, `repl_stop`, `repl_list`. Confirm a persistent REPL process round-trips evaluations.
- [ ] **12.7 Cross-session search.** With 2+ terminal sessions containing distinct output, use FTS to search across all sessions' output; confirm hits from both.
- [ ] **12.8 Processes panel.** Enable `processes` (default-off), `Ctrl+Shift+Y`. Start an indexing or agent job; confirm it's listed with live status and can be inspected/cancelled from here.
- [ ] **12.9 MCP terminal observability (read-only).** Via MCP: `nexus_terminal_get_screen`, `nexus_terminal_get_scrollback`, `nexus_terminal_get_cursor`, `nexus_terminal_get_cwd`, `nexus_terminal_get_last_exit` against a running shell terminal session. Confirm all five reflect live state.
- [ ] **12.10 TUI terminal panel (line-buffered, not raw PTY).** Press `T`. Type a command into the buffer, `Enter` to flush as one line, `Ctrl+C` sends SIGINT to the child, `Ctrl+D` kills the session, `Esc` hides (session/scrollback survive, reopening `T` resumes). **[KNOWN GAP]** confirm a full-screen program like `vim` launched here does *not* receive live raw keystrokes — this is documented, not a bug.

---

## 13. Git

- [ ] **13.1 Init & info.** `git init $FORGE` (or `nexus forge init` on a git-tracked dir), `nexus git info`, `nexus git status`. Confirm branch/HEAD/dirty state.
- [ ] **13.2 Diff / blame / log.** Edit a tracked file, `nexus git diff`, `nexus git diff notes/hello.md`, `nexus git blame notes/hello.md`, `nexus git log --limit 5`.
- [ ] **13.3 Stage / unstage (whole file + hunks).** `nexus git stage notes/hello.md`, `nexus git unstage notes/hello.md`, then make a multi-hunk edit and `nexus git stage-hunk notes/hello.md 0`, `nexus git unstage-hunk notes/hello.md 0`. Confirm partial-file staging works.
- [ ] **13.4 Commit.** `nexus git commit --message "uvt: test commit"`. Confirm it lands in `git log`.
- [ ] **13.5 Branch CRUD.** `nexus git branch create uvt-branch`, switch, `nexus git branch` (list), delete it after merging/discarding.
- [ ] **13.6 Tag CRUD.** `nexus git tag v-uvt -m "test tag"`, list (`nexus git tag`), `nexus git tag -d v-uvt`.
- [ ] **13.7 Stash.** Make a dirty change, `nexus git stash`, confirm working tree clean, `nexus git stash` pop/list per its subcommand shape, confirm change restored.
- [ ] **13.8 Remotes / fetch / pull (C49 finding).** Add a remote (`git remote add origin <local-bare-repo-path>` works fine for testing without network). `nexus git remotes` (list), `nexus git fetch`, `nexus git pull`. Confirm `fetch` does **not** touch the working tree (only updates remote-tracking refs) while `pull` does.
- [ ] **13.9 Push & sync wrapper.** `nexus git push`, then `nexus sync` (fetch→pull→push convenience wrapper) with `--no-push` to confirm it honors the flag.
- [ ] **13.10 Merge / conflicts / abort.** Create a conflicting merge on purpose (two branches editing the same line), `nexus git merge <branch>`, confirm `nexus git conflicts` lists it, resolve or `nexus git merge --abort` to confirm clean restoration.
- [ ] **13.11 Rebase / cherry-pick / abort.** `nexus git rebase <branch>` and `nexus git cherry-pick <commit>`; test the `--abort` variant of each mid-conflict.
- [ ] **13.12 LFS status.** `nexus git lfs-status` (works whether or not LFS is configured — confirm graceful "no LFS" output if not).
- [ ] **13.13 Auto-commit.** `nexus git auto-commit --enable --interval 60` (writes `app.toml`), dirty a file, wait past the interval, confirm an automatic commit appears. `nexus git auto-commit --disable` to turn it back off.
- [ ] **13.14 SSH passphrase caching.** `nexus git set-passphrase id_ed25519` (prompts, caches in OS keyring), confirm a subsequent push over SSH doesn't re-prompt; `nexus git clear-passphrase id_ed25519` to remove it.
- [ ] **13.15 Shell git panel.** Activity bar → Source Control. Stage a file, write a commit message, commit; switch branches via the picker; confirm the commit log updates live. Status-bar branch/dirty indicator (`gitStatus`) should update in lockstep.
- [ ] **13.16 Editor git actions.** In the editor tab context menu / command palette: "Toggle Inline Git Blame" (gutter annotations appear), "Open Diff for Active File" (diff view against HEAD opens).
- [ ] **13.17 Push button (shell, C49).** After a commit, use the git panel's **Pull** button (new); confirm it derives remote/branch from the current HEAD's upstream and surfaces a conflict banner if the pull can't fast-forward.
- [ ] **13.18 MCP git tools.** `nexus_git_remotes`, `nexus_git_fetch`, `nexus_git_pull` via an MCP client. Confirm parity with 13.8.

---

## 14. Comments

*Domain: block-anchored, persistent, non-destructive review threads (C74 finding gave this CLI/MCP parity).*

- [ ] **14.1 Shell comments.** Select text in the editor, add a comment via the gutter/toolbar. Confirm it's listed in the right-panel **Comments** section and survives closing/reopening the note.
- [ ] **14.2 CLI — full lifecycle.** `nexus comments list notes/hello.md` (empty), `nexus comments create-thread notes/hello.md "needs a rewrite" --block-index 0`, `nexus comments add-reply notes/hello.md <thread-id> "agreed"`, `nexus comments resolve notes/hello.md <thread-id>`, `nexus comments unresolve notes/hello.md <thread-id>`, `nexus comments edit-comment notes/hello.md <thread-id> <comment-id> "edited body"`, `nexus comments delete-comment notes/hello.md <thread-id> <comment-id>`, `nexus comments delete-thread notes/hello.md <thread-id>`. Confirm every verb round-trips and `create-thread`'s block-index anchoring resolves correctly (confirm an out-of-range `--block-index` errors with a clear message instead of crashing).
- [ ] **14.3 File-as-truth sidecar.** Confirm comments live at `$FORGE/.forge/comments/notes/hello.md.json` (JSON sidecar), not embedded in the note body — editing the note body should not disturb existing comment threads.
- [ ] **14.4 MCP comment tools.** Via an MCP client: `nexus_comment_list`, `nexus_comment_create_thread`, `nexus_comment_add_reply`, `nexus_comment_set_resolved`, `nexus_comment_edit_comment`, `nexus_comment_delete_comment`, `nexus_comment_delete_thread`. Confirm `nexus_comment_create_thread`'s internal open→get_tree→stamp_block anchor-resolution chain succeeds without the caller having to manage block ids manually.

---

## 15. Export

- [ ] **15.1 HTML export (CLI).** `nexus content export notes/hello.md` produces markdown; for the **styled HTML** exporter (C66/C67 findings — convention-aware, doesn't mangle wikilinks/callouts), use the shell path or `nexus_export_html` (15.3) since the CLI's plain `export` is markdown-only.
- [ ] **15.2 HTML export (shell).** Right-click the tab → "Export as HTML...". Pick a save location. Open the resulting file in a browser; confirm wikilinks, callouts, task checkboxes, and MDX components render as intended HTML, not raw/mangled markdown syntax.
- [ ] **15.3 HTML export (MCP).** `nexus_export_html` — confirm it returns either inline HTML or a `{written, dest}` reply depending on whether a destination path was supplied.
- [ ] **15.4 PDF export.** Shell: use the browser-print-to-PDF path scoped to the active note's preview only (C65 finding — confirm the print stylesheet hides all shell chrome, sidebars, and other tabs, printing *only* the `#nexus-print-root` content).
- [ ] **15.5 Canvas export.** Covered in 10.7 — PNG export with the 8192px edge cap.
- [ ] **15.6 Notion import/export.** `nexus import notion <path-to-export.zip>` and `nexus export notion <dir>` (or the shell's "Import from Notion zip…" command). Confirm notes + frontmatter + attachment links come through, and the reverse export round-trips.

---

## 16. Protocol hosts — LSP, DAP, ACP + debugger

- [ ] **16.1 LSP host.** Configure a real language server (e.g. `rust-analyzer`) for a code block's language via plugin contribution or config. Exercise: completions, hover, definition, references, code_actions, format, rename, execute_command (mirrors 11.7 from the editor side — this is the host/handler side).
- [ ] **16.2 DAP host — launch/attach.** Configure a debug adapter. Enable `debugger` (default-off), `Ctrl+Shift+D`. Set a breakpoint, launch; confirm execution pauses and the call stack / scopes / variables populate. Exercise continue/next/step-in/step-out/pause, and `evaluate` in the watch panel.
- [ ] **16.3 ACP host + inbound server.** `nexus acp serve` on stdio. From a compatible ACP client, spawn an agent process and exercise propose/accept/reject on the allow-listed `agent/*` verbs (confirm it's a fixed 3-verb allowlist, not the full agent surface).
- [ ] **16.4 Plugin protocol contributions (BL-113).** Install a community plugin declaring a `[protocol_hosts]` manifest block (needs `protocol.host.contribute`, HIGH-risk, invoker-only). Confirm `nexus plugin enable <id>` wires the contribution in and `disable` unwires it cleanly.

---

## 17. MCP — server & host

### 17.1 MCP server (`nexus mcp serve`) — 61 static tools

**Setup:** `nexus mcp serve` on stdio, connected from a real MCP client (Claude Desktop, Claude Code, or any MCP-compatible client) pointed at `$FORGE`.

- [ ] **17.1.1 Notes CRUD.** `nexus_read_note`, `nexus_create_note`, `nexus_update_note`, `nexus_delete_note`, `nexus_list_notes`.
- [ ] **17.1.2 Search & graph.** `nexus_search`, `nexus_semantic_search`, `nexus_backlinks`, `nexus_outgoing_links`, `nexus_graph_status`, `nexus_list_tags`.
- [ ] **17.1.3 Tasks.** `nexus_list_tasks`, `nexus_toggle_task`.
- [ ] **17.1.4 AI / RAG.** `nexus_ask`.
- [ ] **17.1.5 Skills.** `nexus_list_skills`, `nexus_render_skill`.
- [ ] **17.1.6 Code intel (BL-115).** `nexus_context`, `nexus_impact`, `nexus_detect_changes`. Confirm these always report `degraded: true` with a fixed reason (declarations-only index, no call-edge traversal) — expected, not a bug.
- [ ] **17.1.7 Kernel stats.** `nexus_kernel_stats` — read-only BL-093 metrics snapshot.
- [ ] **17.1.8 Export.** `nexus_export_html`.
- [ ] **17.1.9 Git.** `nexus_git_remotes`, `nexus_git_fetch`, `nexus_git_pull`.
- [ ] **17.1.10 Comments.** All 7 `nexus_comment_*` tools (see 14.4).
- [ ] **17.1.11 Agent / workflow.** `nexus_agent_run`, `nexus_agent_sessions`, `nexus_workflow_list`, `nexus_workflow_run` (see 7.13 / 9.9).
- [ ] **17.1.12 Memory.** All ~19 `nexus_memory_*` tools (see section 5).
- [ ] **17.1.13 Terminal observability.** All 5 `nexus_terminal_get_*` tools (see 12.9).
- [ ] **17.1.14 Sandbox.** `nexus_sandbox_policy`, `nexus_sandbox_download`.
- [ ] **17.1.15 Resources.** Confirm forge notes are also browsable as MCP **resources** under `mcp://nexus/notes/<path>` (`list_resources`/`read_resource`) — a second, resource-oriented path to the same content alongside the tool-call path.
- [ ] **17.1.16 Dynamic tool registry (DG-39).** If a plugin calls `register_tool` at runtime, confirm the new tool appears in the client's tool list without a server restart, and that its name can't collide with the reserved `nexus_` prefix.

### 17.2 MCP host (external servers)

- [ ] **17.2.1 Register & connect a real external server.** Write `$FORGE/.forge/mcp.toml` per 0.5 (e.g. the `npx @modelcontextprotocol/server-filesystem` stdio server). `nexus mcp servers` (list configured), `nexus mcp tools <server>` (list its tools), `nexus mcp call <server> <tool> --arguments '{...}'`.
- [ ] **17.2.2 Shell MCP panel.** Enable `mcp` (default-off). Add a server config via the UI, refresh, confirm it shows connected and its tools become agent-invokable.
- [ ] **17.2.3 OAuth flow (if a configured server needs it).** Confirm the 30s auth timeout and that a `bearer`/`api_key`/`oauth_client_credentials` block in `mcp.toml` resolves correctly.

---

## 18. Notifications

**Setup:** desktop channel needs nothing; Discord/Telegram/email need real credentials in `$FORGE/.forge/notifications.toml`:
```toml
[sources.workflow]
on = ["com.nexus.workflow.run_completed"]
route = ["desktop", "discord"]
min_severity = "warn"

[channels.discord]
webhook_url = "https://discord.example/webhook"
```

- [ ] **18.1 Desktop toast (CLI).** `nexus notify send "hello from uvt" --channel desktop`. Confirm an OS toast appears.
- [ ] **18.2 Source-routed send.** `nexus notify send "routed message" --source cli --severity warn`. Confirm it routes per the `[sources.cli]` block (or the default fallback if none configured).
- [ ] **18.3 Discord/Telegram/email.** With real credentials configured (0.4): shell **Settings → Notifications**, enter the webhook/token, click "Send test". Confirm delivery in the real channel within a few seconds.
- [ ] **18.4 Inbox.** Generate a handful of notifications (18.1-18.3, or let an agent run / workflow completion fire one). Shell: **Notification Center** — confirm list, unread badge count, mark-read, dismiss all work and persist across a restart (SQLite-backed, `<forge>/.forge/notifications/inbox.db`, capped at 1000 rows / 30-day retention).
- [ ] **18.5 Routing rules.** Add a second `[sources.*]` block routing a different bus topic to a different channel set; confirm only the matching topic's events reach that channel.

---

## 19. Audio (STT/TTS)

- [ ] **19.1 Backend status.** Command "Audio: Show backend status". Confirm it reports local Whisper vs. OpenAI provider availability.
- [ ] **19.2 Transcribe.** "Audio: Transcribe microphone" (needs the `audio.record` HIGH-risk capability, and a real microphone). Speak a sentence; confirm transcribed text is inserted where expected.
- [ ] **19.3 Synthesize.** "Audio: Speak text…" on a selected passage. Confirm audio playback occurs (TTS-only path only needs `audio.synthesize`, Low — should work even if `audio.record` isn't granted).
- [ ] **19.4 Model download.** If no local model is cached yet, confirm the first STT/TTS use triggers a model download from the configured URL (whisper.cpp ggml from HuggingFace) and caches it locally for subsequent runs.

---

## 20. Theming & appearance

- [ ] **20.1 Bundled themes.** Command palette or `Ctrl+Shift+T` → theme picker. Switch through several of the 11 bundled themes including `nexus-manuscript` (warm sepia). Confirm the whole UI recolors live and persists across a restart.
- [ ] **20.2 Snippet cascade.** Toggle/reorder theme snippets (small CSS-var overrides layered on the base theme). Confirm ordering affects the final computed colors (later snippet wins on conflict).
- [ ] **20.3 Theme Builder.** Open the Build tab in the theme picker. Adjust a color via the live picker, confirm live preview updates, then export as TOML and confirm the file is well-formed and re-importable.
- [ ] **20.4 Plugin-contributed theme overrides.** If a community plugin ships theme overrides (`set_plugin_overrides`), confirm they layer correctly without needing a restart.
- [ ] **20.5 Zoom.** `Ctrl+=`/`Ctrl+-` repeatedly, confirm UI scales and clamps at sane bounds; `Ctrl+0` resets to 100%.
- [ ] **20.6 Forge-local persistence.** Switch themes, restart the shell pointed at the same forge; confirm the theme choice survived (stored under `<forge>/.forge/`, not global app state) — open a *different* forge and confirm it does **not** inherit the first forge's theme choice unless explicitly configured to.

---

## 21. Templates

- [ ] **21.1 CLI.** `nexus template list`, `nexus template apply <name> --arg title="My Page" --target notes/from-template.md`. Confirm parameter substitution fills correctly; re-run with `--overwrite` vs. without and confirm the safety behavior differs (should refuse to clobber without the flag); `--dry-run` should report without writing.
- [ ] **21.2 Shell.** Activity bar "Templates" icon, or command "New from template…". Pick a template, fill parameters, confirm the created note lands in the chosen folder.
- [ ] **21.3 Built-ins present.** Confirm the bundled built-in templates are listed alongside any user templates under `$FORGE/.forge/templates/`.

---

## 22. Plugin system

- [ ] **22.1 Scaffold — script (default).** `nexus plugin scaffold --template script --id com.uvt.hello --name "UVT Hello" --author "Tester" -o /tmp/uvt-hello`. Confirm `plugin.json`/`index.ts`/`package.json`/`tsconfig.json`/`README.md` are emitted.
- [ ] **22.2 Scaffold — core/community (WASM).** `nexus plugin scaffold --template community --id com.uvt.wasm --name "UVT Wasm" -o /tmp/uvt-wasm`. Confirm a WASM Cargo project layout is emitted.
- [ ] **22.3 Install & capability grant prompt.** `nexus plugin install /tmp/uvt-hello`, or via shell **Plugin Manager** (`Ctrl+Shift+X`) → install from folder. This is the one place you should actually see the **capability grant modal** — confirm it lists required vs. optional capabilities with HIGH-risk ones (fs.*.external, net.http, process.spawn, ipc.call, ai.config.write, audio.record, protocol.host.contribute, security.write, security.audit.write, network.bind) visually distinguished. Deny an optional one and confirm the plugin still activates in a degraded mode; grant it and confirm the behavior it gates becomes available.
- [ ] **22.4 List / enable / disable.** `nexus plugin list`, `nexus plugin list --shell`, `nexus plugin disable com.uvt.hello`, `nexus plugin enable com.uvt.hello` (confirm DAP/LSP/MCP/ACP contribution wiring, if any, toggles with it).
- [ ] **22.5 Call directly.** `nexus plugin call com.uvt.hello <command> --args '{}'`. Confirm the community plugin's IPC handler responds.
- [ ] **22.6 Revoke a capability live.** `nexus plugin revoke com.uvt.hello <capability>`. Confirm the plugin loses that capability immediately (both live and in the persisted `granted_caps.json`), and that a subsequent call needing it now fails cleanly instead of crashing the plugin or the kernel.
- [ ] **22.7 Reset from crash quarantine.** Force the plugin to crash 3 times in a row (e.g. make its handler panic); confirm it's auto-quarantined after the 3rd strike, then `nexus plugin reset com.uvt.hello` clears the quarantine and it loads again.
- [ ] **22.8 Verify signature.** `nexus plugin verify /tmp/uvt-hello --keys-dir ~/.nexus/keys` against an unsigned scaffold (expect a clear failure) and, if you sign it with a real key, against a signed one (expect success).
- [ ] **22.9 Hot reload (dev mode, C80 finding).** Edit `/tmp/uvt-hello/index.ts` while the plugin is loaded (dev mode). Confirm the watcher (`notify-debouncer-mini`) picks up the change and reloads without a full app restart; confirm a bad edit triggers rollback rather than leaving the plugin half-loaded.
- [ ] **22.10 Marketplace stub.** `nexus plugin install some-marketplace-id` (bare id, no local dir). Confirm it exits with code 2 and a clear "not yet implemented" message (Phase-5 stub, documented, not a bug).
- [ ] **22.11 Uninstall / remove.** `nexus plugin uninstall com.uvt.hello`, and separately `nexus plugin remove <shell-plugin-id> -y` for a shell-only plugin dir. Confirm cleanup.
- [ ] **22.12 Settings.** `nexus plugin settings com.uvt.hello --set '{"key":"value"}'`. Confirm per-plugin settings persist and are readable back.
- [ ] **22.13 mermaid community plugin.** Enable `community.mermaid` (default-off) via Plugins manager. Write a ` ```mermaid ` flowchart fence in a note, view in reading mode; confirm it renders as an SVG diagram, and "View Source" toggles back to text on a `.mermaid` file.
- [ ] **22.14 Sandboxed plugin HTTP/download (needs `sandbox.toml` edit).** By default the WASM/script sandbox is closed (no HTTP, no downloads, per 0.6). Edit `$FORGE/.forge/sandbox.toml`'s `[http]`/`[downloads]` blocks to allow a specific domain, then exercise a plugin calling `platform.net.request`/a brokered download via the **Sandbox** panel commands ("Sandbox: Show Policy", "Sandbox: Brokered Download"). Confirm it's denied before the edit and succeeds after, always going through the broker rather than a direct network call.

---

## 23. Security, capabilities & audit

- [ ] **23.1 Credential vault.** Store a secret for one plugin (e.g. via `set-passphrase`, 13.14, or a plugin's own credential flow); confirm a *different* plugin cannot read it (namespaced by `{plugin_id}:{name}` in the OS keyring).
- [ ] **23.2 Audit log — read.** `nexus logs list` / `nexus logs export` (persisted audit log via `com.nexus.security`). Confirm every capability-gated call you made in earlier sections shows up with plugin id, capability, and outcome (granted/denied).
- [ ] **23.3 Audit log — retention & clear.** Confirm the 90-day retention note in docs matches what you can observe (old rows aging out is not practically testable in one session, but confirm `nexus logs clear` truncates on demand and requires `security.audit.write`, HIGH-risk).
- [ ] **23.4 Capability denial path.** Force a capability-gated call to fail (e.g. attempt a plugin action after `plugin revoke`d the needed capability, 22.6). Confirm the denial is itself audited (not silently swallowed) and surfaces a clear error to the caller.
- [ ] **23.5 Path validation (traversal/TOCTOU).** Attempt `nexus content read ../outside-forge.md` or similar path-escaping input. Confirm it's rejected with a clear "invalid relpath"-style error, not a successful read outside the forge root.
- [ ] **23.6 Cap matrix completeness (developer-facing, optional).** If you have a dev environment: `cargo test -p nexus-bootstrap` and confirm `cap_matrix_complete` / `dep_invariants` tests pass — every IPC handler is classified and the kernel never depends on a subsystem crate.
- [ ] **23.7 TUI kernel-stats modal.** In the TUI, `Shift+K`. Confirm live queue depth, top-10 IPC calls, top-10 bus publishes, and top-10 capability checks (denials rendered in red) reflect the actions you've taken this session.

---

## 24. Collaboration

**Setup:** set `[collab] enabled = true` in `$FORGE/.forge/config.toml`; you'll want two shell instances (two machines, or two profiles) pointed at the same forge/relay.

- [ ] **24.1 Relay serve.** `nexus collab serve --port 7700` (binds `0.0.0.0`, needs `network.bind`, HIGH-risk). Confirm it starts and reports listening.
- [ ] **24.2 Join from a second client.** `nexus collab join ws://<host>:7700 --peer-id tester2 --display-name "Tester Two"` (or the shell's Collaboration panel "Focus Collaboration Panel"). Confirm the relay shows two connected peers.
- [ ] **24.3 Live presence.** With both clients open on the same note, move the cursor/selection in one; confirm the other sees a live caret/selection indicator with the peer's display name.
- [ ] **24.4 Token management.** `nexus collab token set <value> --save-token` / `nexus collab token clear`. Confirm the token is stored in the OS keyring (`nexus.collab.token`) and a join without a required token is rejected.
- [ ] **24.5 CRDT sync + conflict.** Both clients edit the *same block* concurrently. Confirm the operation-based CRDT (RGA for text) merges non-conflicting keystrokes automatically, and a genuine conflict (e.g. both delete + both insert at the same position) surfaces the `crdtConflict` toast with a manual pick-a-side resolution.
- [ ] **24.6 Git merge driver shim.** Configure the CRDT-aware git merge driver (`nexus crdt install-merge-driver --apply`, `nexus crdt enable-transport`). Create a real git merge conflict on a collaboratively-edited file across two branches; confirm the shim resolves it CRDT-aware rather than leaving raw `<<<<<<<` conflict markers.
- [ ] **24.7 Stop relay.** `relay_status` then stop; confirm both clients gracefully detect the disconnect (no crash, a clear "disconnected" state in the panel).

---

## 25. Shell observability panels

*(Shell-only diagnostic/meta panels — no CLI/MCP equivalent for most of these.)*

- [ ] **25.1 Kernel Health.** Enable `healthPanel` (default-off), "Show Kernel Health". Perform a batch of actions across several domains; confirm IPC counts, p50/p95/p99 latency, capability denials, and event-bus queue depth update live (polls every 5s).
- [ ] **25.2 Diagnostics.** Enable `diagnostics` (default-off). Open a file with LSP-reported lint/type errors (needs 16.1 configured); confirm errors group by file/severity with click-to-jump, and "Open All Diagnostics in Multibuffer" assembles a combined excerpt view.
- [ ] **25.3 Activity Timeline.** Enable `activityTimeline`. Perform a save, a git commit, and an AI call in sequence; confirm each appears as a timeline entry in chronological order. "Clear Activity Timeline" empties it.
- [ ] **25.4 Architecture panel (os-template forges only).** In a forge created with `--template os` (1.1), open the **Architecture** panel (default-off). Confirm it renders `architecture.md` as a domain→task hierarchy and flags any task whose referenced skill/workflow is missing.
- [ ] **25.5 Observability panel (os-template).** Open **Observability** (default-off), 3 tabs: AI usage rollup (reflects section 6's calls), foundation-workflow status (manual "run" button — confirm it updates the status tab), vault activity feed.
- [ ] **25.6 View Builder — save/restore layouts.** Enable `viewBuilder` (default-off). Arrange panes into a specific layout, "Save Layout As…" a name, rearrange everything, then "Switch Layout" back. Confirm the exact pane arrangement is restored.

---

## 26. TUI — full pass

*(Consolidates every TUI-specific check; run this as one continuous session after the domain sections above have validated the underlying capabilities.)*

- [ ] **26.1 Launch correctly.** `NEXUS_FORGE_PATH=$FORGE nexus tui` (see 1.8's known gap about `--forge-path` not forwarding).
- [ ] **26.2 File tree navigation.** `j`/`k`/Down/Up to move, Enter/`l`/Right to open a file or expand a dir, `h`/Left to collapse. Tab to flip focus to the viewer (border color changes).
- [ ] **26.3 Viewer scrolling.** `j`/`k`, `g`/Home (top), `G`/End (bottom), Ctrl+D/PageDown, Ctrl+U/PageUp — all on a file long enough to scroll.
- [ ] **26.4 Search overlay.** `Ctrl+F`, type a query, confirm the file tree *live-filters* to matching names while typing, Up/Down to move the highlighted result, Enter opens the top hit and returns focus to the viewer, Esc cancels.
- [ ] **26.5 Find bar.** `/` on an open file, type a substring, Enter/`n` next match (wraps), `N` previous match (wraps), Esc clears and closes.
- [ ] **26.6 Task list.** `t` toggles a read-only task list pane with `(pending, total)` counts in the title. Confirm there's genuinely no per-task navigation/toggle key (documented gap, not a bug).
- [ ] **26.7 Backlinks panel.** `b` toggles a bottom 30% panel under the viewer listing files that link to the open file.
- [ ] **26.8 Terminal panel.** `T`, exercise per 12.10.
- [ ] **26.9 AI panel.** `a`, exercise per 6.7's TUI equivalent — confirm the transcript auto-pins to bottom (no manual scrollback while active) and renders as plain text (not markdown) — both documented gaps.
- [ ] **26.10 Agent panel + approval modal.** `g` (focus not on Viewer — separately confirm `g` on Viewer focus instead jumps to top, the intentional dual-binding), exercise per 7.12.
- [ ] **26.11 Kernel-stats modal.** `Shift+K`, exercise per 23.7.
- [ ] **26.12 Git status badge.** Confirm a read-only branch+dirty indicator is visible somewhere in the chrome, and that there is **no** git write UI anywhere in the TUI (commit/push/branch/merge are CLI/shell-only from this frontend).
- [ ] **26.13 External editor round-trip.** `e` on an open file, per 11.11.
- [ ] **26.14 Dead help hint.** Confirm the status bar's `Ctrl+? help` text does nothing when pressed (documented dead hint).
- [ ] **26.15 No live file-watcher reload.** Edit a forge file from a second terminal while the TUI sits open on it; confirm the tree/viewer do **not** update until you reopen the file or restart — then reopen and confirm it *does* pick up the change.
- [ ] **26.16 Clean exit.** `q` or Ctrl+C from Normal mode. Confirm terminal fully restores (non-alt screen, cursor, colors) with no artifacts, and that an open terminal-panel PTY session is left running server-side rather than force-killed.

---

## 27. Remote forge & multi-frontend parity

- [ ] **27.1 SSH remote forge.** On a second host (or a local SSH-accessible container) with Nexus installed, run any CLI command from your primary machine against `nexus --forge-path ssh://user@host/path/to/forge ...` — e.g. `content list`. Confirm it transparently spawns `nexus serve --stdio` on the remote and routes IPC over the SSH transport.
- [ ] **27.2 Reconnect-on-drop.** Mid-session, kill the SSH connection (e.g. restart sshd or drop the network briefly) and issue another remote command. Confirm the `ReconnectingRuntime` transparently re-spawns the remote server rather than requiring you to restart the local CLI.
- [ ] **27.3 `nexus serve --stdio` directly.** `nexus serve --stdio` piped to a raw JSON-RPC client (or the remote transport's own client library). Confirm the whole kernel IPC + event-bus surface is reachable, and that `nexus serve` **without** `--stdio` errors clearly (no WebSocket/unix-socket transport exists yet, despite help text mentioning future flags — documented gap).
- [ ] **27.4 Cross-frontend consistency spot-check.** Pick 3 capabilities you've already tested via the CLI (e.g. create a note, run a search, toggle a task) and confirm the *same* forge state is visible immediately from the shell and the TUI without a manual reindex — i.e., the file watcher / index stay in sync across all frontends touching one forge concurrently.
- [ ] **27.5 `nexus watch`.** `nexus watch` in one terminal; touch/edit a file from another process; confirm filesystem-change events print live (subscribes to `com.nexus.storage.*`).
- [ ] **27.6 Shell completions.** `nexus completions bash > /tmp/nexus.bash` (and zsh/fish if you use them); source it; confirm tab-completion works for subcommands.
- [ ] **27.7 External community-plugin CLI subcommand.** If a community plugin registers a `[[registrations.cli_subcommand]]`, confirm `nexus <that-subcommand>` dispatches to it, and that an *unregistered* subcommand name prints an ANSI-stripped list of what *is* available rather than a raw/garbled error.

---

## 28. Cross-platform smoke notes

*(Not exhaustive per-OS testing — a light sanity pass if you have access to more than one OS.)*

- [ ] **28.1 Process spawn / kill semantics.** Terminal sessions and `term run --timeout` should clean up child processes correctly on your OS — process-group kill on Linux/macOS, Job Objects (`KILL_ON_JOB_CLOSE`) on Windows. Spawn a process tree (a shell script that itself spawns children) and confirm timeout/kill takes the whole tree down, not just the immediate child.
- [ ] **28.2 Shell detection.** `nexus term env`. Confirm it correctly reports your actual default shell (bash/zsh/fish/sh/cmd/pwsh).
- [ ] **28.3 WSLg (if applicable).** On WSL2, confirm `pnpm --filter nexus-shell tauri:dev` launches correctly with the baked-in `WEBKIT_DISABLE_*`/`GDK_BACKEND=x11` env.

---

## 29. Cleanup & teardown

- [ ] **29.1 Stop all long-running sessions.** Close the shell, exit the TUI (`q`), stop any `nexus mcp serve`/`nexus acp serve`/`nexus serve --stdio`/`nexus daemon`/`nexus collab serve` processes, stop the memory-hub if you started one.
- [ ] **29.2 Confirm forge lock releases.** After everything above is stopped, confirm `$FORGE/.forge/.lock` is releasable — a fresh `nexus --forge-path $FORGE forge status` should succeed immediately (no stale-lock error) as the first command of a new session.
- [ ] **29.3 Trash emptied vs. kept.** Decide whether to empty `$FORGE/.trash/` or keep it for a later recovery-testing pass.
- [ ] **29.4 Revoke test credentials.** Remove/rotate any real webhook URLs, bot tokens, or SMTP credentials you put in `notifications.toml`/`ai.toml`/`mcp.toml` for this pass.
- [ ] **29.5 Discard the scratch forge.** `rm -rf $FORGE` once you're satisfied — everything under it (including `.forge/`, `.trash/`, git history) is disposable test data.

---

## Appendix A — quick reference: capability risk tiers

Use this if any test case above surfaces an unexpected grant prompt or denial. **HIGH-risk** (surfaced prominently at community-plugin install, 22.3): `fs.read.external` / `fs.write.external`, `net.http`, `network.bind`, `process.spawn`, `ipc.call`, `ai.config.write`, `audio.record`, `protocol.host.contribute`, `security.write`, `security.audit.write`. Everything else (fs.read/write within-forge, kv.*, ai.chat/index/session.*, notifications.inbox.*, ui.notify, ai.runtime.observe, security.audit.read) is Low/Medium. First-party frontends (CLI/TUI/shell) never show you this prompt — only community/WASM/script plugins do.

## Appendix B — sign-off

| Section | Tester | Date | Pass/Fail | Notes |
|---|---|---|---|---|
| 0–1 Setup & first boot | | | | |
| 2 Notes & content | | | | |
| 3 Search | | | | |
| 4 Knowledge graph | | | | |
| 5 Memory | | | | |
| 6 AI | | | | |
| 7 Agents | | | | |
| 8 Skills | | | | |
| 9 Workflows | | | | |
| 10 Bases & Canvas | | | | |
| 11 Editor | | | | |
| 12 Terminal | | | | |
| 13 Git | | | | |
| 14 Comments | | | | |
| 15 Export | | | | |
| 16 Protocol hosts | | | | |
| 17 MCP | | | | |
| 18 Notifications | | | | |
| 19 Audio | | | | |
| 20 Theming | | | | |
| 21 Templates | | | | |
| 22 Plugin system | | | | |
| 23 Security | | | | |
| 24 Collaboration | | | | |
| 25 Observability panels | | | | |
| 26 TUI | | | | |
| 27 Remote / multi-frontend | | | | |
| 28 Cross-platform | | | | |
| 29 Cleanup | | | | |
