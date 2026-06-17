// shell/src/plugins/nexus/sandboxPanel/index.ts
//
// Command-palette surface over the OS process sandbox (`com.nexus.security`).
// "Sandbox: Show Policy" introspects the active `.forge/sandbox.toml` config;
// "Sandbox: Brokered Download" runs an allowlisted download on behalf of a
// network-confined process. See docs/0.1.2/os-sandbox.md.

import type { Plugin, PluginAPI, PickItem } from '../../../types/plugin'

const SECURITY_PLUGIN = 'com.nexus.security'
const CMD_POLICY = 'nexus.sandbox.policy'
const CMD_DOWNLOAD = 'nexus.sandbox.download'

/** Subset of the `sandbox_policy` reply the panel renders. */
interface SandboxConfigShape {
  policy?: {
    mode?: string
    writable_roots?: string[]
    network_access?: boolean
  }
  downloads?: {
    enabled?: boolean
    allowed_hosts?: string[]
    max_bytes?: number
  }
}

export const sandboxPanelPlugin: Plugin = {
  manifest: {
    id: 'nexus.sandboxPanel',
    name: 'Sandbox Panel',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.security'],
    contributes: {
      commands: [
        { id: CMD_POLICY, title: 'Sandbox: Show Policy', category: 'Sandbox' },
        { id: CMD_DOWNLOAD, title: 'Sandbox: Brokered Download', category: 'Sandbox' },
      ],
    },
  },

  activate(api: PluginAPI) {
    api.commands.register(CMD_POLICY, async () => {
      const cfg = await api.kernel
        .invoke<SandboxConfigShape>(SECURITY_PLUGIN, 'sandbox_policy', {})
        .catch((e: unknown) => {
          api.notifications.show({
            message: `Sandbox policy failed: ${String(e)}`,
            type: 'error',
          })
          return null
        })
      if (cfg === null) return

      const mode = cfg.policy?.mode ?? 'read-only'
      const net = cfg.policy?.network_access ? 'network: allowed' : 'network: blocked'
      const roots = cfg.policy?.writable_roots ?? []
      const downloads = cfg.downloads?.enabled
        ? `downloads: enabled (${(cfg.downloads.allowed_hosts ?? []).length} allowlisted host(s))`
        : 'downloads: disabled'
      const items: PickItem<string>[] = [
        { label: `mode: ${mode}`, description: net, value: 'mode' },
        {
          label: downloads,
          description: `size cap: ${cfg.downloads?.max_bytes ?? 0} bytes`,
          value: 'downloads',
        },
        ...roots.map((r) => ({ label: `writable root: ${r}`, description: '', value: r })),
      ]
      await api.input.pick(items, { placeholder: 'Active OS-sandbox policy (.forge/sandbox.toml)' })
    })

    api.commands.register(CMD_DOWNLOAD, async () => {
      const url = await api.input.prompt('Brokered download', 'https URL (must be allowlisted)')
      if (url === null) return
      const u = url.trim()
      if (!u) return
      const dest = await api.input.prompt('Destination', 'Absolute path inside a writable root')
      if (dest === null) return
      const d = dest.trim()
      if (!d) return

      const res = await api.kernel
        .invoke<{ bytes_written?: number }>(SECURITY_PLUGIN, 'download', { url: u, dest: d })
        .catch((e: unknown) => {
          // The broker rejects (disabled / off-allowlist / dest outside roots /
          // too large) as an IPC error — surface its message.
          api.notifications.show({ message: `Download refused: ${String(e)}`, type: 'error' })
          return null
        })
      if (res === null) return
      api.notifications.show({
        message: `Downloaded ${res.bytes_written ?? 0} bytes to ${d}`,
        type: 'info',
      })
    })
  },
}
