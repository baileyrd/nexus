# AI Integration Gaps

> Tracker for "where AI *could* go deeper" — concrete, scoped follow-ups identified by the AI integration analysis on 2026-05-05. Same format as [OPEN-ITEMS.md](OPEN-ITEMS.md). Each item has an ID (`AIG-NN`), severity, surfacing evidence, and a clear definition of done.
>
> Cross-references: [AI-INTERACTION-SURFACE-AUDIT.md](AI-INTERACTION-SURFACE-AUDIT.md), [PRDs/12-ai-engine.md](PRDs/12-ai-engine.md), [PRDs/13-skills.md](PRDs/13-skills.md), [PRDs/15-agent-system.md](PRDs/15-agent-system.md), [PRDs/16-workflow-system.md](PRDs/16-workflow-system.md).

---

## AIG-01 — Skill composition / dependency resolution

**Severity:** Should-fix (PRD-13 §5 open)
**Surfaced by:** `crates/nexus-skills/src/lib.rs` — `Skill::depends_on` is parsed and stored, but never resolved when a skill is rendered or activated.
**Status:** Resolved 2026-05-05. Backend resolver was already shipped as **BL-021** (separate `compose` IPC handler rather than overloading `render`); the agent already prefers the composed body. Remaining work was the Skills-panel surface, which this changeset added.

### Outcome
- Confirmed BL-021 already covered the backend (`crates/nexus-skills/src/compose.rs`, 519 LoC): iterative DFS with white/gray/black colouring for cycle detection (`ComposeError::Cycle` carries the offending path), Kahn-style topo sort with `depends_on` declaration order as the tiebreaker, and three error variants (`UnknownRoot`, `MissingDependency`, `Cycle`). Tests in-place for linear chain, diamond, self-cycle, longer cycle, and missing dependency. `com.nexus.skills::compose` (handler id 7) returns `ComposedSkill { root_id, fragments, merged_body, conflicts }`. Agent integration at `crates/nexus-agent/src/core_plugin.rs:885` calls `compose_skill_body` and prefers the resolved body, falling back to the verbatim body on cycle/missing-dep so a broken DAG never wedges planning.
- **Wire types** mirrored in `skillsStore.ts`: `ComposedFragment`, `ComposeConflict` (tagged-union: `parameter_clash` and `restrictions_disagree`), `ComposeResult { rootId, fragments, mergedBody, conflicts }`. Unknown conflict variants are silently dropped at decode so a future kernel-side addition doesn't crash older shells.
- **Store actions**: `composeSkill(api, id)` (single-flight per id; clears stale errors on success and stale results on failure), `toggleComposePanel(api, id)` (opens the panel and triggers a one-shot fetch on first open per session — closing leaves the cache intact so reopen is instant), `clearCompose(id)` (drops both cached result and error).
- **Inline panel** in `SkillsView.tsx`: shown beneath the action row of an expanded skill when `dependsOn.length > 0`. Renders the resolver output as an ordered list (deepest dep first, root labelled with a "root" pill and bolder text) plus a non-fatal conflict block when ancestors disagree on parameters or restrictions, plus a collapsible `<details>` showing the merged body the kernel will hand to the model. Cycle / missing-dep failures from the kernel surface as a red-bordered alert pre. Loading state shows "Resolving `<id>`…" while the IPC is in flight.
- **6 new tests** in `skillsStore.test.ts` — successful compose with cache + stale-error clear; cycle-error replaces stale results; malformed-payload decoder failure; `parameter_clash` conflict round-trip with unknown-kind drop; `toggleComposePanel` open/close/cache; `clearCompose`. Full shell suite: 829/829 pass; typecheck clean; no new lint errors.

### Follow-up (not blocking)
- The DOD originally said "`render` resolves `depends_on`", but in-tree the backend factored composition into a separate `compose` handler — render handles parameter substitution, compose handles dependency layering. That's the better factoring (orthogonal concerns) and the agent uses both; the spec line was speculative.
- Word-level diff inside the merged body would help users spot which ancestor contributed each line; today the merged body is printed verbatim.
- Conflicts are non-fatal warnings only — there's no "auto-resolve" affordance. If clashing parameter defaults become a real pain we can add a pin-this-version chip.

