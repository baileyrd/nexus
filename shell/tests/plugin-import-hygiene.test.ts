/**
 * WI-23 — Shell-side import-hygiene guardrail.
 *
 * Fails if any file under `shell/src/plugins/{core,nexus}/` imports from a
 * forbidden surface:
 *
 *   - `@tauri-apps/*`              — must go through `api.kernel.invoke`
 *                                    or `api.platform.*` (see CONTRIBUTING.md
 *                                    and packages/nexus-extension-api/README.md).
 *   - shell `host/` internals      — only the kernel bridge may touch these.
 *   - shell `registry/` internals  — same.
 *
 * Each forbidden surface has its own allowlist of files known to violate the
 * rule today. WI-25 is the inverse work item: drain those allowlists by
 * routing each plugin through `@nexus/extension-api`. Adding a NEW import
 * outside an allowlisted file fails the test — that is the point.
 *
 * If you have to add a file to an allowlist, explain why in your commit
 * message and link the WI-25 follow-up (or a fresh ADR if `@nexus/extension-api`
 * is genuinely missing the surface you need).
 */

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'

// `node --test` runs from `shell/` (the package directory). Resolve the repo
// root one level up so that allowlist entries can be written as
// `shell/src/plugins/...` — readable when the test fails in CI logs.
const SHELL_DIR = resolve(process.cwd())
const REPO_ROOT = resolve(SHELL_DIR, '..')
const PLUGINS_ROOT = join(SHELL_DIR, 'src', 'plugins')

/**
 * Files permitted to import from `@tauri-apps/*`. These predate the kernel
 * bridge and will be drained by WI-25 (Phase 1 §8 of the implementation plan).
 * Do NOT add to this list without an explicit reason in your commit message.
 */
const TAURI_IMPORT_ALLOWLIST: ReadonlySet<string> = new Set([
  // Each entry: why it stays allowlisted. Goal is to keep this list shrinking.
  //
  // ─── React-component views: no `api` in scope ──────────────────────────────
  // These are standalone *View.tsx components rendered by the shell's view
  // system, not via the plugin's `activate(api)` closure. Until React context
  // for the plugin API is wired up (separate WI), they have no path to
  // `api.platform.*` and continue to import Tauri primitives directly.
  'shell/src/plugins/core/editorArea/EditorAreaView.tsx',          // plugin-fs.readTextFile
  'shell/src/plugins/core/fileExplorer/FileExplorerView.tsx',      // plugin-dialog.open + plugin-fs.readDir
  'shell/src/plugins/core/titleBar/TitleBarView.tsx',              // api/window.getCurrentWindow
  'shell/src/plugins/nexus/launcher/LauncherView.tsx',             // api/window.getCurrentWindow — close button on the workspace picker; same React-component pattern (no `api` in scope)
  //
  // ─── Shell-internal callers: legitimate exceptions ─────────────────────────
  // These plugins call shell-internal Tauri commands (boot_kernel,
  // set_plugin_enabled, get_shell_state, etc.) that are NOT plugin-API
  // capabilities — they're shell-lifecycle / shell-state ops that have no
  // kernel equivalent. Plan §10 risk row anticipates this: "If something
  // truly needs a bespoke Tauri command, allow it in `shell/src/src-tauri/`
  // and document." A future WI may migrate some of these to kernel IPC.
  'shell/src/plugins/core/capabilityPrompt/requestConsent.ts',     // shell-internal (WI-31): get/set_plugin_granted_capabilities
  'shell/src/plugins/core/settings/SettingsCells.tsx',             // plugin-dialog.open for the "load theme from file" picker — relocated here from SettingsPanelView.tsx in the #191 SettingsPanelView split (the three `kernel_invoke` calls were migrated to `api.kernel.invoke` in the A6 drain; this entry stays only because PlatformDialog has no `open()` surface yet). SettingsPanelView.tsx no longer imports @tauri-apps, so its row left this list — a net move, not a growth.
  'shell/src/plugins/nexus/launcher/launcherState.ts',             // shell-internal: get/write/forget shell_state (recents)
  'shell/src/plugins/nexus/memory/index.ts',                       // BL-043: tauri-plugin-global-shortcut — no @nexus/extension-api global-hotkey surface yet
  'shell/src/plugins/nexus/pluginsMgmt/index.ts',                  // shell-internal: set_plugin_enabled
  'shell/src/plugins/nexus/workspace/index.ts',                    // shell-internal: boot_kernel + init_forge + shutdown_kernel + plugin-dialog.open
  'shell/src/plugins/nexus/workspace/useConnectionState.ts',       // BL-140 Phase 3c — shell-internal: kernel_connection_state read + kernel:connection-state event. State lives in the bridge layer's managed-state slot, no kernel plugin owns it, so an api.kernel.invoke surface would just be a thin wrapper around the same Tauri command.
  'shell/src/plugins/nexus/notion/index.ts',                       // plugin-dialog.open for source-zip + dest-folder pickers (no PlatformDialog API surface yet — same drain plan as workspace plugin)
  'shell/src/plugins/nexus/notifications/index.ts',                // BL-133 follow-up: tauri invoke('notify_desktop') for OS-level notifications — no api.notifications.osLevel surface yet
  'shell/src/plugins/nexus/debugger/LaunchConfig.tsx',             // BL-113 follow-up: tauri invoke('scan_plugin_directory') + plugin-fs.readTextFile to resolve metadata.launch_config_schema — no api.plugins.dir surface yet
  //
  // ─── Partial Tauri retention: missing api.platform surface ────────────────
  // fileSystemService routed read/write/etc through api.platform.fs in WI-25
  // Phase 2b but still imports `watch` from `@tauri-apps/plugin-fs` because
  // PlatformFsAPI does not expose a watch() method. Once it does, this entry
  // can be removed.
  'shell/src/plugins/core/fileSystemService/index.ts',             // plugin-fs.watch only (rest moved to api.platform.fs)
])

