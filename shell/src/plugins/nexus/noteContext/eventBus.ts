// Tiny module-level bridge so accordion-section components (which
// the view registry instantiates without a PluginAPI prop) can emit
// shell events. Mirrors the `let events: EventsAPI | null` pattern
// the legacy four plugins used pre-merge.
//
// `index.ts::activate` calls `setEventBus(api.events)` once at boot;
// sections call `useEventBus()` and may get `null` if their component
// rendered before activation completed (the right-panel host could
// in principle render an empty placeholder while the plugin was
// still booting). Sections must null-check.

import type { EventsAPI } from '../../../types/plugin'

let _events: EventsAPI | null = null

export function setEventBus(events: EventsAPI): void {
  _events = events
}

/** Returns the bound EventsAPI, or null if `activate` hasn't fired yet. */
export function useEventBus(): EventsAPI | null {
  return _events
}
