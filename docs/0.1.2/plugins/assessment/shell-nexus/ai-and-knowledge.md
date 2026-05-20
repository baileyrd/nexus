# AI and Knowledge

Plugins that route the shell to the AI / agent / memory stack in the Rust core. They cluster around `com.nexus.ai` (chat, completions, embeddings, enrichment) and `com.nexus.agent` (session-driven multi-step automation), with `nexus.memory` providing the quick-capture inbox that several AI features read back. Every plugin here is Optional for the basic-capability scope — none are required to open a forge, browse markdown, edit, search by text, or commit via git. Most depend on `nexus.ai` (or on a configured provider behind it); turning off `nexus.ai` cascades.

### ai

- **Path:** `shell/src/plugins/nexus/ai/`
- **Surface:** view type `ai-chat` (sidedock + activity-bar item "AI Chat"), overlay view `nexus.ai.cmdI.overlay`, commands `nexus.ai.focus` / `clear` / `openSettings` / `cmdI.open` / `cmdI.close` / `reindexForge`, keybindings `Ctrl/Cmd+Alt+A` (chat focus) and `Ctrl/Cmd+I` (Cmd+I overlay), context key `nexus.ai.cmdI.visible`, configuration block `ai.*` (provider/model/keys, ghost completions, margin-suggest). Hosts the chat-stream subscription, the Cmd+I overlay, BL-034 ghost completions handle, BL-036 margin suggestions, BL-139 edit-prediction handle, and the built-in AI actions registry.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`; subscribes to `com.nexus.ai.stream_*` and invokes `com.nexus.ai::stream_chat` / `set_config` / `index_trigger`.
- **Verdict:** Optional
- **Rationale:** Provides chat, inline AI features, and the kernel-config push for AI providers. Optional for the basic scope; required by `recall`, `enrich`, `linkSuggest`, `semanticSearch`, and (indirectly) by margin-suggestions and ghost completions inside the editor.

### agent

- **Path:** `shell/src/plugins/nexus/agent/`
- **Surface:** view `nexus.agent.view` rendered in the `paneMode` slot, activity-bar item "Agent" (sparkle icon), command `nexus.agent.show`. Drives `com.nexus.agent::session_run` with `auto_approve: false`, subscribes to `com.nexus.agent.round_proposed`, calls `round_decide`, `session_list` / `session_get` / `session_delete`.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.paneMode`; talks to the `com.nexus.agent` core plugin.
- **Verdict:** Optional
- **Rationale:** Multi-step approval-gated agent runner. Heavy feature, opt-in.

### memory

- **Path:** `shell/src/plugins/nexus/memory/`
- **Surface:** overlay view `nexus.memory.captureOverlay`, commands `nexus.memory.captureOpen` / `captureCommit` / `captureCodeOpen`, configuration `memory.hotkey` and `memory.inboxPath`. Registers a system-wide global shortcut via `@tauri-apps/plugin-global-shortcut` (default `CommandOrControl+Alt+N`) that opens the capture overlay even when Nexus is backgrounded; on commit appends a timestamped snippet to `Inbox.md`.
- **Depends on:** nothing AI; pure file-write via `com.nexus.storage`. Tauri global-shortcut plugin native dep.
- **Verdict:** Optional
- **Rationale:** Quick-capture for incoming notes — independent of AI. The inbox is the source that `recall` and (with `nexus.ai`) the embedding index later read from. Not in the basic scope; users who simply edit existing markdown don't need it.

### prompt

- **Path:** `shell/src/plugins/nexus/prompt/`
- **Surface:** overlay view `nexus.prompt.modal`. Backs `api.input.prompt(...)` — styled in-app replacement for `window.prompt`. No commands.
- **Depends on:** none directly; consumed via `PluginAPI.input.prompt`.
- **Verdict:** Useful
- **Rationale:** Infrastructure modal. Mis-categorised here — it has nothing to do with AI despite the directory grouping. Without it, callers of `api.input.prompt(...)` fall back to the platform `window.prompt` (or fail outright depending on host wiring). Several plugins including `semanticSearch` rely on it.