/**
 * Files permitted to reach into `shell/src/host/*` from a plugin. Tracked so
 * WI-25 (and future kernel-bridge work) can drain the list. Each entry should
 * eventually be replaced by an `api.*` call.
 */
const HOST_INTERNALS_ALLOWLIST: ReadonlySet<string> = new Set([
  // Phase 2 (assessment cleanup) deleted the activityBar/commandPalette/
  // editorArea/fileExplorer/statusBar/titleBar core stubs; their allowlist
  // rows are gone with them.
  'shell/src/plugins/core/capabilityPrompt/requestConsent.ts',     // WI-31: host/communityPluginLoader (manifest type)
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // host/shellRegistry + ContextKeyService + EventBus
  'shell/src/plugins/nexus/ai/index.ts',                           // host/ContextKeyService + EventBus — predates @nexus/extension-api context-keys / event surface; track under WI-25 drain
  'shell/src/plugins/nexus/backlinks/BacklinksView.tsx',           // host/EventBus
  'shell/src/plugins/nexus/bases/BasesView.tsx',                   // host/ContextKeyService — BL-030 mirrors the canvas active-handle pattern; @nexus/extension-api context-keys not yet wired to React components
  'shell/src/plugins/nexus/bases/BasesTable.tsx',                  // host/ContextKeyService — BL-031 publishes `bases.editing` for the cell-clipboard `when:` clauses; same drain plan as BasesView (WI-25)
  'shell/src/plugins/nexus/canvas/CanvasView.tsx',                 // host/ContextKeyService
  'shell/src/plugins/nexus/editor/EditorView.tsx',                 // host/EventBus + shellRegistry
  'shell/src/plugins/nexus/editor/index.ts',                       // #193/R10 inversion seam — registerEditorHostSurface (host/EditorHostSurface) lets the editor plugin REGISTER its surface with the host (the correct plugin→host direction, not a reach into host internals) + the activeEditor projection helpers (host/activeEditor) extracted in #191 so they stay unit-testable without dragging in @tauri-apps. By design; cannot move to @nexus/extension-api without re-coupling the host to the plugin.
  'shell/src/plugins/nexus/graph/GraphGlobalView.tsx',             // host/EventBus
  'shell/src/plugins/nexus/graph/GraphView.tsx',                   // host/EventBus
  'shell/src/plugins/nexus/outline/OutlineView.tsx',               // host/EventBus
  'shell/src/plugins/nexus/pluginsMgmt/index.ts',                  // host/communityPluginLoader
  'shell/src/plugins/nexus/processes/index.ts',                    // host/communityPluginLoader
  'shell/src/plugins/nexus/viewBuilder/ViewBuilderView.tsx',       // BL-067: introspection tool — host/layoutSnapshot is exactly the surface this panel exists to surface; no @nexus/extension-api equivalent, by design (the builder reads the shell, it doesn't run inside its sandbox)
  'shell/src/plugins/nexus/workspace/index.ts',                    // V16 inversion seam — registerWorkspaceHostSurface (host/WorkspaceHostSurface) lets the workspace plugin REGISTER its root-path surface with the host (plugin→host direction, same shape as the editor seam above). By design; cannot move to @nexus/extension-api without re-coupling the host to the plugin.
])

/**
 * Files permitted to reach into `shell/src/registry/*` from a plugin. Same
 * drain plan as HOST_INTERNALS_ALLOWLIST.
 */
