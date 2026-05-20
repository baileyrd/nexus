// shell/src/plugins/nexus/recall/recallApi.ts
//
// BL-044 — module-scope handle to the recall plugin's IPC + config
// surface so the overlay component (rendered by the slot system
// without a PluginAPI prop) can reach `semantic_search` +
// `memory.inboxPath`.
//
// Phase 4.1 narrowing: the stored handle is a `RecallApi` adapter
// exposing only the two operations the plugin uses, built once at
// `setRecallApi()` time from the broader `PluginAPI`. Reading this
// file now shows exactly the IPC + config surface recall depends on,
// without grepping downstream for `api.kernel.invoke` /
// `api.configuration.getValue` strings.

import type { PluginAPI } from '../../../types/plugin'

const AI_PLUGIN_ID = 'com.nexus.ai'
const HANDLER_SEMANTIC_SEARCH = 'semantic_search'
const CONFIG_INBOX_PATH = 'memory.inboxPath'
const DEFAULT_INBOX_PATH = 'Inbox.md'

/** Kernel response shape for `com.nexus.ai::semantic_search`.
 *  Matches `nexus_ai::ipc::SemanticSearchReply` — `matches` is the
 *  only field consumers read. */
export interface SemanticSearchResult {
  matches?: unknown
}

/**
 * Narrow surface used by the recall overlay. Every method maps 1:1
 * to a kernel or configuration call; declared explicitly here so the
 * plugin's IPC + config dependencies are visible at a glance.
 */
export interface RecallApi {
  /** `com.nexus.ai::semantic_search` wrapper. */
  semanticSearch(query: string, limit: number): Promise<SemanticSearchResult>
  /** Live read of `memory.inboxPath` (BL-043). Returns `null` when the
   *  configuration registry is not active (e.g. memory plugin off);
   *  the runtime treats `null` as "no inbox-scope filter". */
  getInboxPath(): string | null
}

let _api: RecallApi | null = null

/** Build the narrow adapter from the activate-time PluginAPI. Called
 *  once from `nexus.recall/index.ts::activate`. */
export function setRecallApi(api: PluginAPI): void {
  _api = {
    semanticSearch: (query, limit) =>
      api.kernel.invoke<SemanticSearchResult>(
        AI_PLUGIN_ID,
        HANDLER_SEMANTIC_SEARCH,
        { query, limit },
      ),
    getInboxPath: () => {
      try {
        return api.configuration.getValue<string>(
          CONFIG_INBOX_PATH,
          DEFAULT_INBOX_PATH,
        )
      } catch {
        // Configuration registry not active (memory plugin disabled).
        return null
      }
    },
  }
}

/** Component-side accessor. Throws if `activate` hasn't fired yet,
 *  which would be a host bug. */
export function getRecallApi(): RecallApi {
  if (!_api) {
    throw new Error(
      '[nexus.recall] RecallApi not yet bound — getRecallApi called before activate.',
    )
  }
  return _api
}
