# Nexus v0.1 — Development Roadmap

**Version:** 0.1
**Date:** 2026-04-11
**Status:** Approved (brainstorming session output)
**Scope:** Top-level roadmap for Nexus v0.1 — sequencing, scope cuts, milestone gates, risks. Per-phase implementation plans are produced separately.

---

## 1. Frame

Nexus is a personal-use IDE / dev environment specified across 17 PRDs in `PRDs/`. This roadmap turns those PRDs into a buildable plan under explicit constraints.

### Operating constraints

| Constraint | Decision |
|---|---|
| **Approach** | Strict PRD order (across phases). Within a phase, allow parallelism where the actual dependency graph permits. |
| **Operating model** | One human + AI agents. Throughput bottleneck is human review/decisions, not coding hours. |
| **Distribution** | Personal tool. Single user, single machine. No outside contributors, no commercial product. |
| **PRD authority** | Strong default, open to challenge. Overbuilds for personal-tool framing are flagged and cut where the user agrees. |
| **Dogfood target** | Full daily dev environment. The success test is "I do my entire dev day in Nexus." |

### Goals

1. Build Nexus v0.1 as a personal-use IDE following the (slimmed) 17-PRD specification.
2. Implement subsystems in **strict PRD order**: each phase complete to its acceptance criteria before the next begins.
3. Optimize for **clean, stable contracts at PRD boundaries** so agent work can be delegated cleanly per-PRD without holding the whole graph in human context.
4. End state: Nexus is the user's daily dev environment, capable across editing, terminal, database, git, AI, agents, and workflows.

### Non-goals

1. **Outside contributors / OSS hygiene.** No contributor docs, issue templates, code-of-conduct work in v0.1.
2. **Multi-user / multi-tenant scenarios.** Single-user threat model is sufficient.
3. **Commercial polish.** No marketing site, telemetry/billing, support burden planning.
4. **Cross-platform.** PRD 17 (Cross-Platform Strategy / M6) is deferred indefinitely as personal-tool YAGNI. v0.1 builds for the one machine in use.
5. **Mid-phase dogfooding milestones.** Strict PRD order explicitly accepts that nothing is user-visible until end of M2. No "dogfood gate" injected mid-phase.

---

## 2. Milestone Structure

Six milestones, one per PRD phase, in the index's order, with M6 marked as deferred.

| # | Milestone | PRDs | Gate (high-level) |
|---|---|---|---|
| **M1** | Foundation | 01 Kernel, 02 Security (slimmed), 03 Storage, 04 Plugins (slimmed), 04a Plugin Templates, 05 CLI | All M1 PRD acceptance criteria pass. The `nexus` CLI can drive the kernel headlessly: load a plugin, register a capability, read/write a file through the storage engine, dispatch an event. *No GUI yet.* |
| **M2** | Core Surfaces | 06 File Formats, 07 Theming/UI (slimmed), 08 Editor | Desktop app launches. User can open a file tree, open and edit markdown/MDX/Canvas/Bases files, save them, see their theme. **First milestone where Nexus is visibly Nexus.** |
| **M3** | Developer Power Features | 09 Terminal, 10 Database, 11 Git | Three independent feature subsystems land. User can spawn a terminal, query a `.bases` database, commit to git — all from inside Nexus. |
| **M4** | Intelligence Layer | 12 AI Engine, 13 Skills (slimmed), 14 MCP | AI assistance works across editor, terminal, storage. Skills can be authored. Nexus can both consume external MCP servers and expose itself as one. |
| **M5** | Autonomy & Automation | 15 Agents (slimmed), 16 Workflows (slimmed) | Agents can run multi-step tasks against the workspace with approval gates. Workflows can fire on file events, cron, manual triggers, and webhooks; can run agents and AI steps as workflow steps. |
| **M6** | Cross-Platform | 17 Cross-Platform | **Deferred indefinitely.** Listed for completeness only. Not in v0.1 scope. |

### Within-milestone parallelism

Strict PRD order applies *across* phases. *Within* a phase, PRDs run in parallel where the actual dependency graph allows (this is a slight liberalization of the index's strict chains where the chains weren't load-bearing).

| Milestone | Sequencing | Max concurrent agent streams |
|---|---|---|
| M1 | Mostly serial: 01 → 02 → 03 → 04 → 05; 04a parallel with late-stage 04 | 1 (mostly), 2 late |
| M2 | 06 ∥ 07, then 08 | 2 |
| M3 | 09 ∥ 10 ∥ 11 (fully independent per index) | 3 |
| M4 | 12 first, then 13 ∥ 14 | 2 |
| M5 | 15 → 16 (workflows want to invoke agents as steps) | 1 |

