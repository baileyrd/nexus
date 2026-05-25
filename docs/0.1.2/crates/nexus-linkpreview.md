# nexus-linkpreview

> Kind: lib · IPC plugin id: com.nexus.linkpreview · CorePlugin: yes · Has settings: no (compile-time constants only) · As of: 2026-05-25

## Overview
`nexus-linkpreview` is the link-preview service plugin. Given an `http`/`https` URL, it fetches the page over the network, parses Open Graph / Twitter-card / plain HTML metadata, and returns a `LinkPreview` struct (`url`, `title`, `description`, `image_url`, `site_name`, `favicon_url`) that the shell's canvas link-node overlay renders into a link card. Every field is optional; the shell renders whatever it gets and falls back to the raw URL when everything is missing.

The crate is deliberately small and regex-based rather than pulling in a full HTML parser — production-perfect parsing is overkill for OG `<meta>` tags, and avoiding an HTML-parser dependency keeps compile cost low. Fetches are best-effort and tightly bounded: a 5-second timeout (covering DNS + connect + read) and a 512 KiB body cap enforced at the transport layer via `Read::take`, so a slow or hostile host can't hang a canvas render or read megabytes into memory. The plugin is stateless — every call hits the network fresh; the shell layer owns caching so previews survive tab switches without a second request.