### enrich

- **Path:** `shell/src/plugins/nexus/enrich/`
- **Surface:** overlay view `nexus.enrich.gate` (accept-gate panel), command `nexus.enrich.force` ("Force enrich current file"), configuration block `nexus.enrich` (empty schema, just a settings section). Subscribes to `files:saved`, throttles per-file (5 s), calls `com.nexus.ai::enrich_file`, surfaces a proposal panel, and on accept calls `enrich_apply` to merge tags/summary/related-notes into YAML frontmatter.
- **Depends on:** `nexus.ai`. Default OFF in the plugin catalog.
- **Verdict:** Optional
- **Rationale:** Ships disabled. Not in basic scope.

### dreamCycle

- **Path:** `shell/src/plugins/nexus/dreamCycle/`
- **Surface:** view `nexus.dreamCycle.view` in the `paneMode` slot, activity-bar item "Dream Cycle" (moon icon), commands `nexus.dreamCycle.show` / `refresh`. Subscribes to `com.nexus.dream_cycle.proposals` for toast notifications + inbox refresh; reads `com.nexus.storage::list_draft_relations`; approve/skip routes through `entity_get` + `entity_upsert`.
- **Depends on:** `nexus.paneMode`, `nexus.activityBar`; reads `com.nexus.storage` entity graph (which the `dream_cycle` cycle in `nexus-bootstrap` populates).
- **Verdict:** Optional
- **Rationale:** Niche review surface for nightly-cycle LLM relation proposals. Not in basic scope; depends on the upstream `dream_cycle` job running.

### skills

- **Path:** `shell/src/plugins/nexus/skills/`
- **Surface:** sidedock view type `skills`, activity-bar item "Skills" (book icon), commands `nexus.skills.refresh` / `show`. Lists skills via `com.nexus.skills::list`; the in-view `SkillEditor` writes back through the same core plugin.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`; reads from `com.nexus.skills` core plugin.
- **Verdict:** Optional
- **Rationale:** UI for the skills feature (agent-callable prompts/templates stored in the forge). Not in basic scope.

### linkSuggest

- **Path:** `shell/src/plugins/nexus/linkSuggest/`
- **Surface:** configuration block `nexus.linkSuggest` (`ai.linkSuggest.enabled` + tuning knobs). No view, no command, no runtime — it is a config-only activation shim. The actual ghost-rendering CodeMirror extension lives at `editor/cm/linkSuggest.ts` and reads the same config keys via `configStore`; it reaches the AI handler through `nexus.ai`'s `getGhostApi()`.
- **Depends on:** `nexus.ai` (implicit — without it `getGhostApi()` is null and the CM extension no-ops).
- **Verdict:** Optional
- **Rationale:** Inline wiki-link ghost suggestions backed by semantic search. AI-dependent, not in basic scope. The plugin itself is borderline-Removable as a unit — its sole job is to register a config schema; the underlying behaviour lives inside the editor plugin's CM extensions.

## Category verdict

| Plugin       | Verdict   | Notes                                                            |
|--------------|-----------|------------------------------------------------------------------|
| ai           | Optional  | Chat + Cmd+I + ghost/margin AI surfaces.                         |
| agent        | Optional  | Session-driven approval-gated agent.                             |
| memory       | Optional  | Quick-capture global hotkey + Inbox.md append. Not AI-dependent. |
| prompt       | Useful    | Infrastructure modal for `api.input.prompt`; mis-categorised.    |
| enrich       | Optional  | Ships default-off; AI auto-enrichment on save.                   |
| dreamCycle   | Optional  | Inbox for nightly-cycle relation proposals.                      |
| skills       | Optional  | UI over the `com.nexus.skills` core plugin.                      |
| linkSuggest  | Optional  | Config-only shim; behaviour lives in `editor/cm/linkSuggest.ts`. |
