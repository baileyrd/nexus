# ADR 0008: Tech Stack Defaults

**Date:** 2026-04-11
**Status:** Accepted
**Forward-pointer (2026-05-12):** Embedding-backend defaults extended
by [ADR 0018 — Local Embedding Backend (fastembed-rs)](0018-embedding-backend.md#addendum-2026-05-12--adr-0008-tech-stack-defaults-update).
ADR 0018's addendum is the operative tech-stack-defaults update for
the embeddings row.

## Context

PRDs 01–05 leave many crate choices open. We need locked defaults to avoid
per-PRD bikeshedding during implementation.

## Decision

See M1 spec §3 for the full table. Key picks for PRD 01:

- Async runtime: `tokio` 1.35+, full features, no abstraction layer.
- Logging: `tracing` + `tracing-subscriber` + `tracing-appender`.
- Serialization: `serde` 1.0 + `serde_json` 1.0.
- Error handling: `thiserror` in libraries, `anyhow` in binary.
- Async traits: `async-trait` until native support stabilizes.
- TOML: `toml` 0.8 for reads.
- Utilities: `uuid` 1.0, `chrono` 0.4 with `serde` feature.
- Test runner: `nextest` (replaces `cargo test`).
- MSRV: latest stable Rust at M1 start.

## Alternatives considered

- Tokio alternatives (smol, async-std): rejected for ecosystem reasons.
- `log` crate instead of `tracing`: rejected — `tracing` has structured
  fields and spans we need for the slimmed audit log.
- `anyhow` everywhere: rejected — libraries need typed errors.

## Consequences

- Versions pinned in workspace root `Cargo.toml`. Bumps require an ADR.
