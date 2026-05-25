# nexus-notifications

> Kind: lib · IPC plugin id: com.nexus.notifications · CorePlugin: yes · Has settings: NotificationsConfig · As of: 2026-05-25

## Overview

`nexus-notifications` is the BL-133 multi-channel notification dispatcher. Nexus agent and workflow output is otherwise only visible in the active frontend session; a background workflow that finishes at 02:00 with the shell closed has no delivery surface. This crate provides one `send` IPC handler that routes a single notification to one or more configured channels — **desktop** (bridged through the kernel bus to the shell's toast surface), **Discord** (HTTP webhook), **Telegram** (Bot API `sendMessage`), and **email** (SMTP submission via `lettre` over rustls TLS). Per-channel transport errors surface as IPC errors so the caller can retry, fall back, or surface to the user.

The crate sits inside the microkernel as a `CorePlugin` (`com.nexus.notifications`) registered by `nexus-bootstrap`. It holds an immutable `HashMap<Channel, Box<dyn Transport>>` built at construction from the `[channels.*]` config blocks, plus a live-reloadable `Router` (BL-135) that maps a producer `source` tag to a channel list, applying severity and quiet-hours filters. Two dispatch paths exist: an **override path** (caller names an explicit `channel`, bypassing the router) and a **router path** (caller supplies a `source` tag and the router consults `notifications.toml`).

Network egress is via blocking `reqwest` (Discord/Telegram) and `lettre` (SMTP). The crate performs no explicit in-process capability check itself; capability gating (`notifications.inbox.read` / `notifications.inbox.write` on the inbox handlers, and the downstream `ui.notify` gate on the desktop toast transport) is applied at the kernel IPC dispatch layer per the manifest. Credentials (Discord webhook URL, Telegram bot token, SMTP username/password) live in plain text in `notifications.toml` (or the legacy `config.toml::[notifications.*]` blocks) for v1; routing them through the `nexus-security` keyring is a tracked BL-133 follow-up.

The crate also owns the BL-136 / ADR 0029 persistent inbox: a SQLite-backed history of every dispatched notification, surfaced via four additional IPC handlers (`inbox_list`, `inbox_mark_read`, `inbox_dismiss`, `inbox_stats`).

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-kernel` (EventBus, EventFilter, NexusEvent, KernelPluginContext), `nexus-plugin-api`, `nexus-plugins` (`CorePlugin`, `PluginError`, `define_dispatch_helpers!`), `nexus-types` (`plugin_ids::NOTIFICATIONS`).
- **Notable external deps:** `reqwest` (features `blocking`, `json` — Discord/Telegram), `lettre` (SMTP, rustls TLS, default connection pooling), `rusqlite` (inbox persistence), `notify` (config-file live-reload watcher), `tokio` (rt/sync/macros — the AI-runtime subscriber task), `toml` + `serde`/`serde_json` (config parse), `chrono` (timestamps, current-minute-of-day), `uuid` (inbox row ids), `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature.
- **Dev deps:** `tempfile`, `tokio` (rt-multi-thread/macros/time).
- **Crates depending on it:** `nexus-bootstrap` (registers the plugin via `crates/nexus-bootstrap/src/plugins/notifications.rs`). Registered *before* `nexus-ai-runtime` (the AI-runtime topic prefix is mirrored as a `const` here rather than depending on that crate, so the dep doesn't invert).

## Public API surface

**`lib.rs`** — notification types + the transport trait and its four implementations:
- `Channel` (enum `Desktop`/`Discord`/`Telegram`/`Email`, snake_case wire form, append-only) — `as_str()` helper.
- `Notification { message: String, title: Option<String> }` — payload threaded through every transport.
- `SendError` (enum `NotConfigured(&'static str)` / `Http(String)` / `Bus(String)` / `Smtp(String)`).
- `Transport` trait — `channel()` + `send(&Notification) -> Result<(), SendError>`; lives behind a trait so transports can be mocked.
- `DesktopTransport` — publishes `com.nexus.notifications.delivered` on the bus (no bus ⇒ `NotConfigured`).
- `DiscordWebhook` — POSTs `{ username: "Nexus", content }` to the webhook URL; lazy `OnceLock<reqwest::blocking::Client>` (so the inner sync runtime isn't built inside an async Tauri command); empty URL ⇒ `NotConfigured`.
- `TelegramBot` — POSTs `{ chat_id, text }` to `…/bot<TOKEN>/sendMessage`; `split_at_byte_limit()` chunks the body at UTF-8 char boundaries under `max_bytes`; empty token or chat id ⇒ `NotConfigured`.
- `SmtpConfig` (host/port/username/password/from/to/subject_template) and `SmtpTransport` — `is_configured()` gate, `compose_subject()` template resolver, port-keyed TLS (465 = implicit SMTPS, else STARTTLS), comma-split multi-recipient `To`.
- Consts: `INBOX_DB_RELPATH`, `DEFAULT_TELEGRAM_MAX_BYTES` (4096), `INBOX_APPENDED_TOPIC`, `NOTIFICATIONS_CONFIG_RELPATH`, `NOTIFICATION_DELIVERED_TOPIC`.

**`config.rs`** (BL-135 config schema):
- `Severity` (enum `Debug<Info<Warn<Error`, `PartialOrd` ordering drives the filter) — `as_str()`.
- `QuietHours { start_min, end_min }` — `parse("HH:MM-HH:MM")`, `contains(min_of_day)` (overnight ranges wrap).
- `SourceConfig` (`on`, `route`, `min_severity`, `quiet_hours`) — one `[sources.<name>]` block.
- `ResolvedSource { channels, min_severity, quiet_hours }` — pre-resolved hot-path view.
- `DiscordChannel` / `TelegramChannel` / `EmailChannel` (`to_smtp_config()`) / `ChannelsConfig` / `InboxConfig`.
- `NotificationsConfig { sources, channels, inbox }` — `load_from(path)` (missing file ⇒ default), `parse(text)`, `resolve_sources()` (drops unknown channel names with a warn).
- `ConfigError` (`Read`/`Parse`/`QuietHours`), `channel_from_str()`.

**`router.rs`** (BL-135 routing):
- `Router` (`Arc<RwLock<…>>` over a `BTreeMap<String, ResolvedSource>`) — `empty()`, `from_config()`, `swap_config()` (live reload), `resolve(source, severity, min_of_day) -> Resolution`, `source_names()`.
- `Resolution` (enum `UnknownSource` / `Filtered` / `Routed(Vec<Channel>)`).
- `current_min_of_day()` free fn.

**`inbox.rs`** (BL-136 / ADR 0029 persistence):
- `Inbox` (SQLite, one `Mutex<Connection>`) — `open()`, `in_memory()`, `insert()`, `enforce_row_cap()`, `enforce_age_cap()`, `mark_read()`, `dismiss()` (also marks read), `list()`, `stats()`, `get()`, `snapshot_user_state()`, `clear()`, `apply_user_state()`.
- `InboxEntry`, `NewEntry<'a>`, `StatusFilter` (`All`/`Unread`/`Dismissed`), `InboxStats { total, unread, by_source }`, `UserStateRow`, `InboxError` (`Sql`/`Encoding`).
- Consts `DEFAULT_MAX_ROWS` (1000), `DEFAULT_MAX_AGE_DAYS` (30).

**`core_plugin.rs`** (the `CorePlugin`):
- `NotificationsCorePlugin` — owns transports + router + config + optional inbox + bus + ctx. Constructors: `from_config`, `from_config_with_inbox`, `with_defaults` (legacy BL-133 signature), `with_transports`, `with_transports_and_config`, `with_transports_and_inmemory_inbox`. Accessors `inbox()`, `router()`. `reload_config_from_disk()`, `dispatch_routed()`, `dispatch_routed_with_payload()`.
- IPC arg/reply types: `SendArgs`, `SendReply`, `ChannelFailure`, `InboxListArgs`, `InboxIdsArgs`, `InboxUpdatedReply`.
- Handler-id consts (`HANDLER_SEND`=1 … `HANDLER_INBOX_STATS`=5), `IPC_HANDLERS` table (SD-06 single source of truth), `PLUGIN_ID`, `AI_RUNTIME_TOPIC_PREFIX`, `SOURCE_AI_RUNTIME`.

## IPC handlers

| Command | Args | Returns | Capability | Description |
|---------|------|---------|------------|-------------|
| `send` (id 1) | `SendArgs { channel?: Channel, source?: String, severity?: Severity, message: String, title?: String }` (deny_unknown_fields; one of `channel`/`source` required, else error) | `SendReply { delivered: bool, channel?: Channel, channels: Vec<Channel>, failures: Vec<ChannelFailure>, routing: String }` | — (downstream `ui.notify` gates the desktop toast transport) | Override path (`channel` set) dispatches to one transport, transport failure surfaces as a top-level IPC error. Router path (`source` set) resolves channels via `notifications.toml`, fans out, collects per-channel `failures` without aborting; `routing` is `"override"`/`"routed"`/`"unknown_source"`/`"filtered"`. Writes an inbox row on success (override sends use source `"override"` when no source supplied). |
| `inbox_list` (id 2) | `InboxListArgs { since?: i64, status?: String, source?: String, limit?: u32 }` (deny_unknown_fields; `status` ∈ all/unread/dismissed, default all; `limit` default 100, capped 1000) | `Vec<InboxEntry>` (newest-first) | `notifications.inbox.read` | List inbox rows under filters. Errors if inbox not wired. |
| `inbox_mark_read` (id 3) | `InboxIdsArgs { ids: Vec<String> }` | `InboxUpdatedReply { updated: u32 }` | `notifications.inbox.write` | Mark rows read; count reflects only newly-flipped rows. |
| `inbox_dismiss` (id 4) | `InboxIdsArgs { ids: Vec<String> }` | `InboxUpdatedReply { updated: u32 }` | `notifications.inbox.write` | Dismiss rows (also marks read via `COALESCE`). |
| `inbox_stats` (id 5) | (ignored) | `InboxStats { total, unread, by_source }` | `notifications.inbox.read` | Aggregate counts for the inbox panel header. |

Each command is also registered under a `.v1` alias (ADR 0021) by `with_v1_aliases` in bootstrap. Capability values above are sourced from `docs/0.1.2/ipc-handlers.md`; they are enforced at the kernel IPC-dispatch layer — this crate's `dispatch` does no explicit `capability` check itself.

## Capabilities

- **`notifications.inbox.read`** — `inbox_list`, `inbox_stats`.
- **`notifications.inbox.write`** — `inbox_mark_read`, `inbox_dismiss`.
- **`send`** carries no inbox capability; the desktop transport's toast delivery is gated downstream by `ui.notify`.
- **Network egress** (Discord/Telegram HTTP, SMTP) is not guarded by an explicit in-crate capability check in v1; gating is left to the kernel-mediated IPC path and manifest. No dedicated `net`/`credential.read` capability constant is referenced in this crate's source — flagged as a gap below.

## Settings / Config

Config type is `NotificationsConfig`, loaded from `<forge>/.forge/notifications.toml` (`NOTIFICATIONS_CONFIG_RELPATH`). Missing file ⇒ `NotificationsConfig::default()`. Top-level shape (`deny_unknown_fields`):

```toml
[sources.<name>]              # SourceConfig — one per producer tag
on = ["com.nexus.workflow.run_completed"]   # bus topics (parsed, not yet auto-subscribed)
route = ["desktop", "discord"]               # channel names; unknown names dropped w/ warn
min_severity = "warn"                         # debug|info|warn|error, default info
quiet_hours = "22:00-08:00"                   # optional; overnight ranges valid

[channels.discord]
webhook_url = "https://discord.example/webhook"

[channels.telegram]
bot_token = "bot:token"
chat_id = "12345"
max_bytes = 4096               # optional; None/0 ⇒ DEFAULT_TELEGRAM_MAX_BYTES (4096)

[channels.email]               # EmailChannel → SmtpConfig
host = "smtp.example.com"
port = 587                     # 465 = implicit TLS, else STARTTLS
username = "..."               # empty disables SMTP AUTH
password = "..."               # plaintext (v1) — keyring routing is a follow-up
from = "nexus@example.com"
to = "ops@example.com"         # comma-separated multi-recipient
subject_template = "Nexus: {title}"

[inbox]                        # InboxConfig (BL-136 retention)
max_rows = 1000                # default DEFAULT_MAX_ROWS
max_age_days = 30              # default DEFAULT_MAX_AGE_DAYS
```

Field defaults: every `[channels.*]` and `[sources.*]` field is `#[serde(default)]`; `min_severity` defaults to `Info` (the `Default` derive on `Severity`); `inbox.max_rows`/`max_age_days` are `Option<u32>` falling back to the crate consts.

**Legacy fallback:** when `notifications.toml` is absent, `nexus-bootstrap::load_notifications_config` reads the legacy `config.toml::[notifications.discord|telegram|email]` blocks and synthesises default `desktop` routes for sources `workflow`, `agent`, `cli`, `ai_runtime`. In that mode `config_path` is `None`, so live-reload is disabled.

**Where secrets live:** Discord webhook URL, Telegram bot token, SMTP username/password are stored in plaintext in `notifications.toml` (or legacy `config.toml`). No keyring integration in v1 — tracked as a BL-133 follow-up tail.

## Events

- **Published:**
  - `com.nexus.notifications.delivered` (`NOTIFICATION_DELIVERED_TOPIC`) — emitted by `DesktopTransport::send` (payload `{ channel, title, message }`) and by the AI-runtime subscriber task (payload adds `source: "ai_runtime"`). The shell toast surface subscribes to this.
  - `com.nexus.notifications.inbox.appended` (`INBOX_APPENDED_TOPIC`) — emitted after a new inbox row lands (payload `{ id, source, severity, ts }`) so the shell can bump its unread badge without polling.
- **Subscribed:** in `on_start`, when a tokio runtime is present, the plugin subscribes to the `com.nexus.ai.runtime.*` prefix (`AI_RUNTIME_TOPIC_PREFIX`). `translate_ai_runtime_event` maps `finished`/`failed` events into a `Notification` and republishes them on `NOTIFICATION_DELIVERED_TOPIC`; token chunks / tool calls / lifecycle noise are intentionally silent. (Phase-1 only republishes to the toast topic; it does not yet fan out to transports — the transport snapshot is taken for shape and dropped.) No per-source bus auto-subscription exists yet (`SourceConfig.on` is parsed but unused).

## Internals & notable implementation details

- **Two dispatch paths.** `dispatch_send` rejects args with neither `channel` nor `source`. Override path: looks up the single transport, returns a top-level IPC error on transport failure, records an inbox row under source `"override"` (or the supplied `source`). Router path: `Router::resolve` → `Resolution`; `Routed` fans out via `fan_out` (collecting `ChannelFailure` per channel without aborting), then writes one inbox row tagged with the routed channels. `channel` (singular) in the reply echoes the first delivered channel for v1 back-compat.
- **Transports.** Desktop = bus publish. Discord/Telegram use a lazy `OnceLock<reqwest::blocking::Client>` — built on first `send` via the natural call site rather than at boot, to dodge `reqwest`'s debug-assertion panic when a blocking client is built inside a tokio async context. Telegram splits the body at UTF-8 char boundaries under `max_bytes` (default 4096) and posts each chunk in order; a single oversize char is force-advanced to avoid an infinite loop. SMTP uses `lettre` with port-keyed TLS (465 implicit / else STARTTLS), connection pooling on, optional credentials, comma-split `To`, and a `compose_subject` template (`{title}` substitution with layered fallbacks to `"Nexus notification"`). All transports fail at `send` time (`NotConfigured`) rather than at construction, so partial config never crashes boot.
- **Router live-reload.** `Router` wraps `Arc<RwLock<RouterState>>`; `swap_config` replaces the resolved-source map in place so in-flight dispatches finish on the old rules. `spawn_config_watcher` runs a dedicated `notify` watcher thread on the *parent* directory (atomic-rename editors recreate the inode), filtering by filename and reload-worthy event kinds; failures log and drop to a no-watcher state. `reload_config_from_disk` swaps routing rules only — transport credentials are *not* hot-reloaded (requires restart).
- **Inbox persistence.** Single `inbox` table (id TEXT PK / source / severity / title / body / channels JSON / ts / read_at / dismissed_at / payload_json) with indexes on `ts DESC`, partial unread (`read_at IS NULL`), and `(source, ts DESC)`. One `Mutex<Connection>`; dispatch is the single writer. Row-cap enforced after every insert (ordered by `rowid` to break same-second `ts` ties); age-cap swept once at `on_start`. `dismiss` also stamps `read_at` via `COALESCE`. Corrupt `channels` JSON degrades to an empty channel set with a warn rather than failing the query. `mark_read`/`dismiss` only flip rows not already in the target state, so `updated` counts are accurate. `snapshot_user_state`/`clear`/`apply_user_state` support a rebuild flow (library-only; no IPC/CLI surface yet).
- **No rate limiting.** Despite the task framing, there is no rate-limiting code in the crate. The closest analogues are the per-source `quiet_hours` window and `min_severity` floor (both router-side filters), the Telegram per-message byte cap, and inbox retention caps. Flagged as a gap.
- **No retry.** Transport failures are surfaced (override path) or collected as `failures` (router path); the crate performs no automatic retry — retry/fallback is left to the caller.

## Tests

All tests are inline `#[cfg(test)]` modules; the crate has no `tests/` directory.

- **`lib.rs`** — `Channel` snake_case serde round-trips (all four variants); `DesktopTransport` not-configured-without-bus, payload-publish, title-default-to-"Nexus"; `DiscordWebhook` empty-URL ⇒ NotConfigured; `TelegramBot` empty token / empty chat-id ⇒ NotConfigured, `split_at_byte_limit` short/long/UTF-8-boundary (🦀) cases; `SmtpTransport` empty/partial config ⇒ NotConfigured, `compose_subject` four template/title permutations, invalid-from ⇒ `Smtp`.
- **`config.rs`** — empty + full-schema parse, unknown-top-level-field rejection (deny_unknown_fields), `resolve_sources` drops unknown channels / parses + rejects quiet_hours, `QuietHours` overnight + daytime windows, severity ordering, missing-file default, `channel_from_str` table, `EmailChannel → SmtpConfig`.
- **`router.rs`** — unknown source, route fan-out, min_severity filter, quiet_hours filter, empty-route ⇒ Filtered, `swap_config` swap, `source_names`.
- **`core_plugin.rs`** — override-path dispatch (via `MockTransport`/`ShimTransport`), unknown-channel error, override transport failure ⇒ IPC error, missing-message / unknown-field / neither-channel-nor-source rejection, unknown-handler-id; router-path fan-out, unknown_source, severity filter, per-channel failures without abort, explicit-channel-wins-over-source; `reload_config_from_disk` swap + no-op-when-unset; `translate_ai_runtime_event` finished/failed/token-chunk.
- **`inbox.rs`** — insert round-trip via `get`, `mark_read` only-flips-unread, `dismiss` also marks read, list unread/source filters, list newest-first, row-cap trim + zero-disables, `stats` unread/per-source, user-state snapshot round-trip, channels-array JSON round-trip, empty-channels case.

Coverage is strong at the unit level. No integration test hits the real network or a live SMTP/Discord/Telegram endpoint — every transport with external I/O is exercised only on its `NotConfigured` / arg-validation / splitting paths, never an end-to-end successful HTTP/SMTP send.
