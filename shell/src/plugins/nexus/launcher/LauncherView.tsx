// src/plugins/nexus/launcher/LauncherView.tsx
//
// Obsidian-style workspace picker. Renders as a centered modal dialog
// over a dimmed backdrop whenever no workspace is open. The titlebar
// strip stays interactive (window drag + window controls keep working).
// Returns null when a root path is set, hiding itself for the rest of
// the session.

import { useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useLauncherStore, type RemoteForgeRecent } from './launcherState'
import { Modal } from '../../../shell/Modal'
import { zIndex } from '../../../shell/zIndex'

// Height of the shell titlebar (matches .shell-titlebar in shell.css).
// The launcher starts below it so window drag + close keep working.
const TITLEBAR_HEIGHT = 36

interface LauncherViewProps {
  onOpenFolder: () => void
  /** BL-054 Phase 1 follow-up: pick a folder + scaffold the OS layout
   *  before booting. Surfaced as the "Create OS workspace" action. */
  onOpenWithOsTemplate: () => void
  /** BL-148: open the remote-connection dialog. */
  onOpenRemote: () => void
  onActivatePath: (path: string) => void
  onActivateRemote: (entry: RemoteForgeRecent) => void
}

interface RecentRowEntry {
  key: string
  primary: string
  secondary: string
  isRemote: boolean
}

function basename(path: string): string {
  // Handle both POSIX and Windows separators
  const trimmed = path.replace(/[\\/]+$/, '')
  const lastSlash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  return lastSlash >= 0 ? trimmed.slice(lastSlash + 1) : trimmed
}

function RecentsList({
  onActivate,
  onActivateRemote,
}: {
  onActivate: (path: string) => void
  onActivateRemote: (entry: RemoteForgeRecent) => void
}) {
  const recents = useLauncherStore((s) => s.recents)
  const remoteRecents = useLauncherStore((s) => s.remoteRecents)
  const forget = useLauncherStore((s) => s.forgetPath)
  const forgetRemote = useLauncherStore((s) => s.forgetRemote)
  const [menuForKey, setMenuForKey] = useState<string | null>(null)

  const entries: RecentRowEntry[] = [
    ...recents.map((p) => ({
      key: `local:${p}`,
      primary: basename(p),
      secondary: p,
      isRemote: false,
    })),
    ...remoteRecents.map((r) => ({
      key: `remote:${r.uri}`,
      primary: r.label?.trim() ? r.label : r.uri.replace(/^ssh:\/\//, ''),
      secondary: r.uri,
      isRemote: true,
    })),
  ]

  if (entries.length === 0) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          color: 'var(--text-faint)',
          fontFamily: 'var(--font-interface)',
          fontSize: 'var(--ui-size, 13px)',
        }}
      >
        No recent workspaces
      </div>
    )
  }

  const activate = (entry: RecentRowEntry) => {
    if (entry.isRemote) {
      const match = remoteRecents.find((r) => r.uri === entry.secondary)
      if (match) onActivateRemote(match)
    } else {
      onActivate(entry.secondary)
    }
  }

  const forgetEntry = (entry: RecentRowEntry) => {
    if (entry.isRemote) {
      void forgetRemote(entry.secondary)
    } else {
      void forget(entry.secondary)
    }
  }

  return (
    <div
      style={{
        overflowY: 'auto',
        padding: '12px 0',
        height: '100%',
      }}
    >
      {entries.map((entry) => (
        <div
          key={entry.key}
          onClick={() => activate(entry)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '8px 16px',
            cursor: 'pointer',
            fontFamily: 'var(--font-interface)',
            position: 'relative',
          }}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLDivElement).style.background = 'var(--background-modifier-hover)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLDivElement).style.background = 'transparent'
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                color: 'var(--text-normal)',
                fontSize: 'var(--ui-size, 13px)',
                fontWeight: 600,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                display: 'flex',
                alignItems: 'center',
                gap: 6,
              }}
            >
              {entry.isRemote && (
                <span
                  aria-label="remote"
                  title="Remote forge"
                  style={{
                    fontSize: 10,
                    fontWeight: 700,
                    letterSpacing: '0.04em',
                    padding: '1px 5px',
                    borderRadius: 3,
                    background: 'var(--background-modifier-hover)',
                    color: 'var(--text-muted)',
                    textTransform: 'uppercase',
                  }}
                >
                  ssh
                </span>
              )}
              <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {entry.primary}
              </span>
            </div>
            <div
              style={{
                color: 'var(--text-muted)',
                fontSize: 11,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                marginTop: 2,
              }}
            >
              {entry.secondary}
            </div>
          </div>

          <button
            onClick={(e) => {
              e.stopPropagation()
              setMenuForKey(menuForKey === entry.key ? null : entry.key)
            }}
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--text-muted)',
              cursor: 'pointer',
              fontSize: 16,
              lineHeight: 1,
              padding: '2px 6px',
              borderRadius: 4,
            }}
            aria-label="More actions"
          >
            ⋯
          </button>

          {menuForKey === entry.key && (
            <div
              style={{
                position: 'absolute',
                top: '100%',
                right: 12,
                background: 'var(--background-secondary)',
                border: '1px solid var(--background-modifier-border)',
                borderRadius: 6,
                boxShadow: 'var(--shadow)',
                zIndex: 1,
                padding: 4,
                minWidth: 180,
              }}
            >
              <button
                onClick={(e) => {
                  e.stopPropagation()
                  setMenuForKey(null)
                  forgetEntry(entry)
                }}
                style={{
                  display: 'block',
                  width: '100%',
                  textAlign: 'left',
                  background: 'transparent',
                  border: 'none',
                  color: 'var(--text-normal)',
                  padding: '6px 10px',
                  borderRadius: 4,
                  cursor: 'pointer',
                  fontSize: 'var(--ui-size, 13px)',
                  fontFamily: 'var(--font-interface)',
                }}
                onMouseEnter={(e) => {
                  (e.currentTarget as HTMLButtonElement).style.background = 'var(--background-modifier-hover)'
                }}
                onMouseLeave={(e) => {
                  (e.currentTarget as HTMLButtonElement).style.background = 'transparent'
                }}
              >
                Remove from recents
              </button>
            </div>
          )}
        </div>
      ))}
    </div>
  )
}