M3 is the maximum-parallelism milestone and the highest review-bandwidth load on the human. If review bandwidth is the bottleneck, any two of {09, 10, 11} can be serialized without breaking the plan.

---

## 3. Per-PRD Scope Cuts

These are the "personal-tool YAGNI" cuts approved during brainstorming. Net effect is **~10–12% of PRD spec volume**, concentrated in distribution/community/multi-user infrastructure. Cuts are deferred (PRDs stay on disk with cut sections marked), not deleted — they can be revived if a need surfaces.

### PRD 02 — Security Model
- **Cut:** Sync & replication threats (CRDT peer auth, relay E2E encryption, peer impersonation, relay data harvesting).
- **Cut:** Audit log subsystem (rotation, JSONL export, merkle tamper-detection). Keep simple debug logging only.
- **Cut:** Plugin code review/approval workflows (community submission review, author verification, escalation).
- **Cut:** Credentials key rotation / emergency revocation ceremony. OS keychain integration is enough.
- **Hard keep:** WASM sandbox (defense-in-depth still matters even with self-authored plugins, especially with third-party MCP servers), threat matrix (in simplified form), capability checks.

### PRD 04 — Plugin System
- **Cut:** WASM runtime vendor justification (Wasmtime vs Wasmer trade-off discussion). Decision is made.
- **Cut:** Plugin marketplace, community registry, plugin discovery UI, ratings, auto-update from registry.
- **Keep (user override):** Plugin Settings UI (per-plugin config panels, JSON Schema generator).
- **Hard keep:** WASM sandbox + capability system, plugin manifest format.

### PRD 04a — Plugin Templates
- **Keep (user override):** Both `core-plugin` and `community-plugin` template variants.
- **Hard keep:** Core manifest, source/test/Cargo specs.

### PRD 07 — Theming & UI
- **Cut:** Theme marketplace, community themes, theme gallery, theme publishing/import workflows.
- **Cut:** WCAG 2.1 AA accessibility scope. Add only if a personal need arises.
- **Keep (user decision):** Native platform chrome variants (Windows Mica/Acrylic). Cost flagged in Risks (R6) — re-evaluate if Tauri integration on WSL+Windows proves painful.
- **Hard keep:** CSS variable engine, workspace layout (split-panes, tabs, persistence).

### PRD 13 — Skills
- **Cut:** Skill authoring SDK polish (`nexus skill create --interactive`, `nexus skill lint`, `nexus skill test` UX).
- **Cut:** Skill effectiveness tracking, opt-in telemetry, dashboards.
- **Cut:** Full skill browser/discovery UI. `nexus skills list` is enough.
- **Hard keep:** File format, registry, activation, composition, parameters, prompt-injection safety, built-in skills, basic `nexus skill create` scaffolder.

### PRD 14 — MCP Integration
- **Keep both host AND server sides (user decision).** ~500-line cut reverted. Full PRD as written.
- Rationale: user wants Nexus to be addressable as an MCP server by other AI clients (Claude Code, Cursor, etc.), not host-only.

### PRD 15 — Agent System
- **Cut:** Multi-agent coordination (agent-to-agent messaging, shared memory pools, delegation patterns).
- **Hard keep:** Human-in-the-loop approval gates (essential safety), all 6 archetypes (Coding, Research, Refactor, Documentation, Review, Automation — user decision), agent trait + execution engine, tool access control.

### PRD 16 — Workflow System
- **Cut:** Workflow sharing / publish / community gallery.
- **Cut:** MCP event triggers, process event triggers (advanced trigger types).
- **Keep (user override):** Webhook triggers, HTTP actions, notification actions.
- **Hard keep:** Definition format, variables, conditions, control flow (sequential/parallel/conditional/loops), file/db/terminal/AI step actions, testing, CLI, history, all 10 built-in workflow templates (user decision).

### PRDs not flagged (clean as-is)
**01 Kernel, 03 Storage, 05 CLI, 06 File Formats, 08 Editor, 09 Terminal, 10 Database, 11 Git, 12 AI Engine** — implemented as written.

---

## 4. Milestone-by-Milestone Detail

### M1 — Foundation

**PRDs:** 01 Kernel, 02 Security (slimmed), 03 Storage, 04 Plugins (slimmed), 04a Plugin Templates, 05 CLI

