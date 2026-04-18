// React chat panel backed by `com.nexus.ai` core-plugin dispatch
// (PRD-12 §6). Registers as content-type `"com.nexus.ai.chat"` and
// streams assistant tokens via the `ai:stream_*` Tauri events that
// the `nexus-ai-event-forwarder` publishes off the kernel bus.
//
// Conversation state + system prompt persist to localStorage across
// app reloads. A "Clear" button drops the transcript; once plugin-
// backed session storage lands (PRD-12 §8) the store will move over
// without changing the UX.

import type { CSSProperties, KeyboardEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  aiConfig,
  aiSessionDelete,
  aiSessionList,
  aiSessionLoad,
  aiSessionSave,
  type ChatSessionSummary,
  aiStreamAsk,
  aiStreamChat,
  onAiStreamChunk,
  onAiStreamDone,
  onAiStreamStart,
  type AiConfigSnapshot,
  type ChatMessage,
  type RagSource,
} from "../../ipc/ai";
import {
  agentExecuteStep,
  agentPlan,
  agentRun,
  agentRunPlan,
  onAgentRunDone,
  onAgentRunStart,
  onAgentStepDone,
  onAgentStepStart,
  type AgentArchetype,
  type AgentPlan,
  type StepResult,
  type Observation,
} from "../../ipc/agent";

type Turn = {
  role: "user" | "assistant";
  content: string;
  pending?: boolean;
  sources?: RagSource[];
  /** When set, the turn is an agent plan awaiting user approval —
   *  the UI renders an Approve / Cancel pair instead of a normal
   *  assistant bubble. Cleared once the plan runs or is dismissed. */
  pendingPlan?: AgentPlan;
  /** Progress cursor for stepwise execution. Only meaningful while
   *  `pendingPlan` is set. `stepResults[i]` is the StepResult from
   *  `pendingPlan.steps[i]`. Step i is the "next" step to run. */
  stepCursor?: number;
  stepResults?: StepResult[];
  /** Transient checklist populated from the `agent:step_start` /
   *  `agent:step_done` Tauri events while an agent plan is in flight.
   *  Dropped from persistence. */
  agentProgress?: string[];
};

interface Persisted {
  /// Session id — when present, routes save to
  /// `chat/sessions/<id>.json` instead of the legacy single-session
  /// file. Optional so legacy payloads still decode.
  id?: string;
  /// Caller-assigned title, surfaced in the session picker. Defaults
  /// to the first user turn truncated to 48 chars on save.
  title?: string;
  updated_at?: string;
  turns: Turn[];
  systemPrompt: string;
}

const ACTIVE_SESSION_KEY = "nexus.chat.active-session-id";
const DEFAULT_SESSION_ID = "default";

function readActiveSessionId(): string | null {
  try {
    return localStorage.getItem(ACTIVE_SESSION_KEY);
  } catch {
    return null;
  }
}

function writeActiveSessionId(id: string): void {
  try {
    localStorage.setItem(ACTIVE_SESSION_KEY, id);
  } catch {
    // ignore — non-essential cross-run memory
  }
}

function newChatSessionId(): string {
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function deriveTitle(turns: Turn[]): string | undefined {
  const first = turns.find((t) => t.role === "user");
  if (!first) return undefined;
  const trimmed = first.content.trim().replace(/\s+/g, " ");
  return trimmed.length > 48 ? `${trimmed.slice(0, 48)}…` : trimmed;
}

function decodePersisted(value: unknown): Persisted {
  if (typeof value !== "object" || value === null) {
    return { turns: [], systemPrompt: "" };
  }
  const obj = value as Partial<Persisted>;
  const turns = Array.isArray(obj.turns)
    ? obj.turns.filter(isTurn).map((t) => ({ ...t, pending: false }))
    : [];
  const systemPrompt =
    typeof obj.systemPrompt === "string" ? obj.systemPrompt : "";
  const id = typeof obj.id === "string" ? obj.id : undefined;
  const title = typeof obj.title === "string" ? obj.title : undefined;
  return { id, title, turns, systemPrompt };
}

function isTurn(v: unknown): v is Turn {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    (r.role === "user" || r.role === "assistant") &&
    typeof r.content === "string"
  );
}

function isRagSource(v: unknown): v is RagSource {
  if (typeof v !== "object" || v === null) return false;
  const r = v as Record<string, unknown>;
  return (
    typeof r.file_path === "string" &&
    typeof r.chunk_text === "string" &&
    typeof r.block_id === "number" &&
    typeof r.score === "number"
  );
}

