> **Archived 2026-04-26** — Reference capture of the deleted legacy `ChatPanel.tsx` produced for the WI-01 port. Current chat lives at `shell/src/plugins/nexus/ai/ChatView.tsx`.

# WI-01 Reference — Legacy ChatPanel extraction

> **Historical reference** — Describes the pre-migration the legacy shell's `ChatPanel.tsx` (deleted under Phase 4 WI-37, 2026-04-24). Post-migration the equivalent lives at `shell/src/plugins/nexus/ai/ChatView.tsx` (also `AiChatView.tsx`). Line numbers below refer to the historical source.

**Source:** legacy the legacy shell's `ChatPanel.tsx` (~1275 LOC, now deleted) — ported to `shell/src/plugins/nexus/ai/ChatView.tsx` (also `AiChatView.tsx`)
**Audience:** WI-01 Slice A/B/C implementation agents
**Style:** descriptive only — no proposed redesigns

## 1. Component tree

```
ChatPanel  (default export, ChatPanel.tsx:196)
├── header bar  (ChatPanel.tsx:836)
│   ├── provider label  (:846)
│   ├── Agent toggle button  (:847)
│   ├── Preview toggle button  (:856)
│   ├── Archetype <select>  (:866)  values: general | writer | coder | researcher
│   ├── RAG toggle button  (:883)
│   ├── System toggle button  (:893)
│   ├── Session <select>  (:901)
│   ├── New / Delete / Clear buttons  (:920–945)
├── system-prompt drawer  (:948, conditional on showSystem)
│   └── <textarea>  (:966)
├── transcript scroll region  (:986, ref=scrollRef)
│   └── per-turn block (:1003)
│       ├── role label "You" / "Assistant · streaming…"  (:1014)
│       ├── PendingPlanCard  (local closure :1146) — when turn.pendingPlan set
│       │   └── <ol> step list + Step / Approve / Cancel buttons
│       └── plain bubble div  (:1029) — pre-wrap, role-styled bg
│       └── RAG source chips row  (:1049, assistant + sources only)
├── error banner  (:1085, conditional on `error`)
└── composer footer  (:1098)
    ├── <textarea>  (:1106, onKeyDown=onKeyDown)
    └── Send button  (:1125)
```

`PendingPlanCard` and `stepBadge` are local closures in the same file
(:1146, :1262). All other helpers (`aiSessionLoad`, `agentRun`, etc.)
were imported from the legacy shell's `ipc/ai.ts` and `ipc/agent.ts` (both removed — post-migration call `ctx.ipc_call("com.nexus.ai", ...)` via `shell/src/plugins/nexus/ai/aiRuntime.ts` and `ctx.ipc_call("com.nexus.agent", ...)` via `shell/src/plugins/nexus/agent/agentStore.ts`). No
nested route components, no portals.

## 2. State shape

All state is local — **there is no Zustand store for chat**. State
lives entirely inside the `ChatPanel` component:

| Name | Type | Triggered by |
|---|---|---|
| `turns` | `Turn[]` (:197) | `send`, stream events, agent events, hydration, `clearConversation`, plan approval/cancel/step |
| `systemPrompt` | `string` (:198) | system drawer textarea, hydration, `newSession` |
| `hydrated` | `boolean` (:199) | flips true after `aiSessionLoad` settles (:275) — gate for persistence write |
| `chatSessionId` | `string` (:200) | initialized from `localStorage["nexus.chat.active-session-id"]` or `"default"`; updated by `switchSession`, `newSession`, `deleteCurrentSession` |
| `sessionSummaries` | `ChatSessionSummary[]` (:203) | `aiSessionList()` after mount, after `chatSessionId` or `turns.length` change |
| `showSystem` | `boolean` (:206) | header chip click |
| `useRag` | `boolean` (:207) | header chip; mutually exclusive with `useAgent` (Agent disables RAG) |
| `useAgent` | `boolean` (:208) | header chip |
| `previewAgentPlans` | `boolean` (:209) | header chip; disabled when `!useAgent` |
| `archetype` | `AgentArchetype` (:210) | header `<select>` |
| `input` | `string` (:211) | composer textarea |
| `sending` | `boolean` (:212) | `send` start; cleared on stream_done, agent done, error, plan settle |
| `error` | `string \| null` (:213) | catch blocks; cleared on next `send` / `clearConversation` |
| `config` | `AiConfigSnapshot \| null` (:214) | one-shot `aiConfig()` on mount |
| `sessionRef` | `useRef<string \| null>` (:215) | per-stream request id; matched against incoming events |
| `assistantIndexRef` | `useRef<number \| null>` (:216) | index of the in-flight assistant turn so chunks can land into it |
| `scrollRef` | `useRef<HTMLDivElement \| null>` (:217) | autoscroll target |

