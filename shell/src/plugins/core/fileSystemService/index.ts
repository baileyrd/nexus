// src/plugins/core/fileSystemService/index.ts
// Service plugin — delegates filesystem ops to the sanctioned `api.platform.fs`
// adapter surface (WI-25 Phase 2b). Retains a narrow `watch` import from
// `@tauri-apps/plugin-fs` only, because `api.platform.fs` has no watch()
// equivalent yet; orchestrator will keep this file allowlisted under WI-23
// with that tight justification.

import type { Plugin, PluginAPI, FileEntry, FsEvent } from '../../../types/plugin'
import { watch } from '@tauri-apps/plugin-fs'
import { clientLogger } from '../../../clientLogger'

type PlatformFs = PluginAPI['platform']['fs']

export class FilesystemService {
  constructor(private platformFs: PlatformFs) {}

  async read(path: string): Promise<string> {
    return this.platformFs.readText(path)
  }

  async write(path: string, content: string): Promise<void> {
    return this.platformFs.writeText(path, content)
  }

  async list(path: string): Promise<FileEntry[]> {
    const entries = await this.platformFs.readDir(path)
    return entries.map(e => ({
      name: e.name,
      path: `${path}/${e.name}`,
      isDirectory: e.isDirectory,
    }))
  }

  async exists(path: string): Promise<boolean> {
    return this.platformFs.exists(path)
  }

  async mkdir(path: string): Promise<void> {
    return this.platformFs.mkdir(path, { recursive: true })
  }

  async delete(path: string): Promise<void> {
    return this.platformFs.remove(path)
  }

  async rename(from: string, to: string): Promise<void> {
    return this.platformFs.rename(from, to)
  }

  async watch(path: string, handler: (event: FsEvent) => void): Promise<() => void> {
    // Tauri's plugin-fs watcher emits `{ type, paths }` records; the
    // SDK's published type is loose, so we narrow locally.
    interface RawWatchEvent { type?: unknown; paths?: unknown }
    const unwatch = await watch(path, (event) => {
      const raw = event as RawWatchEvent
      const kind = String(raw.type ?? '')
      const paths = Array.isArray(raw.paths) ? raw.paths : []
      handler({
        kind: kind.includes('create') ? 'created'
            : kind.includes('remove') ? 'deleted'
            : kind.includes('rename') ? 'renamed'
            : 'modified',
        path: String(paths[0] ?? path),
      })
    })
    return unwatch
  }
}

export const fileSystemServicePlugin: Plugin = {
  manifest: {
    id: 'core.filesystem-service',
    name: 'Filesystem Service',
    version: '1.0.0',
    core: true,
    activationEvents: ['onStartup'],
    contributes: {},
  },

  activate(api: PluginAPI) {
    api.internal!.registerInternalService('fsService', new FilesystemService(api.platform.fs))
    clientLogger.info('[core.filesystem-service] ready')
  },
}