const REGISTRY_INTERNALS_ALLOWLIST: ReadonlySet<string> = new Set([
  'shell/src/plugins/core/activityBar/activityBarStore.ts',        // P2-02: registry/priorityOverrides — this file IS the activity-bar registry; the helper lives in registry/ alongside the other sort registries it sibling-serves
  'shell/src/plugins/core/configurationService/index.ts',          // registry/ConfigurationRegistry
  'shell/src/plugins/core/panelArea/panelAreaStore.ts',            // P2-02: registry/priorityOverrides — same rationale as activityBarStore
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // registry/KeybindingRegistry — overrides UI (WI-04)
  'shell/src/plugins/core/statusBar/StatusBarView.tsx',            // registry/StatusBarRegistry
  'shell/src/plugins/nexus/viewBuilder/ViewBuilderView.tsx',       // BL-067: introspection tool — reads SlotRegistry to surface chrome contributions in its inventory pane. Same rationale as the HOST_INTERNALS allowlist entry.
])

/**
 * V16 — the INVERSE direction: host/chrome code (everything under
 * `shell/src/` EXCEPT `src/plugins/`) must not import plugin internals
 * (`plugins/{core,nexus,community}/...`). The shell starts empty; plugins
 * register their surfaces with the host (EditorHostSurface,
 * WorkspaceHostSurface), never the other way round. `plugins/catalog` is
 * exempt by the regex — it IS the composition root the host loads.
 *
 * Each entry should eventually be replaced by a host-owned seam the
 * plugin registers into (the WorkspaceHostSurface treatment, V16) or a
 * context key the plugin publishes (the AA-04/P3-03 treatment).
 */
const HOST_TO_PLUGIN_ALLOWLIST: ReadonlySet<string> = new Set([
  'shell/src/main.tsx',                                            // composition root — runInstallTimeConsent (capabilityPrompt) runs BEFORE the ExtensionHost exists, so there is no plugin-registered seam to call through yet
  'shell/src/host/communityPluginLoader.ts',                       // pluginsMgmt/capabilityInfo — parseManifestCapabilities is pure manifest parsing that should live host-side; extract it from the pluginsMgmt plugin to drain this entry
  'shell/src/workspace/WorkspaceRenderer.tsx',                     // editor/editorStore — dirty-tab close guard reads isDirty; needs an EditorHostSurface extension (isDirty by relpath) to drain
  'shell/src/workspace/RightPanelFooter.tsx',                      // editor/editorStore + noteContext/backlinksDataStore — per-document stats footer; needs EditorHostSurface content stats + a backlinks seam to drain
  'shell/src/workspace/ForgeSelector.tsx',                         // launcher/launcherState — recents list for the forge-switcher menu; needs a launcher-owned seam (or recents moving host-side) to drain
])