function ActionRow({
  heading,
  description,
  buttonLabel,
  variant,
  disabled,
  onClick,
}: {
  heading: string
  description: string
  buttonLabel: string
  variant: 'accent' | 'neutral' | 'disabled'
  disabled?: boolean
  onClick?: () => void
}) {
  const buttonStyle =
    variant === 'accent'
      ? {
          background: 'var(--interactive-accent)',
          color: 'var(--interactive-accent-ink)',
        }
      : variant === 'disabled'
      ? {
          background: 'var(--background-modifier-hover)',
          color: 'var(--text-normal)',
          opacity: 0.5,
          cursor: 'not-allowed' as const,
        }
      : {
          background: 'var(--background-modifier-hover)',
          color: 'var(--text-normal)',
        }

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 16,
        padding: '14px 18px',
        borderBottom: '1px solid var(--divider-color)',
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            color: 'var(--text-normal)',
            fontSize: 'var(--ui-size, 13px)',
            fontWeight: 600,
            marginBottom: 3,
          }}
        >
          {heading}
        </div>
        <div
          style={{
            color: 'var(--text-muted)',
            fontSize: 12,
          }}
        >
          {description}
        </div>
      </div>
      <button
        onClick={disabled ? undefined : onClick}
        disabled={disabled}
        style={{
          border: 'none',
          borderRadius: 'var(--radius-s)',
          padding: '6px 16px',
          fontSize: 'var(--ui-size, 13px)',
          fontFamily: 'var(--font-interface)',
          fontWeight: 500,
          cursor: disabled ? 'not-allowed' : 'pointer',
          flexShrink: 0,
          ...buttonStyle,
        }}
      >
        {buttonLabel}
      </button>
    </div>
  )
}