---

## AIG-02 — Agent step-approval UI

**Severity:** Should-fix (safety-critical; half-built)
**Surfaced by:** `crates/nexus-agent/` — `StepPolicy` slot reserved; shell `nexus.agent` plugin routes approvals manually with no native confirm dialog.
**Status:** Resolved 2026-05-05. Implemented shell-side; the kernel `StepPolicy` slot stays reserved for a future Rust-side policy implementation.

### Outcome
- **Risk classifier** (`shell/src/plugins/nexus/agent/riskClassifier.ts`): pure function mapping `(target_plugin_id, command_id)` → `'safe' | 'write' | 'exec' | 'network'`. Conservative default — unknown plugins fall through to `write`. Storage reads / git log / AI / skills metadata classified as `safe`; storage writes / commits as `write`; terminal/processes/workflow runs as `exec`; git push/pull/fetch and MCP host calls as `network`.
- **`StepPolicy` enum** in `sessionStore.ts`: `'always_ask' | 'ask_on_risky' | 'auto_approve'` with `DEFAULT_STEP_POLICY = 'ask_on_risky'`. Lives shell-side because the shell *is* the policy decision-maker via the kernel's `BusBridgePolicy` — no IPC/wire-format change needed.
- **Auto-decide in `agentRuntime.handleTopic`**: when a `round_proposed` arrives, the policy is consulted; `auto_approve` short-circuits to `submitDecision('approve_all')`, `ask_on_risky` short-circuits when `isRoundEntirelySafe(toolCalls)`, and `always_ask` always surfaces the card. Optimistic transcript append still happens so the run is visible.
- **Composer policy picker** with three options, tooltips explaining when to pick each, disabled while a session is running.
- **Approval-card additions**: per-tool risk badge (colour-coded: green/safe, amber/write, red/exec+network) plus a left-border accent on the row matching the highest-risk colour. Three buttons: **Approve** (or "Approve selected" when not all are checked), **Approve & continue** (flips policy to `auto_approve` for the rest of the session, then submits the current round), **Reject** (opens the existing reason form).
- **Diff preview for `write_file`**: when a tool call's target is `com.nexus.storage::write_file` and args expose `path: string + contents: string`, the row replaces the raw-JSON preview with a unified line diff against the current on-disk contents (fetched via a new `runtime.readFile()` helper that calls `com.nexus.storage::read_file`). Implemented as an LCS-based whole-line diff in `diffPreview.ts`, capped at 200 lines with a "diff truncated" footer; degrades to a "new file" preview when the file doesn't yet exist; "no changes" hint when contents are identical.
- **Decision still recorded in session history** through the existing `ToolCallRecord.{approved, reason}` path — no schema change.
- 16 new tests under `aig02.test.ts` (risk classifier, diff helper, auto-decide policy paths, `readFile` happy/error). Full shell suite: 823/823 pass; typecheck clean; no new lint errors.

