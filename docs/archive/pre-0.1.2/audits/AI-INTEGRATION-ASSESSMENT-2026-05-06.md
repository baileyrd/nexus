# AI Integration Assessment
_Assessed: 2026-05-06_

## Overall: 8/10 — Solid first-class foundation with specific gaps

The architecture gets the fundamentals right. AI routes exclusively through the `com.nexus.ai` IPC
boundary — no shell-to-Rust direct linkage, no bespoke Tauri commands for AI features, capability
gating enforced everywhere. That discipline means every frontend (CLI, TUI, MCP, shell, agent,
workflow) gets the same AI surface without duplication.

---

## Where AI genuinely is first-class

**Streaming.** Per-token kernel-bus events (`com.nexus.ai.stream_*`) consumed by three distinct
surfaces (chat panel, margin suggestions, inline completion). No polling, no per-surface
reimplementation.

**Tool calling.** Registry → dispatch → MCP bridge loop is complete. The `stream_chat` agentic loop
(up to 8 rounds) is how an LLM that cares about the forge would actually operate. Policy gating
(`auto`, `auto_readonly`, `auto_with_mcp`) gives the right control surface.

**Agent + approval.** Planning, archetype prompts, step-level approval, history persistence — these
are the bones of a system that treats AI as an *actor*, not just a text generator.

**RAG with citations.** Sources tracked through retrieval and rendered as file-path chips with hover
previews. That's the right UX primitive for a knowledge forge.

**Skills as prompt composition.** The skill dependency DAG feeding into agent planning lets AI
behavior be customized by the user without touching code.

**MCP bidirectional.** Nexus as MCP server *and* MCP host simultaneously. Auto-discovery of external
tools at plan time is genuinely powerful.

---

## Where it's still second-class

### 1. Privacy redaction is partial
`Redactor` runs on RAG-retrieved chunks but not on user-typed prompts or provider HTTP bodies. A
user who types a secret into the chat gets no protection. For a personal knowledge forge, this is a
trust issue.

### 2. Ambient AI surfaces are stubs
`marginSuggest.ts` and `CmdIOverlay.tsx` are skeleton/not wired to the shell UI. The vision of AI
suggestions appearing *while you write* (BL-034/035/036) is not yet reality — AI is still
opt-in invocation, not ambient presence.

### 3. Local embeddings are inactive by default
The fastembed wrapper exists (422 lines in `local_embedding.rs`) but is feature-gated out. Users
must send forge content to Anthropic/OpenAI to get RAG. For a personal knowledge app, that's a
meaningful gap.

### 4. Token budget is advisory, not enforced
The budget is tracked and warnings generated, but providers still receive payloads that may exceed
the context window. The system knows it's over-budget and proceeds anyway.

### 5. Agent session-run approval is incomplete
The multi-round session loop runs with `auto_approve: true`. Phase 2b (user approval callbacks
mid-session, ADR 0024) exists as a stub. For any task that could *write* to the forge, this should
be mandatory before shipping.

### 6. TUI has no AI surface
`Mode::Ai` exists in the enum, but nothing is implemented. If AI is first-class, it should work
from every frontend.

---

## The core question: is AI integral?

For the **chat + RAG + tool use** path: **yes**. The IPC discipline, streaming architecture, and
multi-frontend sharing are genuinely well-engineered.

For the **ambient / proactive AI** vision — suggestions appear as you write, enrichment happens on
save, the agent monitors the forge and surfaces insights — **not yet**. The scaffolding exists and
the architectural path is clear, but the features themselves are stubs.

The system is first-class in its *plumbing* and second-class in its *presence*. Closing the gap
means wiring the ambient surfaces (margin suggestions, auto-enrichment, continuous indexing
feedback) rather than adding more infrastructure.

---

## Scorecard

| Dimension | Score | Notes |
|---|---|---|
| Provider abstraction | 10/10 | Trait-based, 3 implementations, runtime detection |
| Chat streaming | 10/10 | Per-token events, multi-frontend, no polling |
| RAG pipeline | 9/10 | Full retrieval, citations, token budgeting; privacy incomplete |
| Tool calling | 9/10 | Registry, dispatch, MCP bridge, policy-gated |
| Agent system | 9/10 | Planning, approval, history, archetypes; session approval incomplete |
| Workflow integration | 8/10 | AI steps, triggers, conditions; no parallel/retry |
| IPC boundary integrity | 10/10 | Enforced everywhere, capability gating throughout |
| Ambient AI presence | 4/10 | Margin suggestions, inline rewrite, auto-enrichment are stubs |
| Local-first operation | 5/10 | Local embeddings opt-in only; remote required for RAG |
| Testing | 7/10 | Library-level unit tests solid; E2E sparse |

---

## Key files

```
crates/nexus-ai/src/core_plugin.rs       — 20 IPC handlers
crates/nexus-ai/src/provider.rs          — AiProvider trait
crates/nexus-ai/src/rag.rs               — Retrieval pipeline
crates/nexus-ai/src/tools/               — Registry, MCP bridge, built-ins
crates/nexus-ai/src/privacy.rs           — Redaction (partial)
crates/nexus-ai/src/local_embedding.rs   — fastembed wrapper (inactive)
crates/nexus-agent/src/llm.rs            — LlmAgent + ChatDriver trait
crates/nexus-agent/src/executor.rs       — PlanExecutor
crates/nexus-workflow/src/ai_steps.rs    — ai_prompt / ai_decision step types
shell/src/plugins/nexus/ai/ChatView.tsx  — Primary chat surface
shell/src/plugins/nexus/ai/aiRuntime.ts  — IPC bridge
shell/src/plugins/nexus/ai/marginSuggest.ts  — Stub (not wired)
shell/src/plugins/nexus/ai/CmdIOverlay.tsx   — Stub (not wired)
```
