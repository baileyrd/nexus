# Claude Code Agentic OS — Implementation Plan

**Source:** "Build Your CUSTOM Claude Code Agentic OS (3 Steps)" — Chase AI, 2026-05-04
**Runtime:** ~17 min, three steps + intro/overview
**Author of this plan:** Claude, against Nano's stack
**Revision:** v2 — incorporates findings from frame extraction (architecture diagram metadata, execution taxonomy, LightRAG observation)

---

## TL;DR — Chase's framework in one line

> **Domains → Tasks → Skills → Automations → Architecture** — wrapped in a **Memory layer** (Obsidian) and an **Observability layer** (dashboard + usage panels).

He's emphatic that **Step 1 (Architecture) is the load-bearing one**:
> *"if you do nothing else with this whole [agentic OS] system... if you just stopped here, you would get a ton of value."*

Memory and Observability multiply Step 1's value but don't replace it.

---

## What Chase actually prescribes

### Step 1 — Architecture (0:37 → 7:31)

The pipeline:

1. **Inventory** every recurring thing you do, personal + business.
2. **Bucket into domains** — his examples: *memory, productivity, research, content, community*. Yours will differ.
3. **Decompose each domain into discrete tasks** — e.g., research → YouTube lookup, deep research, light-rag query, morning report, competitor watch.
4. **Codify each task as a skill** via Claude Code's skill creator.
5. **Wrap recurring skills in automations** — local or remote. Chase explicitly says don't worry about which: tell CC the goal, let it pick. ("I want to create a local automation or remote automation and it will be able to figure it out.")

**Domain metadata** — Chase's diagram annotates each domain with two attributes the transcript never mentions:

- **`FOUNDATIONS · always on`** vs. **`CAPABILITIES · modular`** — foundations are always-running infrastructure (Inbox Triage, Calendar Brief, Drive Sync, Daily Review, Morning Routine). Capabilities are modular skills you invoke as needed.
- **Memory destination per task** — every task in the diagram carries a label: `raw`, `wiki`, or `project`. The architecture artifact *is* the memory-routing contract; there's no separate document for it.

**Execution taxonomy at the top of the diagram**: `Manual / Skills / Commands / Agent`. Tasks aren't all skills — some are slash-commands, some are sub-agents, some stay manual. Default to skills, but classify deliberately.

**Full domain set visible in his diagram**: Memory · Productivity · Research · Content · Community · Agency · Sales · Finance · Dev (nine, not the five he lists in dialogue).

**Method:** open Claude Code, mic on, stream-of-consciousness brain-dump of what you do. CC iteratively interviews you, clusters into domains, proposes tasks, asks for each: *"Should this be a skill? Should the skill be an automation?"* He sells the exact prompt in his Skool community; the structure I've reconstructed below is faithful to what he describes.

**Why this matters beyond just you:** Chase's framing is that codifying everything as skills = packageable. Hand the whole OS to a teammate or client; they get CC's power without ever opening a terminal (which is what Step 3 enables).

### Step 2 — Memory (7:31 → 12:53)

- **Obsidian, primarily.** Chase says in dialogue: *"For 99.9% of people, you don't need even something as lightweight as light rag. You don't need a vector database. It's too much."* But his own architecture diagram lists `LightRAG Query` as one of his Research tasks. **Trust the architecture, not the dialogue** — markdown is the substrate, vector retrieval is reserved for specific tasks where corpus size warrants it.
- **Vault** = a designated folder. Run CC inside it. Everything lives there.
- **Karpathy's three-folder pattern** as the starting structure:
  - `raw/` — dumping ground (research, scratch, transcripts)
  - `wiki/` — codified articles synthesized from `raw/`
  - `output/` — final deliverables (slide decks, reports)
- **Chase's own variant** adds: `archive/`, `content/`, `ops/`, `personal/`, `projects/` alongside `raw/` and `wiki/`. Customize, but make it make sense.
- **Critical:** a vault-root `CLAUDE.md` that documents the memory structure. Without it, CC burns tokens guessing the layout. With it, CC navigates efficiently and you do too.

### Step 3 — Observability (12:53 → 16:14)

