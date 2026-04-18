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
import { useCallback, useEffect, useRef, useState } from "react";

import {
  aiConfig,
  aiStreamAsk,
  aiStreamChat,
  onAiStreamChunk,
  onAiStreamDone,
  onAiStreamStart,
  type AiConfigSnapshot,
  type ChatMessage,
  type RagSource,
} from "../../ipc/ai";
import { agentRun, type Observation } from "../../ipc/agent";

type Turn = {
  role: "user" | "assistant";
  content: string;
  pending?: boolean;
  sources?: RagSource[];
};

const STORAGE_KEY = "nexus.chat.v1";

interface Persisted {
  turns: Turn[];
  systemPrompt: string;
}

function loadPersisted(): Persisted {
  if (typeof window === "undefined") return { turns: [], systemPrompt: "" };
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return { turns: [], systemPrompt: "" };
    const parsed = JSON.parse(raw) as Partial<Persisted>;
    const turns = Array.isArray(parsed.turns)
      ? parsed.turns.filter(isTurn).map((t) => ({ ...t, pending: false }))
      : [];
    const systemPrompt = typeof parsed.systemPrompt === "string"
      ? parsed.systemPrompt
      : "";
    return { turns, systemPrompt };
  } catch {
    return { turns: [], systemPrompt: "" };
  }
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
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    /* quota or privacy mode — ignore */
  }
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
  const initial = loadPersisted();
  const [turns, setTurns] = useState<Turn[]>(initial.turns);
  const [systemPrompt, setSystemPrompt] = useState(initial.systemPrompt);
  const [showSystem, setShowSystem] = useState(false);
  const [useRag, setUseRag] = useState(false);
  const [useAgent, setUseAgent] = useState(false);
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
      setTurns((prev) => {
        const idx = assistantIndexRef.current;
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
      assistantIndexRef.current = null;
      sessionRef.current = null;
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
    if (sending) return;
    const cleanTurns = turns.map((t) => ({ ...t, pending: false }));
    persist({ turns: cleanTurns, systemPrompt });
  }, [turns, systemPrompt, sending]);

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
      if (useAgent) {
        // Agent mode: dispatch to com.nexus.agent::run which plans +
        // executes tool calls. No token streaming today — the panel
        // shows a pending turn and replaces it with a rendered
        // observation when the plugin returns.
        const observation = await agentRun(trimmed);
        const content = formatObservation(observation);
        // Close out the pending turn manually since there's no
        // stream_done event on the agent path.
        setTurns((prev) => {
          const idx = assistantIndexRef.current;
          if (idx === null) return prev;
          const next = prev.slice();
          const current = next[idx];
          if (!current) return prev;
          next[idx] = { ...current, content, pending: false };
          return next;
        });
        assistantIndexRef.current = null;
        sessionRef.current = null;
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
  }, [config, input, sending, turns, systemPrompt, useRag, useAgent]);

  const clearConversation = useCallback(() => {
    if (sending) return;
    setTurns([]);
    setError(null);
  }, [sending]);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void send();
    }
  };

  const provider = config?.ai
    ? `${config.ai.provider}${config.ai.model ? ` (${config.ai.model})` : ""}`
    : "not configured";

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
