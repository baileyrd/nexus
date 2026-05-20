# Plugin Assessment — Nexus 0.1.2

This directory contains a per-plugin assessment for every plugin in the Nexus tree. Each plugin is evaluated on two axes:

1. **Architecture** — what the plugin actually is: entry point, IPC surface (or UI contributions), persistence, settings, external dependencies.
2. **Necessity for basic capabilities** — a verdict on whether the plugin is load-bearing for a minimum-viable Nexus.

## What "basic capabilities" means here

A minimum-viable Nexus desktop install lets a single user:

- open a forge directory,
- browse the markdown tree in the desktop shell,
- edit and save markdown files,
- run global text/symbol search across the forge,
- commit and push via git.

Anything required to deliver that workflow is rated **Essential**. Anything that adds material value on top but is not required is **Useful**. Anything that is a self-contained feature most users would not miss is **Optional**. Anything that could be removed today without anyone noticing (dead, niche, or duplicated by another plugin) is **Removable**.

> A plugin being "Optional" or "Removable" is not a recommendation to delete it — it is an answer to the question "do we need this for basic capabilities?" The plugin may still be central to the product vision.

## Verdict scale

| Verdict     | Meaning                                                                                              |
|-------------|------------------------------------------------------------------------------------------------------|
| Essential   | Removing this breaks the basic workflow above, or several Essential plugins depend on it transitively. |
| Useful      | Not required for the basic workflow, but a typical user would notice quickly if it were gone.        |
| Optional    | A discrete feature that users opt into; absence is fine for the basic workflow.                      |
| Removable   | Dead, stub, niche, or duplicated; can be cut today without functional loss.                          |

## Layout

| Folder            | Coverage                                                          | Granularity              |
|-------------------|-------------------------------------------------------------------|--------------------------|
| `core-rust/`      | The 23 native Rust core plugins listed in `docs/0.1.2/plugins/core.md`. | One file per plugin. |
| `shell-core/`     | The 17 infrastructure plugins under `shell/src/plugins/core/`.   | One file per plugin.     |
| `shell-nexus/`    | The ~65 feature plugins under `shell/src/plugins/nexus/`.        | Grouped MDs by category. |

Community example plugins (`hello-world`, `mermaid`) are not assessed here — they exist as authoring references, not product features.

## File template (core-rust and shell-core)

```markdown
# <plugin-id>

- **Path:** `<crate or directory>`
- **Tier:** Core Rust | Shell Core
- **Bootstrap order:** N (Rust core only)

## Architecture
- Entry point and key modules
- IPC handlers (Rust) or UI contributions (shell)
- Persistence: DB tables, files in `.forge/`, in-memory only
- Settings owned (cite `docs/0.1.2/settings/`)
- External dependencies of note (native libs, syscalls, network)

## Surface
- Concrete list of handlers / commands / views / status items

## Necessity
- **Verdict:** Essential | Useful | Optional | Removable
- **Required for basic capabilities?** Yes / No — rationale
- **Depended on by:** plugins that need this
- **Depends on:** plugins this needs
- **What breaks if removed:** concrete consequences

## Notes
- Tech debt, recent activity, risks
```

## Category template (shell-nexus)

Each shell-nexus MD covers a related cluster of feature plugins with a brief shared intro, then one short subsection per plugin using a compact form of the same template.

## Index

### Rust core plugins (`core-rust/`)

In bootstrap registration order:

1. [security](core-rust/security.md)
2. [storage](core-rust/storage.md)
3. [formats](core-rust/formats.md)
4. [database](core-rust/database.md)
5. [editor](core-rust/editor.md)
6. [terminal](core-rust/terminal.md)
7. [git](core-rust/git.md)
8. [ai](core-rust/ai.md)
9. [ai-runtime](core-rust/ai-runtime.md)
10. [agent](core-rust/agent.md)
11. [skills](core-rust/skills.md)
12. [templates](core-rust/templates.md)
13. [workflow](core-rust/workflow.md)
14. [comments](core-rust/comments.md)
15. [linkpreview](core-rust/linkpreview.md)
16. [notifications](core-rust/notifications.md)
17. [theme](core-rust/theme.md)
18. [mcp](core-rust/mcp.md)
19. [lsp](core-rust/lsp.md)
20. [dap](core-rust/dap.md)
21. [acp](core-rust/acp.md)
22. [audio](core-rust/audio.md)
23. [collab](core-rust/collab.md)