function persist(state: Persisted): void {
  // Fire-and-forget — persistence is best-effort. Any failure is
  // logged but never blocks the UI. Sessions survive app restarts
  // via com.nexus.ai::session_save; a fresh forge sees an empty
  // session on first load.
  aiSessionSave({
    ...state,
    title: state.title ?? deriveTitle(state.turns),
    updated_at: new Date().toISOString(),
  }).catch((err) => {
    // eslint-disable-next-line no-console
    console.warn("[chat] session_save failed", err);
  });
}

function makeSessionId(): string {
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function formatObservation(obs: Observation): string {
  const header = obs.success
    ? "Plan completed."
    : "Plan ended with failures.";
  const lines: string[] = [header, ""];
  for (const step of obs.steps) {
    const badge =
      step.status === "ok"
        ? "✓"
        : step.status === "denied"
          ? "⊘"
          : step.status === "failed"
            ? "✗"
            : "·";
    let line = `${badge} [${step.status}] ${step.step_id}`;
    if (step.response !== null && step.response !== undefined) {
      const preview = JSON.stringify(step.response);
      line += ` — ${preview.length > 200 ? `${preview.slice(0, 200)}…` : preview}`;
    }
    lines.push(line);
  }
  return lines.join("\n");
}

const chipButtonStyle: CSSProperties = {
  padding: "2px 10px",
  fontSize: 11,
  border: "1px solid var(--color-border)",
  borderRadius: 4,
  background: "transparent",
  color: "inherit",
  cursor: "pointer",
};

export function ChatPanel(): JSX.Element {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [systemPrompt, setSystemPrompt] = useState("");
  const [hydrated, setHydrated] = useState(false);
  const [chatSessionId, setChatSessionId] = useState<string>(
    () => readActiveSessionId() ?? DEFAULT_SESSION_ID,
  );
  const [sessionSummaries, setSessionSummaries] = useState<ChatSessionSummary[]>(
    [],
  );
  const [showSystem, setShowSystem] = useState(false);
  const [useRag, setUseRag] = useState(false);
  const [useAgent, setUseAgent] = useState(false);
  const [previewAgentPlans, setPreviewAgentPlans] = useState(false);
  const [archetype, setArchetype] = useState<AgentArchetype>("general");
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [config, setConfig] = useState<AiConfigSnapshot | null>(null);
  const sessionRef = useRef<string | null>(null);
  const assistantIndexRef = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    aiConfig()
      .then((cfg) => {
        if (!cancelled) setConfig(cfg);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Hydrate the transcript from com.nexus.ai::session_load for the
  // currently-selected chat session. Flips `hydrated` on completion
  // (even on failure) so the persistence effect below can start
  // writing. Re-runs when the user picks a different session in the
  // picker — the effect below flushes the outgoing session before
  // this fires.
  useEffect(() => {
    let cancelled = false;
    setHydrated(false);
    // On very first boot try multi-session `chatSessionId`; if that
    // returns null and id == DEFAULT_SESSION_ID, fall back to the
    // legacy single-session path so existing users don't lose their
    // transcript. The first save will then migrate the content into
    // the new tree because persist() writes with `id`.
    aiSessionLoad(chatSessionId)
      .then((value) => {
        if (cancelled) return;
        if (value !== null) {
          const { turns: loadedTurns, systemPrompt: loadedPrompt } =
            decodePersisted(value);
          setTurns(loadedTurns);
          setSystemPrompt(loadedPrompt);
          return;
        }
        if (chatSessionId !== DEFAULT_SESSION_ID) {
          setTurns([]);
          setSystemPrompt("");
          return;
        }
        return aiSessionLoad().then((legacy) => {
          if (cancelled) return;
          const { turns: loadedTurns, systemPrompt: loadedPrompt } =
            decodePersisted(legacy);
          setTurns(loadedTurns);
          setSystemPrompt(loadedPrompt);
        });
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("[chat] session_load failed", err);
      })
      .finally(() => {
        if (!cancelled) setHydrated(true);
      });
    return () => {
      cancelled = true;
    };
  }, [chatSessionId]);

  // Keep the list of sessions fresh. Runs once on mount + any time
  // the active session changes (so a newly-created one appears in
  // the picker immediately after first save).
  useEffect(() => {
    let cancelled = false;
    aiSessionList()
      .then((list) => {
        if (!cancelled) setSessionSummaries(list);
      })
      .catch(() => {
        // Plugin may not be available (e.g. still booting). Non-fatal.
      });
    return () => {
      cancelled = true;
    };
  }, [chatSessionId, turns.length]);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    const appendChunk = (sessionId: string, chunk: string) => {
      if (sessionRef.current !== sessionId) return;
      setTurns((prev) => {
        const idx = assistantIndexRef.current;
        if (idx === null) return prev;
        const next = prev.slice();
        const current = next[idx];
        if (!current) return prev;
        next[idx] = { ...current, content: current.content + chunk };
        return next;
      });
    };

    const finalizeTurn = (
      sessionId: string,
      finalText?: string,
      sources?: RagSource[],
    ) => {
      if (sessionRef.current !== sessionId) return;
      // Capture the pending-turn index before clearing the ref:
      // reducer closures run at commit time, after this function's
      // sync tail, so reading the ref from inside the updater would
      // see the post-`= null` value.
      const idx = assistantIndexRef.current;
      assistantIndexRef.current = null;
      sessionRef.current = null;
      setTurns((prev) => {
        if (idx === null) return prev;
        const next = prev.slice();
        const current = next[idx];
        if (!current) return prev;
        next[idx] = {
          ...current,
          content: finalText ?? current.content,
          pending: false,
          sources: sources && sources.length > 0 ? sources : current.sources,
        };
        return next;
      });
      setSending(false);
    };

    onAiStreamStart(() => {
      // No-op for now — the assistant turn row is created eagerly in `send()`
      // so streaming tokens have somewhere to land immediately.
    }).then((fn) => unlisteners.push(fn));

    onAiStreamChunk((ev) => appendChunk(ev.session_id, ev.chunk)).then((fn) =>
      unlisteners.push(fn),
    );

    onAiStreamDone((ev) => {
      const sources = Array.isArray(ev.sources)
        ? ev.sources.filter(isRagSource)
        : undefined;
      finalizeTurn(ev.session_id, ev.text, sources);
    }).then((fn) => unlisteners.push(fn));

    // Agent plan-execution progress: writes an ASCII checklist into
    // the pending turn's content while the plan runs. The final
    // `agentRun` / `agentRunPlan` resolution overwrites this with
    // the formatted observation, so we don't need to clear it here.
    const updatePendingAgentTurn = (
      mutate: (current: Turn) => Turn,
    ) => {
      setTurns((prev) => {
        const idx = assistantIndexRef.current;
        if (idx === null) return prev;
        const next = prev.slice();
        const current = next[idx];
        if (!current || !current.pending) return prev;
        next[idx] = mutate(current);
        return next;
      });
    };

    onAgentRunStart((ev) => {
      updatePendingAgentTurn((current) => ({
        ...current,
        content: `Running plan · ${ev.steps} step${ev.steps === 1 ? "" : "s"}…`,
        agentProgress: [],
      }));
    }).then((fn) => unlisteners.push(fn));

    onAgentStepStart((ev) => {
      updatePendingAgentTurn((current) => {
        const lines = (current.agentProgress ?? []).slice();
        lines[ev.index] = `▶ [${ev.index + 1}] ${ev.description}`;
        return {
          ...current,
          content: lines.join("\n"),
          agentProgress: lines,
        };
      });
    }).then((fn) => unlisteners.push(fn));

    onAgentStepDone((ev) => {
      const badge =
        ev.status === "ok"
          ? "✓"
          : ev.status === "failed"
            ? "✗"
            : ev.status === "skipped"
              ? "·"
              : "?";
      updatePendingAgentTurn((current) => {
        const lines = (current.agentProgress ?? []).slice();
        const prior = lines[ev.index] ?? `[${ev.index + 1}] step`;
        const stripped = prior.replace(/^▶\s/, "");
        lines[ev.index] = `${badge} ${stripped}${
          ev.error ? ` — ${ev.error}` : ""
        }`;
        return {
          ...current,
          content: lines.join("\n"),
          agentProgress: lines,
        };
      });
    }).then((fn) => unlisteners.push(fn));

    onAgentRunDone(() => {
      // No-op — the awaited agentRun / agentRunPlan resolution
      // overwrites `content` with the full formatted observation.
    }).then((fn) => unlisteners.push(fn));

    return () => {
      for (const fn of unlisteners) fn();
    };
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [turns]);

  // Persist transcript + system prompt whenever either settles. The
  // check on `sending` keeps us from writing half-streamed turns to
  // disk — they'd replay as `pending: true` markers the next session,
  // which looks broken without a live stream underneath.
  useEffect(() => {
    if (sending || !hydrated) return;
    // Drop transient UI state before writing to disk: pending
    // streaming markers + pendingPlan approval dialogs shouldn't
    // replay on the next load.
    const cleanTurns = turns
      .filter((t) => !t.pendingPlan)
      .map((t) => ({
        ...t,
        pending: false,
        stepCursor: undefined,
        stepResults: undefined,
        agentProgress: undefined,
      }));
    persist({
      id: chatSessionId,
      turns: cleanTurns,
      systemPrompt,
    });
  }, [turns, systemPrompt, sending, hydrated, chatSessionId]);

  const send = useCallback(async () => {
    const trimmed = input.trim();
    if (!trimmed || sending) return;
    if (!config?.ai) {
      setError(
        "No AI provider configured. Set NEXUS_AI_PROVIDER (anthropic, openai, ollama) and rerun.",
      );
      return;
    }
    setError(null);
    setInput("");
    setSending(true);

    const sessionId = makeSessionId();
    sessionRef.current = sessionId;

    const nextTurns: Turn[] = [
      ...turns,
      { role: "user", content: trimmed },
      { role: "assistant", content: "", pending: true },
    ];
    assistantIndexRef.current = nextTurns.length - 1;
    setTurns(nextTurns);

    const history: ChatMessage[] = nextTurns
      .slice(0, -1)
      .filter((t) => t.content.length > 0)
      .map((t) => ({ role: t.role, content: t.content }));

    try {
      const trimmedSystem = systemPrompt.trim();
      if (useAgent && previewAgentPlans) {
        // Preview mode: ask the planner for a plan, park it in the
        // pending turn as `pendingPlan`, and wait for the user to
        // click Approve / Cancel. No tool calls yet.
        const plan = await agentPlan(trimmed, archetype);
        // Capture the pending-turn index before clearing the ref —
        // reducer closures run during React's render phase, after the
        // synchronous tail of this function, so reading the ref from
        // inside the updater would see the post-`= null` value.
        const pendingIdx = assistantIndexRef.current;
        assistantIndexRef.current = null;
        sessionRef.current = null;
        setTurns((prev) => {
          if (pendingIdx === null) return prev;
          const next = prev.slice();
          const current = next[pendingIdx];
          if (!current) return prev;
          next[pendingIdx] = {
            ...current,
            content: "",
            pending: false,
            pendingPlan: plan,
          };
          return next;
        });
        setSending(false);
      } else if (useAgent) {
        // Agent mode without preview: dispatch to com.nexus.agent::run
        // which plans + executes tool calls in one pass.
        const observation = await agentRun(trimmed, archetype);
        const content = formatObservation(observation);
        const pendingIdx = assistantIndexRef.current;
        assistantIndexRef.current = null;
        sessionRef.current = null;
        // Close out the pending turn manually since there's no
        // stream_done event on the agent path.
        setTurns((prev) => {
          if (pendingIdx === null) return prev;
          const next = prev.slice();
          const current = next[pendingIdx];
          if (!current) return prev;
          next[pendingIdx] = { ...current, content, pending: false };
          return next;
        });
        setSending(false);
      } else if (useRag) {
        // RAG mode: stream_ask injects retrieved chunks as the system
        // prompt inside the plugin, so the user-configured prompt is
        // ignored for this turn. The retrieval happens server-side.
        await aiStreamAsk(history, { sessionId });
      } else {
        await aiStreamChat(history, {
          sessionId,
          system: trimmedSystem ? trimmedSystem : undefined,
        });
      }
    } catch (err) {
      sessionRef.current = null;
      assistantIndexRef.current = null;
      setSending(false);
      setError(String(err));
      setTurns((prev) =>
        prev.map((t, i) =>
          i === nextTurns.length - 1
            ? { ...t, pending: false, content: t.content || "[error]" }
            : t,
        ),
      );
    }
  }, [
    config,
    input,
    sending,
    turns,
    systemPrompt,
    useRag,
    useAgent,
    previewAgentPlans,
    archetype,
  ]);

  const clearConversation = useCallback(() => {
    if (sending) return;
    setTurns([]);
    setError(null);
  }, [sending]);

  const cancelPendingPlan = useCallback((turnIndex: number) => {
    setTurns((prev) => {
      const next = prev.slice();
      const current = next[turnIndex];
      if (!current || !current.pendingPlan) return prev;
      // Preserve any partial step results in the cancellation summary
      // so users can see what ran before they bailed out.
      const results = current.stepResults ?? [];
      const summary =
        results.length > 0
          ? formatObservation({
              plan_id: current.pendingPlan?.id ?? "",
              steps: results,
              success: false,
            }) + "\n\n[cancelled before completing plan]"
          : "Plan cancelled.";
      next[turnIndex] = {
        ...current,
        content: summary,
        pendingPlan: undefined,
        stepCursor: undefined,
        stepResults: undefined,
      };
      return next;
    });
  }, []);

  const stepPendingPlan = useCallback(
    async (turnIndex: number) => {
      if (sending) return;
      const target = turns[turnIndex];
      if (!target?.pendingPlan) return;
      const plan = target.pendingPlan;
      const cursor = target.stepCursor ?? 0;
      if (cursor >= plan.steps.length) return;

      setSending(true);
      setError(null);
      let result: StepResult;
      try {
        result = await agentExecuteStep(plan, cursor);
      } catch (err) {
        setError(String(err));
        setSending(false);
        // Record the failure as a step entry so the UI still renders
        // partial progress.
        setTurns((prev) => {
          const next = prev.slice();
          const current = next[turnIndex];
          if (!current) return prev;
          const prior = current.stepResults ?? [];
          next[turnIndex] = {
            ...current,
            stepResults: [
              ...prior,
              {
                step_id: plan.steps[cursor]?.id ?? `step-${cursor}`,
                response: null,
                status: "failed",
              },
            ],
            stepCursor: cursor + 1,
          };
          return next;
        });
        return;
      }

      const nextCursor = cursor + 1;
      const done = nextCursor >= plan.steps.length;
      setTurns((prev) => {
        const next = prev.slice();
        const current = next[turnIndex];
        if (!current) return prev;
        const prior = current.stepResults ?? [];
        const updatedResults = [...prior, result];
        if (done) {
          // Synthesize an Observation-shaped content so persistence
          // looks identical to the approve-all path.
          const observation: Observation = {
            plan_id: plan.id,
            steps: updatedResults,
            success: updatedResults.every((r) => r.status === "ok"),
          };
          next[turnIndex] = {
            ...current,
            pending: false,
            pendingPlan: undefined,
            stepCursor: undefined,
            stepResults: undefined,
            content: formatObservation(observation),
          };
        } else {
          next[turnIndex] = {
            ...current,
            stepCursor: nextCursor,
            stepResults: updatedResults,
          };
        }
        return next;
      });
      setSending(false);
    },
    [sending, turns],
  );

  const approvePendingPlan = useCallback(
    async (turnIndex: number) => {
      if (sending) return;
      const target = turns[turnIndex];
      if (!target?.pendingPlan) return;
      const plan = target.pendingPlan;
      const cursor = target.stepCursor ?? 0;
      const priorResults = target.stepResults ?? [];

      setSending(true);
      setError(null);
      setTurns((prev) => {
        const next = prev.slice();
        const current = next[turnIndex];
        if (!current) return prev;
        next[turnIndex] = {
          ...current,
          pending: true,
          pendingPlan: undefined,
          stepCursor: undefined,
          stepResults: undefined,
          content: cursor > 0 ? "Executing remaining steps…" : "Executing…",
        };
        return next;
      });

      try {
        let observation: Observation;
        if (cursor > 0) {
          // Stepwise partial — run the remainder via execute_step so
          // we don't re-execute completed steps.
          const results = priorResults.slice();
          for (let i = cursor; i < plan.steps.length; i += 1) {
            // eslint-disable-next-line no-await-in-loop
            const r = await agentExecuteStep(plan, i);
            results.push(r);
          }
          observation = {
            plan_id: plan.id,
            steps: results,
            success: results.every((r) => r.status === "ok"),
          };
        } else {
          observation = await agentRunPlan(plan);
        }
        const content = formatObservation(observation);
        setTurns((prev) => {
          const next = prev.slice();
          const current = next[turnIndex];
          if (!current) return prev;
          next[turnIndex] = {
            ...current,
            content,
            pending: false,
          };
          return next;
        });
      } catch (err) {
        setError(String(err));
        setTurns((prev) => {
          const next = prev.slice();
          const current = next[turnIndex];
          if (!current) return prev;
          next[turnIndex] = {
            ...current,
            content: `[error] ${String(err)}`,
            pending: false,
          };
          return next;
        });
      } finally {
        setSending(false);
      }
    },
    [sending, turns],
  );

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  const provider = config?.ai
    ? `${config.ai.provider}${config.ai.model ? ` (${config.ai.model})` : ""}`
    : "not configured";

  const switchSession = useCallback(
    (nextId: string) => {
      if (nextId === chatSessionId || sending) return;
      writeActiveSessionId(nextId);
      setChatSessionId(nextId);
    },
    [chatSessionId, sending],
  );

  const newSession = useCallback(() => {
    if (sending) return;
    const id = newChatSessionId();
    writeActiveSessionId(id);
    setTurns([]);
    setSystemPrompt("");
    setChatSessionId(id);
  }, [sending]);

  const deleteCurrentSession = useCallback(async () => {
    if (sending) return;
    // Legacy / default session is the only fallback — never delete it
    // via this path so there's always somewhere to land after delete.
    if (chatSessionId === DEFAULT_SESSION_ID) return;
    try {
      await aiSessionDelete(chatSessionId);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("[chat] session_delete failed", err);
    }
    writeActiveSessionId(DEFAULT_SESSION_ID);
    setChatSessionId(DEFAULT_SESSION_ID);
  }, [chatSessionId, sending]);

  // Build the picker options: every known session + the active one
  // even if session_list hasn't returned yet (avoids a flash-to-default
  // on first mount).
  const sessionOptions = useMemo(() => {
    const seen = new Map<string, string>();
    for (const s of sessionSummaries) {
      seen.set(s.id, s.title || s.id);
    }
    if (!seen.has(chatSessionId)) {
      seen.set(chatSessionId, chatSessionId);
    }
    if (!seen.has(DEFAULT_SESSION_ID)) {
      seen.set(DEFAULT_SESSION_ID, "default");
    }
    return Array.from(seen.entries());
  }, [sessionSummaries, chatSessionId]);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100%",
        minHeight: 0,
        fontFamily: "var(--font-ui, sans-serif)",
        color: "var(--color-fg)",
        background: "var(--color-bg)",
      }}
    >
      <div
        style={{
          padding: "6px 10px",
          borderBottom: "1px solid var(--color-border)",
          fontSize: 12,
          display: "flex",
          alignItems: "center",
          gap: 8,
        }}
      >
        <span style={{ opacity: 0.75, flex: 1 }}>AI · {provider}</span>
        <button
          type="button"
          onClick={() => setUseAgent((v) => !v)}
          style={chipButtonStyle}
          aria-pressed={useAgent}
          title="When on, each message is sent to com.nexus.agent.run — the planner produces a plan and the executor runs every tool call. Blocks until the full plan completes."
        >
          {useAgent ? "Agent ●" : "Agent"}
        </button>
        <button
          type="button"
          onClick={() => setPreviewAgentPlans((v) => !v)}
          style={chipButtonStyle}
          aria-pressed={previewAgentPlans}
          disabled={!useAgent}
          title="When on, agent plans are shown for approval before any tool calls run. Click Approve to execute or Cancel to discard."
        >
          {previewAgentPlans ? "Preview ●" : "Preview"}
        </button>
        <select
          value={archetype}
          onChange={(e) => setArchetype(e.target.value as AgentArchetype)}
          disabled={!useAgent}
          style={{
            ...chipButtonStyle,
            padding: "2px 6px",
            cursor: useAgent ? "pointer" : "not-allowed",
          }}
          aria-label="Agent archetype"
          title="Planner archetype. Writer biases toward markdown authoring; Coder toward code + git + build; Researcher toward search + RAG."
        >
          <option value="general">general</option>
          <option value="writer">writer</option>
          <option value="coder">coder</option>
          <option value="researcher">researcher</option>
        </select>
        <button
          type="button"
          onClick={() => setUseRag((v) => !v)}
          style={chipButtonStyle}
          aria-pressed={useRag}
          title="When on, each turn retrieves matching chunks from indexed docs and prepends them as context."
          disabled={useAgent}
        >
          {useRag ? "RAG ●" : "RAG"}
        </button>
        <button
          type="button"
          onClick={() => setShowSystem((v) => !v)}
          style={chipButtonStyle}
          aria-pressed={showSystem}
        >
          {systemPrompt.trim() ? "System ●" : "System"}
        </button>
        <select
          value={chatSessionId}
          onChange={(e) => switchSession(e.target.value)}
          disabled={sending}
          aria-label="Chat session"
          title="Active chat session. Each session persists to its own file under `.forge/chat/sessions/`."
          style={{
            ...chipButtonStyle,
            padding: "2px 6px",
            cursor: sending ? "not-allowed" : "pointer",
            maxWidth: 180,
          }}
        >
          {sessionOptions.map(([id, label]) => (
            <option key={id} value={id}>
              {label}
            </option>
          ))}
        </select>
        <button
          type="button"
          onClick={newSession}
          disabled={sending}
          style={chipButtonStyle}
          title="Start a fresh chat session — current conversation is preserved on disk."
        >
          New
        </button>
        <button
          type="button"
          onClick={deleteCurrentSession}
          disabled={sending || chatSessionId === DEFAULT_SESSION_ID}
          style={chipButtonStyle}
          title="Delete the active session. The default session can't be deleted."
        >
          Delete
        </button>
        <button
          type="button"
          onClick={clearConversation}
          disabled={sending || turns.length === 0}
          style={chipButtonStyle}
        >
          Clear
        </button>
      </div>

      {showSystem && (
        <div
          style={{
            padding: "6px 10px",
            borderBottom: "1px solid var(--color-border)",
            background: "var(--color-bg-alt, rgba(127,127,127,0.06))",
          }}
        >
          <label
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 4,
              fontSize: 11,
              opacity: 0.8,
            }}
          >
            System prompt
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              placeholder="Optional instructions that prefix every turn."
              rows={2}
              style={{
                resize: "vertical",
                padding: "4px 6px",
                fontFamily: "inherit",
                fontSize: 12,
                background: "var(--color-bg)",
                color: "var(--color-fg)",
                border: "1px solid var(--color-border)",
                borderRadius: 4,
              }}
            />
          </label>
        </div>
      )}

      <div
        ref={scrollRef}
        style={{
          flex: 1,
          minHeight: 0,
          overflowY: "auto",
          padding: "8px 10px",
          display: "flex",
          flexDirection: "column",
          gap: 10,
        }}
      >
        {turns.length === 0 && (
          <div style={{ opacity: 0.6, fontSize: 13 }}>
            Ask anything. Messages stream live from the configured provider.
          </div>
        )}
        {turns.map((turn, i) => (
          <div
            key={i}
            style={{
              display: "flex",
              flexDirection: "column",
              gap: 2,
              alignSelf: turn.role === "user" ? "flex-end" : "flex-start",
              maxWidth: "85%",
            }}
          >
            <div style={{ fontSize: 11, opacity: 0.6 }}>
              {turn.role === "user" ? "You" : "Assistant"}
              {turn.pending ? " · streaming…" : ""}
            </div>
            {turn.pendingPlan ? (
              <PendingPlanCard
                plan={turn.pendingPlan}
                stepCursor={turn.stepCursor}
                stepResults={turn.stepResults}
                onApprove={() => void approvePendingPlan(i)}
                onStep={() => void stepPendingPlan(i)}
                onCancel={() => cancelPendingPlan(i)}
                running={sending}
              />
            ) : (
              <div
                style={{
                  whiteSpace: "pre-wrap",
                  padding: "6px 10px",
                  borderRadius: 6,
                  background:
                    turn.role === "user"
                      ? "var(--color-bg-alt, rgba(127,127,127,0.12))"
                      : "transparent",
                  border: "1px solid var(--color-border)",
                  fontSize: 14,
                  lineHeight: 1.4,
                }}
              >
                {turn.content || (turn.pending ? "…" : "")}
              </div>
            )}
            {turn.role === "assistant" &&
              turn.sources &&
              turn.sources.length > 0 && (
                <div
                  style={{
                    display: "flex",
                    flexWrap: "wrap",
                    gap: 4,
                    marginTop: 4,
                  }}
                >
                  {turn.sources.map((src, i) => (
                    <span
                      key={`${src.file_path}-${src.block_id}-${i}`}
                      title={`${src.chunk_text.slice(0, 240)}${
                        src.chunk_text.length > 240 ? "…" : ""
                      }\n\nscore: ${src.score.toFixed(3)}`}
                      style={{
                        fontSize: 10,
                        fontFamily: "var(--font-mono, monospace)",
                        padding: "1px 6px",
                        border: "1px solid var(--color-border)",
                        borderRadius: 10,
                        opacity: 0.85,
                        maxWidth: "100%",
                        whiteSpace: "nowrap",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                      }}
                    >
                      {src.file_path}
                    </span>
                  ))}
                </div>
              )}
          </div>
        ))}
      </div>

      {error && (
        <div
          style={{
            padding: "6px 10px",
            borderTop: "1px solid var(--color-border)",
            fontSize: 12,
            color: "var(--color-error, #d00)",
          }}
        >
          {error}
        </div>
      )}

      <div
        style={{
          padding: 8,
          borderTop: "1px solid var(--color-border)",
          display: "flex",
          gap: 6,
        }}
      >
        <textarea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Type a message. Enter to send, Shift+Enter for newline."
          rows={2}
          disabled={sending}
          style={{
            flex: 1,
            resize: "none",
            padding: "6px 8px",
            fontFamily: "inherit",
            fontSize: 13,
            background: "var(--color-bg)",
            color: "var(--color-fg)",
            border: "1px solid var(--color-border)",
            borderRadius: 4,
          }}
        />
        <button
          type="button"
          onClick={() => void send()}
          disabled={sending || !input.trim()}
          style={{
            padding: "6px 12px",
            fontSize: 13,
            border: "1px solid var(--color-border)",
            borderRadius: 4,
            background: "var(--color-bg-alt, transparent)",
            color: "var(--color-fg)",
            cursor: sending ? "not-allowed" : "pointer",
          }}
        >
          {sending ? "…" : "Send"}
        </button>
      </div>
    </div>
  );
}

