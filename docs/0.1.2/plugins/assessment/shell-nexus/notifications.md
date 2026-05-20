# Notifications

Three plugins fan the kernel `com.nexus.notifications` surface (BL-133 / BL-136)
into the shell: a live toast bridge (`notifications`), a persistent inbox panel
(`notificationsInbox`), and a per-channel credential settings tab
(`notificationsSettings`). Note that the in-app toast renderer itself lives in
`core/notificationService`; the plugins here are the bus-to-UI bridges.

### notifications

- **Path:** `shell/src/plugins/nexus/notifications/`
- **Surface:**
  - No UI contributions; pure event bridge.
  - Subscribes to `com.nexus.notifications.delivered`, projects each payload
    into a toast `type` (info / warning / error / success) and routes it
    through `api.notifications.show`.
  - When the window is unfocused, also fires the Tauri `notify_desktop`
    bridge command so backgrounded windows still surface the alert.
- **Depends on:** `com.nexus.notifications` (Desktop channel) for the bus
  event; `core/notificationService` for the toast renderer; Tauri's
  `notify_desktop` bridge command (best-effort).
- **Verdict:** Useful
- **Rationale:** Without this plugin every `com.nexus.notifications.delivered`
  bus event is a no-op in the shell. Most plugins emit notifications (git,
  diagnostics, workflow, agent), so silencing them globally is a sharp edge.
  Not strictly required for plain markdown editing, but the absence is felt
  the first time any background task fails.

### notificationsInbox

- **Path:** `shell/src/plugins/nexus/notificationsInbox/`
- **Plugin id:** `nexus.notificationsInbox`, display name "Notification
  Center".
- **Surface:**
  - Pane-mode view `nexus.notificationsInbox.view` with unread/total count,
    per-source filter chips, mark-read / dismiss actions, and a
    "Jump to run →" link for rows carrying a `task_id`.
  - Activity-bar item priority 57 (bell icon) between activity timeline (55)
    and processes (60).
  - Commands `nexus.notificationsInbox.show` /
    `nexus.notificationsInbox.refresh`.
  - Hydrates from `com.nexus.notifications::inbox_list` and subscribes to
    `com.nexus.notifications.inbox.appended`; mark-read / dismiss go through
    `inbox_mark_read` / `inbox_dismiss`.
- **Depends on:** `nexus.paneMode`, `nexus.activityBar`; reads
  `com.nexus.notifications`; optionally forwards to
  `nexus.aiRuntime.revealTask` or emits
  `nexus.notificationsInbox:jump-to-task`.
- **Verdict:** Useful
- **Rationale:** Toasts fade; the inbox is the only place to review
  notification history. Default-on in the catalog. Not required for the
  basic-capability flow, but the first thing a user reaches for after they
  miss a toast.

### notificationsSettings

- **Path:** `shell/src/plugins/nexus/notificationsSettings/`
- **Surface:**
  - Registers a Settings → Notifications tab (`id: notifications`, group
    `options`, priority 50) backed by `NotificationsSettings.tsx`.
  - Per-channel credential entry (Discord webhook; Telegram bot token + chat
    id; SMTP host / port / user / password / recipient) backed by the
    `nexus-security` keyring.
  - "Send test" buttons dispatch `com.nexus.notifications::send` directly to
    verify the round-trip.
- **Depends on:** `core/settings` (tab host), `com.nexus.security` (keyring),
  `com.nexus.notifications` (send).
- **Verdict:** Optional
- **Rationale:** Required only when the user actually wants Discord /
  Telegram / SMTP routing; default channels (Desktop toast, OS notification)
  don't need credentials. The basic-capability flow never opens this tab.

## Category verdict

| Plugin                | Verdict  | Required for basic capabilities |
|-----------------------|----------|---------------------------------|
| notifications         | Useful   | No — but absence silences every plugin's alerts |
| notificationsInbox    | Useful   | No — toast history; first thing missed when gone |
| notificationsSettings | Optional | No — credential entry for non-Desktop channels |
