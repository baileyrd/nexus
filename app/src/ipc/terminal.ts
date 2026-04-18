// Typed wrappers for the terminal core plugin Tauri commands.
//
// These forward straight to `com.nexus.terminal` via kernel IPC (see
// `crates/nexus-app/src/terminal.rs`). Mirrors the helper module
// pattern in `ipc/editor.ts` and the sibling
// `nexus_bootstrap::terminal` Rust helper used by `nexus-tui` — the
// Tauri shell reaches the terminal engine through `invoke(…)` and
// never links `nexus-terminal` directly.

import { invoke } from "@tauri-apps/api/core";

/** Structured line emitted by `term_read_output`. */
export interface OutputLine {
  /** Milliseconds since Unix epoch at first ingestion. */
  timestamp_ms: number;
  /** ANSI-stripped text content (no trailing newline). */
  content: string;
  /** Raw bytes as received from the PTY (includes ANSI sequences). */
  raw: number[];
  /** Adjacent-repeat counter. */
  repeats: number;
}

/** Session metadata returned by `term_get_session_info` / `term_list_sessions`. */
export interface SessionInfo {
  id: string;
  name: string;
  shell: string;
  working_dir: string | null;
  line_count: number;
  created_at: number;
}

/** Arguments accepted by `term_create_session`; every field is optional. */
export interface CreateSessionArgs {
  name?: string;
  shell?: string;
  shellArgs?: string[];
  workingDir?: string;
  env?: [string, string][];
}

interface CreateSessionResponse {
  id: string;
}

interface PumpResponse {
  bytes: number;
}

/**
 * Spawn a new PTY-backed session. Returns the fresh session id.
 *
 * All fields on `args` are optional — omit them to accept the
 * platform-default shell, cwd, and environment.
 */
export async function termCreateSession(
  args: CreateSessionArgs = {},
): Promise<string> {
  const resp = await invoke<CreateSessionResponse>("term_create_session", {
    name: args.name,
    shell: args.shell,
    shellArgs: args.shellArgs,
    workingDir: args.workingDir,
    env: args.env,
  });
  return resp.id;
}

/** Gracefully shut down a session via the §5.1 signal ladder. */
export function termCloseSession(id: string): Promise<void> {
  return invoke<void>("term_close_session", { id });
}

/**
 * Flush `input` (plus a trailing newline if absent) into the session's
 * stdin. Shell-level behaviour only — use [`termSendRawInput`] for
 * control sequences.
 */
export function termSendInput(id: string, input: string): Promise<void> {
  return invoke<void>("term_send_input", { id, input });
}

/** Write raw bytes to the PTY. No newline is added. */
export function termSendRawInput(id: string, data: number[]): Promise<void> {
  return invoke<void>("term_send_raw_input", { id, data });
}

/**
 * Drain the PTY into the session's line buffer. Returns the byte count
 * drained in this pump. Frontend polling loops read this to decide
 * whether to re-fetch the line snapshot.
 */
export async function termPump(
  id: string,
  timeoutMs = 50,
): Promise<number> {
  const resp = await invoke<PumpResponse>("term_pump", { id, timeoutMs });
  return resp.bytes;
}

/** Fetch a range of lines from the session's buffer. */
export function termReadOutput(
  id: string,
  start?: number,
  count?: number,
): Promise<OutputLine[]> {
  return invoke<OutputLine[]>("term_read_output", { id, start, count });
}

/** Literal or regex search. Returns line indices into the current buffer. */
export function termSearchOutput(
  id: string,
  query: string,
  isRegex = false,
): Promise<number[]> {
  return invoke<number[]>("term_search_output", { id, query, isRegex });
}

/** Metadata for one session. */
export function termGetSessionInfo(id: string): Promise<SessionInfo> {
  return invoke<SessionInfo>("term_get_session_info", { id });
}

/** Every session the server knows about. */
export function termListSessions(): Promise<SessionInfo[]> {
  return invoke<SessionInfo[]>("term_list_sessions");
}

// ─── Saved commands (PRD-09 §14.1) ─────────────────────────────────────────
//
// JSON-compatible view of `nexus_terminal::SavedCommand`. Field names use
// snake_case to match the Rust struct's default serde layout — the
// frontend store converts to/from camelCase at the boundary.

export interface SavedCommandDto {
  slug: string;
  name: string;
  shell: string;
  shell_cmd: string;
  working_dir: string | null;
  env_vars: Record<string, string>;
  env_file: string | null;
  icon: string;
  auto_restart: boolean;
  auto_restart_delay_ms: number;
  memory_limit_mb: number | null;
  sidebar_order: number | null;
  pre_commands: string[];
  created_at: number;
  updated_at: number;
}

export function termSavedList(): Promise<SavedCommandDto[]> {
  return invoke<SavedCommandDto[]>("term_saved_list");
}

export function termSavedCreate(
  command: SavedCommandDto,
): Promise<SavedCommandDto> {
  return invoke<SavedCommandDto>("term_saved_create", { command });
}

export function termSavedUpdate(
  command: SavedCommandDto,
): Promise<SavedCommandDto> {
  return invoke<SavedCommandDto>("term_saved_update", { command });
}

export function termSavedDelete(slug: string): Promise<void> {
  return invoke<void>("term_saved_delete", { slug });
}

export function termSavedReorder(
  slug: string,
  sidebarOrder: number | null,
): Promise<void> {
  return invoke<void>("term_saved_reorder", { slug, sidebarOrder });
}
