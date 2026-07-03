# TODOs, Stubs & Coming-Soon

> **As of:** 2026-05-22. Every TODO/FIXME/XXX/HACK marker plus every "coming soon" / "not yet implemented" / stub indicator the code surfaces. Scope: `crates/`, `shell/`, `packages/`. Excludes test sentinel assertions (`panic!("expected X, got Y")` style) which are intentional test scaffolding, not unfinished work.

## TL;DR

| Category | Count | Notes |
|----------|------:|-------|
| Rust `todo!()` / `unimplemented!()` ship-blockers | **0** | None in production paths |
| Rust production TODO/FIXME/HACK comments | **3** | All in `nexus-audio`; all cross-linked to BL tickets |
| Rust user-facing "coming soon" stubs | **2 subcommands** | `nexus sync`, `nexus run` — centralized in `stubs.rs` |
| Shell user-facing "coming soon" (settings panel) | **~57** | Entire Obsidian-parity settings tabs ship as placeholders |
| Shell user-facing "coming soon" (other plugins) | **~15** | Canvas drag sources, editor tab-actions, plugin inspectors |
| Shell stub plugins (whole plugin = "Not yet implemented" placeholder) | **5** | `allProperties`, `bookmarks`, `fileProperties`, `outgoingLinks`, `tags` |
| Shell host TODOs | **1** | `defineSlot` API surface not yet implemented |
| Phase-named follow-ups (audit §E) | **7** | Deferred work with named phases but no inline `TODO` marker — see §9 |
| Test-code panic sentinels | **380+** | Intentional `assert!`-style scaffolding; not unfinished work |

The codebase is honest about its stubs — every "coming soon" surfaces a toast or a placeholder pane rather than silently doing nothing. **There are zero hidden landmines.** The volume is concentrated in Obsidian-parity settings UI that ships scaffolded but unwired.

---

## 1. Ship-blockers (Rust `todo!()` / `unimplemented!()` / panic-sentinel)

**None in production code.** Every `panic!()` found in `crates/*/src/` is either:
- An expected-value assertion inside a test (`panic!("expected Custom, got {other:?}")`), or
- A documented sentinel for a path that's compile-time-guarded behind `#[cfg(...)]` and won't ship in release builds.

No production `todo!()` or `unimplemented!()` macros.

---

## 2. Production Rust TODOs

| File | Line | Marker | What |
|------|------|--------|------|
| `crates/nexus-audio/src/provider_backend.rs` | 316 | `TODO(BL-102 follow-up)` | Route audio's reqwest client through the same TLS-pinning gate `nexus-ai::http_client::build_client` uses, so audio + chat share one pin policy |
| `crates/nexus-audio/src/local_backend.rs` | 29 | `TODO` | macOS `say -o` outputs AIFF wrapped in WAV via hound; bare AIFF passthrough would be cleaner |
| `crates/nexus-audio/src/local_backend.rs` | 86 | `TODO` | Thread `forge_root` cleanly through `AudioConfig::local_model_dir`; currently local-audio runs require the kernel to chdir |

### False positives (excluded from the count above)

| File | Line | Why |
|------|------|-----|
| `crates/nexus-storage/src/code_index.rs` | 335 | `// TODO` appears inside a doc comment that's *explaining a parser edge case* ("treat a stray `// TODO` comment as separator"). Not a TODO marker. |
| `crates/nexus-editor/src/markdown/id.rs` | 152 | `assert!(parse_stable_id_marker("<!-- TODO -->").is_none())` — test assertion that the parser doesn't accept random HTML comments. Not a TODO. |

### TypeScript SDK TODOs

| File | Line | Marker | What |
|------|------|--------|------|
| `packages/nexus-extension-api/src/sandbox/context.ts` | 41 | `TODO` | "configuration — TODO for a later iteration. Sandboxed plugins" — settings hook not surfaced yet to sandboxed plugins |

### Shell CSS TODOs

| File | Line | Marker | What |
|------|------|--------|------|
| `shell/src/shell/shell.css` | 39 | `TODO` | A CSS variable isn't routed through the theme resolver yet ("future BL") |

---

## 3. CLI "coming soon" stubs

Centralized through `crates/nexus-cli/src/stubs.rs::not_implemented(command_name)`. Stub subcommands print an error to stderr and exit `1`.

