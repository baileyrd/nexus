# ADR 0011: Adopt the Plugin-First Shell (`shell/`) and Retire the Legacy Shell (`app/` + `crates/nexus-app`)

**Date:** 2026-04-23
**Accepted:** 2026-04-23
**Status:** Accepted

## Context

The repo currently carries two parallel Tauri desktop shells at different
architectural generations:

1. **Legacy shell** — `app/` (React/Vite/Zustand frontend) paired with
   `crates/nexus-app` (Rust Tauri host, ~95 `#[tauri::command]` handlers
   registered in the `generate_handler!` block of `lib.rs`).
   Hardcoded tri-pane layout (ForgeTopBar, WorkspaceView, SplitPane,
   TabStrip, StatusBar). Tight integration with specific UI affordances:
   multi-tab editor, xterm.js terminal, streaming AI/agent panels. Still
   actively maintained through 2026-04-17 (sessions focused on layout
   persistence, multi-tab, Vim mode).

2. **Plugin-first shell** — `shell/` (React/pnpm/Vite frontend) paired with
   `shell/src-tauri` (crate name `nexus-shell`, excluded from the Cargo
   workspace via `exclude = ["shell"]` in the workspace `Cargo.toml`).
   "The shell starts completely empty. There is no sidebar, no title bar,
   no editor, no status bar until plugins load them." Implements the
   contribution/slot/registry model described in `shell/docs/architecture.md`
   and the kernel bridge in `docs/shell-kernel-bridge-plan.md`. Already
   registers 32 `nexus.*` plugins covering editor, canvas, bases, graph,
   outline, backlinks, search, AI, agent, workflow, skills, MCP, terminal,
   processes, command palette, status bar, activity bar, title bar, pane
   mode, launcher, confirm, tags, bookmarks, file properties, all
   properties, outgoing links, git status, sidebar, right panel.

All twelve post-Phase-B planning documents in `docs/` (`leaf-architecture`,
`leaf-migration-plan`, `shell-kernel-bridge-plan`, `editor-transaction-*`,
`canvas-shell-plan`, `bases-shell-plan`, `global-graph-view-plan`,
`notion-block-ux-plan`, `tab-context-menu-plan`, `editor-shell-auditor`,
`FORGE-UI-PLAN`) are written against the new shell's file layout
(`shell/src/plugins/nexus/...`, `shell/src-tauri/src/...`). Canvas, Bases,
and Notion-style block UX all mark Phases 1–6 complete against the new
shell as of 2026-04-22.

Both shells are functional. They are not interoperable. They produce
separate binaries, persist to separate state files, and implement
overlapping IPC adapters over a shared kernel (`nexus-bootstrap`).

## Decision

**Adopt `shell/` + `shell/src-tauri` (`nexus-shell`) as the single Tauri
desktop shell for Nexus going forward. Retire `app/` + `crates/nexus-app`
once feature parity is reached.**

The migration is phased (see `docs/planning/INTEGRATION-REVIEW.md` §5):

1. **Freeze** new capability work on the legacy shell. All new Tauri
   commands land as IPC handlers in the relevant service crate (reachable
   from every frontend), and any UI that needs them is implemented as a
   plugin in `shell/src/plugins/nexus/<feature>`.
2. **Complete the Leaf/ViewRegistry and shell-kernel bridge foundations**
   (`leaf-migration-plan` Phases 0–4, `shell-kernel-bridge-plan` Phase 4
   polish).
3. **Migrate the ~95 legacy Tauri commands' UX** to plugin views in the new
   shell, walking a parity checklist derived from the handlers in
   `crates/nexus-app/src/*.rs`.
4. **Delete `app/`** and the `crates/nexus-app` member from
   `Cargo.toml::workspace.members`. Archive the tree at a tag
   (`v0.1.0-legacy-shell`).

## Alternatives considered

### A. Keep both shells indefinitely
Both shells already exist and work. We could maintain them as parallel
products (e.g., `app/` for power users, `shell/` for new users).

**Rejected** because:
- Every new Tauri command requires two implementations or accepts drift.
- Every frontend plugin loader bug must be fixed twice.
- Security hardening (JS plugin sandbox, CSP, capability prompts) must be
  done twice. The audit findings in `MICROKERNEL-AUDIT.md` and
  `UI-AUDIT.md` apply to both.
- The planned `@nexus/extension-api` TypeScript package cannot target both
  shells cleanly; plugin authors would need two codebases.
- Users pick one and the other atrophies anyway.

### B. Keep `app/`, rewrite its architecture in place
Refactor `app/src/` to adopt a plugin-first substrate without creating a
new directory. Keep the `crates/nexus-app` Tauri host but generalize its
command set.

