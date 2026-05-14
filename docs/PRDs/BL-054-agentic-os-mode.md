# BL-054 — Nexus OS Mode

**Source**: AI Integration Assessment (2026-05-06) + analysis of Chase AI's "Agentic OS" framework  
**Reference**: `docs/audits/AI-INTEGRATION-ASSESSMENT-2026-05-06.md`, `docs/research/agentic-os-implementation-plan.md`  
**Status**: **All five phases shipped.** Verified 2026-05-14.
**Crates**: `nexus-skills` (`invoke` handler id 8), `shell/src/plugins/nexus/{workspace,launcher,osArchitecture,observability,skills}/`, `nexus-bootstrap` (`forge_template` module + templates).
**Related**: BL-037 (activity timeline), BL-052 (universal activity timeline), PRD-13 (skills), PRD-15 (agent), PRD-16 (workflow)

## Shipped status (2026-05-14)

| Phase | Status | Evidence |
|---|---|---|
| 1. Forge OS template | ✅ Shipped | `crates/nexus-bootstrap/src/forge_template.rs` ships the `ForgeTemplate::Os` scaffolder + 5 unit tests. `crates/nexus-cli/src/commands/forge.rs::init` accepts `--template os`. `shell/src-tauri/src/bridge.rs::init_forge` accepts `template: Option<String>`. `nexus.workspace.openWithTemplate` command + the launcher's "Create OS workspace" action at `LauncherView.tsx:452` cover the DoD's "shell new-forge flow offers OS layout as an option alongside blank" bullet. Templates live at `crates/nexus-bootstrap/templates/os/{CLAUDE.md,architecture.md}`. |
| 2. Architecture panel | ✅ Shipped | `shell/src/plugins/nexus/osArchitecture/` with `architectureParser.ts`, `driftDetect.ts`, `osArchitectureStore.ts`, `OsArchitectureView.tsx`, `OsArchitecturePaneView.tsx`. Registered in `shell/src/plugins/catalog.ts:477`. |
| 3. Skills invocation | ✅ Shipped | `crates/nexus-skills/src/core_plugin.rs::HANDLER_INVOKE = 8` + `dispatch_async` arm; 7 unit tests in `core_plugin.rs::tests` cover the path. SkillsPanel surfaces the Run affordance per `BL-067-068-builders.md`. |
| 4. Observability panels | ✅ Shipped | `shell/src/plugins/nexus/observability/` with usage aggregation (`usageAggregate.ts`), three-tab pane view, and store. Registered in `shell/src/plugins/catalog.ts:487`. |
| 5. OS Setup skill | ✅ Shipped | `crates/nexus-skills/builtins/os-setup.skill.md` seeded; runs the architecture elicitation interview and writes `architecture.md`. |

Each phase keeps its DoD bullets in §"Phased implementation" below — preserved for traceability.

---

## What this is

The "Agentic OS" pattern — popularized by Chase AI — is a methodology for organizing recurring work into a
**Domains → Tasks → Skills → Automations** hierarchy, with a memory layer (markdown vault) and an
observability layer (dashboard + usage panels). The framework is typically built *on top of* Claude Code
as an external tool.

Nexus is already 85% of the substrate for this pattern. The forge is the vault. `com.nexus.skills` is
the skill system. `com.nexus.agent` is the sub-agent execution layer. `com.nexus.workflow` is the
automation layer with cron/file/webhook triggers. RAG with citations handles the vector retrieval
tasks. MCP is bidirectional with auto-discovery at plan time.

What's missing is not infrastructure — it's the *methodological layer*: the conventions, scaffolding,
and UI affordances that let the forge act as a coherent operating system rather than a collection of
capable-but-disconnected features.

**The goal of this BL is to close that gap.**

---

## What the framework prescribes (and how Nexus maps)

| Framework concept | Nexus equivalent | Gap |
|---|---|---|
| Markdown vault | The forge — `File-as-truth` is invariant #1 | None |
| `raw/wiki/output/projects` folder layout | No prescribed structure today | Forge scaffolding template needed |
| Vault-root `CLAUDE.md` | Per-forge `CLAUDE.md` already loaded | None (convention, not code) |
| Skill definitions | `com.nexus.skills` — `.skill.md`, registry, composition | None on backend; Run affordance missing in UI |
| Skill execution from UI | Read-only skills panel today | `com.nexus.skills::invoke` handler + UI button |
| Sub-agents | `com.nexus.agent` with archetypes + step approval | None |
| Automations (cron/file/webhook) | `com.nexus.workflow` — full trigger set shipped | None on backend; no Foundations/Capabilities UX |
| Vector retrieval for research | RAG in `com.nexus.ai` with semantic search | None |
| MCP tool registry | Bidirectional MCP host + server | None |
| Activity audit | `AI_ACTIVITY_LOG` + timeline panel | Usage/automation status panels missing |
| `architecture.md` canonical registry | No equivalent today | New concept: domain/task/skill hierarchy file |
| Observability dashboard | The shell IS the dashboard architecturally | Usage panels + automation status panels needed |
| FOUNDATIONS vs CAPABILITIES | Workflows handle scheduling; no UX distinction | Visual distinction + run-status per foundation |
| Architecture elicitation workflow | No equivalent | "OS Setup" skill that produces `architecture.md` |