### Shell core plugins (`shell-core/`)

- [activityBar](shell-core/activityBar.md)
- [capabilityPrompt](shell-core/capabilityPrompt.md)
- [commandPalette](shell-core/commandPalette.md)
- [configurationService](shell-core/configurationService.md)
- [editorArea](shell-core/editorArea.md)
- [fileExplorer](shell-core/fileExplorer.md)
- [fileSystemService](shell-core/fileSystemService.md)
- [notificationService](shell-core/notificationService.md)
- [panelArea](shell-core/panelArea.md)
- [rightPanel](shell-core/rightPanel.md)
- [settings](shell-core/settings.md)
- [sidebar](shell-core/sidebar.md)
- [statusBar](shell-core/statusBar.md)
- [terminal](shell-core/terminal.md)
- [themeService](shell-core/themeService.md)
- [titleBar](shell-core/titleBar.md)
- [zoom](shell-core/zoom.md)

### Shell-nexus feature categories (`shell-nexus/`)

- [search-and-navigation](shell-nexus/search-and-navigation.md) — search, searchPanel, semanticSearch, launcher, commandPalette, pick, recall, outline, graph, backlinks, outgoingLinks, tags
- [ai-and-knowledge](shell-nexus/ai-and-knowledge.md) — ai, agent, memory, prompt, enrich, dreamCycle, skills, linkSuggest
- [files-and-properties](shell-nexus/files-and-properties.md) — files, fileProperties, allProperties, bookmarks
- [editor-views](shell-nexus/editor-views.md) — editor, multibufferSync, paneMode, viewBuilder, canvas, notion
- [collaboration](shell-nexus/collaboration.md) — collab, comments, crdtConflict
- [git](shell-nexus/git.md) — gitPanel, gitStatus
- [diagnostics-and-observability](shell-nexus/diagnostics-and-observability.md) — debugger, diagnostics, healthPanel, observability, processes, status, osArchitecture
- [notifications](shell-nexus/notifications.md) — notifications, notificationsInbox, notificationsSettings
- [workspace-chrome](shell-nexus/workspace-chrome.md) — workspace, sidebar, rightPanel, statusBar, activityTimeline, themePicker
- [extension-system](shell-nexus/extension-system.md) — pluginsMgmt, workflow, templates, bases
- [interaction-and-io](shell-nexus/interaction-and-io.md) — confirm, mcp, audio, terminal

## Top-line summary

See [SUMMARY.md](SUMMARY.md) for the per-plugin verdict roll-up and headline findings.

## Dependency graph

See [DEPENDENCIES.md](DEPENDENCIES.md) for a ground-truth dependency analysis: per-plugin declared/runtime/event/compile-time deps, the inverse "who depends on whom" index, transitive closure for the basic-capability scope, cycles and reverse couplings, and hidden coupling list. Raw extraction data is in `_extract-rust-deps.md` and `_extract-shell-deps-A.md` / `_extract-shell-deps-B.md`.

## Implementation plan

See [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md) for a sequenced six-phase plan to address every finding: doc fixes → declare hidden couplings → dead-code deletion → manifest-schema extensions → behavior-preserving refactors → strategic decisions. Each task lists effort, risk, prerequisites, and acceptance criteria.

## Phase 5 decisions

See [PHASE5_DECISIONS.md](PHASE5_DECISIONS.md) for the resolution of each strategic-decision item in Phase 5 (ship/cut `com.nexus.acp`, `audio` defaults, DAP/LSP status, cycle intent). 5 of 6 items are resolved; only the audio default backend remains open and is documented with options + recommendation.