// `from '@tauri-apps/...'` — covers default, named, and type-only imports.
const TAURI_RE = /from\s+['"]@tauri-apps\//
// `from '../../host/...'`, `from '../../../host/...'`, etc. — any number of `../`.
const HOST_RE = /from\s+['"](?:\.\.\/)+host\/|from\s+['"]@\/host\//
const REGISTRY_RE = /from\s+['"](?:\.\.\/)+registry\/|from\s+['"]@\/registry\//

// V16 — `from '../plugins/nexus/...'`, `from './plugins/core/...'`,
// `from '@/plugins/community/...'`, etc. Deliberately does NOT match
// `plugins/catalog` (the composition root).
const PLUGIN_INTERNALS_RE = /from\s+['"][^'"]*\bplugins\/(?:core|nexus|community)\//

/** Bare-specifier side-effect imports of the same surfaces (e.g. `import '@tauri-apps/...'`). */
const TAURI_SIDE_EFFECT_RE = /import\s+['"]@tauri-apps\//
const HOST_SIDE_EFFECT_RE = /import\s+['"](?:\.\.\/)+host\/|import\s+['"]@\/host\//
const REGISTRY_SIDE_EFFECT_RE = /import\s+['"](?:\.\.\/)+registry\/|import\s+['"]@\/registry\//
const PLUGIN_INTERNALS_SIDE_EFFECT_RE = /import\s+['"][^'"]*\bplugins\/(?:core|nexus|community)\//

/** Recursively yield .ts/.tsx files under `dir`, skipping dotfiles and node_modules. */
function* walk(dir: string): Generator<string> {
  let entries: import('node:fs').Dirent[] = []
  try {
    entries = readdirSync(dir, { withFileTypes: true, encoding: 'utf8' })
  } catch {
    return
  }
  for (const entry of entries) {
    const name = entry.name
    if (name.startsWith('.') || name === 'node_modules') continue
    const full = join(dir, name)
    // Use lstat-equivalent via withFileTypes; explicitly skip symlinks for safety.
    if (entry.isSymbolicLink()) continue
    if (entry.isDirectory()) {
      yield* walk(full)
    } else if (entry.isFile() && (name.endsWith('.ts') || name.endsWith('.tsx'))) {
      yield full
    }
  }
}

/** Convert an absolute path to a `shell/src/...` relative path with forward slashes. */
function toRepoRel(abs: string): string {
  return relative(REPO_ROOT, abs).split(sep).join('/')
}

interface Rule {
  readonly importRe: RegExp
  readonly sideEffectRe: RegExp
  readonly allowlist: ReadonlySet<string>
}

function findViolations(
  rule: Rule,
  root: string = PLUGINS_ROOT,
  skip?: (rel: string) => boolean,
): string[] {
  const out: string[] = []
  for (const file of walk(root)) {
    const rel = toRepoRel(file)
    if (skip?.(rel)) continue
    if (rule.allowlist.has(rel)) continue
    const src = readFileSync(file, 'utf8')
    if (rule.importRe.test(src) || rule.sideEffectRe.test(src)) {
      out.push(`  ${rel}`)
    }
  }
  return out
}

const HELP_FOOTER = `
See:
  - CONTRIBUTING.md (plugin authoring guide)
  - packages/nexus-extension-api/README.md (the supported surface)
  - docs/planning/PHASE-1-IMPLEMENTATION-PLAN.md §4 (this guardrail) and §8 (WI-25 drain)

If a primitive truly has no @nexus/extension-api equivalent, add the file to
the corresponding allowlist in shell/tests/plugin-import-hygiene.test.ts and
explain why in the commit message. WI-25 is the inverse process — keep the
allowlist shrinking, never growing.`

test('no new @tauri-apps/* imports outside the WI-25 allowlist', () => {
  const violations = findViolations({
    importRe: TAURI_RE,
    sideEffectRe: TAURI_SIDE_EFFECT_RE,
    allowlist: TAURI_IMPORT_ALLOWLIST,
  })
  assert.equal(
    violations.length,
    0,
    `New @tauri-apps/* imports detected in shell plugins. Use api.kernel.invoke
or api.platform.* via @nexus/extension-api instead.

Violators:
${violations.join('\n')}
${HELP_FOOTER}`,
  )
})

test('no plugin reaches into shell host/ internals outside the allowlist', () => {
  const violations = findViolations({
    importRe: HOST_RE,
    sideEffectRe: HOST_SIDE_EFFECT_RE,
    allowlist: HOST_INTERNALS_ALLOWLIST,
  })
  assert.equal(
    violations.length,
    0,
    `Plugin reached into shell/src/host/* internals. Plugins must talk to the
kernel through @nexus/extension-api, not by importing host modules directly.

Violators:
${violations.join('\n')}
${HELP_FOOTER}`,
  )
})

test('host/chrome code does not import plugin internals outside the allowlist (V16)', () => {
  const SRC_ROOT = join(SHELL_DIR, 'src')
  const violations = findViolations(
    {
      importRe: PLUGIN_INTERNALS_RE,
      sideEffectRe: PLUGIN_INTERNALS_SIDE_EFFECT_RE,
      allowlist: HOST_TO_PLUGIN_ALLOWLIST,
    },
    SRC_ROOT,
    (rel) =>
      // Plugins importing each other is governed by the rules above, not
      // this one. Colocated host *tests* may exercise plugin internals on
      // purpose (PluginAPI.editor.test.ts validates the projection helpers
      // against the REAL editor store) — production host code may not.
      rel.startsWith('shell/src/plugins/') ||
      rel.endsWith('.test.ts') ||
      rel.endsWith('.test.tsx'),
  )
  assert.equal(
    violations.length,
    0,
    `Host/chrome code imported plugin internals. The shell starts empty:
plugins register their surfaces with the host (see host/EditorHostSurface.ts
and host/WorkspaceHostSurface.ts) or publish context keys — the host never
imports from shell/src/plugins/{core,nexus,community}/.

Violators:
${violations.join('\n')}
${HELP_FOOTER}`,
  )
})

test('no plugin reaches into shell registry/ internals outside the allowlist', () => {
  const violations = findViolations({
    importRe: REGISTRY_RE,
    sideEffectRe: REGISTRY_SIDE_EFFECT_RE,
    allowlist: REGISTRY_INTERNALS_ALLOWLIST,
  })
  assert.equal(
    violations.length,
    0,
    `Plugin reached into shell/src/registry/* internals. Plugins must talk to
the kernel through @nexus/extension-api, not by importing registry modules
directly.

Violators:
${violations.join('\n')}
${HELP_FOOTER}`,
  )
})
