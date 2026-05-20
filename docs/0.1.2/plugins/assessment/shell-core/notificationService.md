# notificationService

- **Path:** `shell/src/plugins/core/notificationService/`
- **Tier:** Shell Core
- **Plugin id:** `core.notification-service`

## Architecture
- Entry point: `shell/src/plugins/core/notificationService/index.ts:61`
- Activation: `onStartup`
- Modules:
  - `index.ts` — defines `Notification`, `NotificationQueue`, and registers a `NotificationQueue` instance under the `notificationQueue` internal service
- Persistence: in-memory ring (the per-channel inbox + history is `nexus.notificationsInbox`'s responsibility; this plugin only owns the live toast queue)
- Settings owned: `ui.notificationDurationMs` (default 4000) under category `system`
- External deps: reads `configStore` directly for the duration default

## Surface
- **Internal services registered:** `notificationQueue` — instance of `NotificationQueue` with `push`, `dismiss`, `getAll`, `subscribe`
- **Configuration:** one schema entry — `ui.notificationDurationMs` (number)
- **Commands / keybindings / views:** none
- **Consumes from `@nexus/extension-api`:** `Plugin`, `PluginAPI` types; uses `api.configuration.register`, `api.internal.registerInternalService`
- **Notification shape:** `{ id, message, type: 'info'|'warning'|'error'|'success', duration, actions?, timestamp }`

## Necessity
- **Verdict:** Useful
- **Required for basic capabilities?** No — opening, browsing, editing, searching, and committing markdown do not strictly require toast notifications. Errors would still bubble through `clientLogger`. But removal would noticeably degrade UX (no success toasts on save, no error surfacing for git/AI/file-watch errors).
- **Depended on by:** `nexus.notifications` (consumes the `com.nexus.notifications.delivered` bus event and forwards through `api.notifications.show`), and any plugin calling `api.notifications.show` directly — including `core.terminal`, `core.zoom`, `core.fileExplorer`, `nexus.ai`, `nexus.linkSuggest`, `nexus.bookmarks`, `nexus.editor`, `nexus.canvas`, `nexus.terminal`, plus `nexus.dreamCycle` / `nexus.crdtConflict` / `nexus.diagnostics` which use it as their primary surfacing channel.
- **Depends on:** `core.configuration-service` (reads `ui.notificationDurationMs` via `configStore`)
- **What breaks if removed:** Every `api.notifications.show` call no-ops or throws; toast UI disappears; downstream notification plugins (`nexus.notifications`, `nexus.notificationsInbox`) lose their feed source.

## Notes
- The queue auto-dismisses entries with `duration > 0` via `setTimeout`; `duration <= 0` is sticky-until-dismissed.
- Action buttons are declared (`actions?: { label, command }[]`) but rendering them is the responsibility of whichever overlay surfaces the queue — `core/notificationService` only owns the data.
- The implementation reads `configStore` directly rather than going through `api.configuration`, which couples this plugin to the configStore singleton.
