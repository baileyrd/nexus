// shell/src/plugins/nexus/debugger/DebuggerPanel.tsx
//
// BL-081 — sidebar debugger panel.
//
// Sections: toolbar (Continue / Step Over / Step In / Step Out /
// Pause / Stop) → status row → Threads + Stack → Scopes + Variables
// → Watch → Breakpoints → Output. The shape mirrors VS Code's "Run
// and Debug" sidebar so the muscle-memory transfers; everything is
// driven through the `debuggerStore` actions, which dispatch through
// the typed IPC layer in `debuggerIpc.ts`.

import { useCallback, useEffect, useState } from 'react'

import type { KernelAPI } from '../../../types/plugin'
import { useDebuggerStore } from './debuggerStore'
import type { DapKernelAPI } from './debuggerIpc'
import { LaunchConfig } from './LaunchConfig'

interface DebuggerPanelProps {
  kernel: KernelAPI
}

export function DebuggerPanel({ kernel }: DebuggerPanelProps) {
  const dapKernel = kernel as unknown as DapKernelAPI
  const activeAdapter = useDebuggerStore((s) => s.activeAdapter)
  const running = useDebuggerStore((s) => s.running)
  const stoppedReason = useDebuggerStore((s) => s.stoppedReason)
  const threads = useDebuggerStore((s) => s.threads)
  const frames = useDebuggerStore((s) => s.frames)
  const scopesList = useDebuggerStore((s) => s.scopes)
  const variablesByRef = useDebuggerStore((s) => s.variablesByRef)
  const breakpointsByPath = useDebuggerStore((s) => s.breakpointsByPath)
  const watches = useDebuggerStore((s) => s.watches)
  const output = useDebuggerStore((s) => s.output)
  const error = useDebuggerStore((s) => s.error)

  const doContinue = useDebuggerStore((s) => s.doContinue)
  const doNext = useDebuggerStore((s) => s.doNext)
  const doStepIn = useDebuggerStore((s) => s.doStepIn)
  const doStepOut = useDebuggerStore((s) => s.doStepOut)
  const doPause = useDebuggerStore((s) => s.doPause)
  const endSession = useDebuggerStore((s) => s.endSession)
  const loadVariables = useDebuggerStore((s) => s.loadVariables)
  const addWatch = useDebuggerStore((s) => s.addWatch)
  const removeWatch = useDebuggerStore((s) => s.removeWatch)
  const clearBreakpointsForPath = useDebuggerStore(
    (s) => s.clearBreakpointsForPath,
  )

  const [watchInput, setWatchInput] = useState('')

  // Lazy-load every scope's children on stop. (For deep trees the
  // user opts in by expanding a row — handled inline below.)
  useEffect(() => {
    for (const sc of scopesList) {
      if (variablesByRef[sc.variablesReference] == null) {
        void loadVariables(dapKernel, sc.variablesReference)
      }
    }
  }, [scopesList, variablesByRef, loadVariables, dapKernel])

  const submitWatch = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault()
      const v = watchInput.trim()
      if (v.length === 0) return
      addWatch(v)
      setWatchInput('')
    },
    [watchInput, addWatch],
  )

  const idle = activeAdapter == null
  const stopped = !idle && stoppedReason != null

  if (idle) {
    // BL-113 follow-up — launch picker + launch-config form takes the
    // panel until a session starts. Toolbar + status row reappear once
    // an adapter is active.
    return (
      <div className="nx-debugger-panel">
        <LaunchConfig api={dapKernel} />
      </div>
    )
  }

  return (
    <div className="nx-debugger-panel">
      <div className="nx-debugger-toolbar" role="toolbar" aria-label="Debug">
        <button
          type="button"
          disabled={!stopped}
          onClick={() => void doContinue(dapKernel)}
          title="Continue (F5)"
        >
          ▶
        </button>
        <button
          type="button"
          disabled={!stopped}
          onClick={() => void doNext(dapKernel)}
          title="Step Over (F10)"
        >
          ⤼
        </button>
        <button
          type="button"
          disabled={!stopped}
          onClick={() => void doStepIn(dapKernel)}
          title="Step In (F11)"
        >
          ↓
        </button>
        <button
          type="button"
          disabled={!stopped}
          onClick={() => void doStepOut(dapKernel)}
          title="Step Out (Shift+F11)"
        >
          ↑
        </button>
        <button
          type="button"
          disabled={idle || stopped}
          onClick={() => void doPause(dapKernel)}
          title="Pause"
        >
          ⏸
        </button>
        <button
          type="button"
          disabled={idle}
          onClick={() => void endSession(dapKernel)}
          title="Stop"
        >
          ■
        </button>
      </div>

      <div className="nx-debugger-status">
        {idle && <span className="nx-debugger-status-idle">No session.</span>}
        {!idle && running && stoppedReason == null && (
          <span className="nx-debugger-status-running">
            Running · {activeAdapter}
          </span>
        )}
        {!idle && stoppedReason != null && (
          <span className="nx-debugger-status-stopped">
            Stopped · {stoppedReason}
          </span>
        )}
        {error && <span className="nx-debugger-error">{error}</span>}
      </div>

      <Section title="Threads" count={threads.length}>
        {threads.length === 0 ? (
          <Empty>No threads.</Empty>
        ) : (
          <ul className="nx-debugger-list">
            {threads.map((t) => (
              <li key={t.id}>
                <span className="nx-debugger-list-key">#{t.id}</span> {t.name}
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Call Stack" count={frames.length}>
        {frames.length === 0 ? (
          <Empty>No frames.</Empty>
        ) : (
          <ul className="nx-debugger-list">
            {frames.map((f) => (
              <li key={f.id}>
                <span className="nx-debugger-list-key">{f.name}</span>
                <span className="nx-debugger-list-meta">
                  {f.source?.path ?? f.source?.name ?? '?'}:{f.line}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Variables" count={scopesList.length}>
        {scopesList.length === 0 ? (
          <Empty>No active frame.</Empty>
        ) : (
          scopesList.map((sc) => {
            const vars = variablesByRef[sc.variablesReference] ?? []
            return (
              <div key={sc.variablesReference} className="nx-debugger-scope">
                <div className="nx-debugger-scope-title">{sc.name}</div>
                {vars.length === 0 ? (
                  <Empty>—</Empty>
                ) : (
                  <ul className="nx-debugger-vars">
                    {vars.map((v) => (
                      <li key={`${sc.variablesReference}:${v.name}`}>
                        <span className="nx-debugger-var-name">{v.name}</span>
                        <span className="nx-debugger-var-eq"> = </span>
                        <span className="nx-debugger-var-value">{v.value}</span>
                        {v.type && (
                          <span className="nx-debugger-var-type">
                            {' '}({v.type})
                          </span>
                        )}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )
          })
        )}
      </Section>

      <Section title="Watch" count={watches.length}>
        <form onSubmit={submitWatch} className="nx-debugger-watch-form">
          <input
            type="text"
            value={watchInput}
            onChange={(e) => setWatchInput(e.target.value)}
            placeholder="Add watch expression…"
            aria-label="Watch expression"
          />
          <button type="submit">+</button>
        </form>
        {watches.length === 0 ? (
          <Empty>No watches.</Empty>
        ) : (
          <ul className="nx-debugger-list">
            {watches.map((w) => (
              <li key={w.expression}>
                <span className="nx-debugger-list-key">{w.expression}</span>
                <span className="nx-debugger-list-value">
                  {w.error ? `error: ${w.error}` : w.value ?? '—'}
                </span>
                <button
                  type="button"
                  className="nx-debugger-row-remove"
                  onClick={() => removeWatch(w.expression)}
                  title="Remove"
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section
        title="Breakpoints"
        count={Object.values(breakpointsByPath).reduce(
          (n, l) => n + l.length,
          0,
        )}
      >
        {Object.keys(breakpointsByPath).length === 0 ? (
          <Empty>No breakpoints.</Empty>
        ) : (
          <ul className="nx-debugger-list">
            {Object.entries(breakpointsByPath).map(([path, lines]) =>
              lines.map((bp) => (
                <li key={`${path}:${bp.line}`}>
                  <span className="nx-debugger-list-key">{path}</span>
                  <span className="nx-debugger-list-meta">:{bp.line}</span>
                </li>
              )),
            )}
            <li>
              <button
                type="button"
                className="nx-debugger-row-remove"
                onClick={() => {
                  for (const p of Object.keys(breakpointsByPath)) {
                    void clearBreakpointsForPath(dapKernel, p)
                  }
                }}
              >
                Clear all
              </button>
            </li>
          </ul>
        )}
      </Section>

      <Section title="Output" count={output.length}>
        {output.length === 0 ? (
          <Empty>—</Empty>
        ) : (
          <pre className="nx-debugger-output">
            {output.map((o) => `[${o.category}] ${o.text}`).join('')}
          </pre>
        )}
      </Section>
    </div>
  )
}

function Section({
  title,
  count,
  children,
}: {
  title: string
  count?: number
  children: React.ReactNode
}) {
  return (
    <div className="nx-debugger-section">
      <div className="nx-debugger-section-header">
        <span>{title}</span>
        {count != null && (
          <span className="nx-debugger-section-count">{count}</span>
        )}
      </div>
      <div className="nx-debugger-section-body">{children}</div>
    </div>
  )
}

function Empty({ children }: { children: React.ReactNode }) {
  return <div className="nx-debugger-empty">{children}</div>
}
