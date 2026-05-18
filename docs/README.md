# Nexus Documentation

> **Active docs:** [`0.1.2/`](0.1.2/) — code-as-source-of-truth audit and reference, established 2026-05-17.
>
> **Pre-0.1.2 archive:** [`archive/pre-0.1.2/`](archive/pre-0.1.2/) — the 9 MB curated doc set as it stood before the 0.1.2 audit. Architecture, ADRs (0001–0029), PRDs (01–17), developer/help/users guides, audits (2026-04…2026-05), roadmap, research. Kept verbatim; historical reference only — when it disagrees with the code, the code wins.
>
> **Generated:** [`generated/`](generated/) — auto-emitted from Rust source (`scripts/check_ipc_drift.sh`). The capability inventory lives here.

## What's in 0.1.2/

| Area | File |
|------|------|
| Entry point | [`0.1.2/README.md`](0.1.2/README.md) |
| Architecture overview | [`0.1.2/architecture.md`](0.1.2/architecture.md) |
| Architecture adherence audit | [`0.1.2/architecture-adherence.md`](0.1.2/architecture-adherence.md) |
| Implementation plan (39-item remediation) | [`0.1.2/implementation-plan.md`](0.1.2/implementation-plan.md) |
| Crate inventory (35 crates) | [`0.1.2/crates.md`](0.1.2/crates.md) |
| Shell + packages | [`0.1.2/shell.md`](0.1.2/shell.md) |
| Every IPC handler | [`0.1.2/ipc-handlers.md`](0.1.2/ipc-handlers.md) |
| Every security capability | [`0.1.2/capabilities.md`](0.1.2/capabilities.md) |
| Application feature surface | [`0.1.2/application-capabilities.md`](0.1.2/application-capabilities.md) |
| Per-plugin capability matrix (91 plugins) | [`0.1.2/plugin-capabilities.md`](0.1.2/plugin-capabilities.md) |
| Every settings surface | [`0.1.2/settings/`](0.1.2/settings/) |
| Hardcoded-value remediation | [`0.1.2/settings/hardcoded-rust.md`](0.1.2/settings/hardcoded-rust.md), [`0.1.2/settings/hardcoded-shell.md`](0.1.2/settings/hardcoded-shell.md) |
| AUDIT-flagged handlers | [`0.1.2/reference/audit-flags.md`](0.1.2/reference/audit-flags.md) |
| TODOs / stubs / coming-soon | [`0.1.2/reference/todos.md`](0.1.2/reference/todos.md) |

## Conventions

- Every claim cites `file:line` in the live tree (not the archive).
- When the audit disagrees with archive content, the audit is correct — the archive may be stale.
- New audits in this directory pin a date in the filename or in a leading `> **As of:** YYYY-MM-DD` line.
- `0.1.2/` is versioned, not dated — it tracks the 0.1.2 release cut. A `0.1.3/` should be opened when the next release is cut.
