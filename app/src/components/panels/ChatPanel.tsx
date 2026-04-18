// React chat panel backed by `com.nexus.ai` core-plugin dispatch
// (PRD-12 §6). Registers as content-type `"com.nexus.ai.chat"` and
// streams assistant tokens via the `ai:stream_*` Tauri events that
// the `nexus-ai-event-forwarder` publishes off the kernel bus.
//
// The panel is deliberately thin — it owns no chat memory beyond the
// in-flight turn. Persistence, session history, and RAG injection are
// left to future PRD slices so the shell stays a generic content-type
// host per the microkernel architecture.

import type { KeyboardEvent } from "react";
import { useCallback, useEffect, useRef, useState } from "react";

import {
  aiConfig,
  aiStreamChat,
  onAiStreamChunk,
  onAiStreamDone,
  onAiStreamStart,
  type AiConfigSnapshot,
  type ChatMessage,
} from "../../ipc/ai";

type Turn = { role: "user" | "assistant"; content: string; pending?: boolean };

function makeSessionId(): string {
  return `chat-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export function ChatPanel(): JSX.Element {
  const [turns, setTurns] = useState<Turn[]>([]);
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

    const finalizeTurn = (sessionId: string, finalText?: string) => {
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

    onAiStreamDone((ev) => finalizeTurn(ev.session_id, ev.text)).then((fn) =>
      unlisteners.push(fn),
    );

    return () => {
      for (const fn of unlisteners) fn();
    };
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [turns]);

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
      await aiStreamChat(history, { sessionId });
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
  }, [config, input, sending, turns]);

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
          opacity: 0.75,
        }}
      >
        AI · {provider}
      </div>

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
