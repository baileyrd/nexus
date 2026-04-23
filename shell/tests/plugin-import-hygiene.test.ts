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
  // Each entry: file uses Tauri primitive X, drained by WI-25.
  'shell/src/plugins/core/editorArea/EditorAreaView.tsx',          // plugin-fs.readTextFile
  'shell/src/plugins/core/fileExplorer/FileExplorerView.tsx',      // plugin-dialog.open + plugin-fs.readDir
  'shell/src/plugins/core/fileExplorer/index.ts',                  // plugin-dialog.open
  'shell/src/plugins/core/fileSystemService/index.ts',             // plugin-fs.* (the legacy FS service)
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // api/core.invoke
  'shell/src/plugins/core/titleBar/TitleBarView.tsx',              // api/window.getCurrentWindow
  'shell/src/plugins/core/titleBar/index.ts',                      // api/window.getCurrentWindow
  'shell/src/plugins/nexus/editor/index.ts',                       // plugin-shell.open
  'shell/src/plugins/nexus/launcher/launcherState.ts',             // api/core.invoke
  'shell/src/plugins/nexus/pluginsMgmt/index.ts',                  // api/core.invoke
  'shell/src/plugins/nexus/workspace/index.ts',                    // plugin-dialog.open + api/core.invoke
])

/**
 * Files permitted to reach into `shell/src/host/*` from a plugin. Tracked so
 * WI-25 (and future kernel-bridge work) can drain the list. Each entry should
 * eventually be replaced by an `api.*` call.
 */
const HOST_INTERNALS_ALLOWLIST: ReadonlySet<string> = new Set([
  'shell/src/plugins/core/activityBar/ActivityBarView.tsx',        // host/shellRegistry
  'shell/src/plugins/core/commandPalette/CommandPaletteView.tsx',  // host/ContextKeyService + shellRegistry
  'shell/src/plugins/core/editorArea/EditorAreaView.tsx',          // host/ContextKeyService
  'shell/src/plugins/core/fileExplorer/FileExplorerView.tsx',      // host/ContextKeyService
  'shell/src/plugins/core/settings/SettingsPanelView.tsx',         // host/shellRegistry + ContextKeyService + communityPluginLoader
  'shell/src/plugins/core/statusBar/StatusBarView.tsx',            // host/shellRegistry
  'shell/src/plugins/nexus/backlinks/BacklinksView.tsx',           // host/EventBus
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
  let entries: ReturnType<typeof readdirSync> = []
  try {
    entries = readdirSync(dir, { withFileTypes: true })
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
  - docs/PHASE-1-IMPLEMENTATION-PLAN.md §4 (this guardrail) and §8 (WI-25 drain)

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
