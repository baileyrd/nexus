// shell/src/plugins/nexus/debugger/debuggerIpc.ts
//
// BL-081 — typed wrappers around `com.nexus.dap::*` IPC.
//
// The Rust handlers accept raw JSON args (camelCase keys per the DAP
// spec on the wire, snake_case keys for the host arg envelopes — see
// `crates/nexus-dap/src/ipc.rs`). This module exposes one function per
// IPC verb so the panel/store/CM6 layers don't repeat the (pluginId,
// commandId) tuple at every call site.

const PLUGIN_ID = 'com.nexus.dap'

/** Subset of `KernelAPI.invoke` we depend on — structurally typed so
 *  tests can mock without fabricating a whole PluginAPI. */
export interface DapKernelAPI {
  invoke<T = unknown>(
    pluginId: string,
    commandId: string,
    args?: unknown,
  ): Promise<T>
}

/** Opaque metadata payload contributed by a plugin's
 *  `protocol_hosts.dap` entry (BL-113). The shell reads
 *  `display_name` for the launch picker and `launch_config_schema`
 *  (relative path inside the plugin directory) to render the launch
 *  form. The shape is intentionally untyped here — first-party DAP
 *  plugins can extend it without churning this interface. */
export interface DapAdapterMetadata {
  display_name?: string
  launch_config_schema?: string
  plugin_id?: string
  root_markers?: string[]
  [key: string]: unknown
}

/** One row from `list_adapters`. */
export interface DapAdapterEntry {
  name: string
  command: string
  args: string[]
  adapter_type: string | null
  file_types: string[]
  disabled: boolean
  connected: boolean
  /** BL-113 follow-up — opaque payload contributed via the plugin
   *  manifest. Present when the adapter was contributed; missing
   *  when it was registered directly via `register_adapter`. */
  metadata?: DapAdapterMetadata | null
}

export interface DapSourceBreakpoint {
  line: number
  condition?: string
  hit_condition?: string
  log_message?: string
}

export interface DapStackFrame {
  id: number
  name: string
  line: number
  column: number
  source?: { path?: string; name?: string }
}

export interface DapScope {
  name: string
  variablesReference: number
  expensive?: boolean
  namedVariables?: number
  indexedVariables?: number
}

export interface DapVariable {
  name: string
  value: string
  type?: string
  variablesReference: number
}

export interface DapThread {
  id: number
  name: string
}

export interface DapStoppedEvent {
  reason: string
  threadId?: number
  description?: string
  text?: string
}

export interface DapOutputEvent {
  category?: string
  output: string
  source?: { path?: string }
  line?: number
}

export function listAdapters(api: DapKernelAPI): Promise<DapAdapterEntry[]> {
  return api.invoke<DapAdapterEntry[]>(PLUGIN_ID, 'list_adapters')
}

export interface LaunchOpts {
  adapter: string
  program: string
  mode?: string
  args?: string[]
  cwd?: string
  env?: Record<string, string>
  stop_on_entry?: boolean
  extra?: unknown
}

export function launch(api: DapKernelAPI, opts: LaunchOpts): Promise<unknown> {
  return api.invoke(PLUGIN_ID, 'launch', opts)
}

export interface AttachOpts {
  adapter: string
  pid?: number
  port?: number
  extra?: unknown
}

export function attach(api: DapKernelAPI, opts: AttachOpts): Promise<unknown> {
  return api.invoke(PLUGIN_ID, 'attach', opts)
}

export function configurationDone(
  api: DapKernelAPI,
  adapter: string,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'configuration_done', { adapter })
}

export function disconnect(
  api: DapKernelAPI,
  adapter: string,
  terminateDebuggee = false,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'disconnect', {
    adapter,
    terminate_debuggee: terminateDebuggee,
  })
}

export function terminate(
  api: DapKernelAPI,
  adapter: string,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'terminate', { adapter })
}

export function setBreakpoints(
  api: DapKernelAPI,
  adapter: string,
  source_path: string,
  breakpoints: DapSourceBreakpoint[],
): Promise<{ breakpoints: Array<{ verified: boolean; line: number }> }> {
  return api.invoke(PLUGIN_ID, 'set_breakpoints', {
    adapter,
    source_path,
    breakpoints,
  })
}

export function continueExecution(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'continue', { adapter, thread_id })
}

export function next(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'next', { adapter, thread_id })
}

export function stepIn(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'step_in', { adapter, thread_id })
}

export function stepOut(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'step_out', { adapter, thread_id })
}

export function pause(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ ok: boolean }> {
  return api.invoke(PLUGIN_ID, 'pause', { adapter, thread_id })
}

export function threads(
  api: DapKernelAPI,
  adapter: string,
): Promise<{ threads: DapThread[] }> {
  return api.invoke(PLUGIN_ID, 'threads', { adapter })
}

export function stackTrace(
  api: DapKernelAPI,
  adapter: string,
  thread_id: number,
): Promise<{ stackFrames: DapStackFrame[]; totalFrames?: number }> {
  return api.invoke(PLUGIN_ID, 'stack_trace', { adapter, thread_id })
}

export function scopes(
  api: DapKernelAPI,
  adapter: string,
  frame_id: number,
): Promise<{ scopes: DapScope[] }> {
  return api.invoke(PLUGIN_ID, 'scopes', { adapter, frame_id })
}

export function variables(
  api: DapKernelAPI,
  adapter: string,
  variables_reference: number,
): Promise<{ variables: DapVariable[] }> {
  return api.invoke(PLUGIN_ID, 'variables', { adapter, variables_reference })
}

export function evaluate(
  api: DapKernelAPI,
  adapter: string,
  expression: string,
  frame_id?: number,
  context: string = 'repl',
): Promise<{ result: string; variablesReference?: number; type?: string }> {
  return api.invoke(PLUGIN_ID, 'evaluate', {
    adapter,
    expression,
    frame_id,
    context,
  })
}