**Dependency shape:**
```
01 Kernel ──> 02 Security ──> 03 Storage ──> 04 Plugins ──> 05 CLI
                                                  └──> 04a Templates (parallel late)
```

**Agent workstream pattern:** Mostly serial — one PRD at a time. Late M1, run 04 and 04a concurrently. This is the milestone with the *least* agent parallelism leverage; the human is most engaged in interface design here.

**Gate:** All acceptance criteria for PRDs 01–05 + 04a pass. The `nexus` CLI can drive the kernel headlessly: load a plugin, register a capability, read/write a file through the storage engine, dispatch an event. No GUI yet — that's M2.

### M2 — Core Surfaces

**PRDs:** 06 File Formats, 07 Theming/UI (slimmed), 08 Editor

**Dependency shape:**
```
06 File Formats ──┐
                  ├──> 08 Editor
07 Theming/UI ────┘
```

**Agent workstream pattern:** Two parallel streams (06, 07), then 08 picks up. First milestone with meaningful concurrent work to track.

**Gate:** Desktop app launches. File tree, open and edit markdown/MDX/Canvas/Bases, save, see theme. **First milestone where Nexus is visibly Nexus.**

### M3 — Developer Power Features

**PRDs:** 09 Terminal, 10 Database, 11 Git

**Dependency shape:**
```
09 Terminal ──┐
10 Database ──┼── all independent (per index)
11 Git ───────┘
```

**Agent workstream pattern:** Three independent agent workstreams running concurrently. Maximum parallelism, maximum review-bandwidth load on the human. Serialize any two of these if review becomes the bottleneck.

**Gate:** All three subsystems pass acceptance criteria. Spawn a terminal, query a `.bases` database, commit to git — all from inside Nexus.

### M4 — Intelligence Layer

**PRDs:** 12 AI Engine, 13 Skills (slimmed), 14 MCP (full)

**Dependency shape:**
```
12 AI Engine ──┬──> 13 Skills
               └──> 14 MCP
```

**Agent workstream pattern:** 12 first as solo workstream, then 13 and 14 as parallel pair.

**Gate:** AI assistance works across editor, terminal, storage. `.skill.md` files activate. Nexus consumes external MCP servers (Postgres, GitHub, etc.) and exposes itself as an MCP server to external clients.

### M5 — Autonomy & Automation

**PRDs:** 15 Agents (slimmed), 16 Workflows (slimmed)

**Dependency shape:**
```
15 Agents ──> 16 Workflows
```

15 grounds first because workflows want to invoke agents as steps. The reverse direction (agents triggered by workflows) is mediated by the event bus, not by direct module dependency.

**Agent workstream pattern:** Two sequential PRDs. Each is large enough to occupy a full agent workstream on its own.

**Gate:** Agents can autonomously run multi-step tasks against the workspace with approval gates. Workflows can fire on file events, cron, manual triggers, and webhooks; can run agents and AI steps as part of their step graph.

### M6 — Cross-Platform *(deferred)*

**Not in v0.1 scope.** PRD 17 is shelved indefinitely. If revived later, it would be its own roadmap exercise — porting a five-milestone-deep system to new platforms is its own significant project.

---

## 5. Cross-Cutting Concerns

### 5.1 Inter-PRD contract freezing

The most expensive failure mode for solo+agents work is contract churn: finishing PRD 04 and then discovering PRD 12 needed a different plugin lifecycle hook, forcing rework.

**Practice:** at the start of each PRD, write down its public interfaces (Rust traits, event-bus event names, capability strings, CLI commands) in a short interface spec. Treat that as the contract for any downstream PRD that depends on it. Only revise it through an explicit "contract amendment" decision — not silent agent edits.

### 5.2 Decision logs (ADRs)

Solo + agents means it is easy to forget *why* a choice was made three weeks later, and an agent picking up a related task will re-decide it the wrong way.

**Practice:** lightweight ADRs per non-trivial decision — one short markdown file per decision, ~10 lines, named `docs/adr/NNNN-short-title.md`. Agents read these before touching adjacent code. **Probably the highest-leverage practice for solo+agents work that no one bothers to do.**

### 5.3 Testing strategy

- **Per-PRD acceptance tests** live in `tests/acceptance/<prd-number>/`, mirroring PRD numbers.
- **Unit tests** live next to code per Rust convention.
- **Cross-PRD integration tests** appear at milestone boundaries: M1 has "kernel + storage + plugins + CLI" integration tests; M2 adds editor-with-storage; M3 adds terminal-into-storage; M4 adds AI-with-everything; M5 adds agents/workflows over the full surface.
- **Milestone gate** is not passed until both per-PRD and milestone-level integration tests pass.