**Rejected** because:
- The in-place rewrite is nearly as large as the migration and lands in a
  single risky PR rather than phased with continuous working software.
- All planning documents assume new-shell file paths; retrofitting them to
  `app/src/` invalidates the reference implementations (notably the
  Phase-6 canvas and bases work).
- The legacy shell has significant hardcoded product content
  (`ForgeTopBar`, hardcoded Nexus branding, tri-pane assumptions) that
  wouldn't survive a principled refactor anyway — the "rewrite in place"
  quickly becomes "rewrite, preserving only a handful of UX files," which
  is just the new-shell migration with extra steps.

### C. Retire `shell/`, double down on `app/`
Declare the plugin-first experiment complete, fold learnings into `app/`,
and delete `shell/`.

**Rejected** because:
- `app/`'s extensibility ceiling is too low for the knowledge-app category
  Nexus competes in (Obsidian, Logseq, Notion, Reflect all have
  plugin-first shells). Community plugins have nowhere structurally to
  mount except the activity bar.
- The security model in `app/` (direct `invoke()` access from the
  webview, no iframe sandbox, no CSP) blocks any public plugin
  marketplace. The new shell's `ExtensionHost` is the enforcement point
  for capability gates.
- The planning work invested in the new-shell architecture (twelve
  integration docs, six complete phases across canvas/bases/notion
  blocks) would have to be unwound.

### D. Evolve both for one more release, then decide
Defer the call. Ship both in the next release and re-evaluate in 3 months.

**Rejected** because:
- Every week of "both shells active" is a week of growing drift,
  duplicated bugs, and plugin-author confusion.
- The decision cost is not the hard part; the migration work is. Deferring
  the decision doesn't save any work, it just delays the return on the
  work.

## Consequences

### Positive
- **Single target for all integration work.** The twelve planning docs
  have one address. Canvas, Bases, Graph, Notion-block, and
  Editor-transaction work all land in `shell/` directly.
- **Enforceable plugin contract.** `ExtensionHost` mediates all
  contributions. `@nexus/extension-api` can target one shell.
  Community plugins get a stable surface.
- **Security gates achievable.** Iframe sandbox for JS plugins, CSP
  re-enablement, and install-time capability prompts only need one
  implementation.
- **Kernel remains the only backend.** All four frontends (CLI, TUI, MCP
  desktop) keep funnelling through `context.ipc_call(...)`, and the
  consolidation makes the CLI/TUI parity story simpler — no `nexus-app`
  vs. `nexus-shell` drift.
- **Plugin-first architecture scales.** New features (marketplace,
  multi-pane/Leaf, remote forges) are natural extensions rather than
  architectural rewrites.

### Negative
- **Feature-parity migration is the biggest item on the roadmap.** Phase 2
  of the integration plan is 6–10 weeks of walking a ~95-handler parity
  checklist. During that window the team ships less other capability.
- **`app/` users lose the legacy UX briefly.** Until the new shell reaches
  parity, some users may prefer running the legacy binary. Mitigation: tag
  the legacy shell at the freeze point so users can build it if needed,
  and don't delete it until parity is verified.
- **Two directories in-tree during migration.** Some contributor confusion
  ("which shell am I editing?") until `app/` is deleted. Mitigation: add
  a `DEPRECATED` banner to `app/README.md` and `crates/nexus-app/src/lib.rs`
  on day one of the freeze.
- **Some `nexus-app`-specific UX paths lose history.** The legacy shell's
  streaming agent panel, multi-tab persistence, and vim-mode toggle are
  individual feature investments. They need re-implementation in the new
  shell, not copy-paste. Mitigation: the parity checklist captures the
  functional contract; re-implementation is a normal feature-work unit.

## Rollback

If Phase 2 reveals that the new-shell architecture cannot reach feature
parity within a reasonable timeline, tag the last legacy-shell commit,
revert this ADR to `Rejected`, and reopen alternative B or C. Bridge
work in the kernel (IPC handlers added during migration) remains
valuable regardless.

## References

- `docs/planning/INTEGRATION-REVIEW.md` — full audit informing this decision
- `docs/shell-kernel-bridge-plan.md` — bridge contract (Phases 0–3
  delivered)
- `docs/leaf-architecture.md` / `docs/leaf-migration-plan.md` — Leaf
  foundation
- `docs/planning/MICROKERNEL-AUDIT.md` — security findings that apply across both
  shells
- `docs/planning/UI-AUDIT.md` — UI-specific security findings
- `shell/README.md` — plugin-first shell philosophy
- `DEPRECATED.md` — existing deprecation policy; extended here to cover
  the legacy shell
