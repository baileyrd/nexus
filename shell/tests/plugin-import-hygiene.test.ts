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
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // shell-internal: set_plugin_enabled
  'shell/src/plugins/nexus/launcher/launcherState.ts',             // shell-internal: get/write/forget shell_state (recents)
  'shell/src/plugins/nexus/memory/index.ts',                       // BL-043: tauri-plugin-global-shortcut — no @nexus/extension-api global-hotkey surface yet
  'shell/src/plugins/nexus/pluginsMgmt/index.ts',                  // shell-internal: set_plugin_enabled
  'shell/src/plugins/nexus/workspace/index.ts',                    // shell-internal: boot_kernel + init_forge + shutdown_kernel + plugin-dialog.open
  'shell/src/plugins/nexus/notion/index.ts',                       // plugin-dialog.open for source-zip + dest-folder pickers (no PlatformDialog API surface yet — same drain plan as workspace plugin)
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
  'shell/src/plugins/core/activityBar/ActivityBarView.tsx',        // host/shellRegistry
  'shell/src/plugins/core/capabilityPrompt/requestConsent.ts',     // WI-31: host/communityPluginLoader (manifest type)
  'shell/src/plugins/core/commandPalette/CommandPaletteView.tsx',  // host/ContextKeyService + shellRegistry
  'shell/src/plugins/core/editorArea/EditorAreaView.tsx',          // host/ContextKeyService
  'shell/src/plugins/core/fileExplorer/FileExplorerView.tsx',      // host/ContextKeyService
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // host/shellRegistry + ContextKeyService + communityPluginLoader
  'shell/src/plugins/core/statusBar/StatusBarView.tsx',            // host/shellRegistry
  'shell/src/plugins/core/titleBar/TitleBarView.tsx',              // host/shellRegistry + ContextKeyService — same React-component pattern as the other *View.tsx entries (no `api` in scope)
  'shell/src/plugins/nexus/ai/index.ts',                           // host/ContextKeyService + EventBus — predates @nexus/extension-api context-keys / event surface; track under WI-25 drain
  'shell/src/plugins/nexus/backlinks/BacklinksView.tsx',           // host/EventBus
  'shell/src/plugins/nexus/bases/BasesView.tsx',                   // host/ContextKeyService — BL-030 mirrors the canvas active-handle pattern; @nexus/extension-api context-keys not yet wired to React components
  'shell/src/plugins/nexus/bases/BasesTable.tsx',                  // host/ContextKeyService — BL-031 publishes `bases.editing` for the cell-clipboard `when:` clauses; same drain plan as BasesView (WI-25)
  'shell/src/plugins/nexus/canvas/CanvasView.tsx',                 // host/ContextKeyService
  'shell/src/plugins/nexus/editor/EditorView.tsx',                 // host/EventBus + shellRegistry
  'shell/src/plugins/nexus/graph/GraphGlobalView.tsx',             // host/EventBus
  'shell/src/plugins/nexus/graph/GraphView.tsx',                   // host/EventBus
  'shell/src/plugins/nexus/outline/OutlineView.tsx',               // host/EventBus
  'shell/src/plugins/nexus/pluginsMgmt/index.ts',                  // host/communityPluginLoader
  'shell/src/plugins/nexus/processes/index.ts',                    // host/communityPluginLoader
])

/**
 * Files permitted to reach into `shell/src/registry/*` from a plugin. Same
 * drain plan as HOST_INTERNALS_ALLOWLIST.
 */
const REGISTRY_INTERNALS_ALLOWLIST: ReadonlySet<string> = new Set([
  'shell/src/plugins/core/configurationService/index.ts',          // registry/ConfigurationRegistry
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // registry/KeybindingRegistry — overrides UI (WI-04)
  'shell/src/plugins/core/statusBar/StatusBarView.tsx',            // registry/StatusBarRegistry
])

// `from '@tauri-apps/...'` — covers default, named, and type-only imports.
const TAURI_RE = /from\s+['"]@tauri-apps\//
// `from '../../host/...'`, `from '../../../host/...'`, etc. — any number of `../`.
const HOST_RE = /from\s+['"](?:\.\.\/)+host\/|from\s+['"]@\/host\//
const REGISTRY_RE = /from\s+['"](?:\.\.\/)+registry\/|from\s+['"]@\/registry\//

/** Bare-specifier side-effect imports of the same surfaces (e.g. `import '@tauri-apps/...'`). */
const TAURI_SIDE_EFFECT_RE = /import\s+['"]@tauri-apps\//
const HOST_SIDE_EFFECT_RE = /import\s+['"](?:\.\.\/)+host\/|import\s+['"]@\/host\//
const REGISTRY_SIDE_EFFECT_RE = /import\s+['"](?:\.\.\/)+registry\/|import\s+['"]@\/registry\//

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

function findViolations(rule: Rule): string[] {
  const out: string[] = []
  for (const file of walk(PLUGINS_ROOT)) {
    const rel = toRepoRel(file)
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
