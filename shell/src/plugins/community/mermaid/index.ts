// BL-008 — community.mermaid plugin.
//
// Registers a fenced-code renderer for the `mermaid` language tag via
// the BL-008 fenced-code-renderer registry. Mermaid itself is loaded
// lazily through `import('mermaid')` only when the first matching
// fence is rendered — Vite chunks the ~150 KB gzipped library into a
// separate asset so the cost is paid by users who actually use it.
//
// Architecture note. The community-plugin loader's Blob-URL path
// can't resolve bare-specifier imports (`import('mermaid')` from a
// Blob URL has no module graph), so this plugin is registered through
// the shell catalog (`DEFAULT_OFF_PLUGINS`) instead. The user opts in
// via Settings → Plugins; Vite then resolves the dynamic mermaid
// import. `plugin.json` is informational only (`enabled: false`).

import type { Plugin, PluginAPI } from '../../../types/plugin'

const PLUGIN_ID = 'community.mermaid'
const LANGUAGE = 'mermaid'

let mermaidPromise: Promise<MermaidLike> | null = null
let mermaidInitialized = false
let renderCounter = 0

interface MermaidLike {
  initialize(config: Record<string, unknown>): void
  render(id: string, source: string): Promise<{ svg: string }>
}

async function loadMermaid(): Promise<MermaidLike> {
  if (!mermaidPromise) {
    mermaidPromise = import('mermaid').then((mod) => {
      const m = (mod.default ?? mod) as unknown as MermaidLike
      return m
    })
  }
  return mermaidPromise
}

function detectTheme(): 'dark' | 'default' {
  if (typeof document === 'undefined') return 'default'
  const attr = document.documentElement.dataset.theme
  if (attr === 'dark') return 'dark'
  if (attr === 'light') return 'default'
  return 'default'
}

async function renderMermaid(source: string): Promise<HTMLElement> {
  const mermaid = await loadMermaid()
  if (!mermaidInitialized) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: 'strict',
      theme: detectTheme(),
    })
    mermaidInitialized = true
  }
  renderCounter++
  const id = `nexus-mermaid-${Date.now()}-${renderCounter}`
  const result = await mermaid.render(id, source)
  const wrap = document.createElement('div')
  wrap.className = 'nexus-mermaid-diagram'
  wrap.innerHTML = result.svg
  return wrap
}

let registeredDispose: (() => void) | null = null

const plugin: Plugin = {
  manifest: {
    id: PLUGIN_ID,
    name: 'Mermaid Diagrams',
    version: '1.0.0',
    core: false,
    activationEvents: ['*'],
    apiVersion: 1,
  },
  activate(api: PluginAPI) {
    ensureStylesheet()
    registeredDispose = api.editor.registerFencedCodeRenderer(LANGUAGE, async (source) => {
      try {
        return await renderMermaid(source)
      } catch (err) {
        return buildErrorBox(err)
      }
    })
  },
  deactivate() {
    if (registeredDispose) {
      registeredDispose()
      registeredDispose = null
    }
  },
}

function buildErrorBox(err: unknown): HTMLElement {
  const message = err instanceof Error ? err.message : String(err)
  const box = document.createElement('div')
  box.className = 'nexus-mermaid-error'
  const tag = document.createElement('span')
  tag.className = 'nexus-mermaid-error-tag'
  tag.textContent = 'mermaid'
  const msg = document.createElement('pre')
  msg.className = 'nexus-mermaid-error-msg'
  msg.textContent = message
  box.append(tag, msg)
  return box
}

let stylesheetInstalled = false
function ensureStylesheet(): void {
  if (stylesheetInstalled) return
  if (typeof document === 'undefined') return
  stylesheetInstalled = true
  // Side-effect import of the plugin stylesheet. Vite tracks it as a
  // CSS asset and ships it in the editor bundle's stylesheet graph.
  void import('./mermaid.css').catch((err) => {
    console.warn('[community.mermaid] mermaid.css load failed:', err)
  })
}

export const mermaidPlugin = plugin
export default plugin