- **A UI outside the terminal** — every skill and automation becomes a clickable button.
- **Click → headless CC invocation** (`claude -p` / `--print`).
- **Status panels** for: 5-hour usage window, weekly window, routine counts, recent vault changes, vault-state forecast.
- **Real value isn't for you.** It's for **non-terminal team members and clients** who get CC's power without touching CC. Chase: *"I could take anybody, put them in this chair, put them in front of the agent OS and say do X, Y, and Z. Here's the skills. They can do it."*
- Same conversational build pattern as Step 1: paste his prompt, tell CC which skills to surface, what observability widgets you want, iterate.

**Note on his "dashboard":** the visual at 0:42 he keeps gesturing at is **excalidraw.com — a planning artifact, not a running tool**. The implementation is derived from it. Worth being explicit about: you're building two things, the planning diagram and the actual dashboard, and they should stay in sync.

---

## How this maps to your stack

You already have most of the substrate. Don't rebuild what you've built.

| Chase prescribes | What you already have | Recommendation |
|---|---|---|
| Obsidian vault | llm-wiki-kit + ingest + vault CLI; remind-me MCP | **Use llm-wiki-kit's structure as the starting point.** Add Karpathy's `raw/wiki/output` as semantic top-level namespaces. |
| Vault-root `CLAUDE.md` | nano-voice-guide.md + design-principles CLAUDE.md | **Compose, don't replace.** Add a memory-map section naming every top-level folder. |
| Skill creator skill | CC's built-in skill primitive; existing `/mnt/skills/user/*` | **Use as-is.** Your existing skill set is already the right pattern (shell-ui-architecture-audit, microkernel-architecture-audit, watch). |
| Stream-of-consciousness domain elicitation | (none yet) | **Highest-leverage 30 minutes in the whole plan.** Run it once, capture as `architecture.md`. |
| Dashboard with skill/automation buttons | Datastar Pro fullstack template progression; FastAPI/SQLModel/SSE | **Build on Datastar Pro.** Strictly better than whatever Chase uses. |
| Usage tracking | (none) | **Add.** Parse `~/.claude/` logs + Anthropic usage API. |
| Markdown-first memory + targeted vector retrieval | local-ragstack + Qdrant + Ollama running; LightRAG familiar | **Mirror Chase's actual pattern.** Markdown as substrate, LightRAG/Qdrant for tasks whose corpus exceeds CC's grep/glob navigability. See divergence section. |
| Excalidraw planning diagram | (none yet) | **Build one.** Mirrors `architecture.md` visually; useful for stakeholder review and team handoff. |

---

## Phased implementation

### Phase 0 — Decide the OS root (15 min)

Two reasonable options:

1. **Inside llm-wiki-kit** — reuse existing structure, add an `os/` namespace at top level.
2. **New vault alongside llm-wiki-kit** — keeps the OS clean; llm-wiki-kit becomes one of its `output/` consumers.

**Recommendation: option 2** for the first pilot. Keep the OS architecture pure. Write one-way bridges into llm-wiki-kit (and later FORGE) as you need them.

Concrete location: `~/vaults/agentic-os/`

### Phase 1 — Architecture inventory (45–90 min)

Goal: every recurring thing you do becomes an explicit, named, codified task with a four-attribute tag. No more ad-hoc "claude do X."