| File | Line | What |
|------|------|------|
| `crates/nexus-cli/src/main.rs` | 135 | `Sync` subcommand — `/// Sync operations (coming soon)` |
| `crates/nexus-cli/src/main.rs` | 139 | `Run` subcommand — `/// Run a script or task (coming soon)` |
| `crates/nexus-cli/src/main.rs` | 1700 | `StubArgs` struct — `/// Subcommand and arguments (not yet implemented)` |
| `crates/nexus-cli/src/stubs.rs` | 1-4 | `pub fn not_implemented(command_name) → "Error: '{command_name}' is not yet implemented."` |

Status: these are intentional Phase 5 placeholders. Users hitting `nexus sync` or `nexus run` get a clear stderr message + exit 1, not a crash.

---

## 4. Settings panel — Obsidian-parity placeholders

`shell/src/plugins/core/settings/SettingsPanelView.tsx` ships **57 "Coming soon" controls** across 7 placeholder tabs. The intent is documented in-source (line 667): *"Shared pieces for the 'Coming soon' pages (General, Editor, Files and links, Keychain, Community plugins). Each control fires an info toast"*.

These render visually (so users see the planned shape) but every interaction surfaces a `${label} — coming soon.` toast.

### Per-tab breakdown

| Tab | Stub controls | Notable items |
|-----|--------------:|---------------|
| **General** | 5 | Automatic updates, Language, Help, Startup-time notification, Command line interface |
| **Editor** | 21 | Default view for new tabs, Default editing mode, Show editing mode in status bar, Readable line length, Strict line breaks, Properties in document, Fold heading/indent, Line numbers, Indentation guides, RTL, Spellcheck (+languages), Auto-pair brackets/Markdown, Smart lists, Indent tabs + visual width, Convert pasted HTML, Vim key bindings |
| **Files and links** | 14 | Default file to open, Default location for new notes/attachments, New link format, Auto-update internal links, Use Wikilinks, Show all file types, Confirm before deleting, Delete attachments when deleting files, Deleted files target, Excluded files, Override config folder, Allow URI callbacks, Rebuild forge cache |
| **Keychain** | 1 | Add secret (button) — wiring to the credential vault deferred |
| **Canvas** | 8 | Default canvas location, Mouse wheel behavior, Ctrl+Drag behavior, Show card names, Snap to grid, Snap to objects, Zoom threshold for hiding card content, etc. |
| **Backlinks** | 1 | Show backlinks at the bottom of notes |
| **Daily notes** | 3 | Date format, Daily note location, Daily note template |
| **File recovery** | 4 | Snapshot interval, History length, View snapshots, Clear file recovery history |
| **Note composer** | 4 | Text after extraction, Template, Confirm file merge, etc. |

**Verdict:** intentional — the comment block at line 874-876 says: *"none of these toggles are wired to real preferences yet; they render in their Obsidian default state and surface a 'Coming soon' toast on interaction. Real per-plugin editor settings already live under their respective plugins."*

Real settings live in the plugins themselves (see [`settings/plugin-manifest-defaults.md`](../settings/plugin-manifest-defaults.md) §"Schema defaults"). The settings panel itself is the unfinished surface.

---

## 5. Plugin-internal "coming soon" stubs (non-settings)

### Tab context menu — `shell/src/plugins/nexus/editor/`

12 tab-action stubs surface a `${stub.label} — coming soon` toast. Centralized through `TabContextMenu.tsx::COMING_SOON_TOOLTIP`. Per `editor/index.ts:107-142`:

| Stub command id | Label |
|-----------------|-------|
| `nexus.editor.stub.splitRight` | Split right |
| `nexus.editor.stub.splitDown` | Split down |
| `nexus.editor.stub.openInNewWindow` | Open in new window |
| `nexus.editor.stub.openLinkedView` | Open linked view |
| `nexus.editor.stub.rename` | Rename file |
| `nexus.editor.stub.moveTo` | Move file to… |
| `nexus.editor.stub.bookmark` | Bookmark file |
| `nexus.editor.stub.addProperty` | Add property |
| `nexus.editor.stub.backlinksInDocument` | Backlinks in document |
| `nexus.editor.stub.versionHistory` | Version history |
| `nexus.editor.stub.mergeFile` | Merge file |
| `nexus.editor.stub.exportPdf` | Export to PDF… |

