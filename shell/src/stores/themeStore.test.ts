// shell/src/stores/themeStore.test.ts
//
// WI-02 part 2 — kernel-sync theme store. Verifies that the store
// hydrates from the kernel, mirrors `com.nexus.theme.changed`
// events, and applies resolved variables to :root.
//
// DOM strategy: we don't run jsdom. `document` is undefined under
// `node --import tsx --test`, and the store's `applyResolvedVariables`
// short-circuits on `typeof document === 'undefined'` — so DOM
// assertions become "the store tracks `appliedVariableNames`
// correctly", which is the mechanism that drives :root writes.
// This is enough to catch the "snippet removal leaves orphan vars"
// regression that motivated the tracking; live :root verification
// happens in e2e (Part 3 settings UI).

// @ts-expect-error tsc lib doesn't include node builtins
import { test } from 'node:test'
// @ts-expect-error tsc lib doesn't include node builtins
import assert from 'node:assert/strict'
import {
  useThemeStore,
  THEME_PLUGIN_ID,
  THEME_CHANGED_EVENT,
  type AppliedTheme,
  type KernelThemeConfig,
  type ThemeManifestEntry,
} from './themeStore.ts'
import type { PluginAPI } from '../types/plugin.ts'

// ── Mock plumbing ──────────────────────────────────────────────────────
//
// We only stub the slice of PluginAPI the store actually touches
// (`api.kernel.invoke` + `api.kernel.on`). Casting via `unknown`
// keeps tsc happy without dragging the full PluginAPI surface area
// into the mock.

type InvokeStub = (
  pluginId: string,
  commandId: string,
  args?: unknown,
) => Promise<unknown>

interface SubscribeRecord {
  topic: string
  handler: (topic: string, payload: unknown) => void
}

function makeMockApi(invoke: InvokeStub): {
  api: PluginAPI
  subscribers: SubscribeRecord[]
  invocations: Array<{ pluginId: string; commandId: string; args: unknown }>
} {
  const subscribers: SubscribeRecord[] = []
  const invocations: Array<{ pluginId: string; commandId: string; args: unknown }> = []
  const api = {
    kernel: {
      invoke: async (pluginId: string, commandId: string, args?: unknown) => {
        invocations.push({ pluginId, commandId, args })
        return invoke(pluginId, commandId, args)
      },
      on: async (
        topicPrefix: string,
        handler: (topic: string, payload: unknown) => void,
      ) => {
        subscribers.push({ topic: topicPrefix, handler })
        return () => {}
      },
      available: async () => true,
    },
  } as unknown as PluginAPI
  return { api, subscribers, invocations }
}

function reset(): void {
  // Wipe both legacy + kernel-mirrored state so each test starts clean.
  useThemeStore.setState({
    theme: 'dark',
    density: 'cozy',
    availableThemes: [],
    activeThemeId: null,
    resolvedVariables: {},
    enabledSnippets: [],
    loaded: false,
    appliedVariableNames: [],
  })
}

// ── Tests ──────────────────────────────────────────────────────────────

test('hydrate: populates store from get_theme_config + get_available_themes + compute_variables', async () => {
  reset()

  const themes: ThemeManifestEntry[] = [
    { id: 'nexus-dark', name: 'Nexus Dark' },
    { id: 'nexus-light', name: 'Nexus Light' },
  ]
  const config: KernelThemeConfig = {
    theme_id: 'nexus-dark',
    mode: 'dark',
    enabled_snippets: ['snip-a'],
  }
  const variables: Record<string, string> = {
    '--background': '#111',
    '--foreground': '#eee',
  }

  const { api, invocations } = makeMockApi(async (_p, cmd) => {
    if (cmd === 'get_theme_config') return config
    if (cmd === 'get_available_themes') return themes
    if (cmd === 'compute_variables') return variables
    throw new Error(`unexpected command: ${cmd}`)
  })

  await useThemeStore.getState().hydrate(api)

  const after = useThemeStore.getState()
  assert.equal(after.activeThemeId, 'nexus-dark')
  assert.deepEqual(after.enabledSnippets, ['snip-a'])
  assert.deepEqual(after.availableThemes, themes)
  assert.deepEqual(after.resolvedVariables, variables)
  assert.equal(after.loaded, true)

  // Confirms hydrate calls the three expected handlers against
  // the right plugin id; saves us from a wire-rename regression.
  const cmdsForThemePlugin = invocations
    .filter((i) => i.pluginId === THEME_PLUGIN_ID)
    .map((i) => i.commandId)
  assert.ok(cmdsForThemePlugin.includes('get_theme_config'))
  assert.ok(cmdsForThemePlugin.includes('get_available_themes'))
  assert.ok(cmdsForThemePlugin.includes('compute_variables'))
})

