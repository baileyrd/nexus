// src/registry/priorityOverrides.ts
// P2-02 — per-entry priority overrides for the slot, activity-bar,
// panel-area, and status-bar registries.
//
// Cascade semantics:
//   1. The plugin's contributed `priority` is the default. Same as
//      before P2-02 when no override exists.
//   2. A user can write `nexus.priority.<scope>.<entryId> = N` into
//      `<forge>/.forge/app.toml [settings]` to override the default.
//      The lower the number, the earlier the entry sorts.
//
// At register time each registry calls [`resolveEffectivePriority`]
// with the scope + entry id + declared priority; that helper consults
// the configStore (which mirrors the forge `[settings]` table) and
// returns the user value if present, otherwise the declared default.
//
// The configStore is populated by the configurationService plugin on
// `workspace:opened`. When it hydrates *after* a registry has already
// run register() with the default, the registry needs to re-sort to
// honour the now-visible override. Each registry should subscribe to
// `config:changed:nexus.priority.*` and refresh its sort order — see
// [`subscribePriorityChanges`].

import { useConfigStore } from '../stores/configStore'
import { eventBus } from '../host/EventBus'

/**
 * Scopes correspond to the four sorting registries. Keep the strings
 * short and stable — they appear verbatim in the user-facing setting
 * key. Renaming a scope is a breaking change for users with existing
 * overrides.
 */
export type PriorityScope =
  | 'slot' // generic SlotRegistry entries (overlay, titleBar, …)
  | 'activityBar' // ActivityBarStore
  | 'panelArea' // PanelAreaStore
  | 'statusBar' // StatusBarRegistry

/**
 * Build the canonical setting key for a (scope, entryId) pair.
 * Exposed so subscribers can construct the `config:changed:<key>`
 * topic name when watching a specific entry.
 */
export function priorityKeyFor(scope: PriorityScope, entryId: string): string {
  return `nexus.priority.${scope}.${entryId}`
}

/**
 * Setting-key prefix the four registries listen on to know when
 * any priority override changed. Equal to `nexus.priority.`.
 */
export const PRIORITY_KEY_PREFIX = 'nexus.priority.'

/**
 * Return the override for `(scope, entryId)` if the configStore is
 * hydrated AND holds a numeric entry, otherwise the declared default.
 * Non-numeric override values fall through to the default (logged at
 * the call site if useful — the helper stays silent to keep the hot
 * register path cheap).
 */
export function resolveEffectivePriority(
  scope: PriorityScope,
  entryId: string,
  declared: number,
): number {
  const store = useConfigStore.getState()
  if (!store.hydrated) return declared
  const raw = store.get<unknown>(priorityKeyFor(scope, entryId), null)
  if (typeof raw === 'number' && Number.isFinite(raw)) return raw
  return declared
}

/**
 * Subscribe to every `config:changed:nexus.priority.*` event and
 * invoke `onChange` when the topic matches the given scope. Returns
 * a teardown function that removes the listener.
 *
 * The configStore emits per-key change events; this helper centralises
 * the prefix-match so individual registries don't repeat the logic.
 */
export function subscribePriorityChanges(
  scope: PriorityScope,
  onChange: () => void,
): () => void {
  const prefix = `config:changed:${PRIORITY_KEY_PREFIX}${scope}.`
  // EventBus exposes wildcard subscribe via `onAll(handler(event,
  // payload))` — we filter to topics in our scope so unrelated
  // changes don't trigger a re-sort.
  return eventBus.onAll((topic) => {
    if (topic.startsWith(prefix)) onChange()
  })
}
