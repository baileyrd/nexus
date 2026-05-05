# AI Integration Gaps

> Tracker for "where AI *could* go deeper" — concrete, scoped follow-ups identified by the AI integration analysis on 2026-05-05. Same format as [OPEN-ITEMS.md](OPEN-ITEMS.md). Each item has an ID (`AIG-NN`), severity, surfacing evidence, and a clear definition of done.
>
> Cross-references: [AI-INTERACTION-SURFACE-AUDIT.md](AI-INTERACTION-SURFACE-AUDIT.md), [PRDs/12-ai-engine.md](PRDs/12-ai-engine.md), [PRDs/13-skills.md](PRDs/13-skills.md), [PRDs/15-agent-system.md](PRDs/15-agent-system.md), [PRDs/16-workflow-system.md](PRDs/16-workflow-system.md).

---

## AIG-01 — Skill composition / dependency resolution

**Severity:** Should-fix (PRD-13 §5 open)
**Surfaced by:** `crates/nexus-skills/src/lib.rs` — `Skill::depends_on` is parsed and stored, but never resolved when a skill is rendered or activated.
**Status:** Open.

### Problem
Skills can declare `depends_on: [other-skill-id]` in frontmatter, but the registry doesn't compose them. An agent activating a skill receives only that skill's body — its dependencies are silently dropped.

### Definition of done
- `com.nexus.skills::render` resolves `depends_on` transitively (cycle-detected, deterministic order) and returns the composed body.
- Agent auto-activation (`com.nexus.agent::plan`) respects composition.
- Skills panel surfaces the resolved chain on hover/expand.
- Unit tests cover: linear chain, diamond, cycle rejection, missing-dep error.

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
**Status:** Open.

### Problem
Workflows can only fire on a schedule or by hand. The PRD specifies `file-event` (on-create / on-modify / on-delete with glob filters) and `webhook` (HTTP endpoint registered with the kernel) triggers.

### Definition of done
- `file-event` trigger subscribes to `nexus-storage` file watcher, filters by glob, debounces.
- `webhook` trigger registers a POST endpoint via the kernel HTTP surface (or Tauri sidecar), validates a per-workflow secret, fires the workflow with the request body as input.
- Validation: `nexus workflow validate` rejects malformed trigger configs.
- Integration test: temp forge → write file → workflow fires.

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
**Status:** Open.

### Problem
`local_embedding` feature exists but isn't surfaced in `set_config` or the chat-settings UI. Users wanting fully-local RAG (Ollama chat + local embeddings) can't enable it without rebuilding.

### Definition of done
- Feature compiled in by default with a fallback path (no model bundled — pulls on first use).
- `set_config` accepts `embedding_provider: "local"` with a model identifier.
- Settings tab exposes the toggle with a clear "downloads ~N MB on first use" hint.
- Status handler reports embedding model + dimension.

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
| 3 | **AIG-01** Skill composition | Backend-only; unblocks skill ecosystem. |
| 4 | **AIG-03** Workflow triggers | Moderate; storage watcher already exists. |
| 5 | **AIG-05** Local embeddings | Mostly scaffolded; mostly config plumbing. |
| 6 | **AIG-06** Enrich/recall polish | UX iteration; needs user feedback loop. |
| 7 | **AIG-07** TUI chat | Largest scope; lowest user-visible payoff while shell is the primary frontend. |
