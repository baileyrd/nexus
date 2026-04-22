// Forge-styled top bar: three-column grid [cluster | breadcrumb | win-controls].
// The breadcrumb carries a sync dot + the active document name; both will wire
// up to real state once the Doc workspace lands. Window controls use the
// custom Tauri titlebar; the central strip is a drag region.

import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { Ic } from '../../../shell/icons'

export function TitleBarView() {
  const [isMaximized, setIsMaximized] = useState(false)

  useEffect(() => {
    const win = getCurrentWindow()
    win.isMaximized().then(setIsMaximized)
    let unlisten: (() => void) | undefined
    win.onResized(async () => setIsMaximized(await win.isMaximized()))
      .then(fn => { unlisten = fn })
    return () => { unlisten?.() }
  }, [])

  const win = getCurrentWindow()

  return (
    <div className="forge-topbar" data-tauri-drag-region>
      <div className="cluster">
        <button className="icon-btn" title="Open folder"><Ic.folder /></button>
        <button className="icon-btn" title="Search"><Ic.search /></button>
        <button className="icon-btn" title="Bookmarks"><Ic.bookmark /></button>
      </div>

      <div className="breadcrumb" data-tauri-drag-region>
        <span className="sync" title="forge synced" />
        <span>Workspace</span>
        <span style={{ color: 'var(--fg-dim)' }}>/</span>
        <b>Untitled</b>
      </div>

      <div className="win-controls">
        <button className="icon-btn" title="Tweaks"><Ic.sliders /></button>
        <button className="icon-btn" title="Backlinks"><Ic.link /></button>
        <button className="icon-btn" title="Right panel"><Ic.panel /></button>
        <div style={{ width: 14 }} />
        <button className="icon-btn" onClick={() => win.minimize()} title="Minimize"><Ic.min /></button>
        <button
          className="icon-btn"
          onClick={() => (isMaximized ? win.unmaximize() : win.maximize())}
          title={isMaximized ? 'Restore' : 'Maximize'}
        >
          <Ic.max />
        </button>
        <button className="icon-btn" onClick={() => win.close()} title="Close"><Ic.x /></button>
      </div>
    </div>
  )
}
