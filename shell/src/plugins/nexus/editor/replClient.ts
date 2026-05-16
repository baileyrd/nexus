// BL-142 Phase 2a — typed wrapper over the `com.nexus.terminal` REPL
// surface (handlers ids 26–29). Mirrors the shape of `kernelClient.ts`
// for the editor — pure transport, no UI state. The Phase 2b CM6
// extensions consume this client; Phase 2a ships it alongside the
// `repl: bool` block-tree flag + config schema + session store so the
// Phase 2b PR has a clean surface to plug into.
//
// Why this lives under `shell/src/plugins/nexus/editor/` rather than
// under `shell/src/plugins/nexus/terminal/`:
//
// - The REPL UI is an *editor* surface (code-cell gutter, inline
//   output below cells, Shift-Enter in the editor) — the terminal
//   plugin has no UI binding here.
// - The terminal plugin's existing TS code calls
//   `api.kernel.invoke('com.nexus.terminal', ...)` directly without
//   a typed wrapper; introducing a typed REPL client over there would
//   create a one-off split. Better to colocate with the consumer.

import type { KernelAPI } from '../../../types/plugin.ts'

/** Reverse-DNS id of the terminal core plugin (see
 *  `crates/nexus-terminal/src/core_plugin.rs:38`). */
export const TERMINAL_PLUGIN_ID = 'com.nexus.terminal'

/** Command strings exposed by the bootstrap manifest for the REPL
 *  surface (see `crates/nexus-bootstrap/src/plugins/terminal.rs`). */
const CMD = {
  replStart: 'repl_start',
  replEval: 'repl_eval',
  replStop: 'repl_stop',
  replList: 'repl_list',
} as const

/** BL-142 Phase 1 wire shape for `repl_start` arguments. Mirrors
 *  `nexus_terminal::ReplStartArgs`. */
export interface ReplStartArgs {
  /** Caller-supplied language tag (`"python"`, `"node"`, …). */
  lang: string
  /** Absolute path or `$PATH`-resolvable program name (e.g.
   *  `"python3"`). */
  program: string
  /** Args appended after `program` (e.g. `["-i"]`). */
  args?: string[]
  /** Working directory for the spawned kernel. */
  working_dir?: string
  /** Env overrides merged on top of the inherited environment. */
  env?: Array<[string, string]>
}

/** Response shape from `repl_start`. */
export interface ReplStartResponse {
  /** Fresh session id (same shape as `create_session`'s response). */
  id: string
  /** Echo of the caller-supplied `lang`. */
  lang: string
}

/** Entry shape returned by `repl_list`. Mirrors
 *  `nexus_terminal::ReplInfo`. */
export interface ReplInfo {
  id: string
  lang: string
  program: string
  args: string[]
  /** Unix epoch milliseconds at `repl_start` time. */
  started_at_ms: number
}

/**
 * Client exposing the four REPL handlers as typed methods. Construct
 * once per consumer (the editor REPL plugin); the `api` parameter
 * lets tests mock `invoke` without standing up a kernel.
 */
export class ReplClient {
  private readonly api: KernelAPI

  constructor(api: KernelAPI) {
    this.api = api
  }

  /** Spawn a language kernel and register it as a REPL session.
   *  See `crates/nexus-terminal/src/core_plugin.rs::dispatch_repl_start`. */
  start(args: ReplStartArgs): Promise<ReplStartResponse> {
    return this.api.invoke<ReplStartResponse>(TERMINAL_PLUGIN_ID, CMD.replStart, args)
  }

  /** Send `code` to the REPL session's PTY stdin. Output streams
   *  asynchronously on `com.nexus.terminal.output.<id>`. */
  async eval(id: string, code: string): Promise<void> {
    await this.api.invoke(TERMINAL_PLUGIN_ID, CMD.replEval, { id, code })
  }

  /** Close a REPL session and remove its bookkeeping entry. */
  async stop(id: string): Promise<void> {
    await this.api.invoke(TERMINAL_PLUGIN_ID, CMD.replStop, { id })
  }

  /** Snapshot every currently-registered REPL session. */
  list(): Promise<ReplInfo[]> {
    return this.api.invoke<ReplInfo[]>(TERMINAL_PLUGIN_ID, CMD.replList, {})
  }
}

/** Convenience constructor mirroring `makeEditorClient`. */
export function makeReplClient(api: KernelAPI): ReplClient {
  return new ReplClient(api)
}
