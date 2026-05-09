// BL-077 — typed shell-side wrapper around the `com.nexus.lsp` IPC
// surface (BL-076).
//
// Mirrors `kernelClient.ts`'s shape — the CM6 LSP extension never
// touches `KernelAPI` directly so unit tests can swap a stub here
// rather than a fake `KernelAPI`. Each method is a thin envelope
// over `kernel.invoke('com.nexus.lsp', '<verb>', args)`. We
// deliberately leak `unknown` for response shapes — the host is a
// transparent JSON proxy and consumers cast at the use site, where
// the LSP spec is the authoritative shape.

import type { KernelAPI } from '../../../../types/plugin.ts'

/** Reverse-DNS id of the LSP host plugin (mirrors `nexus_lsp::core_plugin::PLUGIN_ID`). */
export const LSP_PLUGIN_ID = 'com.nexus.lsp'

/** Topic prefix the host republishes server-pushed notifications under. */
export const LSP_NOTIFICATION_PREFIX = 'com.nexus.lsp.'

/** Diagnostic-event topic — `params` is the raw LSP `PublishDiagnosticsParams`. */
export const LSP_DIAGNOSTICS_TOPIC = 'com.nexus.lsp.textDocument.publishDiagnostics'

/** Args mirror — see `crates/nexus-lsp/src/ipc.rs::LspOpenFileArgs`. */
export interface LspOpenFileArgs {
  path: string
  content: string
  language_id?: string
  version?: number
}

export interface LspChangeFileArgs {
  path: string
  content: string
  version: number
}

export interface LspPositionArgs {
  path: string
  line: number
  character: number
}

export interface LspReferencesArgs extends LspPositionArgs {
  include_declaration?: boolean
}

export interface LspRenameArgs extends LspPositionArgs {
  new_name: string
}

export interface LspCodeActionsArgs {
  path: string
  range: {
    start: { line: number; character: number }
    end: { line: number; character: number }
  }
}

/** Args for `execute_command` (handler 12). BL-077 follow-up —
 *  drives `workspace/executeCommand` for code actions whose `edit`
 *  field is missing but whose `command` field carries a server-side
 *  action name. `path` is the routing hint used to pick the
 *  configured server. */
export interface LspExecuteCommandArgs {
  path: string
  command: string
  arguments?: unknown[]
}

/** Reply mirror for `open_file` — `null` when no server is routed for the path. */
export interface LspOpenFileReply {
  uri: string
  server: string
}

/** One row in `list_servers`. */
export interface LspServerEntry {
  name: string
  command: string
  args: string[]
  file_types: string[]
  disabled: boolean
}

/** LSP `PublishDiagnosticsParams` — the bus-republished topic payload. */
export interface PublishDiagnosticsParams {
  uri: string
  version?: number
  diagnostics: LspDiagnostic[]
}

export interface LspDiagnostic {
  range: LspRange
  severity?: 1 | 2 | 3 | 4 // 1 Error, 2 Warning, 3 Info, 4 Hint
  code?: string | number
  source?: string
  message: string
}

export interface LspRange {
  start: LspPosition
  end: LspPosition
}

export interface LspPosition {
  line: number
  character: number
}

/** Subset of `KernelAPI` the adapter actually uses; keeps tests easy to mock. */
export interface LspKernelHandle {
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
  ): Promise<T>
  on<T = unknown>(
    topicPrefix: string,
    handler: (topic: string, payload: T) => void,
  ): Promise<() => void>
}

/**
 * Typed adapter over `com.nexus.lsp`. Every method is one IPC call;
 * errors propagate as the kernel's `"<Variant>: <message>"` string.
 */
export class LspIpc {
  private readonly api: LspKernelHandle

  constructor(api: LspKernelHandle | KernelAPI) {
    // KernelAPI satisfies LspKernelHandle structurally; the cast is
    // narrowing rather than widening.
    this.api = api as LspKernelHandle
  }

  listServers(): Promise<LspServerEntry[]> {
    return this.api.invoke<LspServerEntry[]>(LSP_PLUGIN_ID, 'list_servers')
  }

  /** Returns `null` for paths not routed to any configured server. */
  openFile(args: LspOpenFileArgs): Promise<LspOpenFileReply | null> {
    return this.api.invoke<LspOpenFileReply | null>(
      LSP_PLUGIN_ID,
      'open_file',
      args,
    )
  }

  closeFile(path: string): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'close_file', { path })
  }

  changeFile(args: LspChangeFileArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'change_file', args)
  }

  completions(args: LspPositionArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'completions', args)
  }

  hover(args: LspPositionArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'hover', args)
  }

  definition(args: LspPositionArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'definition', args)
  }

  references(args: LspReferencesArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'references', args)
  }

  rename(args: LspRenameArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'rename', args)
  }

  codeActions(args: LspCodeActionsArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'code_actions', args)
  }

  /**
   * BL-077 follow-up — `workspace/executeCommand`. Reply is whatever
   * the server returns for the named command (often `null`).
   */
  executeCommand(args: LspExecuteCommandArgs): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'execute_command', args)
  }

  format(path: string): Promise<unknown> {
    return this.api.invoke(LSP_PLUGIN_ID, 'format', { path })
  }

  /**
   * Subscribe to server-pushed `publishDiagnostics` events for the
   * lifetime of the returned unsubscribe handle.
   */
  onDiagnostics(
    handler: (params: PublishDiagnosticsParams) => void,
  ): Promise<() => void> {
    return this.api.on<PublishDiagnosticsParams>(
      LSP_DIAGNOSTICS_TOPIC,
      (_topic, payload) => handler(payload),
    )
  }
}