test('hydrate: tracks applied variable names so subsequent compute can clear orphans', async () => {
  reset()

  let pass = 0
  const { api } = makeMockApi(async (_p, cmd) => {
    if (cmd === 'get_theme_config') {
      return { theme_id: 'nexus-dark', mode: 'dark', enabled_snippets: [] }
    }
    if (cmd === 'get_available_themes') return []
    if (cmd === 'compute_variables') {
      pass += 1
      // First pass: two vars. Second pass: only one — the orphan
      // should drop out of `appliedVariableNames`.
      return pass === 1
        ? { '--a': '#aaa', '--b': '#bbb' }
        : { '--a': '#ccc' }
    }
    throw new Error(`unexpected: ${cmd}`)
  })

  await useThemeStore.getState().hydrate(api)
  assert.deepEqual(
    [...useThemeStore.getState().appliedVariableNames].sort(),
    ['--a', '--b'],
  )

  await useThemeStore.getState().hydrate(api)
  assert.deepEqual(useThemeStore.getState().appliedVariableNames, ['--a'])
})

test('setActiveTheme: invokes apply_theme and writes returned variables', async () => {
  reset()

  const applied: AppliedTheme = {
    id: 'nexus-light',
    name: 'Nexus Light',
    variables: { '--background': '#fff', '--foreground': '#000' },
  }
  const { api, invocations } = makeMockApi(async (_p, cmd, args) => {
    if (cmd === 'apply_theme') {
      const a = args as { id: string }
      assert.equal(a.id, 'nexus-light')
      return applied
    }
    throw new Error(`unexpected: ${cmd}`)
  })

  await useThemeStore.getState().setActiveTheme(api, 'nexus-light')

  const after = useThemeStore.getState()
  assert.equal(after.activeThemeId, 'nexus-light')
  assert.deepEqual(after.resolvedVariables, applied.variables)
  // appliedVariableNames is populated even in the no-DOM env via
  // the writeVariablesToRoot return path — guards against future
  // refactors that bypass the tracker.
  assert.deepEqual(
    [...after.appliedVariableNames].sort(),
    ['--background', '--foreground'],
  )

  const apply = invocations.find((i) => i.commandId === 'apply_theme')
  assert.ok(apply, 'apply_theme must be invoked')
  assert.equal(apply!.pluginId, THEME_PLUGIN_ID)
})

test('setMode: invokes set_mode and updates legacy theme field for non-system modes', async () => {
  reset()

  const { api, invocations } = makeMockApi(async (_p, cmd) => {
    if (cmd === 'set_mode') return null
    throw new Error(`unexpected: ${cmd}`)
  })

  await useThemeStore.getState().setMode(api, 'light')
  assert.equal(useThemeStore.getState().theme, 'light')
  const set = invocations.find((i) => i.commandId === 'set_mode')
  assert.ok(set)
  assert.deepEqual(set!.args, { mode: 'light' })

  // 'system' mode does not flip the legacy attr — kernel resolves it.
  reset()
  await useThemeStore.getState().setMode(api, 'system')
  assert.equal(useThemeStore.getState().theme, 'dark', 'system mode preserves prior local theme')
})

test('toggleSnippet: invokes toggle_snippet and lets event echo drive state', async () => {
  reset()

  const { api, invocations } = makeMockApi(async (_p, cmd, args) => {
    if (cmd === 'toggle_snippet') {
      assert.deepEqual(args, { id: 'snip-x' })
      return ['snip-x']
    }
    throw new Error(`unexpected: ${cmd}`)
  })

  await useThemeStore.getState().toggleSnippet(api, 'snip-x')
  // No optimistic update — store waits for the .changed event echo.
  assert.deepEqual(useThemeStore.getState().enabledSnippets, [])
  const tog = invocations.find((i) => i.commandId === 'toggle_snippet')
  assert.ok(tog)
})

test('event echo: a com.nexus.theme.changed notification triggers re-hydrate', async () => {
  reset()

  // Simulates the themeService plugin's subscription wiring: the
  // store doesn't subscribe itself (the plugin does), but the
  // re-hydrate path is the same code path the plugin invokes.
  // We assert that re-hydrate after a "change" picks up the new
  // config/variables, which is the contract callers depend on.
  let configState: KernelThemeConfig = {
    theme_id: 'nexus-dark',
    mode: 'dark',
    enabled_snippets: [],
  }
  let varsState: Record<string, string> = { '--bg': '#000' }

  const { api } = makeMockApi(async (_p, cmd) => {
    if (cmd === 'get_theme_config') return configState
    if (cmd === 'get_available_themes') return []
    if (cmd === 'compute_variables') return varsState
    throw new Error(`unexpected: ${cmd}`)
  })

  await useThemeStore.getState().hydrate(api)
  assert.equal(useThemeStore.getState().activeThemeId, 'nexus-dark')

  // Server-side change — equivalent to a `com.nexus.theme.changed`
  // event arriving with a new config.
  configState = { theme_id: 'nexus-light', mode: 'light', enabled_snippets: ['s1'] }
  varsState = { '--bg': '#fff' }

  await useThemeStore.getState().hydrate(api)
  const after = useThemeStore.getState()
  assert.equal(after.activeThemeId, 'nexus-light')
  assert.deepEqual(after.enabledSnippets, ['s1'])
  assert.deepEqual(after.resolvedVariables, { '--bg': '#fff' })
})

test('event constants are stable wire identifiers', () => {
  // Smoke-test: these literals are referenced by the kernel
  // (crates/nexus-theme/src/core_plugin.rs) and any rename here
  // would break the cross-process contract.
  assert.equal(THEME_PLUGIN_ID, 'com.nexus.theme')
  assert.equal(THEME_CHANGED_EVENT, 'com.nexus.theme.changed')
})
