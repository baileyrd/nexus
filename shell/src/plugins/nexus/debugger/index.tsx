// shell/src/plugins/nexus/debugger/index.tsx
//
// BL-081 — `nexus.debugger` plugin.
//
// Sidebar leaf with toolbar + Call Stack + Variables + Watch +
// Breakpoints + Output sections. Subscribes to `com.nexus.dap.*`
// events on activation and dispatches store actions in response.

import { createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'

import type { Plugin, PluginAPI, KernelEventEnvelope } from '../../../types/plugin'
import { ViewBase, workspace, type Leaf } from '../../../workspace'
import { DebuggerPanel } from './DebuggerPanel'
import { useDebuggerStore } from './debuggerStore'
import type { DapKernelAPI } from './debuggerIpc'
import './debugger.css'

const VIEW_TYPE = 'debugger-panel'
const COMMAND_FOCUS = 'nexus.debugger.focus'

const TOPIC_INITIALIZED = 'com.nexus.dap.initialized'
const TOPIC_STOPPED = 'com.nexus.dap.stopped'
const TOPIC_CONTINUED = 'com.nexus.dap.continued'
const TOPIC_TERMINATED = 'com.nexus.dap.terminated'
const TOPIC_EXITED = 'com.nexus.dap.exited'
const TOPIC_THREAD = 'com.nexus.dap.thread'
const TOPIC_OUTPUT = 'com.nexus.dap.output'

class DebuggerPaneView extends ViewBase {
  readonly viewType = VIEW_TYPE
  private root: Root | null = null
  private readonly render: () => React.ReactElement

  constructor(leaf: Leaf, render: () => React.ReactElement) {
    super(leaf)
    this.render = render
  }

  onOpen(el: HTMLElement): void {
    this.root = createRoot(el)
    this.root.render(this.render())
  }

  onClose(): void {
    this.root?.unmount()
    this.root = null
  }
}

export const debuggerPlugin: Plugin = {
  manifest: {
    id: 'nexus.debugger',
    name: 'Debugger',
    version: '0.1.0',
    core: false,
    activationEvents: [`onCommand:${COMMAND_FOCUS}`, `onView:${VIEW_TYPE}`],
    contributes: {
      commands: [
        {
          id: COMMAND_FOCUS,
          title: 'Debugger: Focus Panel',
          category: 'Debug',
        },
      ],
      keybindings: [
        // VS Code's "Run and Debug" muscle memory.
        {
          command: COMMAND_FOCUS,
          key: 'ctrl+shift+d',
          mac: 'cmd+shift+d',
        },
      ],
    },
  },

  activate(api: PluginAPI) {
    const dapKernel = api.kernel as unknown as DapKernelAPI

    api.viewRegistry.register(VIEW_TYPE, (leaf) =>
      new DebuggerPaneView(leaf, () =>
        createElement(DebuggerPanel, { kernel: api.kernel }),
      ),
    )

    api.commands.register(COMMAND_FOCUS, async () => {
      const leaf = await workspace.ensureLeafOfType(VIEW_TYPE, 'right')
      workspace.revealLeaf(leaf)
    })

    // ── DAP event bridge ────────────────────────────────────────────────
    // Every `com.nexus.dap.<event>` notification arrives as a kernel
    // event whose `payload` is the adapter's original event body.
    const store = useDebuggerStore.getState

    api.events.on(TOPIC_INITIALIZED, () => {
      // Adapter is ready to receive breakpoints; the store's
      // `startSession` already issued them, so just mark running.
    })

    api.events.on(TOPIC_STOPPED, (env: KernelEventEnvelope) => {
      const body = env.payload as
        | { reason?: string; threadId?: number; description?: string }
        | undefined
      const tid = body?.threadId ?? store().currentThread ?? 1
      const reason = body?.reason ?? 'paused'
      void store().refreshAfterStop(dapKernel, tid, reason)
    })

    api.events.on(TOPIC_CONTINUED, () => {
      // After `continued` we have no current frame until the next
      // stop — clearing keeps the panel honest.
      useDebuggerStore.setState({
        stoppedReason: null,
        currentFrame: null,
        frames: [],
        scopes: [],
      })
    })

    api.events.on(TOPIC_TERMINATED, () => {
      store().markTerminated()
    })

    api.events.on(TOPIC_EXITED, (env: KernelEventEnvelope) => {
      const body = env.payload as { exitCode?: number } | undefined
      if (body?.exitCode != null) {
        store().pushOutput('exited', `Process exited with code ${body.exitCode}\n`)
      }
      store().markTerminated()
    })

    api.events.on(TOPIC_THREAD, (env: KernelEventEnvelope) => {
      const body = env.payload as
        | { reason?: string; threadId?: number }
        | undefined
      if (body?.reason === 'exited' && body.threadId != null) {
        const tid = body.threadId
        useDebuggerStore.setState((s) => ({
          threads: s.threads.filter((t) => t.id !== tid),
        }))
      }
    })

    api.events.on(TOPIC_OUTPUT, (env: KernelEventEnvelope) => {
      const body = env.payload as
        | { category?: string; output?: string }
        | undefined
      if (body?.output != null) {
        store().pushOutput(body.category ?? 'console', body.output)
      }
    })

    api.events.on('workspace:closed', () => {
      store().reset()
    })
  },
}
