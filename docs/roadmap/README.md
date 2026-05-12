# Roadmap

In-flight planning docs — work that is active, exploratory, or formally
deferred. Distinct from:

- **Architecture** ([`../architecture/`](../architecture/)) — load-bearing
  structural docs that describe how the system is built.
- **PRDs** ([`../PRDs/`](../PRDs/)) — authoritative product specifications.
- **ADRs** ([`../adr/`](../adr/)) — point-in-time decisions, immutable once accepted.
- **Archive** ([`../archive/`](../archive/)) — completed plans, superseded designs, point-in-time audits.

A doc lives in this directory if it describes work that *has not shipped
yet* but is intended to. Once a plan ships, move it under `archive/` with
a `> **Archived <date>** — <reason>` header; load-bearing details that
outlast the plan should be promoted to `architecture/` separately.

## Active

| Doc | What it covers |
|---|---|
| [`DOC-GAPS.md`](./DOC-GAPS.md) | 46 doc-bugs and product-gaps surfaced by the 2026-05-12 traceability audit ([`../audits/traceability-2026-05-12.md`](../audits/traceability-2026-05-12.md)). DG-01 through DG-31 are doc bugs (fix the doc); DG-32 through DG-46 are product gaps cross-listed in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Doc-audit-surfaced product gaps". |
| [`OPEN-ITEMS.md`](./OPEN-ITEMS.md) | One open item from the 2026-04-24 Phase 4 leaf-migration sweep: **OI-05** (Rust dep duplication, blocked upstream). The 21 resolved OIs live verbatim at [`../archive/OPEN-ITEMS-resolved-2026-04-26.md`](../archive/OPEN-ITEMS-resolved-2026-04-26.md). Cross-listed in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Post-migration carryover gaps". |
| [`REQUIRED-FOR-FORMAL-RELEASE.md`](./REQUIRED-FOR-FORMAL-RELEASE.md) | Work deferred from personal-tool scope to formal-release scope: WI-41 (auto-updater), WI-42 (Sentry), WI-44 (marketplace), WI-46 (beta→GA). Indexed from [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Formal release scope (deferred)". |

## Exploratory

Design rationale for ideas that may or may not be promoted into the
backlog. Indexed from [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md)
"Future directions (exploratory, not phased)".

| Doc | What it covers |
|---|---|
| [`AI-INTEGRATION-DIRECTIONS.md`](./AI-INTEGRATION-DIRECTIONS.md) | Where the AI surface area could go. Promote a direction into a scoped backlog item when work begins. |
| [`AI-MEMORY-LAYER-PLAN.md`](./AI-MEMORY-LAYER-PLAN.md) | Personal-memory-layer thinking — how Nexus could become a memory substrate for an AI assistant. |
| [`AI-AMBIENT-COPILOT-PLAN.md`](./AI-AMBIENT-COPILOT-PLAN.md) | Ambient copilot UX integration — what a non-modal AI presence in the shell would look like. |
| [`Nexus_Growth_Plan.md`](../PRDs/Nexus_Growth_Plan.md) | Long-term growth and roadmap planning. (Currently filed under `PRDs/`; not actually a PRD — should be relocated here on a follow-up.) |

## How a planning doc lifecycle works

1. **Idea phase.** Write in `roadmap/` (or in `roadmap/exploratory/` for
   ideas without a committed delivery date). Mark `Status: exploratory`
   if it's not yet promoted.
2. **Committed phase.** Once the work has owners and a target, the doc
   stays in `roadmap/` but gains scoped backlog items in `BACKLOG.md`
   tracking its delivery.
3. **Shipped phase.** Move the doc to `archive/` with a `> **Archived
   <date>** — shipped` header. If anything in the doc describes
   load-bearing architecture (an invariant, a contract, a wire format),
   promote that section to a parallel doc in `architecture/` *before*
   archiving so future readers can find it without spelunking.
4. **Superseded phase.** Same as shipped, but with `> **Archived <date>**
   — superseded by <pointer>`.

