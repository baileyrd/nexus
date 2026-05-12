# Audits

Point-in-time audit snapshots. Each describes what was true *at the date in
the filename*; behavior since then may have changed. For current shipped
state see [`../PRDs/IMPLEMENTATION_STATUS.md`](../PRDs/IMPLEMENTATION_STATUS.md);
for follow-up work see [`../roadmap/`](../roadmap/) and
[`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md).

## Subsystem implementation assessments (2026-05-06)

Cross-subsystem deep-dives done in a single sweep. Each scores the subsystem
1–10 and lists material gaps with file:line evidence.

| Audit | Scored |
|---|---|
| [`KERNEL-INTEGRATION-ASSESSMENT-2026-05-06.md`](KERNEL-INTEGRATION-ASSESSMENT-2026-05-06.md) | 9/10 |
| [`STORAGE-INTEGRATION-ASSESSMENT-2026-05-06.md`](STORAGE-INTEGRATION-ASSESSMENT-2026-05-06.md) | 9.5/10 |
| [`GIT-INTEGRATION-ASSESSMENT-2026-05-06.md`](GIT-INTEGRATION-ASSESSMENT-2026-05-06.md) | 8/10 |
| [`SECURITY-INTEGRATION-ASSESSMENT-2026-05-06.md`](SECURITY-INTEGRATION-ASSESSMENT-2026-05-06.md) | 8/10 |
| [`EDITOR-INTEGRATION-ASSESSMENT-2026-05-06.md`](EDITOR-INTEGRATION-ASSESSMENT-2026-05-06.md) | 8.5/10 |
| [`TERMINAL-INTEGRATION-ASSESSMENT-2026-05-06.md`](TERMINAL-INTEGRATION-ASSESSMENT-2026-05-06.md) | 7.5/10 |
| [`AI-INTEGRATION-ASSESSMENT-2026-05-06.md`](AI-INTEGRATION-ASSESSMENT-2026-05-06.md) | 8/10 |

## AI interaction surface

| Audit | Purpose |
|---|---|
| [`AI-INTERACTION-SURFACE-AUDIT-2026-05-04.md`](AI-INTERACTION-SURFACE-AUDIT-2026-05-04.md) | What AI can actually interact with in Nexus today. Code-level audit with file:line citations. Source for ADR 0022, 0023. |
| [`AI-GAPS-2026-05-05.md`](AI-GAPS-2026-05-05.md) | Follow-up tracker — concrete `AIG-NN` items from the surface audit. |

## Shell UI

| Audit | Purpose |
|---|---|
| [`shell-ui-audit-2026-05-01.md`](shell-ui-audit-2026-05-01.md) | Shell UI architecture audit. |
| [`shell-ui-audit-backlog-2026-05-01.md`](shell-ui-audit-backlog-2026-05-01.md) | Companion backlog. |

## Architecture

| Audit | Purpose |
|---|---|
| [`architecture-audit-2026-05-01.md`](architecture-audit-2026-05-01.md) | Multi-dimension architecture audit. Source for ADR 0021. |
| [`architecture-audit-2026-05-01-plan.md`](architecture-audit-2026-05-01-plan.md) | Remediation plan derived from the audit. |

## Documentation

| Audit | Purpose |
|---|---|
| [`DOCS-AUDIT-2026-04-30.md`](DOCS-AUDIT-2026-04-30.md) | Doc-set audit that drove the `docs-reorg` branch. Kept as audit-trail snapshot. |
