import { useCallback } from 'react'

import { getGitPanelApi } from '../gitPanelRuntime'

const GIT_ID = 'com.nexus.git'

/**
 * BL-084 status banner shown atop the git panel whenever the
 * repository is in a non-`Clean` operation state (Merge / Rebase /
 * RebaseInteractive / CherryPick / Revert / Bisect). Provides the
 * `Abort` button routing to the right handler for the active
 * operation; resolution itself happens in the per-file
 * `ConflictView` reached by selecting a `Conflicted` file from the
 * Changes tab.
 *
 * Revert and Bisect have no abort handlers in `com.nexus.git` today,
 * so for those states we render a status-only banner without an
 * abort button rather than offering a handler that doesn't exist.
 */
export function ConflictBanner({
  repoState,
  conflictCount,
}: {
  repoState: string
  conflictCount: number
}) {
  const abort = useAbortHandler(repoState)

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 10px',
        background: 'var(--background-modifier-error-rgb, rgba(229,62,62,0.12))',
        borderBottom: '1px solid var(--background-modifier-border)',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        flexShrink: 0,
      }}
    >
      <strong style={{ color: 'var(--text-error, #c53030)' }}>{labelFor(repoState)}</strong>
      <span style={{ color: 'var(--text-muted)' }}>
        {conflictCount > 0
          ? `${conflictCount} unresolved file${conflictCount === 1 ? '' : 's'}`
          : 'No unresolved files — stage and commit to finish.'}
      </span>
      {abort && (
        <button
          type="button"
          onClick={abort.run}
          style={{
            marginLeft: 'auto',
            padding: '3px 10px',
            fontSize: 12,
            fontFamily: 'var(--font-interface)',
            background: 'var(--interactive-normal)',
            color: 'var(--text-normal)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 3,
            cursor: 'pointer',
          }}
        >
          {abort.label}
        </button>
      )}
    </div>
  )
}

function labelFor(state: string): string {
  switch (state) {
    case 'Merge':
      return 'Merge in progress'
    case 'Rebase':
    case 'RebaseInteractive':
      return 'Rebase in progress'
    case 'CherryPick':
      return 'Cherry-pick in progress'
    case 'Revert':
      return 'Revert in progress'
    case 'Bisect':
      return 'Bisect in progress'
    default:
      return state
  }
}

interface AbortHandler {
  label: string
  run: () => void
}

function useAbortHandler(state: string): AbortHandler | null {
  const callMerge = useCallback(() => {
    void getGitPanelApi().kernel.invoke(GIT_ID, 'abort_merge', {})
  }, [])
  const callRebase = useCallback(() => {
    void getGitPanelApi().kernel.invoke(GIT_ID, 'abort_rebase', {})
  }, [])
  const callCherry = useCallback(() => {
    void getGitPanelApi().kernel.invoke(GIT_ID, 'abort_cherry_pick', {})
  }, [])

  switch (state) {
    case 'Merge':
      return { label: 'Abort merge', run: callMerge }
    case 'Rebase':
    case 'RebaseInteractive':
      return { label: 'Abort rebase', run: callRebase }
    case 'CherryPick':
      return { label: 'Abort cherry-pick', run: callCherry }
    // Revert / Bisect have no IPC-exposed abort today; render the
    // banner without a button rather than offer something we can't
    // wire up.
    default:
      return null
  }
}