The new `nexus.editor.exportHtml` ("Export as HTML…") command (C66 #419) is fully wired from the start — it renders through `com.nexus.formats::export_html` and saves via the native save dialog, so it was never added to the stub list above.

### Canvas right rail — `shell/src/plugins/nexus/canvas/CanvasRightRail.tsx:8-9`

| Inspector item | Status |
|----------------|--------|
| Snap to objects | stub — Coming soon |
| Read-only | stub — Coming soon |

### Canvas drag rail — `shell/src/plugins/nexus/canvas/CanvasDragRail.tsx:5`

Drag sources fire a "Coming soon" toast on drag-end. Comment: *"Nexus has no vault [sources] yet"*.

### Canvas generic toast — `shell/src/plugins/nexus/canvas/CanvasView.tsx:1130`

`message: ${label} — coming soon.` — generic helper used by canvas toolbar items not yet wired.

### Shell host API — `shell/src/host/PluginAPI.ts:641`

```ts
clientLogger.warn('[PluginAPI] defineSlot is not yet implemented')
```

The `defineSlot` SDK surface is declared but warns rather than implementing dynamic slot definition. Plugins can still contribute to existing slots; what's missing is the ability for a plugin to *define a new slot* for other plugins to fill.

---

## 6. Whole-plugin stubs (placeholder inspectors)

Five plugins in `shell/src/plugins/nexus/` are **fully stubbed** — their entire pane renders "Not yet implemented" with a one-sentence description of the planned feature.

| Plugin id | Folder | What it'll do |
|-----------|--------|---------------|
| `nexus.allProperties` | `allProperties/index.tsx:23` | List every frontmatter property on the active note, including inherited values |
| `nexus.bookmarks` | `bookmarks/index.tsx:19` | List saved bookmarks |
| `nexus.fileProperties` | `fileProperties/index.tsx:23` | Show the active note's file properties |
| `nexus.outgoingLinks` | `outgoingLinks/index.tsx:22` | List outgoing links from the current buffer (tab + command stubbed) |
| `nexus.tags` | `tags/index.tsx:22` | Surface the active note's tags |

These are real plugins — they register with `ExtensionHost`, claim a slot, and ship a placeholder. (**Correction to [`plugin-capabilities.md`](../plugin-capabilities.md):** these 5 are not "component-only folders" — they ARE plugins, just stubs. See the section update below.)

---

## 7. Other minor stubs

| File | Line | What |
|------|------|------|
| `crates/nexus-audio/src/audio/stub_backend.rs` | various | Compile-time stub backend returning `BackendNotEnabled` when audio cargo features not enabled. By design. |
| `crates/nexus-audio/src/config.rs` | 23 | Comment re: "shipped build stubs" for local-whisper cargo feature |
| `crates/nexus-audio/src/lib.rs` | 5-15 | Library docstring describing stub backends |

These are compile-time feature stubs, not runtime placeholders.

---

## 8. Test / bench panic sentinels

380+ `panic!("expected X, got {other:?}")` style assertions across:
- `crates/nexus-kernel/tests/` (context_impl, event_bus)
- `crates/nexus-dap/tests/` (config, pool, transport)
- `crates/nexus-terminal/tests/core_plugin.rs` (90+)
- `crates/nexus-database/tests/views.rs` (15+)
- `crates/nexus-cli/tests/cli-integration.rs`
- many others

All deliberate — test code uses `panic!()` to fail with a descriptive message when an expected enum variant doesn't match. Not unfinished work.

---

## 9. Phase-named follow-ups (deferred work, no inline `TODO` marker)

The 2026-05-21 gaps audit (§E) called out 8 code-level "deferred work" sites worth surfacing here. The sandbox-context configuration row is also in §2; the rest are unique to this section. Each has a named phase in the surrounding comments so it's traceable without becoming a `// TODO`.

| File | Line | Phase / WI | What |
|------|------|------------|------|
| `packages/nexus-extension-api/src/sandbox/context.ts` | 41 | (also in §2) | `configuration` API not surfaced to sandboxed plugins; they persist state via `SandboxedPluginContext.storage` until the bridge lands. |
| `shell/src/host/ExtensionHost.ts` | 124 | BL-XXX Phase 3.2 | `dependsOn` accepts both shell-plugin ids (`core.*` / `nexus.*` / `community.*`) and kernel-plugin ids (`com.nexus.*`); the kernel-tier ids are currently documentation only — the loader doesn't yet distinguish them from shell-tier deps. |
| `shell/src/plugins/nexus/search/index.ts` | 55 | BL-XXX Phase 4.5 | Ctrl/Cmd+Shift+F → `nexus.searchPanel` (the richer multi-file find/replace pane) isn't wired. The sidebar overlay stays reachable via the command palette ("Focus Search") and any custom user binding. |
| `shell/src/registry/SnippetRegistry.ts` | 11 | (snippet expansion) | The snippet registry is intentionally data-only: it stores snippet metadata and detects trigger collisions. Actual editor-side expansion is a separate concern; trigger-key detection on input isn't wired. |
| `shell/tests/plugin-import-hygiene.test.ts` | 40-77 | WI-25 | 12 plugin files predate the kernel bridge and are allowlisted for `@tauri-apps/*` imports. Each row has a per-file rationale; WI-25 is the inverse work item — drain the list, do not grow it. |
| `crates/nexus-terminal/src/server.rs` | 462 | (session restart) | `SessionManager` doesn't yet expose the shell string for an existing session, so `restart` can't preserve the pre-command launch state — `SessionInfo.shell` passes through an empty string rather than guessing. |
| `crates/nexus-plugins/src/contributions.rs` | 7-11 | ADR 0027 §Migration | Phase 0a is pure aggregation; the host crates pick up the new shape in Phase 1+ per protocol. Legacy flat-TOML config still merges in during the deprecation window. |
| `crates/nexus-remote/src/uri.rs` | (module doc) | BL-140 Phase 2b | The SSH-spawn path (`ssh user@host:port nexus serve`) ships as Phase 2b client tests only; the actual spawn logic is gated until Phase 2b lands. |

Promote any of these to a `// TODO` marker (or a GitHub issue) when work picks up — and delete the row from this table at the same time.

---

## Correction to plugin-capabilities.md

The previous [`plugin-capabilities.md`](../plugin-capabilities.md) listed 10 folders under `shell/src/plugins/nexus/` as "component-only (no `index.ts`)". The probe used `index.ts` but missed `index.tsx`. Re-checking:

| Folder | Has `index.{ts,tsx}`? | Verdict |
|--------|-----------------------|---------|
| `allProperties` | `index.tsx` | **Stub plugin** (renders "Not yet implemented") |
| `bookmarks` | `index.tsx` | **Stub plugin** |
| `fileProperties` | `index.tsx` | **Stub plugin** |
| `outgoingLinks` | `index.tsx` | **Stub plugin** |
| `tags` | `index.tsx` | **Stub plugin** |
| `debugger` | (none) | True component-only |
| `healthPanel` | (none) | True component-only |
| `searchPanel` | (none) | True component-only |
| `status` | (none) | True component-only |
| `statusBar` | (none) | True component-only |

**Corrected count:** 56 first-party shell plugins (51 fully implemented + 5 intentional stubs) + 5 true component-only folders + 1 shared module = 62 entries. Total plugins across the system: 23 backend + 17 shell core + 56 shell first-party = **96** (was previously stated as 91).

The plugin-capabilities.md tables remain accurate for the 51 implemented plugins; the 5 stubs are now documented above in §6.

---

## How to graduate a stub

For settings-panel stubs (the biggest category):
1. Identify which plugin should own the setting (often the answer is a backend plugin, e.g. `com.nexus.editor` for editor settings).
2. Add a `configuration.schema.<key>` to that plugin's manifest (`shell/src/plugins/{core,nexus}/<plugin>/index.ts`).
3. Wire the control: replace `onChange={comingSoon('Label')}` with `onChange={(v) => writeSetting('key', v)}`.
4. Delete the row from §4 above.

For inspector stubs (§6):
1. The placeholder describes the intended feature — implement against the relevant backend handlers (`com.nexus.storage::read_frontmatter` for `allProperties`, `query_tags` for `tags`, `outgoing_links` for `outgoingLinks`).
2. Wire data flow with `ctx.ipc.call(...)`.
3. Replace the `<div>Not yet implemented…</div>` body with the real view.

For CLI stubs (§3):
1. Decide ownership (`sync` → storage / git; `run` → workflow / skills).
2. Implement the subcommand body, remove the `Sync(StubArgs)` line, route to the new module.
3. Delete the row from §3 above.

For the Rust TODOs (§2):
1. `nexus-audio` TLS pinning + AIFF + forge_root — three small, well-scoped tickets. Pick up as part of any audio-engine work.