### Why the observability dashboard is not a separate app

Chase's framework builds a separate FastAPI + Datastar web app that invokes `claude -p` headless for
each skill button. In Nexus this is unnecessary — the shell plugin system IS that dashboard, the IPC
boundary IS the invocation path, and SSE streaming from the kernel bus IS the live output stream.
Building a second app would replicate the host. The right move is new shell plugins against existing
infrastructure.

---

## Phased implementation

### Phase 1 — Forge OS template (0.5 days)

Add a `--template os` option to forge initialization that scaffolds the recommended layout:

```
<forge>/
├── CLAUDE.md                  # OS-level system prompt + memory map (template provided)
├── architecture.md            # placeholder — filled in by Phase 5 OS Setup skill
├── raw/                       # append-only dumping ground; research, transcripts, scratch
├── wiki/                      # synthesized articles, one concept per file
├── output/                    # final deliverables; read-only after publish
├── projects/                  # active project memory
│   └── <project>/
│       ├── decisions.md       # ADR-style append-only log
│       ├── state.md           # current state, in-flight work
│       └── learnings.md       # what worked / didn't
├── ops/                       # SOPs, runbooks, troubleshooting
├── personal/                  # non-work
├── archive/                   # frozen past projects
└── .forge/skills/             # skill definitions (already scanned by com.nexus.skills)
```

Template `CLAUDE.md` documents the memory map so the AI navigates without burning tokens guessing
the layout. Includes memory write rules (research → `raw/`, synthesized → `wiki/`, per-project
decisions → `projects/<name>/decisions.md`).

**No new IPC handlers.** Storage already supports any folder layout; this is a `nexus forge init`
CLI flag + a file template, not a structural change.

**Definition of done:**
- `nexus forge init --template os <path>` creates the layout above with the template `CLAUDE.md`
- Shell new-forge flow offers "OS layout" as an option alongside blank
- Template `CLAUDE.md` passes `scripts/check_ipc_drift.sh` (no IPC changes)

---

### Phase 2 — Architecture panel (1.5 days)

A new shell plugin `nexus.osArchitecture` renders `architecture.md` (if present at the forge root)
as a structured domain/task hierarchy and cross-references it against actual `.skill.md` files and
`.workflow.toml` files to surface drift.

**Four-attribute tag format** (from the framework):

```
daily-trend-scan  [skill | foundation | raw | local cron 0700]
deep-research     [skill | capability | raw | none]
inbox-triage      [skill | foundation | wiki | local cron 0530]
```

Fields: `execution-type` (skill/agent/command/manual) · `class` (foundation/capability) ·
`memory-dest` (raw/wiki/project/output/none) · `automation` (local cron schedule / webhook / none)

**Drift detection:**
- Task tagged `[skill | …]` with no matching `.skill.md` by id → flagged "skill missing"
- Task tagged `[… | foundation | … | local cron …]` with no matching `.workflow.toml` trigger → flagged "automation missing"
- `.skill.md` file present with no entry in `architecture.md` → flagged "undocumented skill"

The panel renders the hierarchy as a collapsible tree with per-task badges. Drift items surface as
inline warnings with "Create skill" / "Create workflow" action buttons.

**Definition of done:**
- `nexus.osArchitecture` plugin panel visible in palette and sidebar
- Renders domain/task hierarchy from `architecture.md` parse (tolerant of missing file — shows empty state)
- Drift detection against `.forge/skills/` and `.forge/workflows/` directories
- Per-task badges: execution-type chip, class chip (FOUNDATION / CAPABILITY), memory-dest label, automation status (scheduled / manual / missing)

---

### Phase 3 — Skills invocation (1 day)

Skills are read-only today. Running a skill requires invoking the agent with the skill body as the
system prompt and user-supplied input. This needs:

**New IPC handler:** `com.nexus.skills::invoke` (handler id 8)

