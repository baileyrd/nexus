# Research

Comparative research, portability assessments, and ecosystem snapshots. None
of these is authoritative for Nexus design — they're inputs into roadmap and
ADR decisions. For active plans see [`../roadmap/`](../roadmap/); for accepted
decisions see [`../adr/`](../adr/).

## Comparisons against other tools

| Doc | What it covers |
|---|---|
| [`affine-portability-assessment.md`](affine-portability-assessment.md) | AFFiNE capability audit. Adopt / Adapt / Skip per item. **Adopt + Adapt items cross-listed** in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Research-surfaced ideas". |
| [`anything-llm-assessment.md`](anything-llm-assessment.md) | AnythingLLM feature audit. Adopt / Adapt / Skip per item. **Adapt items cross-listed** in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Research-surfaced ideas" (scoped API tokens, audio crate, browser STT/TTS). |
| [`commandbook-evaluation.md`](commandbook-evaluation.md) | Command-Book product evaluation. |
| [`gitnexus-capability-assessment.md`](gitnexus-capability-assessment.md) | GitNexus capability mapping. **7 scoped ports** cross-listed in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Research-surfaced ideas" (cross-repo code intel, diff→symbol detection, impact handler, BM25, MCP tools, doc generator, community pass). |
| [`nexus-vs-tolaria.md`](nexus-vs-tolaria.md) | Side-by-side architectural comparison with Tolaria. |
| [`nexus-vs-tolaria-ui.md`](nexus-vs-tolaria-ui.md) | UI/shell deep-dive comparison with Tolaria. |
| [`nexus-borrowings-from-tolaria.md`](nexus-borrowings-from-tolaria.md) | What Nexus could borrow from Tolaria, ranked. |
| [`nexus-shell-borrowings-from-tolaria.md`](nexus-shell-borrowings-from-tolaria.md) | Shell-narrow companion to the borrowings list. |

## Obsidian parity

| Doc | What it covers |
|---|---|
| [`obsidian-vs-nexus-api.md`](obsidian-vs-nexus-api.md) | Obsidian extension API vs Nexus, gap analysis. |
| [`obsidian-parity-canvas-bases.md`](obsidian-parity-canvas-bases.md) | Canvas + Bases parity assessment. |
| [`obsidian-top-50-plugins.md`](obsidian-top-50-plugins.md) | Top-50 plugin landscape; what would carry over. |
| `obsidian-community-plugins.json`, `obsidian-community-plugin-stats.json` | Raw data tables. |

## Exploratory plans

| Doc | What it covers |
|---|---|
| [`agentic-os-implementation-plan.md`](agentic-os-implementation-plan.md) | "Agentic OS" mode implementation outline. Source for BL-054. |
| [`hermes-agent-implementation-plan.md`](hermes-agent-implementation-plan.md) | Hermes Agent native-Rust port plan. 7 features (iteration budget, memory persistence, skills, context compression, session FTS search, multi-agent delegation, ACP adapter) scoped with merge order. **Cross-listed** in [`../PRDs/BACKLOG.md`](../PRDs/BACKLOG.md) "Research-surfaced ideas". |
| [`settings-stubs-audit.md`](settings-stubs-audit.md) | Settings-tab stub inventory. |
