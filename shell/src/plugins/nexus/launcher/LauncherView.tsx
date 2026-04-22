// src/plugins/nexus/launcher/LauncherView.tsx
//
// Obsidian-style workspace picker. Renders as a full-window overlay
// (minus the titlebar strip, which stays interactive so window drag
// and close controls keep working) whenever no workspace is open.
// Returns null when a root path is set, hiding itself for the rest
// of the session.

import { useState } from 'react'
import { useWorkspaceStore } from '../workspace/workspaceStore'
import { useLauncherStore } from './launcherState'

// Height of the shell titlebar (matches .shell-titlebar in shell.css).
// The launcher starts below it so window drag + close keep working.
const TITLEBAR_HEIGHT = 36

interface LauncherViewProps {
  onOpenFolder: () => void
  onActivatePath: (path: string) => void
}

function basename(path: string): string {
  // Handle both POSIX and Windows separators
  const trimmed = path.replace(/[\\/]+$/, '')
  const lastSlash = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'))
  return lastSlash >= 0 ? trimmed.slice(lastSlash + 1) : trimmed
}

function RecentsList({ onActivate }: { onActivate: (path: string) => void }) {
  const recents = useLauncherStore((s) => s.recents)
  const forget = useLauncherStore((s) => s.forgetPath)
  const [menuForPath, setMenuForPath] = useState<string | null>(null)

  if (recents.length === 0) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          height: '100%',
          color: 'var(--fg-dim)',
          fontFamily: 'var(--f-ui)',
          fontSize: 'var(--ui-size, 13px)',
        }}
      >
        No recent workspaces
      </div>
    )
  }

  return (
    <div
      style={{
        overflowY: 'auto',
        padding: '12px 0',
        height: '100%',
      }}
    >
      {recents.map((path) => (
        <div
          key={path}
          onClick={() => onActivate(path)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '8px 16px',
            cursor: 'pointer',
            fontFamily: 'var(--f-ui)',
            position: 'relative',
          }}
          onMouseEnter={(e) => {
            (e.currentTarget as HTMLDivElement).style.background = 'var(--bg-hover)'
          }}
          onMouseLeave={(e) => {
            (e.currentTarget as HTMLDivElement).style.background = 'transparent'
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <div
              style={{
                color: 'var(--fg)',
                fontSize: 'var(--ui-size, 13px)',
                fontWeight: 600,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {basename(path)}
            </div>
            <div
              style={{
                color: 'var(--fg-muted)',
                fontSize: 11,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                marginTop: 2,
              }}
            >
              {path}
            </div>
          </div>

          <button
            onClick={(e) => {
              e.stopPropagation()
              setMenuForPath(menuForPath === path ? null : path)
            }}
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--fg-muted)',
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

          {menuForPath === path && (
            <div
              style={{
                position: 'absolute',
                top: '100%',
                right: 12,
                background: 'var(--bg-raised)',
                border: '1px solid var(--line)',
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
                  setMenuForPath(null)
                  void forget(path)
                }}
                style={{
                  display: 'block',
                  width: '100%',
                  textAlign: 'left',
                  background: 'transparent',
                  border: 'none',
                  color: 'var(--fg)',
                  padding: '6px 10px',
                  borderRadius: 4,
                  cursor: 'pointer',
                  fontSize: 'var(--ui-size, 13px)',
                  fontFamily: 'var(--f-ui)',
                }}
                onMouseEnter={(e) => {
                  (e.currentTarget as HTMLButtonElement).style.background = 'var(--bg-hover)'
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
          background: 'var(--accent)',
          color: 'var(--accent-ink)',
        }
      : variant === 'disabled'
      ? {
          background: 'var(--bg-hover)',
          color: 'var(--fg)',
          opacity: 0.5,
          cursor: 'not-allowed' as const,
        }
      : {
          background: 'var(--bg-hover)',
          color: 'var(--fg)',
        }

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 16,
        padding: '14px 18px',
        borderBottom: '1px solid var(--line-soft)',
      }}
    >
      <div style={{ flex: 1, minWidth: 0 }}>
        <div
          style={{
            color: 'var(--fg)',
            fontSize: 'var(--ui-size, 13px)',
            fontWeight: 600,
            marginBottom: 3,
          }}
        >
          {heading}
        </div>
        <div
          style={{
            color: 'var(--fg-muted)',
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
          borderRadius: 'var(--r)',
          padding: '6px 16px',
          fontSize: 'var(--ui-size, 13px)',
          fontFamily: 'var(--f-ui)',
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

export function LauncherView({ onOpenFolder, onActivatePath }: LauncherViewProps) {
  const rootPath = useWorkspaceStore((s) => s.rootPath)
  const manageReturnTo = useLauncherStore((s) => s.manageReturnTo)
  const clearReturnTo = useLauncherStore((s) => s.setManageReturnTo)

  // Once the user has a workspace open the launcher disappears. The
  // shell's regular tri-pane layout takes over. This is cheaper than
  // unmounting the plugin — keeping it in the slot lets it reappear
  // instantly if the workspace is later closed.
  if (rootPath) return null

  const onDismiss = manageReturnTo
    ? () => {
        const target = manageReturnTo
        clearReturnTo(null)
        void onActivatePath(target)
      }
    : null

  return (
    <div
      style={{
        position: 'fixed',
        top: TITLEBAR_HEIGHT,
        left: 0,
        right: 0,
        bottom: 0,
        // Below the shell overlay ceiling (9999) and above every pane,
        // but leaves the 36px titlebar strip uncovered so Tauri drag
        // and window controls keep working.
        zIndex: 9000,
        display: 'flex',
        pointerEvents: 'auto',
        background: 'var(--bg)',
        color: 'var(--fg)',
        fontFamily: 'var(--f-ui)',
      }}
    >
      {onDismiss && (
        <button
          type="button"
          aria-label="Close launcher"
          title="Return to current forge"
          onClick={onDismiss}
          style={{
            position: 'absolute',
            top: 12,
            right: 16,
            width: 28,
            height: 28,
            display: 'inline-grid',
            placeItems: 'center',
            background: 'transparent',
            border: 'none',
            color: 'var(--fg-muted)',
            cursor: 'pointer',
            borderRadius: 4,
            fontSize: 18,
            lineHeight: 1,
            zIndex: 1,
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.background = 'var(--bg-hover)'
            e.currentTarget.style.color = 'var(--fg)'
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.background = 'transparent'
            e.currentTarget.style.color = 'var(--fg-muted)'
          }}
        >
          ✕
        </button>
      )}
      {/* Left column — recents */}
      <div
        style={{
          width: '33%',
          minWidth: 240,
          maxWidth: 420,
          background: 'var(--bg)',
          borderRight: '1px solid var(--line-soft)',
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
            color: 'var(--fg-muted)',
            flexShrink: 0,
          }}
        >
          Recent
        </div>
        <div style={{ flex: 1, minHeight: 0 }}>
          <RecentsList onActivate={onActivatePath} />
        </div>
      </div>

      {/* Right column — title + actions */}
      <div
        style={{
          flex: 1,
          background: 'var(--bg-raised)',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          padding: 40,
          overflowY: 'auto',
        }}
      >
        <div style={{ width: '100%', maxWidth: 560 }}>
          <h1
            style={{
              color: 'var(--accent)',
              fontFamily: 'var(--f-ui)',
              fontSize: 32,
              fontWeight: 600,
              textAlign: 'center',
              marginBottom: 32,
              letterSpacing: '-0.01em',
            }}
          >
            Nexus
          </h1>

          <div
            style={{
              background: 'var(--bg)',
              border: '1px solid var(--line-soft)',
              borderRadius: 'var(--r-lg)',
              overflow: 'hidden',
            }}
          >
            <ActionRow
              heading="Create new workspace"
              description="Create a new Nexus workspace under a folder."
              buttonLabel="Create"
              variant="accent"
              onClick={onOpenFolder}
            />
            <ActionRow
              heading="Open folder as workspace"
              description="Choose an existing folder."
              buttonLabel="Open"
              variant="neutral"
              onClick={onOpenFolder}
            />
            {/* Last row — strip the bottom border */}
            <div style={{ borderBottom: 0 }}>
              <ActionRow
                heading="Clone from git"
                description="Clone a remote workspace."
                buttonLabel="Sign in"
                variant="disabled"
                disabled
              />
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