### 5.4 Agent delegation pattern

For each PRD:
1. **Human reads the PRD** and writes the interface spec (5.1). Agents are bad at fresh contract design.
2. **Human drafts a per-PRD implementation plan** via the writing-plans skill.
3. **Agents execute task-level work** one logical chunk at a time.
4. **Human reviews each chunk** before moving on.

**Don't delegate the planning step.** Agents executing a plan is fast and reliable; agents inventing a plan is slow and lossy.

### 5.5 Post-milestone dogfooding

Even though Approach 1 has no mid-phase dogfood gates, **after each milestone, the user actually uses the slice they just shipped for ~1 week of real work** before starting the next milestone. After M2: a week of writing notes in Nexus. After M3: a week of real terminal+git+database work in it. After M4: a week with AI assistance on real code. This is the feedback loop that strict PRD order otherwise denies.

### 5.6 PRD↔roadmap drift

When reality contradicts a PRD during build, **edit the PRD to match reality** (don't just code around it) and add a one-line note in the ADR log. Otherwise the PRDs become a fictional artifact that is no longer trustworthy as documentation.

---

## 6. Risks

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| **R1** | **Foundation drift.** By M4, M1 decisions (capability system, event bus shape, plugin manifest) need to change, forcing rework. | Medium | High | Inter-PRD contracts (5.1); aggressive integration tests at M1 gate; willingness to revise M1 contracts via amendment. |
| **R2** | **Long no-feedback stretch in M1.** Approach 1's biggest cost is months of foundation work with no visible product. Risk of motivation collapse or scope creep. | High | Medium | After PRD 05 lands, dogfood the `nexus` CLI itself for non-Nexus workspace tasks. Catches usability issues + provides visible-progress moments. |
| **R3** | **Agent context overflow.** Several PRDs are 50–60KB. Even Opus 4.6 can lose detail mid-task on a 60KB spec. | Medium | Medium | Per-PRD interface specs (5.1) become the agent's primary input. Raw PRD is reference; interface spec is the contract. Smaller, focused agent prompts. |
| **R4** | **Tech-stack breakage.** Many moving parts (Tauri, wasmtime, comrak, sqlite-vec, git2, CodeMirror 6, Tantivy). One breaking change blocks progress. | Medium | Medium | Pin all versions in Cargo.toml/package.json from day 1. `cargo update` deliberately, not on autopilot. |
| **R5** | **Personal-tool cuts come back as needs.** Cut sync threats / audit logging / plugin marketplace / etc. — some may turn out to be needed. | Low–Medium | Medium | Cuts are *deferred*, not *deleted*. PRDs stay on disk with cut sections marked. If a need surfaces, the spec is still there to implement against. |
| **R6** | **WSL+Windows Tauri development.** Tauri desktop development on WSL+Windows has historically been rough at the edges, especially native chrome (Windows Mica APIs). | Medium | Medium | Smoke test Tauri startup very early in M2 before deep editor work. If painful, native chrome (PRD 07) is the easiest cut to revisit. |
| **R7** | **Integration creep at milestone gates.** Passing the gate becomes a project of its own as integration tests reveal latent problems. | Medium | Medium | Budget explicit "stabilization time" at the end of each milestone. Don't treat the last PRD's "done" as the milestone's "done". |
| **R8** | **Solo decision bottleneck.** Agents finish in parallel, all need review at once. M3 (3 parallel streams) is the worst case. | Medium | Medium | Cap concurrent agent streams at what review can absorb. Roadmap *allows* 3-way parallel; doesn't *require* it. |

---

## 7. Definition of v0.1 Done

Nexus v0.1 is "done" when **all five** of the following are true:

1. **All M1–M5 milestone gates pass.** Per-PRD acceptance criteria + cross-PRD milestone-boundary integration tests, all green. (M6 explicitly excluded.)

2. **The post-M5 dogfood week succeeds.** After M5 ships, the user spends one full work week using Nexus exclusively for real dev work — code, notes, terminal, git, AI assistance, agents, workflows. At the end of the week, the user can answer "yes" to: *"If I had to give up either Nexus or every other dev tool I used to use, would I keep Nexus?"* If the answer is no, the result is a punch list, not v0.1 — iterate until the answer flips.

3. **No load-bearing PRD has unresolved deviations.** Anywhere reality contradicted a PRD during build, the PRD has been updated to match reality (per 5.6). PRDs remain trustworthy as documentation at v0.1. They are not a fictional artifact.

4. **Decision log is complete enough to onboard self in 6 months.** If the user stops working on Nexus for 6 months and comes back, the ADRs in `docs/adr/` plus the (now-up-to-date) PRDs should let them regain context without re-reading the source.

5. **The cuts you made still feel right.** At v0.1 freeze, re-read the deferred sections from PRDs 02/04/04a/07/13/15/16. Confirm none surfaced as a real need during build. Anything that did → note in the post-v0.1 backlog. Otherwise, cuts are validated for v0.1; revisit at v0.2 if needed.

### Explicitly NOT part of v0.1 done

- ❌ Cross-platform support (M6 deferred)
- ❌ Outside contributor onboarding
- ❌ Public release / OSS hygiene (no GitHub README, CI, release artifacts unless personally wanted)
- ❌ All possible plugins / themes / skills authored (built-ins from each PRD only; user-authored extras are post-v0.1)
- ❌ Performance optimization beyond PRD acceptance criteria
- ❌ Every TODO/FIXME in the codebase resolved (only those that block acceptance criteria)

v0.1 is the first usable cut, not a polished release. v0.2+ can iterate on polish, performance, and revived backlog items as the dogfood experience reveals what actually matters.

---

## 8. Post-v0.1 Backlog

These are the deferred-but-documented items from scope cuts and the M6 deferral. They live in `BACKLOG.md` at v0.1 so future work knows what's available to revive.

| Item | Source | Trigger to revive |
|---|---|---|
| Multi-peer CRDT sync (Ed25519 auth, relay E2E) | PRD 02 (cut) | If syncing Nexus across devices |
| Audit log subsystem (rotation, JSONL export, merkle) | PRD 02 (cut) | If compliance/forensics needed |
| Plugin marketplace + community registry + ratings | PRD 04 (cut) | If open-sourcing Nexus |
| Community plugin code review/approval workflows | PRD 02 (cut) | Same as above |
| Theme marketplace + community themes | PRD 07 (cut) | Same as above |
| WCAG 2.1 AA accessibility | PRD 07 (cut) | If a personal accessibility need arises |
| Skill authoring SDK polish (lint/test/interactive) | PRD 13 (cut) | If authoring many skills |
| Skill effectiveness telemetry | PRD 13 (cut) | Probably never |
| Workflow community gallery + publish | PRD 16 (cut) | If open-sourcing Nexus |
| MCP event triggers, process event triggers | PRD 16 (cut) | If a workflow needs them |
| Agent multi-agent coordination (agent-to-agent messaging) | PRD 15 (cut) | If single-agent execution proves limiting |
| **M6: Cross-Platform Strategy** | PRD 17 (deferred) | If Nexus on mobile/web/another machine becomes a need |
| PRD↔code alignment final pass | (process artifact) | At every minor version bump |

---

## 9. Next Steps

This roadmap is the **A-artifact** from the brainstorming session. The **B-artifact** is a focused Phase 1 (M1) implementation plan, produced via:

1. **Phase 1 brainstorming session** — focused spec for M1 at the level of "what does the kernel actually expose, what does the slimmed security model look like in detail, what does the storage engine's public API look like, what does the plugin loader's lifecycle look like, what `nexus` CLI commands does M1 actually ship with." This is its own brainstorming flow with M1-specific clarifying questions.

2. **Phase 1 implementation plan** — output of the writing-plans skill, consuming the M1 spec. Task-level plan that can be handed to agents one PRD at a time.

3. **Execution** — agents implement against the plan; human reviews each chunk; ADRs captured for non-trivial decisions; PRDs updated when reality contradicts them.

After M1 ships and its post-milestone dogfood week (CLI-only) completes, the cycle repeats for M2: brainstorm M2 spec → write M2 plan → execute → dogfood. And so on through M5.

### Open follow-ups (out of this roadmap's scope)

- **PRD↔code naming alignment in template files.** The `forge`→`nexus` rename pass touched PRDs but not `PRDs/templates/` (deferred). Handle when M1 PRD 04a work begins.
- **05-cli.md §9.1 vs §3.1.3 short/long config command form.** Agent normalized to long form (`nexus forge config`); revisit if a top-level `nexus config` alias is wanted.
- **13-skills.md singular/plural CLI naming** (`nexus skill` vs `nexus skills`). Unify in a follow-up pass if it bothers you in practice.

---

**End of roadmap. v0.1 is approved when this document is signed off.**
