// Forge-styled editor area: a .forge-tabbar across the top + a .surface
// hosting the Doc renderer. Tabs carry a dirty dot, hover-only close
// button, and top accent bar on the active tab — matching docs/test.

import { useEffect, useState } from 'react'
import { readTextFile } from '@tauri-apps/plugin-fs'
import { useEditorStore, type EditorTab } from './editorStore'
import { useContextKeyStore } from '../../../host/ContextKeyService'
import { useDocStore } from '../../../stores/docStore'
import { Ic } from '../../../shell/icons'
import { MarkdownDoc } from './MarkdownDoc'

export function EditorAreaView() {
  const { tabs, activeTabId, closeTab, setActiveTab } = useEditorStore()
  const activeTab = tabs.find(t => t.id === activeTabId) ?? null

  return (
    <div
      className="forge-center"
      onClick={() => useContextKeyStore.getState().set('editorFocus', true)}
    >
      {tabs.length === 0 ? (
        <>
          <div className="forge-tabbar" />
          <WelcomeSurface />
        </>
      ) : (
        <>
          <TabBar
            tabs={tabs}
            activeTabId={activeTabId}
            onActivate={setActiveTab}
            onClose={closeTab}
          />
          {activeTab && <DocSurface tab={activeTab} />}
        </>
      )}
    </div>
  )
}

function TabBar({ tabs, activeTabId, onActivate, onClose }: {
  tabs: EditorTab[]
  activeTabId: string | null
  onActivate: (id: string) => void
  onClose: (id: string) => void
}) {
  return (
    <div className="forge-tabbar">
      {tabs.map(tab => (
        <div
          key={tab.id}
          className={'forge-tab ' + (tab.id === activeTabId ? 'active' : '')}
          onClick={() => onActivate(tab.id)}
          onAuxClick={e => { if (e.button === 1) onClose(tab.id) }}
        >
          <Ic.doc className="ficon" />
          <span className="tname">{tab.title}</span>
          {tab.isDirty && <span className="dirty" />}
          <span
            className="x"
            onClick={e => { e.stopPropagation(); onClose(tab.id) }}
            title="Close"
          >
            <Ic.x style={{ width: 10, height: 10 }} />
          </span>
        </div>
      ))}
      <div className="tab-plus" title="New tab"><Ic.plus style={{ width: 14, height: 14 }} /></div>
    </div>
  )
}

function WelcomeSurface() {
  return (
    <div className="forge-welcome">
      <div style={{ textAlign: 'center' }}>
        <h2>Forge</h2>
        <p>Open a file from the sidebar to begin</p>
        <p style={{ marginTop: 14 }}>Press <kbd>⌘⇧P</kbd> for the command palette</p>
      </div>
    </div>
  )
}

function DocSurface({ tab }: { tab: EditorTab }) {
  const [contents, setContents] = useState<string | null>(null)
  const [error, setError]       = useState<string | null>(null)
  const setHeadings             = useDocStore(s => s.setHeadings)
  const setActiveHeading        = useDocStore(s => s.setActiveHeading)

  useEffect(() => {
    let cancelled = false
    setContents(null); setError(null)
    readTextFile(tab.path)
      .then(t => { if (!cancelled) setContents(t) })
      .catch(e => { if (!cancelled) setError(String(e?.message ?? e)) })
    return () => { cancelled = true }
  }, [tab.path])

  // Clear outline when switching tabs until the new file's headings arrive.
  useEffect(() => { setHeadings([]); setActiveHeading(null) }, [tab.id, setHeadings, setActiveHeading])

  const isMarkdown = /\.(md|markdown|mdx)$/i.test(tab.path)

  return (
    <div className="surface">
      {error ? (
        <div className="doc"><p style={{ color: 'var(--risk)' }}>Failed to load: {error}</p></div>
      ) : contents == null ? (
        <div className="doc"><p style={{ color: 'var(--text-faint)' }}>Loading…</p></div>
      ) : isMarkdown ? (
        <MarkdownDoc
          source={contents}
          title={tab.title.replace(/\.(md|markdown|mdx)$/i, '')}
          onHeadings={setHeadings}
          onActiveHeading={setActiveHeading}
        />
      ) : (
        <div className="doc">
          <div className="title">{tab.title}</div>
          <div className="metaline">
            <span className="chip">plain</span>
            <span>{tab.path}</span>
          </div>
          <pre><code>{contents}</code></pre>
        </div>
      )}
    </div>
  )
}