**Reconstructed prompt** (Chase doesn't share his — this captures the structure he describes, expanded with the metadata dimensions visible in his diagram):

```
You are helping me architect a Claude Code Agentic OS following Chase AI's
Domains → Tasks → Skills → Automations pattern.

Step 1 — Interview me about what I do day-to-day. I'll talk freely. Cluster
what I describe into domains. Examples: memory, productivity, research,
content, community, agency, sales, finance, dev. Mine may differ.

Step 2 — For each domain, list discrete recurring tasks. A task is specific
enough that it could be invoked the same way every time.

Step 3 — For each task, decide:
  (a) Should this become a skill, slash-command, sub-agent, or stay manual?
      Default: skill. Use slash-command for high-frequency keystroke
      shortcuts. Use sub-agent for tasks that spawn parallel work or
      need their own context window. Manual for things too judgment-heavy
      to codify.
  (b) Is the parent domain a FOUNDATION (always-on infrastructure) or a
      CAPABILITY (modular, on-demand)? Foundations imply scheduled
      automation; capabilities imply on-demand invocation.
  (c) Where does the task's output live: raw/, wiki/, project/<name>/,
      or output/? Tag this on the task itself.
  (d) If the skill should be wrapped in an automation: local (cron /
      systemd / launchd) or remote (GitHub Action, Anthropic schedule)?

Step 4 — Write the result to vault/architecture.md as a hierarchy I can
maintain. For each task, emit four annotations:
    [<execution-type> | <foundation|capability> | <memory-dest> | <automation>]
e.g.:
    daily-trend-scan  [skill | foundation | raw | local cron 0700]
    deep-research     [skill | capability | raw | none]
    inbox-triage      [skill | foundation | wiki | local cron 0530]

Ask only when ambiguous; otherwise pick reasonable defaults and flag.

Context: I work in DoD environments with NIPR/SIPR constraints.
Local-first, sovereign infrastructure is a hard preference. Default to
local automation unless remote is materially better.
```

**Expected output** — `~/vaults/agentic-os/architecture.md`:

```
research/                                    [CAPABILITIES · modular]
  daily-trend-scan      [skill   | foundation | raw     | local cron 0700]
  deep-research         [skill   | capability | raw     | none]
  competitor-watch      [skill   | foundation | wiki    | local cron weekly]
  youtube-lookup        [skill   | capability | raw     | none]
  llm-wiki-kit-ingest   [skill   | capability | wiki    | local cron daily]
  lightrag-query        [skill   | capability | raw     | none]

productivity/                                [FOUNDATIONS · always on]
  inbox-triage          [skill   | foundation | wiki    | local cron 0530]
  calendar-brief        [skill   | foundation | project | local cron 0600]
  daily-review          [skill   | foundation | project | local cron 1800]
  morning-routine       [skill   | foundation | project | local cron 0700]

content/                                     [CAPABILITIES · modular]
  briefing-to-dashboard [skill   | capability | output  | none]   # built
  manpower-update       [skill   | capability | output  | none]

ops/                                         [CAPABILITIES · modular]
  dev-cleanup           [command | capability | none    | none]   # slash-cmd, exists
  zsh-toggle            [command | capability | none    | none]   # slash-cmd, exists

forge/                                       [CAPABILITIES · modular]
  loe-agent-pipeline    [agent   | capability | project | none]   # sub-agent
  asot-ingest           [skill   | foundation | wiki    | local cron daily]
  twc-mcp-query         [skill   | capability | project | none]   # MCP, exists

nexus-forge/                                 [CAPABILITIES · modular]
  plugin-scaffold       [skill   | capability | project | none]
  shell-audit           [skill   | capability | output  | none]   # /mnt/skills/user/, exists
  microkernel-audit     [skill   | capability | output  | none]   # /mnt/skills/user/, exists
```

The four-attribute tag is the spine. Once every task has it, Phase 4 (automation classification) collapses into "filter by `[foundation | …]` and wire those" — the work is already done in Phase 1. Phase 5 (dashboard) gets free affordances per execution-type.

### Phase 2 — Vault scaffolding (30 min)

```
~/vaults/agentic-os/
├── CLAUDE.md                    # OS-level system prompt + memory map
├── architecture.md              # Phase 1 output — never deleted, only edited
├── architecture.excalidraw      # visual mirror of architecture.md
├── raw/                         # scratch, dumping ground, mic transcripts
├── wiki/                        # synthesized articles, one .md per concept
├── output/                      # final deliverables (PPTX, HTML, .docx)
├── projects/                    # active project memory
│   ├── forge/
│   │   ├── decisions.md         # ADR-style append-only
│   │   ├── state.md             # current state, in-flight work
│   │   └── learnings.md         # what worked / didn't
│   ├── nexus-forge/
│   └── llm-wiki-kit/
├── ops/                         # SOPs, runbooks, troubleshooting
├── personal/                    # non-work
├── archive/                     # frozen past projects
└── skills/                      # vault-local skill defs
                                  # (symlink to ~/.claude/skills/ as taste dictates)
```

**Vault-root `CLAUDE.md` template** — written so CC navigates without burning tokens:

```markdown
# Agentic OS — Claude Code System Prompt

## Identity
You are operating inside Nano Bailey's Claude Code Agentic OS, rooted at
~/vaults/agentic-os/. Every CC session in this directory is part of this OS.
Read this file in full at session start.

## Operating principles
[paste design-principles block from existing CLAUDE.md: composition over
inheritance, explicit over implicit, type hints always, SOLID/DRY/KISS,
async-first, etc.]

## Memory map
- architecture.md         — canonical domain/task/skill hierarchy
- architecture.excalidraw — visual mirror; keep in sync with architecture.md
- raw/                    — append-only dumping ground; never edit, only add
- wiki/                   — synthesized articles, one concept per file
- output/                 — final deliverables; treat as read-only after publish
- projects/<project>/     — per-project memory
  - decisions.md          — ADR-style append-only log
  - state.md              — current state, in-flight work
  - learnings.md          — what worked / didn't
- ops/                    — runbooks, SOPs
- archive/                — frozen; do not modify
- skills/                 — skill definitions (mirror of ~/.claude/skills/)

## Memory write rules
- Research output → raw/<YYYY-MM-DD>-<slug>.md
- After 3+ raw entries on a topic, synthesize into wiki/<slug>.md
- Final deliverables → output/<type>/<YYYY-MM-DD>-<slug>.{pptx,html,docx,md}
- Per-project decisions → projects/<project>/decisions.md (append-only)

## Skill invocation
Skills live in skills/. Each has SKILL.md per Anthropic's contract. Invoke
explicitly when the request matches the skill description; do not improvise
when a skill exists.

## Execution taxonomy
Tasks are classified as one of:
- **skill** — most things; lives in skills/<name>/SKILL.md
- **command** — slash-command shortcuts; high-frequency, low-ambiguity
- **agent** — sub-agent invocations; parallel or isolated-context work
- **manual** — too judgment-heavy to codify; tracked but not invoked

Refer to architecture.md for the canonical task → execution mapping.

## What this OS is NOT
- Not a primary vector store, but vector-augmented for specific retrieval
  tasks (LightRAG over the wiki for deep research; Qdrant for FORGE-scale
  ingest). Embeddings are tools, not architecture.
- For multi-source RAG-scale corpora, defer to FORGE / local-ragstack.
- Not a substitute for ~/.config/nano/CLAUDE.md design principles —
  those still apply.
```

### Phase 3 — Skill packaging (1–2 hours per domain, parallelizable)

For each task in `architecture.md` flagged as a skill:

```
skills/<domain>-<task>/
├── SKILL.md
└── references/
    └── <whatever-context-this-skill-needs>.md
```

**SKILL.md skeleton** (Anthropic contract + Chase's reference-folder pattern):

```markdown
---
name: <domain>-<task>
description: One-sentence trigger. Used for skill matching.
allowed-tools: Bash, Read, Edit, Glob, Grep
---

# <Skill name>

## When to use
- [explicit trigger conditions]

## Inputs
- [what the user provides]

## Process
1. [step]
2. [step]
3. Write output to output/<type>/<YYYY-MM-DD>-<slug>.md
4. If findings worth retaining, append to relevant wiki/ file or create new.

## References
- references/<file>.md — [what's in it]
```

**For commands** (slash-commands): write a `.md` file under `~/.claude/commands/<name>.md` with the prompt template; no folder needed.

**For agents** (sub-agents): write a `.md` file under `~/.claude/agents/<name>.md` with the YAML frontmatter (tools, model, isolation, etc.) and the system prompt body.

**Priority order** for first-wave packaging (highest leverage given existing assets):

1. `research-deep-research` — multi-source synthesis to `wiki/`; sub-agent candidate (parallel YouTube/Twitter/GitHub/web)
2. `research-daily-trend-scan` — automation candidate
3. `ops-dev-cleanup` — PowerShell script exists, wrap as **slash-command**
4. `forge-loe-agent-pipeline` — your existing pilot, formalized as **sub-agent**
5. `nexus-forge-shell-audit` — already a `/mnt/skills/user/` entry, mirror into vault

### Phase 4 — Automation classification (30 min)

Most of this work is already done if Phase 1 emitted four-attribute tags. The filter is mechanical:

```bash
# Show every task that needs an automation wired
grep '| foundation |' architecture.md | grep -v '| none\]'
```

Default heuristics for any leftover ambiguity:

| Pattern | Choice |
|---|---|
| Schedule + needs local fs + no external trigger | **Local** — systemd timer / cron on WSL2 |
| Schedule + no fs deps + can run anywhere | **Remote** — GitHub Action, Anthropic schedule |
| External event trigger (email, webhook, file drop) | **Remote** if cloud-side, **local** if local-side |
| On-demand only | **None** — invoke from terminal or dashboard |

**NIPR/SIPR rule:** anything touching DTO data stays local. No exceptions. Remote automations are personal-side only.

### Phase 5 — Observability dashboard (1–2 days)

Build on Datastar Pro + FastAPI. Reuse the FastAPI fullstack pattern from your five-layer template progression as the starting scaffold.

**Architecture:**

```
dashboard (FastAPI + Datastar Pro SSE)
  ├── /skills        — grid of skill buttons → headless CC -p
  ├── /commands      — slash-command palette → terminal copy-to-clipboard
  ├── /agents        — sub-agent launcher → headless CC with --agents flag
  ├── /automations   — list + last-run status + next-run timestamp
  ├── /usage         — Anthropic API usage panels
  ├── /vault         — recent changes feed (file watcher → SSE)
  └── /              — overview tiles
```

The four execution-types from Phase 1 each get their own surface — different affordances for different invocation modes. A skill button streams stdout. A command tile copies the slash-command to clipboard (you still type it in CC). An agent tile spawns a sub-agent invocation and streams its output. This beats Chase's flat button grid — better mapped to how the underlying tasks actually behave.

**Button → headless CC invocation** (the core mechanic Chase describes):

```python
# When a skill button is clicked:
import asyncio
from pathlib import Path
from typing import AsyncIterator

VAULT_ROOT = Path.home() / "vaults" / "agentic-os"

async def run_skill(skill_name: str, user_input: str) -> AsyncIterator[str]:
    """Invoke CC headless and stream stdout back over SSE."""
    prompt = f"Run the {skill_name} skill with input: {user_input}"
    proc = await asyncio.create_subprocess_exec(
        "claude", "-p", prompt,
        cwd=str(VAULT_ROOT),
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.STDOUT,
    )
    assert proc.stdout is not None
    async for line in proc.stdout:
        yield line.decode()
    await proc.wait()
```

Stream stdout over SSE — dashboard shows live output instead of a frozen spinner. Datastar's SSE patterns handle this cleanly.

**Usage panels — three sources:**

1. `~/.claude/` log directory — parse JSONL session transcripts for token counts
2. Anthropic Console API for billing-grade numbers — `GET /v1/organizations/{org}/usage_report`
3. Local SQLite write of every dashboard-triggered run (your own ground truth)

**Vault recent-changes feed:** `watchfiles` + SSE. Emit events on writes under `raw/`, `wiki/`, `output/`.

**Theme:** dark ops-center. Reuse the manpower-dashboard CSS.

---

## What Chase doesn't say but you'll want anyway

Deliberate extensions for your context, not part of the video:

1. **Session-start hook for architecture/skills drift** — at session start, diff `architecture.md` against the actual `skills/` directory and the four-attribute tags. Flag drift. Chase's framework assumes the hierarchy stays in sync; nothing enforces it. A pre-session hook does.

2. **Sub-agents for the heavy domains** — `research-deep-research` should spawn parallel sub-agents for YouTube / Twitter / GitHub / web sources, not run them sequentially. CC's sub-agent pattern fits cleanly. The four-attribute schema already encodes this — `[agent | …]` tasks get sub-agent invocation paths.

3. **Centralize MCP wiring** — your remind-me, TWC, places, watch (when fixed) MCPs belong on the dashboard's tool registry, not buried inside individual skills.

4. **FORGE bridge** — anything in `projects/forge/` that produces structured artifacts (LOE agent outputs, ASoT records) should write to both the vault AND FORGE's SQLModel store. One-way bridge: vault is the human-readable mirror, FORGE is the system of record.

5. **Memory consolidation cron** — Chase doesn't mention it; the broader community pattern does. Nightly skill that scans `raw/` for entries ≥7 days old without a corresponding `wiki/` entry, and either synthesizes or files for review.

6. **Excalidraw ↔ architecture.md sync** — Chase has both implicitly (the diagram and his own task list). You should make the sync explicit: a script that parses `architecture.excalidraw` (Excalidraw stores JSON) and emits/diffs `architecture.md`, or vice versa. Otherwise the two will drift within a month.

---

## Divergence points — where to consciously not follow Chase

| Chase says | You should | Why |
|---|---|---|
| "99.9% of people don't need light rag or vector DB" — *but his own architecture diagram lists `LightRAG Query` as one of his Research tasks* | **Trust his behavior over his words.** Markdown vault for personal OS, **LightRAG or Qdrant for any task whose corpus exceeds what CC's grep/glob can navigate efficiently — including some personal research tasks, definitely FORGE.** | Chase's transcript advice is for users without your scale. His own setup includes a vector layer for research. Your llm-wiki-kit corpus and FORGE multi-source ingest both warrant it; reserve markdown-only for project memory and the wiki/output layers. |
| Voice mic for the architecture interview | Optional | You write fast; typed stream-of-consciousness is fine. Do timeboxes — don't edit while interviewing. |
| Sell this as a packaged product to clients | N/A directly | DoD context. But worth thinking about as a transferable LOE pattern across PM offices — same architecture, different domain inventory. |
| One unified dashboard | **Split: personal at localhost, FORGE separate** | NIPR/SIPR boundaries. Don't mix authority chains in one UI. |
| Karpathy three-folder vault | Adopt + extend | His extensions (`archive/`, `ops/`, `projects/`) align with how you already organize. |
| Treat all tasks as skills | **Use the full execution taxonomy** | His own diagram has a `Manual / Skills / Commands / Agent` row at the top. Some tasks are slash-commands, some sub-agents, some manual. Default to skill but classify explicitly. |

---

## Pilot sequence — one week

| Day | Action | Output |
|---|---|---|
| 1 | Phase 0 + 1 — pick vault, run architecture interview | `architecture.md` with four-attribute tags, `~/vaults/agentic-os/` initialized |
| 2 | Phase 2 — scaffold folders, write `CLAUDE.md`, sketch Excalidraw mirror | Vault navigable + visual planning artifact |
| 3 | Phase 3 — package 3 highest-leverage skills (one of each execution type if possible) | First skill, command, and sub-agent runnable |
| 4 | Phase 4 — wire 1 foundation-tagged skill as a local cron automation, observe 24h | First automation in flight |
| 5 | Phase 5 — minimum dashboard: skill grid + run output streaming | Buttons exist, click → stdout |
| 6 | Add usage panels + vault watch feed + per-execution-type surfaces | Observability online |
| 7 | Audit week 1 — what got used, what didn't, what needs revision | Updated `architecture.md`; prune unused tasks |

Two weeks is more realistic if FORGE work lands in the same window. Week 1 goal is end-to-end function, not coverage.

---

## Open questions

1. **Vault location** — `~/vaults/agentic-os/` confirmed, or somewhere else?
2. **First three tasks to package** — `deep-research` (skill/sub-agent), `daily-trend-scan` (skill+automation), `dev-cleanup` (command) — or different priorities?
3. **Dashboard target** — Datastar Pro fullstack as I'm assuming, or do you want it on Tauri (would slot into Nexus Forge), Textual TUI (terminal-native), or something else?
4. **FORGE bridge** — build now or defer until the personal vault has a real ingest pattern worth bridging from?
5. **Excalidraw maintenance** — manual hand-sync, parse-script, or skip the visual artifact entirely?

---

## Provenance

- **Faithful sections** (Step 1/2/3 prescriptions, divergence comparisons, base architecture pipeline): direct from Chase AI transcript timestamps as cited.
- **Architecture diagram metadata** (FOUNDATIONS/CAPABILITIES split, Manual/Skills/Commands/Agent taxonomy, raw/wiki/project memory routing labels, full nine-domain set, LightRAG observation): extracted from frame analysis of his Excalidraw diagram (frames covering t=0:00–2:30; later visual demos in Memory and Observability chapters were not accessible due to a sandbox mount failure mid-extraction).
- **Stack mapping, phased implementation, "what Chase doesn't say"**: my extensions, calibrated to your existing assets.
- Reconstructed prompts (Phase 1 architecture interview) approximate Chase's described structure — he sells the exact text in his Skool community.

---

## Revision history

- **v1** — initial plan from transcript only
- **v2** — added: domain metadata (foundations/capabilities + memory routing), execution taxonomy (skill/command/agent/manual), reversed LightRAG framing per Chase's actual diagram, updated Phase 1 prompt and example output, added Excalidraw sync as a "what Chase doesn't say" item, updated divergence table
