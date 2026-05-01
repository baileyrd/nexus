// Bottom strip of the left sidedock: current forge name + chevron to
// switch. Mirrors Obsidian's vault-selector affordance. Help and
// Settings affordances live in the activity bar (bottom placement), not
// here, so this row stays focused on forge identity + recents.

import { useEffect, useRef, useState } from 'react'
import { useWorkspaceStore } from '../plugins/nexus/workspace/workspaceStore'
import { useLauncherStore } from '../plugins/nexus/launcher/launcherState'
import { getRegistry } from '../host/shellRegistry'
import { Ic } from '../shell/icons'
import { Modal } from '../shell/Modal'
import { zIndex } from '../shell/zIndex'

const COMMAND_OPEN_FORGE = 'nexus.workspace.open'
const COMMAND_SET_ROOT = 'nexus.workspace.setRoot'
const COMMAND_CLOSE = 'nexus.workspace.close'

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? path
}

export function ForgeSelector(): JSX.Element {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const recents = useLauncherStore((s) => s.recents)
  const [menuOpen, setMenuOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)
  const label = rootPath ? basename(rootPath) : 'Open forge…'

  useEffect(() => {
    if (!menuOpen) return
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (
        menuRef.current?.contains(t ?? null) ||
        anchorRef.current?.contains(t ?? null)
      ) {
        return
      }
      setMenuOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [menuOpen])

  const run = async (commandId: string, arg?: unknown) => {
    const reg = getRegistry()
    if (!reg) return
    await reg.commands.execute(commandId, arg)
  }

  const pickRecent = async (path: string) => {
    setMenuOpen(false)
    if (path === rootPath) return
    await run(COMMAND_SET_ROOT, path)
  }

  // "Manage forges…" drops back into the launcher overlay by closing
  // the current workspace. The launcher auto-shows whenever rootPath
  // is null.
  const openLauncher = async () => {
    setMenuOpen(false)
    // Remember which forge to return to if the user dismisses the
    // launcher without picking something.
    if (rootPath) {
      useLauncherStore.getState().setManageReturnTo(rootPath)
    }
    await run(COMMAND_CLOSE)
  }

  const anchorRect = anchorRef.current?.getBoundingClientRect()

  return (
    <div
      style={{
        flex: '0 0 auto',
        padding: '4px 6px',
        borderTop: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
        background: 'var(--background-secondary, #252526)',
        fontSize: 12,
      }}
    >
      <button
        type="button"
        ref={anchorRef}
        onClick={() => setMenuOpen((v) => !v)}
        title={rootPath ? `Current forge: ${rootPath}` : 'Open a forge'}
        style={{
          width: '100%',
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          background: 'transparent',
          border: 'none',
          color: 'var(--text-normal)',
          cursor: 'pointer',
          padding: '6px 8px',
          borderRadius: 4,
          textAlign: 'left',
          font: 'inherit',
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = 'var(--background-modifier-hover)'
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = 'transparent'
        }}
      >
        <Ic.chev
          width={14}
          height={14}
          style={{ transform: 'rotate(-90deg)', flexShrink: 0, color: 'var(--text-muted)' }}
        />
        <span
          style={{
            flex: '1 1 auto',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {label}
        </span>
      </button>

      {menuOpen && anchorRect && (
        <Modal>
        <div
          ref={menuRef}
          role="menu"
          style={{
            position: 'fixed',
            left: anchorRect.left,
            bottom: window.innerHeight - anchorRect.top + 4,
            minWidth: Math.max(220, anchorRect.width),
            background: 'var(--background-primary, #1e1e1e)',
            border: '1px solid var(--divider-color, var(--background-modifier-border, #333))',
            borderRadius: 6,
            boxShadow: '0 6px 16px rgba(0,0,0,0.4)',
            padding: '4px 0',
            zIndex: zIndex.dropdown,
            fontSize: 12,
            pointerEvents: 'auto',
          }}
        >
          {recents.length === 0 && !rootPath && (
            <div
              style={{
                padding: '8px 12px',
                color: 'var(--text-faint)',
                fontStyle: 'italic',
              }}
            >
              No recent forges
            </div>
          )}
          {recents.map((path) => {
            const active = path === rootPath
            return (
              <button
                key={path}
                type="button"
                onClick={() => void pickRecent(path)}
                title={path}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  width: '100%',
                  background: 'transparent',
                  border: 'none',
                  color: 'var(--text-normal)',
                  cursor: 'pointer',
                  padding: '6px 12px',
                  textAlign: 'left',
                  font: 'inherit',
                }}
                onMouseEnter={(e) => {
                  e.currentTarget.style.background = 'var(--background-modifier-hover)'
                }}
                onMouseLeave={(e) => {
                  e.currentTarget.style.background = 'transparent'
                }}
              >
                <span
                  style={{
                    flex: '1 1 auto',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {basename(path)}
                </span>
                {active && (
                  <Ic.check width={14} height={14} style={{ flexShrink: 0 }} />
                )}
              </button>
            )
          })}
          {(recents.length > 0 || rootPath) && (
            <div
              style={{
                height: 1,
                margin: '4px 0',
                background: 'var(--divider-color, var(--background-modifier-border, #333))',
              }}
            />
          )}
          <button
            type="button"
            onClick={() => void openLauncher()}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              width: '100%',
              background: 'transparent',
              border: 'none',
              color: 'var(--text-normal)',
              cursor: 'pointer',
              padding: '6px 12px',
              textAlign: 'left',
              font: 'inherit',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--background-modifier-hover)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent'
            }}
          >
            <Ic.folder width={14} height={14} style={{ flexShrink: 0, opacity: 0.8 }} />
            <span>Manage forges…</span>
          </button>
          <button
            type="button"
            onClick={() => {
              setMenuOpen(false)
              void run(COMMAND_OPEN_FORGE)
            }}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              width: '100%',
              background: 'transparent',
              border: 'none',
              color: 'var(--text-normal)',
              cursor: 'pointer',
              padding: '6px 12px',
              textAlign: 'left',
              font: 'inherit',
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--background-modifier-hover)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent'
            }}
          >
            <Ic.plus width={14} height={14} style={{ flexShrink: 0, opacity: 0.8 }} />
            <span>Open folder as forge…</span>
          </button>
        </div>
        </Modal>
      )}
    </div>
  )
}
