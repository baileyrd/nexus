// BL-133 follow-up — runtime bridge for the Notifications settings tab.
//
// Captures a narrow kernel handle at activate-time so the React
// component can reach the few `kernel.invoke(...)` calls it actually
// uses without prop-drilling through the generic settings tab
// renderer.
//
// Phase 4.1 narrows the singleton from a full `PluginAPI` to a typed
// interface that lists exactly the IPC handlers this tab depends on
// (4 calls across 2 kernel plugins). That makes the dep surface
// statically visible — a reader of this file can see the entire
// kernel coupling without grepping the consumer.

const SECURITY_PLUGIN_ID = 'com.nexus.security'
const NOTIFICATIONS_PLUGIN_ID = 'com.nexus.notifications'

/** Reply shape from `com.nexus.security::list_secret_names`. */
export interface ListSecretNamesResult {
  names: string[]
}

/** Reply shape from `com.nexus.security::set_secret`. */
export interface SetSecretResult {
  ok: boolean
}

/** Send-notification request body — matches the args
 *  `com.nexus.notifications::send` accepts. */
export interface SendTestArgs {
  source: string
  title: string
  message: string
  channel: string
}

/**
 * Narrow kernel surface used by the Notifications settings tab.
 * Every method maps 1:1 to a `kernel.invoke(<plugin>, <handler>, args)`
 * call; declared explicitly here so the tab's IPC dependencies are
 * visible at a glance.
 */
export interface NotificationsSettingsKernel {
  listSecretNames(plugin_id: string): Promise<ListSecretNamesResult>
  setSecret(plugin_id: string, name: string, value: string): Promise<SetSecretResult>
  deleteSecret(plugin_id: string, name: string): Promise<void>
  sendTest(args: SendTestArgs): Promise<void>
}

/** Minimal shape of the PluginAPI slice we depend on. Kept local so
 *  this module doesn't pull the full `PluginAPI` type into scope. */
interface KernelHandle {
  invoke<T = unknown>(pluginId: string, handler: string, args: unknown): Promise<T>
}

let _kernel: NotificationsSettingsKernel | null = null

/** Called once from `activate()` to wire up the narrow surface. */
export function setNotificationsSettingsKernel(api: { kernel: KernelHandle }): void {
  _kernel = {
    listSecretNames: (plugin_id) =>
      api.kernel.invoke<ListSecretNamesResult>(
        SECURITY_PLUGIN_ID,
        'list_secret_names',
        { plugin_id },
      ),
    setSecret: (plugin_id, name, value) =>
      api.kernel.invoke<SetSecretResult>(
        SECURITY_PLUGIN_ID,
        'set_secret',
        { plugin_id, name, value },
      ),
    deleteSecret: async (plugin_id, name) => {
      await api.kernel.invoke(SECURITY_PLUGIN_ID, 'delete_secret', { plugin_id, name })
    },
    sendTest: async (args) => {
      await api.kernel.invoke(NOTIFICATIONS_PLUGIN_ID, 'send', args)
    },
  }
}

/** Component-side accessor. Throws if the tab is mounted before
 *  `activate()` has run, which would be a host bug. */
export function getNotificationsSettingsKernel(): NotificationsSettingsKernel {
  if (!_kernel) {
    throw new Error('[nexus.notificationsSettings] kernel accessed before activate')
  }
  return _kernel
}