### Follow-up (not blocking)
- Rust-side `StepPolicy`: a kernel-enforced policy would let headless `nexus agent run` honour the same modes the shell does. Today `nexus agent run --auto-approve true` is the only headless option.
- Diff renderer is whole-line LCS — adequate for forge markdown but could grow word-level highlighting if users complain about replace-blocks reading as remove+add.
- `Approve & continue` is session-scoped (lives in the store's `stepPolicy`) — surviving the session means the picker resets to `ask_on_risky` next run, which is the safer default.

---

## AIG-03 — Workflow file-event and webhook triggers

**Severity:** Nice-to-have (PRD-16 specifies, only cron + manual shipped)
**Surfaced by:** `crates/nexus-workflow/src/triggers/` — only `cron` and `manual` variants implemented.
**Status:** Resolved 2026-05-05. Both triggers were already shipped (BL-028g for webhook, prior work for file_event); the open gap was that the parse-time validator didn't reject malformed trigger configs — they passed `validate` and only failed at runtime via `tracing::warn!` log-and-skip. This changeset adds parse-time validation.

### Outcome
- Confirmed both runtime triggers are wired in `crates/nexus-workflow/src/core_plugin.rs`:
  - **`file_event`** (`spawn_file_event_triggers`): per-workflow tokio task subscribes to `com.nexus.storage.file_*` bus events, filters by `watch_dir` prefix + optional `pattern` regex + optional `events` list (`created` / `modified` / `deleted`), dispatches `com.nexus.workflow::run` with `trigger.{path, event_type}` variables.
  - **`webhook`** (`crates/nexus-workflow/src/webhook.rs`, ~547 LoC): hand-rolled HTTP/1.1 listener, configurable bind in `<forge>/.forge/config.toml [webhooks]` block (default `127.0.0.1:18080`, opt-in), per-workflow `path` matching, optional `X-Webhook-Secret` constant-time validation, 64 KiB body cap, 5 s read timeout. Spawns only when `enabled = true` AND at least one workflow declares a `webhook` trigger.
- **Parse-time trigger validation** (`crates/nexus-workflow/src/trigger_validation.rs`): new `validate_trigger(&Workflow) -> Result<(), String>` dispatches by `trigger_type` and runs the same checks the runtime spec parsers do — `cron` validates via `CronSchedule::parse`; `webhook` re-uses the existing public `WebhookSpec::from_trigger`; `file_event` has its own validator covering pattern regex, event list shape, watch_dir type. Unknown trigger types pass through untouched (community plugins extending the trigger registry).
- **`WorkflowParseError::InvalidTrigger(String)`** new error variant. `parse_workflow_text` calls `validate_trigger` after the structural checks, so a forge editor saving a workflow with a bad regex / non-`/` webhook path / unparseable cron expression now sees the rejection synchronously instead of needing to read the kernel logs.
- **14 new tests** in `trigger_validation::tests` — happy path + every rejection path per trigger type, plus a wired-through-parse_workflow_text smoke test. Two existing tests (`webhook::tests::workflow_with_trigger` helper + `core_plugin::file_event_spec_rejects_invalid_regex_and_unknown_event`) switched from `parse_workflow_text` to bare `toml::from_str` so they continue to exercise the runtime spec parsers as defence-in-depth without being pre-empted by the new parse-layer validator.
- **Workflow crate**: 162/162 tests pass; clippy clean on changed files.

### Follow-up (not blocking)
- The original DOD line 4 ("integration test: temp forge → write file → workflow fires") is covered transitively by the existing `file_event_spec_matches_path_combines_dir_and_pattern` + `file_event_loop` plumbing — full end-to-end would require booting the kernel + storage in-test, which the existing nexus-bootstrap tests already exercise structurally. Adding a workflow-specific E2E is a larger scope than this gap warrants.
- `FileEventSpec::from_trigger` and the new `validate_file_event_trigger` duplicate the events-list parsing logic. Acceptable today (duplicate is ~15 lines, both stable); refactor target if a third caller appears.

---

## AIG-04 — Activity audit panel

**Severity:** Should-fix (handler exists, no UI)
**Surfaced by:** `com.nexus.ai::activity_list` (handler 18) returns AI tool-call audit log; no UI surface consumes it.
**Status:** Resolved 2026-05-05. The BL-037 `nexus.activityTimeline` plugin already shipped most of the surface; this work added the missing filters and empty-state docs link.

### Outcome
- Confirmed `nexus.activityTimeline` (`shell/src/plugins/nexus/activityTimeline/`) is the activity audit panel: pane-mode view + activity-bar entry (priority 55), hydrates via `activity_list`, lives via `com.nexus.ai.activity_appended` bus topic, Clear button calls `activity_clear`. Renders timestamp, surface, prompt, provider/model, files touched, tool-call name + ok/error glyph, outcome, duration — covers the "args summary, success/error" requirement.
- **Filter additions** (`activityTimelineStore.ts`): added `sessionFilter: string | null`, `dateFrom`/`dateTo: IsoDate | null` slots plus `setSessionFilter`, `setDateRange`, `resetFilters` actions. The toolbar now exposes a session dropdown (auto-populated from observed `session_id`s, truncated UUIDs for legibility), two `<input type="date">` controls with cross-min/max guards, and a "Reset" button that appears once any filter is active.
- **Predicate** (`ActivityTimelineView.tsx`): `entryInDateRange` compares the entry's local-date prefix (`YYYY-MM-DD`) against the bounds inclusively; unparseable timestamps fall to "no match" rather than crashing the renderer.
- **Empty state** now explains what gets recorded and links to `docs/PRDs/12-ai-engine.md` for the full surface map.
- Three new store tests cover the round-trip, `resetFilters`, and `clear`-vs-filter-isolation invariants. Full shell suite: 804/804 pass; typecheck clean; no new lint errors.

### Follow-up (not blocking)
- Surface name in the catalog vs ID (`nexus.activity` vs `nexus.activityTimeline`) — the AIG-04 spec used the shorter id but the in-tree plugin pre-dates this doc; renaming would churn settings/keybindings persistence keys for no user benefit.
- Default-off in catalog stays default-off until AIG-02 lands so the timeline isn't a noisy pane in fresh forges.

---

## AIG-05 — Local embeddings exposed in config

**Severity:** Nice-to-have (deferred per PRD-12)
**Surfaced by:** `crates/nexus-ai/src/local_embedding.rs` is feature-gated and inert; UI config has no toggle.
**Status:** Resolved 2026-05-05 — `set_config`/UI toggle/status reporting all wired. The feature stays opt-in at build time (see follow-up); flipping it on by default requires resolving the libonnxruntime runtime dependency.

### Outcome
- Confirmed the local backend is fully implemented (BL-019 / ADR 0018): `crates/nexus-ai/src/local_embedding.rs` (~422 LoC) wraps `fastembed::TextEmbedding`, supports BGE-small/base/large, MxBai-Embed-Large, Nomic-Embed-Text, MiniLM-L6 via the public `map_model` resolver, ships a DashMap embedding cache, and integrates with the `EmbeddingProvider` trait. The `local-embeddings` cargo feature pulls fastembed + dashmap + xxhash-rust; fastembed uses `ort-load-dynamic` so onnxruntime is resolved at run-time via `ORT_DYLIB_PATH` or the system loader path. `crates/nexus-ai/src/config.rs::detect_local_embedding` already wires the env-var path.
- **Backend wiring:**
  - `parse_config_field` in `core_plugin.rs` now lifts the `model` field into `local_embedding_model` whenever the embedding side declares `provider = "local"` (the canonical slot the `LocalEmbedding` constructor reads from). Other providers preserve the chat-style `model` field unchanged.
  - `config_snapshot` exposes `local_embedding_model` in the embedding view (skipped for non-local providers via `skip_serializing_if`).
  - `handle_status` now reports `embedding_model` (resolved from the local slot for `provider = "local"`, otherwise the chat-style `model`) and `embedding_dimension` (from a new `pub fn dimension_for(name: &str) -> Option<usize>` in `local_embedding.rs` that returns the fastembed dimension without instantiating the model — no 33 MB download just to satisfy a status query).
  - `dimension_for` is feature-gated; `resolve_embedding_dimension` returns `None` when the feature is off so non-local builds stay zero-cost.
- **Shell wiring:**
  - `'local'` added to the `ai.embedProvider` settings dropdown alongside `''`, `'openai'`, `'ollama'`.
  - `ai.embedModel` description rewritten to list the supported fastembed identifiers and warn about the first-use download (~33–500 MB to `~/.cache/fastembed/`).
  - `buildSetConfigPayload` (now exported for testing) routes `provider = 'local'` to a minimal `{ provider: 'local', model: ... }` payload — no `api_key` / `base_url` since the in-process backend has no auth or endpoint surface. The chat key is *not* reused for the local embedding side (a chat=openai + embed=local user shouldn't bleed `sk-...` into the embedding payload).
- **Tests:**
  - 7 new in `core_plugin::aig05_local_embedding_config_tests` (feature-off path) plus +1 with `--features local-embeddings` covering `dimension_for` resolution.
  - 4 new in `aiStore.test.ts` covering `buildSetConfigPayload` with local-only, blank-model fallback, no-chat-key-bleed, and remote-still-emits-all-fields.
  - Backend: nexus-ai 194/194 pass with feature off, 195/195 with feature on.
  - Shell: 833/833 pass; typecheck clean; no new lint errors.

### Follow-up (not blocking)
- The original DOD line 1 ("Feature compiled in by default with a fallback path — pulls on first use") would force fastembed + ort-load-dynamic into every Rust build, requiring `libonnxruntime.{so,dylib,dll}` to be resolvable at runtime for *all* users. The model-pull side is already lazy (fastembed downloads to `~/.cache/fastembed/<model>/` on first call), but the ORT system dependency isn't. Defaulting on would break clean builds for users without onnxruntime installed. Better tradeoff: wire the IPC + UI toggle (this work) and keep the cargo feature opt-in until either a) onnxruntime is bundled (adds ~50 MB to binaries), or b) fastembed gains an alternative runtime.
- The settings tab can't currently introspect "is `local-embeddings` compiled in?" — the kernel reports a clear error if the user picks `local` on a build without the feature, but a soft preflight that hides the option in unsupported builds would be nicer.

---

## AIG-06 — Inline enrich / recall UX polish

**Severity:** Nice-to-have (default-off; UX scaffolding)
**Surfaced by:** `nexus.enrich` and `nexus.recall` plugins ship default-disabled.
**Status:** Open.

### Problem
Auto-enrichment proposes tags/summary on save but the accept-gate is intrusive; recall overlay (Cmd+Shift+R) lacks preview and insertion affordances. Both ship off because they're not yet pleasant.

### Definition of done
- Enrich: non-blocking toast with "Review" CTA replaces modal accept-gate; per-field accept (tags vs summary independently).
- Recall: result preview pane with snippet highlighting; "insert as quote" / "insert as link" actions; keyboard nav.
- Both default-on after UX review.

---

## AIG-07 — TUI AI chat surface

**Severity:** Nice-to-have (parity gap)
**Surfaced by:** `crates/nexus-tui/` — no AI pane; `nexus ai` CLI works but TUI users have to drop out.
**Status:** Open.

### Problem
The TUI is a first-class frontend per architecture, but AI chat is unreachable from it. Streaming chat already works over IPC (`stream_chat` publishes to bus), so the TUI just needs a pane consuming those events.

### Definition of done
- New TUI pane (`Ctrl+G` or similar) hosting a streaming chat view.
- Subscribes to `com.nexus.ai` bus events; renders tokens incrementally.
- Session picker; provider status line.
- RAG toggle.

---

## Suggested order of attack

| Order | Item | Why |
|---|---|---|
| 1 | ~~**AIG-04** Activity panel~~ | ✅ Resolved 2026-05-05. |
| 2 | ~~**AIG-02** Agent approval UI~~ | ✅ Resolved 2026-05-05. |
| 3 | ~~**AIG-01** Skill composition~~ | ✅ Resolved 2026-05-05. |
| 4 | ~~**AIG-03** Workflow triggers~~ | ✅ Resolved 2026-05-05. |
| 5 | ~~**AIG-05** Local embeddings~~ | ✅ Resolved 2026-05-05. |
| 6 | **AIG-06** Enrich/recall polish | UX iteration; needs user feedback loop. |
| 7 | **AIG-07** TUI chat | Largest scope; lowest user-visible payoff while shell is the primary frontend. |