Microkernel fit: the crate links `reqwest` (the only outbound-HTTP dependency in this part of the tree) and exposes its capability through a single IPC handler, so CLI / TUI / MCP / shell never link reqwest directly. It carries a hardened SSRF guard (issue #78) that refuses URLs and redirect hops resolving to non-public addresses (loopback, link-local incl. the AWS metadata IP, RFC1918, CGNAT, IPv6 ULA, IPv4-mapped-IPv6 smuggling, etc.).

## Position in the dependency graph
- **Direct nexus-\* deps:** `nexus-plugins` (for `CorePlugin`, `PluginError`, `define_dispatch_helpers!`). Notably it does **not** depend on `nexus-kernel` directly.
- **Notable external deps:** `reqwest` (with the `blocking` feature), `regex-lite`, `serde`/`serde_json`, `thiserror`, `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature (off by default) for emitting TypeScript + JSON Schema IPC bindings.
- **Depended on by:** `nexus-bootstrap` (`crates/nexus-bootstrap/src/plugins/linkpreview.rs` registers `LinkPreviewCorePlugin` as a core plugin) and the SSRF regression test crate. No other in-tree crate links it; frontends reach it only via `context.ipc_call("com.nexus.linkpreview", "fetch", …)`.

## Public API surface
**`src/lib.rs`**
- `struct LinkPreview` — result type: `url`, `title`, `description`, `image_url`, `site_name`, `favicon_url` (all `Option<String>` except `url`). `#[serde(deny_unknown_fields)]`; gains `TS`/`JsonSchema` derives under `ts-export`.
- `enum FetchError` — coarse error type: `InvalidUrl(String)`, `Request(String)`, `Status(u16)`.
- `fn fetch_blocking(url: &str) -> Result<LinkPreview, FetchError>` — the entry point the IPC handler calls. Parses + scheme-checks, runs the SSRF guard, fetches, caps the body, parses, fills `site_name` hostname fallback. Blocks the calling thread.
- `fn parse_html(base_url: &str, html: &str) -> LinkPreview` — pure, synchronous metadata extractor; exposed so tests/tooling can exercise the parser without network I/O.
- `fn is_blocked_address(ip: IpAddr) -> bool` — pure SSRF denylist predicate; exhaustively unit-tested. `#[must_use]`.

Private helpers (in `lib.rs`): `resolve_public_address`, `validate_url_target` (SSRF resolution); `find_meta` (`property=`), `find_meta_name` (`name=`), `find_title`, `find_favicon` (regex extractors, both attribute orders supported); `absolutise`/`origin_of`/`hostname` (URL resolution); `decode_entities`, `normalize`, `regex_escape`.

**`src/core_plugin.rs`**
- `const PLUGIN_ID: &str = "com.nexus.linkpreview"`, `const HANDLER_FETCH: u32 = 1`, `const IPC_HANDLERS: &[(&str, u32)] = &[("fetch", 1)]` (SD-06 single source of truth, append-only ids).
- `struct FetchArgs { url: String }` — IPC arg type (`deny_unknown_fields`; `TS`/`JsonSchema` under `ts-export`).
- `struct LinkPreviewCorePlugin` (`Default`, `::new()`) — the `CorePlugin` impl; stateless.

## IPC handlers
| Command | Args | Returns | Capability | Description |
|---------|------|---------|------------|-------------|
| `fetch` (id 1) | `{ "url": String }` (`FetchArgs`) | `LinkPreview` as JSON | none enforced today — flagged in `ipc-handlers.md` as **AUDIT** candidate for `net.http` | Fetch the URL, parse OG/Twitter/HTML metadata, return a preview. On `InvalidUrl` returns an IPC error; on any other `FetchError` (network/4xx/5xx) logs at `debug` and returns a fallback `LinkPreview` with only `url` set, so the shell can always render something. |

Registered via `core_manifest_with_ipc` with `with_v1_aliases(IPC_HANDLERS)` and `LifecycleFlags::NONE` (always-on, no lifecycle gating) — see `crates/nexus-bootstrap/src/plugins/linkpreview.rs`.

## Capabilities
The `fetch` handler performs outbound HTTP to arbitrary URLs but **does not currently declare or check a capability** — `docs/0.1.2/ipc-handlers.md` marks it `AUDIT` as a candidate for a future `net.http` capability. The de-facto safety control is instead the SSRF guard rather than a capability gate:
- **Scheme allowlist:** only `http`/`https`; everything else (`ftp:`, `javascript:`, …) → `InvalidUrl`.
- **`is_blocked_address` denylist** (applied to the initial URL *and* re-checked on every redirect hop): IPv4 loopback / unspecified / multicast / broadcast / `0.0.0.0/8`, RFC1918 private, link-local (`169.254/16`, covers AWS metadata `169.254.169.254`), CGNAT `100.64/10`; IPv6 loopback / unspecified / multicast / ULA `fc00::/7` / link-local `fe80::/10`; and IPv4-mapped IPv6 (recurses into the v4 check to block `::ffff:127.0.0.1`-style smuggling).
- **Residual risk (documented in source, issue #78):** a TOCTOU / DNS-rebinding window between the pre-check resolution and reqwest's own re-resolution at connect time — reqwest is not pinned to the validated IP. Noted as a deeper change deferred.

## Settings / Config
No runtime config, no TOML file, no `Config` struct. All limits are compile-time `const`s in `src/lib.rs`:
- `FETCH_TIMEOUT = 5s` — total request timeout (DNS + connect + read).
- `MAX_BODY_BYTES = 512 * 1024` (512 KiB) — hard body cap.
- `USER_AGENT = "Mozilla/5.0 (Nexus Canvas) AppleWebKit/537.36 (KHTML, like Gecko) Nexus/0.1"` — browser-ish UA to avoid bot-challenge pages.
- Redirect limit: 5 hops (custom `reqwest::redirect::Policy`; tighter than reqwest's default of 10).

These are candidates for promotion to settings but are not currently surfaced.

## Events
None. The plugin neither publishes nor subscribes to events — it is a pure request/response IPC handler.

## Internals & notable implementation details
- **HTTP fetch:** `reqwest::blocking::Client` built per-call with the timeout, UA, and custom redirect policy. `fetch_blocking` blocks the calling thread and is meant to run from a kernel handler thread, not an async context.
- **Body-size cap:** enforced at the transport layer with `resp.take(MAX_BODY_BYTES).read_to_end(...)` (`Response: Read`), so a gigabyte-streaming server can't be fully buffered before the cap kicks in. This replaced an earlier `resp.text()` that decoded the whole body before substring-truncating (the OOM vector called out in issue #78).
- **Charset:** bytes are decoded with `String::from_utf8_lossy` — non-UTF-8 responses degrade rather than error.
- **Metadata extraction:** regex (`regex-lite`, `(?is)` flags) over the raw HTML. Precedence: title = `og:title` → `twitter:title` → `<title>`; description = `og:description` → `twitter:description` → `<meta name="description">`; image = `og:image` → `twitter:image`; `site_name` = `og:site_name` → URL hostname fallback. Each extractor tries both attribute orders (property/name before content and vice versa). Empty `content=""` normalizes to `None`.
- **URL resolution:** `absolutise` handles absolute, protocol-relative (`//host/...`, inherits base scheme), root-relative (`/path`), and path-relative URLs for image/favicon fields.
- **Entity decoding:** minimal hand-rolled `decode_entities` for `&amp; &lt; &gt; &quot; &#39; &apos;` applied to `<title>` fallbacks only (full decoder intentionally avoided).
- **Error mapping in the handler:** `InvalidUrl` surfaces as an IPC error; all other fetch failures are swallowed into a fallback `LinkPreview { url, ..Default }` and logged at `debug`.
- **Favicon:** accepts `rel="icon"`, `rel="shortcut icon"`, `rel="apple-touch-icon"`; returns the first match without size/format selection.

## Tests
- **Inline unit tests** (`#[cfg(test)] mod tests` in `src/lib.rs`, 10 tests): full OG suite parse; Twitter-tag + `<title>` fallbacks; entity decoding in title; `description` from `meta name`; flexible attribute ordering; `absolutise` for protocol-relative/root/path-relative/absolute; empty `content` → `None`; `apple-touch-icon` favicon; rejection of non-`http(s)` schemes via `fetch_blocking` (`ftp:`, `javascript:`).
- **`tests/issue_78_ssrf.rs`** (13 tests): exhaustive `is_blocked_address` coverage — IPv4 loopback, AWS metadata, link-local, RFC1918 (all three blocks), CGNAT, unspecified/broadcast/`0.0.0.0/8`, multicast; IPv6 loopback/unspecified/multicast, ULA, link-local, IPv4-mapped smuggling; plus positive cases confirming public IPv4 (`8.8.8.8`, `1.1.1.1`, `172.32.x`) and public IPv6 are allowed.
- **Gap:** no test stands up an HTTP server, so the redirect-policy SSRF re-check and the streaming body-size cap are not covered by an integration test here — the source notes they are exercised by the wider production-fetch smoke. The TOCTOU/DNS-rebinding residual risk is untested by design.