function PendingPlanCard({
  plan,
  stepCursor,
  stepResults,
  onApprove,
  onStep,
  onCancel,
  running,
}: {
  plan: AgentPlan;
  stepCursor?: number;
  stepResults?: StepResult[];
  onApprove: () => void;
  onStep: () => void;
  onCancel: () => void;
  running: boolean;
}): JSX.Element {
  const cursor = stepCursor ?? 0;
  const results = stepResults ?? [];
  const started = results.length > 0;
  const remaining = plan.steps.length - cursor;
  return (
    <div
      style={{
        padding: "8px 10px",
        borderRadius: 6,
        border: "1px solid var(--color-border)",
        background: "var(--color-bg-alt, rgba(127,127,127,0.06))",
        display: "flex",
        flexDirection: "column",
        gap: 6,
      }}
    >
      <div style={{ fontSize: 11, opacity: 0.75 }}>
        {started
          ? `Plan step ${cursor + 1} of ${plan.steps.length}`
          : `Plan awaiting approval · ${plan.steps.length} step${plan.steps.length === 1 ? "" : "s"}`}
      </div>
      <ol
        style={{
          margin: 0,
          paddingInlineStart: 20,
          fontSize: 13,
          lineHeight: 1.35,
          display: "flex",
          flexDirection: "column",
          gap: 4,
        }}
      >
        {plan.steps.map((step, i) => {
          const result = results[i];
          const isNext = i === cursor && results.length === i;
          return (
            <li
              key={step.id}
              style={{
                opacity: result ? 0.75 : 1,
                fontWeight: isNext ? 500 : undefined,
              }}
            >
              <div>
                {result ? stepBadge(result.status) : isNext ? "▶ " : ""}
                {step.description}
              </div>
              {step.tool_call && (
                <div
                  style={{
                    fontFamily: "var(--font-mono, monospace)",
                    fontSize: 11,
                    opacity: 0.7,
                  }}
                >
                  → {step.tool_call.target_plugin_id}.{step.tool_call.command_id}
                </div>
              )}
            </li>
          );
        })}
      </ol>
      <div style={{ display: "flex", gap: 6, marginTop: 2 }}>
        <button
          type="button"
          onClick={onStep}
          disabled={running || remaining <= 0}
          style={chipButtonStyle}
          title="Run the next step only, then pause for approval again."
        >
          {started ? "Step →" : "Step (one)"}
        </button>
        <button
          type="button"
          onClick={onApprove}
          disabled={running || remaining <= 0}
          style={{
            ...chipButtonStyle,
            background: "var(--color-accent, #3b82f6)",
            color: "var(--color-bg, #fff)",
            borderColor: "transparent",
          }}
          title="Run all remaining steps without stopping."
        >
          {started ? "Approve rest" : "Approve all"}
        </button>
        <button
          type="button"
          onClick={onCancel}
          disabled={running}
          style={chipButtonStyle}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

function stepBadge(status: StepResult["status"]): string {
  switch (status) {
    case "ok":
      return "✓ ";
    case "denied":
      return "⊘ ";
    case "failed":
      return "✗ ";
    case "skipped":
      return "· ";
    default:
      return "";
  }
}
