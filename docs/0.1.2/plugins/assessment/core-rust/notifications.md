# com.nexus.notifications

- **Path:** `crates/nexus-notifications/`
- **Tier:** Core Rust
- **Bootstrap order:** 16

## Architecture
- Entry point: `crates/nexus-notifications/src/core_plugin.rs` (`NotificationsCorePlugin`). Modules: `config`, `inbox`, `router`, plus transport impls in `lib.rs` (`DesktopTransport`, `DiscordWebhook`, `TelegramBot`, `SmtpTransport`).
- BL-133 / BL-135 / BL-136. Source-tagged sends run through the BL-135 `Router` which reads `notifications.toml` to pick channels and applies severity/quiet-hours filtering. Explicit-channel sends bypass the router. Every successful delivery appends to the BL-136 SQLite inbox.
- `on_start` (registered with `LifecycleFlags { on_init: false, on_start: true, on_stop: false }`):
  - Spawns a `notify` filesystem watcher on `notifications.toml` for live config reload.
  - Subscribes to `com.nexus.ai.runtime.*` and translates each event into a source-tagged notification (`source = "ai_runtime"`).
- `Desktop` transport publishes `com.nexus.notifications.delivered` on the kernel bus; the shell renders the toast.

## Persistence
- `<forge>/.forge/notifications.toml` — `NotificationsConfig` (`crates/nexus-notifications/src/config.rs:272`). Path constant: `NOTIFICATIONS_CONFIG_RELPATH` (`lib.rs:103`).
- `<forge>/.forge/notifications/inbox.db` — SQLite inbox (`INBOX_DB_RELPATH`, `lib.rs:84`).
- Inbox row cap default 1000, age cap 30 days (`config.rs:272` block, `inbox.rs:47`).
- Bus event topics: `com.nexus.notifications.delivered` (`NOTIFICATION_DELIVERED_TOPIC`), `com.nexus.notifications.inbox.appended` (`INBOX_APPENDED_TOPIC`).

## Settings owned
- `notifications.toml`: `[sources.<id>]`, `[channels.{desktop,discord,telegram,email}]`, `[inbox]`. Documented in `docs/0.1.2/settings/forge-config.md` lines 118–148.
- `DEFAULT_TELEGRAM_MAX_BYTES = 4096` (`lib.rs:93`); overridable via `channels.telegram.max_bytes`.

## External dependencies of note
- `reqwest` (Discord webhook + Telegram bot API).
- `lettre` (SMTP, rustls TLS — 465 implicit, 587 STARTTLS, no port 25).
- `rusqlite` for the inbox DB.
- `notify` for the config watcher.
- Outbound network for Discord/Telegram/SMTP; OS-bus event for Desktop.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:73`):

| Id | Command | Notes |
|---:|---------|-------|
| 1 | `send` | Either `channel` (override path) or `source` (router path) + `message` / `title` / `severity` |
| 2 | `inbox_list` | Filter by `since` / `status` / `source`; default page 100, cap 1000 |
| 3 | `inbox_mark_read` | By row id |
| 4 | `inbox_dismiss` | By row id |
| 5 | `inbox_stats` | Counts by status |

Publishes: `com.nexus.notifications.delivered`, `com.nexus.notifications.inbox.appended`.
Subscribes: `com.nexus.ai.runtime.*`, filesystem watcher on `notifications.toml`.

## Necessity
- **Verdict:** Useful
- **Required for basic capabilities?** No — markdown browse/edit/search/git completes without ever emitting a notification.
- **Depended on by:** workflow `notify` step type (BL-133 follow-up), agent run-completion auto-notify, `com.nexus.ai.runtime` (republished events become inbox rows), shell-nexus `notifications` / `notificationsInbox` / `notificationsSettings` plugins.
- **Depends on:** kernel event bus.
- **What breaks if removed:** desktop toasts surfacing background work (workflow runs, agent completion, runtime events), the inbox UI, plus all router-driven Discord/Telegram/email/SMTP delivery. Basic workflow keeps working but background activity becomes invisible when the shell is closed — which is the whole point of the subsystem (BL-133's "no delivery channel if the shell is closed at 02:00").

## Notes
- Empty config + no inbox path still produces a functional plugin (router resolves to no channels, `send` returns `routing: "none"`).
- Transports lazily build their `reqwest::blocking::Client` to avoid panicking inside a tokio async context (documented at `lib.rs:243`).
- `category` is not surfaced on the bootstrap manifest.