export function LauncherView({
  onOpenFolder,
  onOpenWithOsTemplate,
  onOpenRemote,
  onActivatePath,
  onActivateRemote,
}: LauncherViewProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const manageReturnTo = useLauncherStore((s) => s.manageReturnTo)
  const clearReturnTo = useLauncherStore((s) => s.setManageReturnTo)

  // The launcher renders when (a) no workspace is loaded yet — first
  // boot or post-close — or (b) it was explicitly opened over a running
  // workspace via "Manage Forges". In the second case we keep the
  // workspace mounted underneath the modal so per-file editor sessions
  // survive the dismiss.
  if (rootPath && !manageReturnTo) return null

  // Dismiss behaviour:
  //   - If a workspace is mounted underneath, closing the modal is just
  //     clearing the manage-return marker; the workspace remains open.
  //   - Otherwise no workspace is loaded and there is nowhere to return
  //     to; closing the OS window is the user's only escape (mirrors
  //     Obsidian's vault picker, which closes the app on dismiss).
  const onDismiss = manageReturnTo
    ? () => clearReturnTo(null)
    : () => {
        void getCurrentWindow().close()
      }

  // Switching forges from the manage-launcher must also retire the
  // marker — otherwise the modal stays up over the new workspace.
  const onActivate = (path: string) => {
    if (manageReturnTo) clearReturnTo(null)
    onActivatePath(path)
  }
  const onActivateRemoteEntry = (entry: RemoteForgeRecent) => {
    if (manageReturnTo) clearReturnTo(null)
    onActivateRemote(entry)
  }
  const onCreate = () => {
    if (manageReturnTo) clearReturnTo(null)
    onOpenFolder()
  }
  const onCreateOs = () => {
    if (manageReturnTo) clearReturnTo(null)
    onOpenWithOsTemplate()
  }
  const onRemote = () => {
    if (manageReturnTo) clearReturnTo(null)
    onOpenRemote()
  }

  return (
    <Modal>
      {/* Dimmed backdrop — covers the workspace area below the titlebar. */}
      <div
        style={{
          position: 'fixed',
          top: TITLEBAR_HEIGHT,
          left: 0,
          right: 0,
          bottom: 0,
          zIndex: zIndex.overlayModal,
          pointerEvents: 'auto',
          background: 'rgba(0, 0, 0, 0.55)',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontFamily: 'var(--font-interface)',
          color: 'var(--text-normal)',
        }}
      >
        {/* Centered dialog */}
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Open or create a forge"
          style={{
            width: 'min(880px, calc(100vw - 64px))',
            height: 'min(560px, calc(100vh - 96px))',
            display: 'flex',
            background: 'var(--background-primary)',
            border: '1px solid var(--divider-color)',
            borderRadius: 'var(--radius-l)',
            overflow: 'hidden',
            boxShadow: 'var(--shadow), 0 24px 64px rgba(0, 0, 0, 0.45)',
            position: 'relative',
          }}
        >
          <button
            type="button"
            aria-label="Close launcher"
            title={manageReturnTo ? 'Return to current forge' : 'Close window'}
            onClick={onDismiss}
            style={{
              position: 'absolute',
              top: 10,
              right: 12,
              width: 28,
              height: 28,
              display: 'inline-grid',
              placeItems: 'center',
              background: 'transparent',
              border: 'none',
              color: 'var(--text-muted)',
              cursor: 'pointer',
              borderRadius: 4,
              fontSize: 18,
              lineHeight: 1,
              zIndex: 1,
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = 'var(--background-modifier-hover)'
              e.currentTarget.style.color = 'var(--text-normal)'
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = 'transparent'
              e.currentTarget.style.color = 'var(--text-muted)'
            }}
          >
            ✕
          </button>

          {/* Left column — recents */}
          <div
            style={{
              width: '38%',
              minWidth: 220,
              maxWidth: 340,
              background: 'var(--background-primary)',
              borderRight: '1px solid var(--divider-color)',
              display: 'flex',
              flexDirection: 'column',
            }}
          >
            <div
              style={{
                padding: '16px 18px 10px',
                fontSize: 11,
                fontWeight: 700,
                letterSpacing: '0.08em',
                textTransform: 'uppercase',
                color: 'var(--text-muted)',
                flexShrink: 0,
              }}
            >
              Recent
            </div>
            <div style={{ flex: 1, minHeight: 0 }}>
              <RecentsList onActivate={onActivate} onActivateRemote={onActivateRemoteEntry} />
            </div>
          </div>

          {/* Right column — branding + actions */}
          <div
            style={{
              flex: 1,
              background: 'var(--background-secondary)',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              padding: 32,
              overflowY: 'auto',
            }}
          >
            <div style={{ width: '100%', maxWidth: 480 }}>
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'center',
                  marginBottom: 24,
                }}
              >
                <img
                  src="/nexus-logo.png"
                  alt="Nexus"
                  style={{ width: 220, height: 'auto', display: 'block' }}
                />
              </div>

              <div
                style={{
                  background: 'var(--background-primary)',
                  border: '1px solid var(--divider-color)',
                  borderRadius: 'var(--radius-l)',
                  overflow: 'hidden',
                }}
              >
                <ActionRow
                  heading="Create new workspace"
                  description="Create a new Nexus workspace under a folder."
                  buttonLabel="Create"
                  variant="accent"
                  onClick={onCreate}
                />
                <ActionRow
                  heading="Create OS workspace"
                  description="Scaffold the Agentic OS layout (raw / wiki / output / projects / ops) plus a memory-map CLAUDE.md."
                  buttonLabel="Create"
                  variant="neutral"
                  onClick={onCreateOs}
                />
                <ActionRow
                  heading="Open folder as workspace"
                  description="Choose an existing folder."
                  buttonLabel="Open"
                  variant="neutral"
                  onClick={onCreate}
                />
                {/* Last row — strip the bottom border */}
                <div style={{ borderBottom: 0 }}>
                  <ActionRow
                    heading="Open remote forge…"
                    description="Connect to a headless nexus serve over SSH (ssh://user@host/path)."
                    buttonLabel="Connect"
                    variant="neutral"
                    onClick={onRemote}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </Modal>
  )
}
