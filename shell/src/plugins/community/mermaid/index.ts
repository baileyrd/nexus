// community.mermaid plugin.
//
// BL-008: registers a fenced-code renderer for the `mermaid` language
// tag via the BL-008 fenced-code-renderer registry.
// BL-009: registers a whole-file viewer that opens `.mermaid` files
// as a rendered SVG with a View-Source toggle.
//
// Mermaid itself is loaded lazily through `import('mermaid')` only
// when the first matching fence or file is rendered — Vite chunks the
// ~150 KB gzipped library into a separate asset so the cost is paid
// by users who actually use it.
//
// Architecture note. The community-plugin loader's Blob-URL path
// can't resolve bare-specifier imports (`import('mermaid')` from a
// Blob URL has no module graph), so this plugin is registered through
// the shell catalog (`DEFAULT_OFF_PLUGINS`) instead. The user opts in
// via Settings → Plugins; Vite then resolves the dynamic mermaid
// import. `plugin.json` is informational only (`enabled: false`).

import type { Leaf, ViewCreator } from '../../../workspace'
import { ViewBase, viewRegistry } from '../../../workspace'
import type { KernelAPI, Plugin, PluginAPI } from '../../../types/plugin'
import { clientLogger } from '../../../clientLogger'

const PLUGIN_ID = 'community.mermaid'
const LANGUAGE = 'mermaid'
const VIEW_TYPE = 'mermaid'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'

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

let registeredFenceDispose: (() => void) | null = null
let registeredViewDispose: (() => void) | null = null
let registeredExtDispose: (() => void) | null = null

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
    registeredFenceDispose = api.editor.registerFencedCodeRenderer(
      LANGUAGE,
      async (source) => {
        try {
          return await renderMermaid(source)
        } catch (err) {
          return buildErrorBox(err)
        }
      },
    )

    // BL-009: whole-file `.mermaid` viewer.
    const creator = mermaidPaneViewCreator(api.kernel)
    registeredViewDispose = viewRegistry.register(VIEW_TYPE, creator)
    registeredExtDispose = viewRegistry.registerExtensions(['mermaid'], VIEW_TYPE)
  },
  deactivate() {
    registeredFenceDispose?.()
    registeredFenceDispose = null
    registeredViewDispose?.()
    registeredViewDispose = null
    registeredExtDispose?.()
    registeredExtDispose = null
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
    clientLogger.warn('[community.mermaid] mermaid.css load failed:', err)
  })
}

// ── BL-009: whole-file .mermaid viewer ────────────────────────────────

interface MermaidViewState {
  relpath?: string
}

class MermaidPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private state: MermaidViewState = {}
  private container: HTMLElement | null = null
  /** Cached file source after the first read so toggling source/render
   *  modes is instant and doesn't re-IPC. */
  private source: string | null = null
  private mode: 'rendered' | 'source' = 'rendered'

  constructor(
    leaf: Leaf,
    private readonly kernel: KernelAPI,
  ) {
    super(leaf)
  }

  override getState(): MermaidViewState {
    return this.state
  }

  getDisplayText(): string {
    const relpath = this.state.relpath
    if (!relpath) return this.viewType
    const i = Math.max(relpath.lastIndexOf('/'), relpath.lastIndexOf('\\'))
    return i >= 0 ? relpath.slice(i + 1) : relpath
  }

  override setState(state: unknown): void {
    if (state && typeof state === 'object' && 'relpath' in state) {
      const relpath = (state as Record<string, unknown>).relpath
      this.state = { relpath: typeof relpath === 'string' ? relpath : undefined }
    } else {
      this.state = {}
    }
  }

  override async onOpen(el: HTMLElement): Promise<void> {
    this.container = el
    el.classList.add('nexus-mermaid-pane')
    await this.refresh()
  }

  override onClose(): void {
    if (this.container) {
      this.container.replaceChildren()
      this.container.classList.remove('nexus-mermaid-pane')
      this.container = null
    }
    this.source = null
  }

  private async refresh(): Promise<void> {
    if (!this.container) return
    const relpath = this.state.relpath
    if (!relpath) {
      this.container.replaceChildren(buildPlaceholder('Mermaid view without a path'))
      return
    }

    if (this.source == null) {
      try {
        this.source = await this.readSource(relpath)
      } catch (err) {
        this.container.replaceChildren(buildErrorBox(err))
        return
      }
    }

    const body =
      this.mode === 'source'
        ? buildSourceView(this.source ?? '')
        : await this.buildRenderedBody()

    this.container.replaceChildren(this.buildToolbar(), body)
  }

  private async buildRenderedBody(): Promise<HTMLElement> {
    try {
      return await renderMermaid(this.source ?? '')
    } catch (err) {
      return buildErrorBox(err)
    }
  }

  private buildToolbar(): HTMLElement {
    const bar = document.createElement('div')
    bar.className = 'nexus-mermaid-toolbar'
    const toggle = document.createElement('button')
    toggle.type = 'button'
    toggle.className = 'nexus-mermaid-toolbar-button'
    toggle.textContent = this.mode === 'source' ? 'Render' : 'View Source'
    toggle.addEventListener('click', () => {
      this.mode = this.mode === 'source' ? 'rendered' : 'source'
      void this.refresh()
    })
    bar.append(toggle)
    return bar
  }

  private async readSource(relpath: string): Promise<string> {
    const resp = await this.kernel.invoke<{ bytes: number[] | null }>(
      STORAGE_PLUGIN_ID,
      'read_file',
      { path: relpath },
    )
    if (resp.bytes == null) {
      throw new Error(`File not found: ${relpath}`)
    }
    return new TextDecoder('utf-8').decode(Uint8Array.from(resp.bytes))
  }
}

function mermaidPaneViewCreator(kernel: KernelAPI): ViewCreator {
  return (leaf) => new MermaidPaneView(leaf, kernel)
}

function buildSourceView(source: string): HTMLElement {
  const pre = document.createElement('pre')
  pre.className = 'nexus-mermaid-source'
  pre.textContent = source
  return pre
}

function buildPlaceholder(message: string): HTMLElement {
  const div = document.createElement('div')
  div.className = 'nexus-mermaid-placeholder'
  div.textContent = message
  return div
}

export const mermaidPlugin = plugin
export default plugin