`Turn` shape (:45–63): `role`, `content`, `pending?`, `sources?`,
`pendingPlan?`, `stepCursor?`, `stepResults?`, `agentProgress?`.
Persisted shape (:65–76) drops `pending`, `pendingPlan`, `stepCursor`,
`stepResults`, `agentProgress` before write (:446–454).

## 3. IPC call sites

| command_id | call site | args | returns | error handling |
|---|---|---|---|---|
| `ai_config` | :221 (`aiConfig()`) | none | `AiConfigSnapshot` | sets `error` on reject |
| `ai_session_load` | :247, :262 (legacy fallback) | `{ id }` or none | `unknown` (decoded by `decodePersisted`) | `console.warn` only |
| `ai_session_save` | :148 (`persist()`) | `Persisted + updated_at` | `void` | `console.warn` |
| `ai_session_list` | :287 | none | `ChatSessionSummary[]` | swallowed (plugin may not be up) |
| `ai_session_delete` | :798 | `{ id }` | `void` | `console.warn`, still proceeds to switch session |
| `ai_stream_chat` | :544 | `{ messages, system, sessionId }` | `StreamChatResult` | catch block clears refs/sending, sets `error`, marks last turn `[error]` |
| `ai_stream_ask` | :542 | `{ messages, sessionId, limit }` | `StreamAskResult` | same catch as above |
| `ai_ask` | **not called** — only stream variants are used |
| `ai_index_file` | **not called from ChatPanel** |
| `ai_vectorstore_count` | **not called from ChatPanel** |
| `ai_status` | **not called from ChatPanel** |

Agent commands (out of scope for ai handlers): `agentPlan` (:497),
`agentRun` (:522), `agentRunPlan` (:729), `agentExecuteStep` (:620,
:720). All wrapped in try/catch → `setError(String(err))`.

## 4. Event subscriptions

All listeners are wired in a single `useEffect` (:299) with empty deps,
torn down on unmount via collected unlisten fns (:427–429).

| Topic | Payload | Handler |
|---|---|---|
| `ai:stream_start` | `{ session_id }` | no-op (:344) — assistant turn already eagerly created in `send()` |
| `ai:stream_chunk` | `{ session_id, chunk, index }` | `appendChunk` (:302) — drops if `session_id !== sessionRef.current`; appends `chunk` to `turns[assistantIndexRef].content` |
| `ai:stream_done` | `{ session_id, text, sources? }` | `finalizeTurn` (:315) — overwrites content with `text` (note: not just trims chunks; it replaces); attaches RAG sources if present and non-empty; clears refs and `sending` |
| `agent:run_start` | `{ steps }` | replaces pending content with "Running plan · N step(s)…", inits `agentProgress: []` |
| `agent:step_start` | `{ index, description }` | sets `agentProgress[index] = "▶ [n] desc"`; rejoins as content |
| `agent:step_done` | `{ index, status, error? }` | swaps "▶" for badge (✓/✗/·/?), appends error |
| `agent:run_done` | none used | no-op — final `agentRun` resolution overwrites content |

There is **no dedup or buffer** — each chunk event mutates state once.
Stale-session matching uses a single `sessionRef.current` string equality
check; events for any other session are silently dropped (:303, :320).