```
Args: { skill_id: String, input: String, archetype?: String }
Effect: calls com.nexus.agent::run with:
  - goal = input
  - system_prompt_extra = composed skill body (via existing compose path)
  - archetype = archetype ?? skill.archetype ?? "general"
Returns: AgentObservation (same as agent::run)
```

**UI changes in `SkillsPanel`:**
- Each skill card gains a "Run" button that opens a modal input prompt
- Confirmation shows the skill's name + description + allowed-tools before dispatching
- Output streams into the Chat panel (or a dedicated output pane — open question for UX)
- Skills tagged `[foundation | …]` get a "Schedule" button that creates a pre-filled `.workflow.toml`

**Definition of done:**
- `com.nexus.skills::invoke` handler registered and tested
- SkillsPanel "Run" button dispatches through the handler and streams output
- `scripts/check_ipc_drift.sh` passes (new IPC type exported)

---

### Phase 4 — Observability panels (2 days)

Three new panels as separate shell plugins (or as tabs within a single `nexus.observability` plugin):

**Usage panel** — token consumption and API cost visibility:
- Parse `~/.claude/` session JSONL for token counts (offline, no API call)
- Optional Anthropic Console API integration for billing-grade numbers (`GET /v1/organizations/{org}/usage_report`)
- Per-session breakdown, daily/weekly rollup, per-surface breakdown (chat / agent / completion / enrichment)
- Powered by `com.nexus.ai::activity_list` which already records surface + token metadata

**Automation status panel** — foundation workflow health:
- Lists all `.workflow.toml` files where trigger is cron or file_event (i.e., foundation class)
- Shows last-run timestamp, last-run outcome (success / failed / skipped), next scheduled fire
- "Run now" button for manual trigger
- Requires workflow executor to persist run history — new `com.nexus.workflow::run_history` handler (small addition)

**Vault feed panel** — recent forge activity:
- File watcher already runs via `nexus-storage`; this panel subscribes to file-change events on the kernel bus
- Filters to `raw/`, `wiki/`, `output/` paths
- Renders as a timestamped feed: file path, change type, size delta
- Search/filter by directory or date range

**Definition of done:**
- All three panels registered as shell plugins and accessible from palette
- Usage panel renders without Anthropic Console API (local log parsing sufficient)
- Automation panel shows last-run for at least one triggered workflow
- Vault feed subscribes to the existing file-change bus topic

---

### Phase 5 — OS Setup skill (0.5 days)

A built-in `.skill.md` seeded into new OS-template forges that runs the architecture elicitation
interview and produces `architecture.md`.

The skill body walks the user through the Chase AI methodology:
1. Stream-of-consciousness brain-dump of recurring work (timed, unedited)
2. Cluster into domains with the user
3. For each domain, enumerate discrete tasks
4. For each task, assign four-attribute tag (execution-type / class / memory-dest / automation)
5. Write `architecture.md` to the forge root

The skill is flagged `capability` class (run once or when the architecture needs revision, not daily).

**Definition of done:**
- `os-setup.skill.md` present in the OS template scaffold
- Skill produces a valid `architecture.md` that the Phase 2 panel can parse
- Seeded into `.forge/skills/builtins/` alongside the existing `code-reviewer`, `daily-journal`, etc.

---

## What this is NOT

- Not a separate application. Everything runs inside the Nexus shell and forge.
- Not a replacement for the user doing the architecture work. The setup skill lowers friction; it cannot inventory a person's life for them.
- Not a new backend. Every phase is UI or thin IPC additions over existing services.
- Not required to be delivered as a unit. Each phase is independently valuable and shippable.

---

## Open questions

1. **Skill output routing** — does "Run" stream into the Chat panel (reuses existing surface) or into a
   new per-skill output pane? Chat panel is simpler and available now; dedicated pane would eventually
   be cleaner for the OS dashboard feel.

2. **`architecture.md` parse strictness** — the panel can be tolerant (best-effort parse, ignore
   lines it doesn't understand) or strict (validation errors surfaced). Tolerant is better for
   adoption; strict is better for drift detection accuracy. Start tolerant.

3. **Automation status persistence** — the workflow executor currently does not write run history.
   Adding `com.nexus.workflow::run_history` is small but requires a new storage file
   (`.forge/.workflows/run_history.json` or similar). Confirm schema before implementing.

4. **Usage panel Anthropic Console API** — requires an API key scoped to the organization. Should
   this be in forge config or shell settings? Shell settings is cleaner (one key across forges).

5. **`architecture.md` as a special file vs. convention** — the panel can hard-code the filename or
   support any file named via forge config (`os.architecture_file = "architecture.md"`). Convention
   first, config later.
