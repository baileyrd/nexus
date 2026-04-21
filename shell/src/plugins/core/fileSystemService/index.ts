// src/plugins/core/fileSystemService/index.ts
// Service plugin — wraps Tauri's filesystem API into a sanctioned abstraction.

import type { Plugin, PluginAPI, FileEntry, FsEvent } from '../../../types/plugin'
import {
  readTextFile,
  writeTextFile,
  readDir,
  exists,
  mkdir,
  remove,
  rename,
  watch,
} from '@tauri-apps/plugin-fs'

export class FilesystemService {
  async read(path: string): Promise<string> {
    return readTextFile(path)
  }

  async write(path: string, content: string): Promise<void> {
    return writeTextFile(path, content)
  }

  async list(path: string): Promise<FileEntry[]> {
    const entries = await readDir(path)
    return entries.map(e => ({
      name: e.name ?? '',
      path: `${path}/${e.name}`,
      isDirectory: e.isDirectory ?? false,
    }))
  }

  async exists(path: string): Promise<boolean> {
    return exists(path)
  }

  async mkdir(path: string): Promise<void> {
    return mkdir(path, { recursive: true })
  }

  async delete(path: string): Promise<void> {
    return remove(path)
  }

  async rename(from: string, to: string): Promise<void> {
    return rename(from, to)
  }

  async watch(path: string, handler: (event: FsEvent) => void): Promise<() => void> {
    const unwatch = await watch(path, (event) => {
      const kind = String((event as any).type ?? '')
      handler({
        kind: kind.includes('create') ? 'created'
            : kind.includes('remove') ? 'deleted'
            : kind.includes('rename') ? 'renamed'
            : 'modified',
        path: String((event as any).paths?.[0] ?? path),
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
    api.internal!.registerInternalService('fsService', new FilesystemService())
    console.info('[core.filesystem-service] ready')
  },
}