## 5. Non-obvious UX details

1. **Autoscroll is unconditional** (:432–435). Every `turns` change
   sets `scrollTop = scrollHeight`. No "user scrolled up, pause" guard.
2. **Enter sends, Shift+Enter newlines** (:763). No Cmd/Ctrl+Enter.
   Composer disabled while `sending`.
3. **Composer clears immediately on send** (:472), before IPC resolves
   — failures surface in error banner, not the input.
4. **No copy-to-clipboard.** Bubble is plain `<div>` with `pre-wrap`.
5. **No markdown rendering.** Code fences render as raw backticks.
6. **RAG source chips** (:1049): tooltip shows first 240 chars of
   `chunk_text` + `score.toFixed(3)`; chip text is `file_path`,
   ellipsized. Key: `${file_path}-${block_id}-${i}`.
7. **`stream_done.text` overwrites accumulated chunks** (:335). If
   chunks and final text disagree, final wins — no rollback.
8. **Session picker disables during `sending`** (:904), but switching
   does NOT cancel an in-flight stream — just blocks the dropdown.
9. **Default session can't be deleted** (:796, button disabled :932).
10. **No focus restoration** after Send (mouse click loses focus).
11. **Persistence gated on `!sending && hydrated`** (:441) — half-
    streamed turns never hit disk; `pendingPlan` filter (:447) drops
    approval dialogs from saved transcripts.
12. **Session title** auto-derived from first user turn, trimmed,
    whitespace collapsed, max 48 chars + ellipsis (:101–106). Set on
    every save. **No inline rename UI exists.**
13. **`useRag` force-disabled when `useAgent` on** (:889).
14. **`previewAgentPlans` + archetype select disabled when `!useAgent`**
    (:861, :869).
15. **No error retry.** Failed turn keeps literal `"[error]"` (:557).
16. **Stepwise plan execution** (:607) tracks `stepCursor` +
    `stepResults`; "Approve rest" uses `agentExecuteStep` in a loop
    when `cursor > 0` (:714) so completed steps aren't re-run.

## 6. Streaming architecture notes

**Request IDs.** `makeSessionId()` (:158) returns
`chat-${Date.now()}-${rand6}`. Stored in `sessionRef.current` (:476)
and passed as `sessionId` to `aiStreamChat`/`aiStreamAsk`. Every
incoming event compares `ev.session_id === sessionRef.current` and
drops mismatches. There is exactly one in-flight stream at a time.

**Done timeout.** None. If `stream_done` never arrives, `sending`
stays `true` indefinitely, the bubble shows "streaming…", and the user
is stuck — there is no client-side timeout, no abort button, no
heartbeat check.

**Reconnection mid-stream.** Not handled. If the kernel restarts
between `stream_start` and `stream_done`, the session id won't match
any post-restart events. The promise from `aiStreamChat` will reject
(via Tauri transport error) and hit the catch block (:549), which
clears refs and sets `error`. No automatic resume.

**Cancellation.** No cancel button. The only way to abandon a stream
is wait for it or reload. `clearConversation` is disabled while
`sending` (:575, :941).

## 7. What's NOT in the legacy that the port should consider

- **No abort/cancel for streams.** Comment at :423 acknowledges the
  pattern (awaited promise carries result, side events are advisory);
  kernel hangs strand the UI.
- **No markdown / syntax highlighting.** Plain text only.
- **No copy buttons** for turn content or RAG chunks.
- **No inline session rename** — title is auto-derived only.
- **`ai_ask` / `ai_status` / `ai_index_file` / `ai_vectorstore_count`
  unused by ChatPanel.** Port shouldn't assume they're exercised here.
- **Legacy fallback path** (:262, comment :233–246) migrates from
  pre-multi-session storage; may be obsolete on greenfield installs.
- **No keyboard shortcuts** for Agent/RAG/System chips — mouse only.
- **No `aria-live` region** on the transcript — screen readers don't
  announce streamed tokens.
