# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow the workspace version in `Cargo.toml`. Started 2026-06-10 (V18,
`docs/0.1.2/audits/repo-review-2026-06-10.md`) — history before that date
lives in the git log and in `docs/0.1.2/audits/`.

## [Unreleased]

### Added
- `security.audit.read` capability gating `query_audit_log` (previously
  unrestricted; cross-plugin telemetry is reconnaissance surface).
- `cargo-deny` supply-chain gate in CI (`deny.toml`): advisories,
  license allowlist, duplicate bans, registry provenance.
- One-shot operator warning when a remote AI provider is configured with
  credentials but without TLS pinning.
- Tauri command-boundary guard now runs on every PR
  (`crates/nexus-bootstrap/tests/tauri_command_boundary.rs`).
- 22 characterization tests over linkpreview's OG/Twitter-card parsing.

### Changed
- Outbound HTTP clients carry timeouts: 10s connect + 300s read backstop
  for AI providers, 10s/30s for notification webhooks.
- Storage knowledge-graph reads recover from lock poison instead of
  aborting the process (`panic=abort`) — #199 tier-1 policy.
- Linkpreview pins each fetch hop to its SSRF-validated IP, closing the
  DNS-rebinding TOCTOU.
- `scripts/` reduced to the five portable value-add helpers; the
  single-machine cargo wrappers were removed.

### Security
- See Added/Changed: audit-log read gating, supply-chain CI gate,
  DNS-rebinding fix, HTTP timeouts. Advisory RUSTSEC-2025-0068
  (`serde_yml`, unsound/unmaintained) is acknowledged and tracked in #248.
