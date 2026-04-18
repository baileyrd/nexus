// Typed wrappers for the AI core-plugin Tauri commands.
//
// Chat completions stream via three side-channel Tauri events:
//   - `ai:stream_start` → `{ session_id }`
//   - `ai:stream_chunk` → `{ session_id, chunk, index }`
//   - `ai:stream_done`  → `{ session_id, text }`
// Subscribe with `listen()` from `@tauri-apps/api/event`. The backing
// kernel events (`com.nexus.ai.stream_*`) are forwarded by the
// `nexus-ai-event-forwarder` thread started in `nexus-app::run`.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type ChatRole = "system" | "user" | "assistant";

export interface ChatMessage {
  role: ChatRole;
  content: string;
}

export interface AiConfig {
  provider: string;
  model: string | null;
  base_url: string | null;
  has_api_key: boolean;
}

export interface AiConfigSnapshot {
  ai: AiConfig | null;
  embedding: AiConfig | null;
}

export interface StreamStart {
  session_id: string;
}

export interface StreamChunk {
  session_id: string;
  chunk: string;
  index: number;
}

export interface StreamDone {
  session_id: string;
  text: string;
}

export interface StreamChatResult {
  session_id: string;
  text: string;
}

export interface RagSource {
  file_path: string;
  block_id: number;
  chunk_text: string;
  score: number;
}

export interface StreamAskResult {
  session_id: string;
  text: string;
  sources: RagSource[];
}

export function aiConfig(): Promise<AiConfigSnapshot> {
  return invoke<AiConfigSnapshot>("ai_config");
}

export function aiStreamChat(
  messages: ChatMessage[],
  options: { system?: string; sessionId?: string } = {},
): Promise<StreamChatResult> {
  return invoke<StreamChatResult>("ai_stream_chat", {
    messages,
    system: options.system ?? null,
    sessionId: options.sessionId ?? null,
  });
}

export function aiStreamAsk(
  messages: ChatMessage[],
  options: { sessionId?: string; limit?: number } = {},
): Promise<StreamAskResult> {
  return invoke<StreamAskResult>("ai_stream_ask", {
    messages,
    sessionId: options.sessionId ?? null,
    limit: options.limit ?? null,
  });
}

export function onAiStreamStart(
  handler: (ev: StreamStart) => void,
): Promise<UnlistenFn> {
  return listen<StreamStart>("ai:stream_start", (e) => handler(e.payload));
}

export function onAiStreamChunk(
  handler: (ev: StreamChunk) => void,
): Promise<UnlistenFn> {
  return listen<StreamChunk>("ai:stream_chunk", (e) => handler(e.payload));
}

export function onAiStreamDone(
  handler: (ev: StreamDone) => void,
): Promise<UnlistenFn> {
  return listen<StreamDone>("ai:stream_done", (e) => handler(e.payload));
}
