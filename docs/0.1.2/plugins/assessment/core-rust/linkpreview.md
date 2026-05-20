# com.nexus.linkpreview

- **Path:** `crates/nexus-linkpreview/`
- **Tier:** Core Rust
- **Bootstrap order:** 15

## Architecture
- Entry point: `crates/nexus-linkpreview/src/core_plugin.rs` (`LinkPreviewCorePlugin`). Library: `src/lib.rs` exposes `fetch_blocking` + `parse_html`.
- Stateless; every IPC call performs a fresh outbound HTTP GET (blocking `reqwest::blocking::Client`).
- Parses `og:*` / `twitter:*` / `<title>` / `<meta name="description">` / `<link rel="icon">` via `regex-lite` — no HTML-parser dep.
- SSRF guard: pre-resolves every URL (and every redirect hop, capped at 5 follows) and refuses non-public addresses (`is_blocked_address` covers loopback, link-local, RFC1918, CGNAT, IPv6 ULA, IPv4-mapped IPv6, EC2 metadata 169.254.169.254). Residual DNS-rebinding window documented in code.
- Hard caps: 5 s timeout (`FETCH_TIMEOUT`), 512 KiB body (`MAX_BODY_BYTES`), browser-ish UA.
- Registered with `LifecycleFlags::NONE` (`crates/nexus-bootstrap/src/plugins/linkpreview.rs:27`).

## Persistence
- None. The shell layer caches previews so repeat hovers don't refetch.

## Settings owned
- None.

## External dependencies of note
- `reqwest` (workspace, blocking feature). Outbound network access required.
- No native libs beyond rustls/openssl chain already in `reqwest`.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:33`):

| Id | Command | Args | Returns |
|---:|---------|------|---------|
| 1 | `fetch` | `{ url: String }` | `LinkPreview` (title / description / image_url / site_name / favicon_url / url) |

Behaviour: invalid URL ⇒ IPC error; transport / non-2xx errors ⇒ fallback empty preview (just the URL echo) so the shell can render *something*.

## Necessity
- **Verdict:** Optional
- **Required for basic capabilities?** No — markdown browse/edit/search/git never invokes link previews. Used by canvas link-node overlays in the shell.
- **Depended on by:** canvas / link-card UI in `shell/src/plugins/nexus/`. No core-Rust plugin depends on it.
- **Depends on:** nothing in-process. External: outbound HTTP to user-supplied URLs.
- **What breaks if removed:** link cards on the canvas, OG/Twitter-card preview overlays. No editor / file / git regressions.

## Notes
- Crate is small (~530 lines) and self-contained. SSRF guard test coverage is solid.
- `core_plugin` swallows fetch errors into a fallback preview — keep this contract stable; the shell relies on always-Ok behaviour for non-`InvalidUrl` errors.
