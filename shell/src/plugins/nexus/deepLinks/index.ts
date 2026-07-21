// C71 (#424) — the `nexus://` deep-link pipeline (tauri.conf.json's
// registered scheme, `on_open_url` → `nexus:url-opened`, main.tsx's
// dispatch into `uriHandlerRegistry`) was fully wired but had zero
// registered handlers, so every `nexus://` link was logged and
// dropped. This plugin is that missing handler:
//
//   nexus://open?path=<relpath>                     — open a file
//   nexus://search?q=<query>                        — focus search, prefilled
//   nexus://new?path=<relpath>&content=<text>        — create-or-open a file
//
// `UriHandlerRegistry` allows exactly one handler per scheme
// (first-match-wins), so all three actions route through a single
// `nexus` registration and dispatch on `uri.hostname` — see
// `deepLinkAction.ts` for why that's where WHATWG URL parsing puts
// the action name for a `scheme://action?...` URL.
//
// Also the missing consumer for the previously-dead "Allow URI
// callbacks" toggle (`nexus.settings.files.allowUriCallbacks`,
// SettingsPanelView.tsx): when enabled and the URI carries an
// `x-success` / `x-error` param (Obsidian's x-callback-url
// convention), the corresponding callback URL is opened after the
// action settles.

import type { Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'
import { useSearchStore } from '../search/searchStore'
import { parseDeepLink } from './deepLinkAction'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const EVENT_FILE_OPEN = 'files:open'
const CONFIG_KEY_ALLOW_CALLBACKS = 'nexus.settings.files.allowUriCallbacks'

const utf8Encoder = new TextEncoder()

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

async function fileExists(api: PluginAPI, relpath: string): Promise<boolean> {
  try {
    const raw = await api.kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_files', {
      prefix: relpath,
    })
    if (!Array.isArray(raw)) return false
    return raw.some((row) => {
      if (!row || typeof row !== 'object') return false
      return (row as Record<string, unknown>).path === relpath
    })
  } catch {
    return false
  }
}

async function handleOpen(api: PluginAPI, path: string): Promise<void> {
  if (!(await fileExists(api, path))) {
    throw new Error(`"${path}" does not exist in this forge`)
  }
  api.events.emit(EVENT_FILE_OPEN, { relpath: path, name: basename(path) })
}

async function handleSearch(api: PluginAPI, query: string): Promise<void> {
  useSearchStore.getState().setQuery(query)
  await api.commands.execute('nexus.search.focus')
}

/**
 * Create-or-open, matching the idempotent daily-note pattern: an
 * existing file is opened as-is (a deep link must never silently
 * clobber a note the user is already working on); a missing one is
 * written with `content` first.
 */
async function handleNew(api: PluginAPI, path: string, content: string): Promise<void> {
  if (!(await fileExists(api, path))) {
    const bytes = Array.from(utf8Encoder.encode(content))
    await api.kernel.invoke(STORAGE_PLUGIN_ID, 'write_file', { path, bytes })
  }
  api.events.emit(EVENT_FILE_OPEN, { relpath: path, name: basename(path) })
}

/** x-callback-url bonus (gated on the "Allow URI callbacks" setting,
 *  default off): open `x-success` on success, `x-error` on failure. */
async function maybeFireCallback(api: PluginAPI, uri: URL, ok: boolean): Promise<void> {
  if (!api.configuration.getValue<boolean>(CONFIG_KEY_ALLOW_CALLBACKS, false)) return
  const callback = uri.searchParams.get(ok ? 'x-success' : 'x-error')
  if (!callback) return
  try {
    await api.platform.shell.openExternal(callback)
  } catch (e) {
    clientLogger.warn('[nexus.deepLinks] failed to open callback URL:', e)
  }
}

export const deepLinksPlugin: Plugin = {
  manifest: {
    id: 'nexus.deepLinks',
    name: 'Deep Links',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.storage', 'nexus.search'],
  },

  activate(api: PluginAPI) {
    api.uri.register('nexus', async (uri) => {
      const action = parseDeepLink(uri)
      if (!action) {
        clientLogger.warn(`[nexus.deepLinks] unrecognized or malformed nexus:// URI: ${uri.href}`)
        return
      }

      let ok = true
      try {
        switch (action.kind) {
          case 'open':
            await handleOpen(api, action.path)
            break
          case 'search':
            await handleSearch(api, action.query)
            break
          case 'new':
            await handleNew(api, action.path, action.content)
            break
        }
      } catch (e) {
        ok = false
        api.notifications.show({
          message: `nexus:// link failed: ${String((e as Error)?.message ?? e)}`,
          type: 'error',
        })
      }
      await maybeFireCallback(api, uri, ok)
    })
  },
}
