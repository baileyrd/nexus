import { useCallback, useEffect, useRef } from 'react'
import { useGitStatusStore } from '../gitStatus/gitStatusStore'
import {
  useGitPanelStore,
  type DiffHunk,
  type GitFileEntry,
  type BranchEntry,
  type LogEntry,
  type StashEntry,
} from './gitPanelStore'
import { getGitPanelApi } from './gitPanelRuntime'
import { ConflictView } from './conflict/ConflictView'
import { ConflictBanner } from './conflict/ConflictBanner'

const GIT_ID = 'com.nexus.git'

// ── Data-loading helpers ──────────────────────────────────────────────────────

async function loadFiles(): Promise<void> {
  const s = useGitPanelStore.getState()
  s.setLoadingFiles(true)
  try {
    const api = getGitPanelApi()
    const files = await api.kernel.invoke<GitFileEntry[]>(GIT_ID, 'file_statuses', {})
    s.setFiles(files)
    // Clear stale diff when file list refreshes
    if (s.selectedFile && !files.find((f) => f.path === s.selectedFile)) {
      s.setSelectedFile(null)
      s.setSelectedHunks([])
    }
  } catch {
    s.setFiles([])
  } finally {
    s.setLoadingFiles(false)
  }
}

async function loadBranches(): Promise<void> {
  const s = useGitPanelStore.getState()
  s.setLoadingBranches(true)
  try {
    const api = getGitPanelApi()
    const branches = await api.kernel.invoke<BranchEntry[]>(GIT_ID, 'branches', {})
    s.setBranches(branches)
  } catch {
    s.setBranches([])
  } finally {
    s.setLoadingBranches(false)
  }
}

async function loadLog(): Promise<void> {
  const s = useGitPanelStore.getState()
  s.setLoadingLog(true)
  try {
    const api = getGitPanelApi()
    const entries = await api.kernel.invoke<LogEntry[]>(GIT_ID, 'log', { limit: 50 })
    s.setLogEntries(entries)
  } catch {
    s.setLogEntries([])
  } finally {
    s.setLoadingLog(false)
  }
}

async function loadStash(): Promise<void> {
  const s = useGitPanelStore.getState()
  s.setLoadingStash(true)
  try {
    const api = getGitPanelApi()
    const entries = await api.kernel.invoke<StashEntry[]>(GIT_ID, 'stash_list', {})
    s.setStashEntries(entries)
  } catch {
    s.setStashEntries([])
  } finally {
    s.setLoadingStash(false)
  }
}

async function loadDiff(path: string, staged: boolean): Promise<void> {
  const s = useGitPanelStore.getState()
  s.setLoadingDiff(true)
  try {
    const api = getGitPanelApi()
    if (staged) {
      const diffs = await api.kernel.invoke<Array<{ path: string; hunks: DiffHunk[] }>>(
        GIT_ID, 'diff_staged', {},
      )
      const entry = diffs.find((d) => d.path === path)
      s.setSelectedHunks(entry?.hunks ?? [])
    } else {
      const hunks = await api.kernel.invoke<DiffHunk[]>(GIT_ID, 'diff_file', { path })
      s.setSelectedHunks(hunks)
    }
  } catch {
    s.setSelectedHunks([])
  } finally {
    s.setLoadingDiff(false)
  }
}

// ── GitPanel ──────────────────────────────────────────────────────────────────

export function GitPanel() {
  const status   = useGitStatusStore((s) => s.status)
  const activeTab = useGitPanelStore((s) => s.activeTab)
  const setActiveTab = useGitPanelStore((s) => s.setActiveTab)

  // Seed data when the panel mounts (kernel may already be running).
  useEffect(() => {
    if (!status) return
    void loadFiles()
    void loadBranches()
    void loadLog()
    void loadStash()
  }, [status])

  if (!status) {
    return (
      <div style={EMPTY_STYLE}>
        <span style={{ fontFamily: 'var(--font-interface)', fontSize: 12, color: 'var(--text-muted)' }}>
          Not a git repository.
        </span>
      </div>
    )
  }

  const TAB = (id: typeof activeTab, label: string) => (
    <button
      key={id}
      onClick={() => setActiveTab(id)}
      style={{
        background: 'transparent',
        border: 0,
        borderBottom: activeTab === id ? '2px solid var(--interactive-accent)' : '2px solid transparent',
        color: activeTab === id ? 'var(--text-normal)' : 'var(--text-muted)',
        fontFamily: 'var(--font-interface)',
        fontSize: 12,
        fontWeight: activeTab === id ? 600 : 400,
        padding: '7px 12px 5px',
        cursor: 'pointer',
        flexShrink: 0,
      }}
    >
      {label}
    </button>
  )

  // BL-084: surface non-Clean repo states (Merge / Rebase / CherryPick / …)
  // with an Abort affordance regardless of which tab is active. The
  // per-file resolution UI lives in `ChangesTab` so it sits next to
  // the file list.
  const conflictedCount = useGitPanelStore((s) =>
    s.files.reduce((n, f) => (f.status === 'Conflicted' ? n + 1 : n), 0),
  )
  const showBanner = !!status && status.repo_state !== 'Clean'

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      {showBanner && (
        <ConflictBanner repoState={status.repo_state} conflictCount={conflictedCount} />
      )}
      {/* Tab bar */}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--background-modifier-border)', flexShrink: 0 }}>
        {TAB('changes', 'Changes')}
        {TAB('branches', 'Branches')}
        {TAB('log', 'Log')}
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
        {activeTab === 'changes' && <ChangesTab />}
        {activeTab === 'branches' && <BranchesTab />}
        {activeTab === 'log' && <LogTab />}
      </div>
    </div>
  )
}

// ── ChangesTab ────────────────────────────────────────────────────────────────

function ChangesTab() {
  const files        = useGitPanelStore((s) => s.files)
  const loadingFiles = useGitPanelStore((s) => s.loadingFiles)
  const selectedFile = useGitPanelStore((s) => s.selectedFile)
  const selectedHunks = useGitPanelStore((s) => s.selectedHunks)
  const loadingDiff  = useGitPanelStore((s) => s.loadingDiff)
  const commitMessage = useGitPanelStore((s) => s.commitMessage)
  const committing   = useGitPanelStore((s) => s.committing)
  const pushAfterCommit = useGitPanelStore((s) => s.pushAfterCommit)
  const setSelectedFile = useGitPanelStore((s) => s.setSelectedFile)
  const setCommitMessage = useGitPanelStore((s) => s.setCommitMessage)
  const setCommitting  = useGitPanelStore((s) => s.setCommitting)
  const setPush        = useGitPanelStore((s) => s.setPushAfterCommit)

  const stashEntries  = useGitPanelStore((s) => s.stashEntries)

  const stagedFiles   = files.filter((f) => f.status === 'Staged' || f.status === 'Added')
  const unstagedFiles = files.filter((f) => f.status !== 'Staged' && f.status !== 'Added')

  // Whether the currently selected file is in the staged section.
  const selectedFileIsStaged = !!selectedFile && stagedFiles.some((f) => f.path === selectedFile)
  // BL-084: route Conflicted files into the conflict resolution UI
  // instead of the regular diff viewer.
  const selectedFileIsConflicted =
    !!selectedFile && files.some((f) => f.path === selectedFile && f.status === 'Conflicted')

  const selectFile = useCallback((entry: GitFileEntry) => {
    const isStaged = entry.status === 'Staged' || entry.status === 'Added'
    useGitPanelStore.getState().setSelectedFile(entry.path)
    void loadDiff(entry.path, isStaged)
  }, [])

  const handleStageHunk = useCallback(async (hunkIndex: number) => {
    if (!selectedFile) return
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stage_hunks', {
      path: selectedFile,
      hunk_indices: [hunkIndex],
    })
    await loadFiles()
    await loadDiff(selectedFile, false)
  }, [selectedFile])

  const handleUnstageHunk = useCallback(async (hunkIndex: number) => {
    if (!selectedFile) return
    await getGitPanelApi().kernel.invoke(GIT_ID, 'unstage_hunks', {
      path: selectedFile,
      hunk_indices: [hunkIndex],
    })
    await loadFiles()
    await loadDiff(selectedFile, true)
  }, [selectedFile])

  const stageFile = useCallback(async (path: string) => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stage_file', { path })
    await loadFiles()
  }, [])

  const unstageFile = useCallback(async (path: string) => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'unstage_file', { path })
    await loadFiles()
  }, [])

  const stageAll = useCallback(async () => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stage_all', {})
    await loadFiles()
  }, [])

  const unstageAll = useCallback(async () => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'unstage_all', {})
    await loadFiles()
  }, [])

  const handleCommit = useCallback(async () => {
    const msg = commitMessage.trim()
    if (!msg || committing) return
    setCommitting(true)
    try {
      const api = getGitPanelApi()
      await api.kernel.invoke(GIT_ID, 'commit', { message: msg })
      setCommitMessage('')
      if (pushAfterCommit) {
        // Get current branch + upstream to determine remote/branch.
        const branches = await api.kernel.invoke<BranchEntry[]>(GIT_ID, 'branches', {})
        const head = branches.find((b) => b.is_head)
        if (head?.upstream) {
          const [remote, ...rest] = head.upstream.split('/')
          const branch = rest.join('/')
          await api.kernel.invoke(GIT_ID, 'push', { remote, branch })
        }
      }
      await loadFiles()
      await loadLog()
    } finally {
      setCommitting(false)
    }
  }, [commitMessage, committing, pushAfterCommit, setCommitting, setCommitMessage])

  const handleStashPush = useCallback(async () => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stash_push', {})
    await loadFiles()
    await loadStash()
  }, [])

  const handleStashPop = useCallback(async (index: number) => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stash_pop', { index })
    await loadFiles()
    await loadStash()
  }, [])

  const handleStashDrop = useCallback(async (index: number) => {
    await getGitPanelApi().kernel.invoke(GIT_ID, 'stash_drop', { index })
    await loadStash()
  }, [])

  const canCommit = stagedFiles.length > 0 && commitMessage.trim().length > 0 && !committing

  return (
    <>
      {/* File list */}
      <div style={{ flex: 1, overflowY: 'auto', minHeight: 0 }}>
        {loadingFiles && files.length === 0 && (
          <div style={MUTED_ROW}>Loading…</div>
        )}
        {!loadingFiles && files.length === 0 && (
          <div style={MUTED_ROW}>Nothing to commit — working tree clean.</div>
        )}

        {/* Staged */}
        {stagedFiles.length > 0 && (
          <>
            <SectionHeader
              label={`Staged (${stagedFiles.length})`}
              action="Unstage all"
              onAction={() => void unstageAll()}
            />
            {stagedFiles.map((f) => (
              <FileRow
                key={f.path}
                entry={f}
                selected={selectedFile === f.path}
                onSelect={() => selectFile(f)}
                actionLabel="−"
                actionTitle="Unstage"
                onAction={() => void unstageFile(f.path)}
              />
            ))}
          </>
        )}

        {/* Unstaged */}
        {unstagedFiles.length > 0 && (
          <>
            <SectionHeader
              label={`Unstaged (${unstagedFiles.length})`}
              action="Stage all"
              onAction={() => void stageAll()}
            />
            {unstagedFiles.map((f) => (
              <FileRow
                key={f.path}
                entry={f}
                selected={selectedFile === f.path}
                onSelect={() => selectFile(f)}
                actionLabel="+"
                actionTitle="Stage"
                onAction={() => void stageFile(f.path)}
              />
            ))}
          </>
        )}
      </div>

      {/* Diff preview / conflict resolution. A `Conflicted` file
          short-circuits the regular diff viewer in favour of the
          BL-084 conflict UI; the working-tree write-back leaves the
          file unstaged so the user finishes via the same Stage +
          Commit flow as any other change. */}
      {selectedFile && selectedFileIsConflicted ? (
        <div
          style={{
            height: 320,
            borderTop: '1px solid var(--background-modifier-border)',
            overflow: 'hidden',
            flexShrink: 0,
            display: 'flex',
            flexDirection: 'column',
          }}
        >
          <ConflictView relpath={selectedFile} />
        </div>
      ) : (selectedFile || loadingDiff) ? (
        <div
          style={{
            height: 180,
            borderTop: '1px solid var(--background-modifier-border)',
            overflowY: 'auto',
            flexShrink: 0,
          }}
        >
          {loadingDiff ? (
            <div style={MUTED_ROW}>Loading diff…</div>
          ) : selectedHunks.length === 0 ? (
            <div style={MUTED_ROW}>No diff available.</div>
          ) : (
            <DiffViewer
              hunks={selectedHunks}
              onStageHunk={!selectedFileIsStaged ? (i) => void handleStageHunk(i) : undefined}
              onUnstageHunk={selectedFileIsStaged ? (i) => void handleUnstageHunk(i) : undefined}
            />
          )}
        </div>
      ) : null}

      {/* Stash section */}
      <StashSection
        entries={stashEntries}
        onStash={() => void handleStashPush()}
        onPop={(i) => void handleStashPop(i)}
        onDrop={(i) => void handleStashDrop(i)}
      />

      {/* Commit area */}
      <div
        style={{
          borderTop: '1px solid var(--background-modifier-border)',
          padding: '8px',
          flexShrink: 0,
          display: 'flex',
          flexDirection: 'column',
          gap: 6,
        }}
      >
        <textarea
          value={commitMessage}
          onChange={(e) => setCommitMessage(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
              e.preventDefault()
              void handleCommit()
            }
          }}
          placeholder="Commit message (⌘Enter to commit)"
          rows={2}
          style={{
            width: '100%',
            background: 'var(--background-primary)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            padding: '5px 7px',
            resize: 'none',
            outline: 0,
            boxSizing: 'border-box',
          }}
        />
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: 5, cursor: 'pointer', flex: 1 }}>
            <input
              type="checkbox"
              checked={pushAfterCommit}
              onChange={(e) => setPush(e.target.checked)}
              style={{ cursor: 'pointer' }}
            />
            <span style={{ fontFamily: 'var(--font-interface)', fontSize: 11, color: 'var(--text-muted)' }}>
              Push after commit
            </span>
          </label>
          <button
            onClick={() => void handleCommit()}
            disabled={!canCommit}
            style={{
              background: canCommit ? 'var(--interactive-accent)' : 'var(--background-modifier-border)',
              color: canCommit ? 'var(--text-on-accent)' : 'var(--text-muted)',
              border: 0,
              borderRadius: 'var(--radius-s)',
              padding: '5px 14px',
              fontFamily: 'var(--font-interface)',
              fontSize: 12,
              fontWeight: 600,
              cursor: canCommit ? 'pointer' : 'default',
              flexShrink: 0,
            }}
          >
            {committing ? 'Committing…' : `Commit${stagedFiles.length > 0 ? ` (${stagedFiles.length})` : ''}`}
          </button>
        </div>
      </div>
    </>
  )
}

// ── BranchesTab ───────────────────────────────────────────────────────────────

function BranchesTab() {
  const branches        = useGitPanelStore((s) => s.branches)
  const loadingBranches = useGitPanelStore((s) => s.loadingBranches)
  const newBranchName   = useGitPanelStore((s) => s.newBranchName)
  const setNewBranchName = useGitPanelStore((s) => s.setNewBranchName)
  const inputRef = useRef<HTMLInputElement | null>(null)

  const switchBranch = useCallback(async (name: string) => {
    try {
      await getGitPanelApi().kernel.invoke(GIT_ID, 'switch_branch', { name })
      await loadBranches()
      await loadFiles()
    } catch (err) {
      getGitPanelApi().notifications.show({
        type: 'error',
        message: `Switch failed: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }, [])

  const createAndSwitch = useCallback(async () => {
    const name = newBranchName.trim()
    if (!name) return
    try {
      await getGitPanelApi().kernel.invoke(GIT_ID, 'create_branch', { name })
      await getGitPanelApi().kernel.invoke(GIT_ID, 'switch_branch', { name })
      setNewBranchName('')
      await loadBranches()
    } catch (err) {
      getGitPanelApi().notifications.show({
        type: 'error',
        message: `Create branch failed: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }, [newBranchName, setNewBranchName])

  const deleteBranch = useCallback(async (name: string) => {
    const ok = await getGitPanelApi().input.confirm(`Delete branch "${name}"?`)
    if (!ok) return
    try {
      await getGitPanelApi().kernel.invoke(GIT_ID, 'delete_branch', { name })
      await loadBranches()
    } catch (err) {
      getGitPanelApi().notifications.show({
        type: 'error',
        message: `Delete failed: ${err instanceof Error ? err.message : String(err)}`,
      })
    }
  }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {loadingBranches && branches.length === 0 && <div style={MUTED_ROW}>Loading…</div>}
        {branches.map((b) => (
          <div
            key={b.name}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 6,
              padding: '6px 12px',
              background: b.is_head ? 'var(--background-modifier-active-hover, var(--interactive-accent-soft))' : 'transparent',
            }}
          >
            <span
              style={{
                fontFamily: 'var(--font-monospace)',
                fontSize: 12,
                color: b.is_head ? 'var(--interactive-accent)' : 'var(--text-normal)',
                flex: 1,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
              }}
            >
              {b.is_head ? '● ' : '○ '}{b.name}
            </span>
            {b.upstream && (
              <span style={{ fontFamily: 'var(--font-interface)', fontSize: 10, color: 'var(--text-faint)', flexShrink: 0 }}>
                → {b.upstream}
              </span>
            )}
            {!b.is_head && (
              <>
                <button
                  onClick={() => void switchBranch(b.name)}
                  title={`Switch to ${b.name}`}
                  style={INLINE_BTN}
                >
                  switch
                </button>
                <button
                  onClick={() => void deleteBranch(b.name)}
                  title={`Delete ${b.name}`}
                  style={{ ...INLINE_BTN, color: 'var(--text-error, #E74C3C)' }}
                >
                  ×
                </button>
              </>
            )}
          </div>
        ))}
      </div>

      {/* Create branch */}
      <div
        style={{
          borderTop: '1px solid var(--background-modifier-border)',
          padding: '8px',
          display: 'flex',
          gap: 6,
          flexShrink: 0,
        }}
      >
        <input
          ref={inputRef}
          type="text"
          value={newBranchName}
          onChange={(e) => setNewBranchName(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') void createAndSwitch() }}
          placeholder="New branch name…"
          style={{
            flex: 1,
            background: 'var(--background-primary)',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            padding: '5px 7px',
            outline: 0,
          }}
        />
        <button
          onClick={() => void createAndSwitch()}
          disabled={!newBranchName.trim()}
          style={{
            background: newBranchName.trim() ? 'var(--interactive-accent)' : 'var(--background-modifier-border)',
            color: newBranchName.trim() ? 'var(--text-on-accent)' : 'var(--text-muted)',
            border: 0,
            borderRadius: 'var(--radius-s)',
            padding: '5px 10px',
            fontFamily: 'var(--font-interface)',
            fontSize: 12,
            fontWeight: 600,
            cursor: newBranchName.trim() ? 'pointer' : 'default',
            flexShrink: 0,
          }}
        >
          Create
        </button>
      </div>
    </div>
  )
}

// ── LogTab ────────────────────────────────────────────────────────────────────

function LogTab() {
  const entries    = useGitPanelStore((s) => s.logEntries)
  const loadingLog = useGitPanelStore((s) => s.loadingLog)
  const status     = useGitStatusStore((s) => s.status)

  if (loadingLog && entries.length === 0) {
    return <div style={MUTED_ROW}>Loading…</div>
  }
  if (entries.length === 0) {
    return <div style={MUTED_ROW}>No commits yet.</div>
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto' }}>
      {entries.map((entry, idx) => {
        const isHead = idx === 0 && entry.hash === status?.head
        const date = formatRelativeDate(entry.date)
        return (
          <div
            key={entry.hash}
            style={{
              padding: '7px 12px',
              borderBottom: '1px solid var(--background-modifier-border)',
              display: 'flex',
              flexDirection: 'column',
              gap: 2,
            }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span
                style={{
                  fontFamily: 'var(--font-monospace)',
                  fontSize: 10,
                  color: isHead ? 'var(--interactive-accent)' : 'var(--text-muted)',
                  flexShrink: 0,
                  background: 'var(--background-modifier-border)',
                  borderRadius: 'var(--radius-xs, 2px)',
                  padding: '1px 4px',
                  cursor: 'pointer',
                }}
                title="Copy hash"
                onClick={() => void navigator.clipboard.writeText(entry.hash)}
              >
                {entry.hash.slice(0, 7)}
              </span>
              {isHead && (
                <span
                  style={{
                    fontFamily: 'var(--font-interface)',
                    fontSize: 10,
                    color: 'var(--interactive-accent)',
                    background: 'var(--background-modifier-active-hover, var(--interactive-accent-soft))',
                    borderRadius: 'var(--radius-full, 9999px)',
                    padding: '1px 5px',
                    flexShrink: 0,
                  }}
                >
                  HEAD
                </span>
              )}
              <span
                style={{
                  fontFamily: 'var(--font-interface)',
                  fontSize: 12,
                  color: 'var(--text-normal)',
                  flex: 1,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {entry.message.split('\n')[0]}
              </span>
            </div>
            <div style={{ display: 'flex', gap: 8 }}>
              <span style={{ fontFamily: 'var(--font-interface)', fontSize: 10, color: 'var(--text-muted)' }}>
                {entry.author}
              </span>
              <span style={{ fontFamily: 'var(--font-interface)', fontSize: 10, color: 'var(--text-faint)' }}>
                {date}
              </span>
            </div>
          </div>
        )
      })}
    </div>
  )
}

// ── DiffViewer ────────────────────────────────────────────────────────────────

function DiffViewer({
  hunks,
  onStageHunk,
  onUnstageHunk,
}: {
  hunks: DiffHunk[]
  onStageHunk?: (hunkIndex: number) => void
  onUnstageHunk?: (hunkIndex: number) => void
}) {
  return (
    <div style={{ fontFamily: 'var(--font-monospace)', fontSize: 11, lineHeight: 1.4 }}>
      {hunks.map((hunk, hi) => (
        <div key={hi}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              padding: '2px 6px 2px 10px',
              background: 'var(--background-modifier-border)',
              color: 'var(--text-muted)',
              userSelect: 'none',
              gap: 6,
            }}
          >
            <span style={{ flex: 1, fontFamily: 'var(--font-monospace)', fontSize: 10 }}>
              @@ -{hunk.old_start},{hunk.old_count} +{hunk.new_start},{hunk.new_count} @@
            </span>
            {onStageHunk && (
              <button
                onClick={() => onStageHunk(hi)}
                title="Stage this hunk"
                style={{
                  background: 'transparent',
                  border: '1px solid var(--interactive-accent)',
                  borderRadius: 'var(--radius-xs, 2px)',
                  color: 'var(--interactive-accent)',
                  cursor: 'pointer',
                  fontFamily: 'var(--font-interface)',
                  fontSize: 10,
                  padding: '1px 6px',
                  flexShrink: 0,
                }}
              >
                Stage hunk
              </button>
            )}
            {onUnstageHunk && (
              <button
                onClick={() => onUnstageHunk(hi)}
                title="Unstage this hunk"
                style={{
                  background: 'transparent',
                  border: '1px solid var(--background-modifier-border-hover, var(--background-modifier-border))',
                  borderRadius: 'var(--radius-xs, 2px)',
                  color: 'var(--text-muted)',
                  cursor: 'pointer',
                  fontFamily: 'var(--font-interface)',
                  fontSize: 10,
                  padding: '1px 6px',
                  flexShrink: 0,
                }}
              >
                Unstage hunk
              </button>
            )}
          </div>
          {hunk.lines.map((line, li) => {
            const isAdded   = line.kind === 'Added'
            const isRemoved = line.kind === 'Removed'
            const prefix = isAdded ? '+' : isRemoved ? '-' : ' '
            return (
              <div
                key={li}
                style={{
                  padding: '0 10px',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-all',
                  background: isAdded
                    ? 'rgba(39,174,96,0.12)'
                    : isRemoved
                      ? 'rgba(231,76,60,0.12)'
                      : 'transparent',
                  color: isAdded
                    ? 'var(--color-green, #27AE60)'
                    : isRemoved
                      ? 'var(--color-red, #E74C3C)'
                      : 'var(--text-muted)',
                }}
              >
                {prefix}{line.content}
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
}

// ── StashSection ─────────────────────────────────────────────────────────────

interface StashSectionProps {
  entries: StashEntry[]
  onStash(): void
  onPop(index: number): void
  onDrop(index: number): void
}

function StashSection({ entries, onStash, onPop, onDrop }: StashSectionProps) {
  return (
    <div
      style={{
        borderTop: '1px solid var(--background-modifier-border)',
        flexShrink: 0,
      }}
    >
      {/* Header row */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          padding: '4px 12px',
          background: 'var(--background-secondary)',
        }}
      >
        <span
          style={{
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            fontWeight: 600,
            color: 'var(--text-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.05em',
            flex: 1,
          }}
        >
          Stash{entries.length > 0 ? ` (${entries.length})` : ''}
        </span>
        <button
          onClick={onStash}
          title="Stash all uncommitted changes"
          style={{
            background: 'transparent',
            border: '1px solid var(--background-modifier-border)',
            borderRadius: 'var(--radius-s)',
            color: 'var(--text-muted)',
            cursor: 'pointer',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            padding: '2px 8px',
          }}
        >
          Stash
        </button>
      </div>

      {/* Stash entries */}
      {entries.map((entry) => (
        <div
          key={entry.index}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '4px 12px',
          }}
        >
          <span
            style={{
              fontFamily: 'var(--font-monospace)',
              fontSize: 10,
              color: 'var(--text-muted)',
              background: 'var(--background-modifier-border)',
              borderRadius: 'var(--radius-xs, 2px)',
              padding: '0 4px',
              flexShrink: 0,
            }}
          >
            {entry.oid}
          </span>
          <span
            style={{
              fontFamily: 'var(--font-interface)',
              fontSize: 11,
              color: 'var(--text-muted)',
              flex: 1,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
            }}
          >
            {entry.message}
          </span>
          <button
            onClick={() => onPop(entry.index)}
            title="Apply and remove this stash"
            style={STASH_BTN}
          >
            Pop
          </button>
          <button
            onClick={() => onDrop(entry.index)}
            title="Discard this stash"
            style={{ ...STASH_BTN, color: 'var(--text-error, #E74C3C)' }}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  )
}

const STASH_BTN: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 'var(--radius-s)',
  color: 'var(--text-muted)',
  cursor: 'pointer',
  fontFamily: 'var(--font-interface)',
  fontSize: 11,
  padding: '1px 6px',
  flexShrink: 0,
}

// ── Helper components ─────────────────────────────────────────────────────────

function SectionHeader({
  label, action, onAction,
}: {
  label: string
  action: string
  onAction(): void
}) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        padding: '4px 12px',
        background: 'var(--background-secondary)',
        borderBottom: '1px solid var(--background-modifier-border)',
        position: 'sticky',
        top: 0,
        zIndex: 1,
      }}
    >
      <span style={{ fontFamily: 'var(--font-interface)', fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', flex: 1, textTransform: 'uppercase', letterSpacing: '0.05em' }}>
        {label}
      </span>
      <button
        onClick={onAction}
        style={{ background: 'transparent', border: 0, color: 'var(--text-muted)', cursor: 'pointer', fontFamily: 'var(--font-interface)', fontSize: 11, padding: '2px 4px' }}
      >
        {action}
      </button>
    </div>
  )
}

interface FileRowProps {
  entry: GitFileEntry
  selected: boolean
  onSelect(): void
  actionLabel: string
  actionTitle: string
  onAction(): void
}

function FileRow({ entry, selected, onSelect, actionLabel, actionTitle, onAction }: FileRowProps) {
  const { color, bg } = statusStyle(entry.status)
  const parts = entry.path.split('/')
  const name = parts[parts.length - 1]
  const dir  = parts.slice(0, -1).join('/')

  return (
    <div
      onClick={onSelect}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        padding: '4px 12px',
        background: selected ? 'var(--background-modifier-active-hover, var(--interactive-accent-soft))' : 'transparent',
        cursor: 'pointer',
      }}
    >
      {/* Status marker */}
      <span
        style={{
          fontFamily: 'var(--font-monospace)',
          fontSize: 11,
          color,
          background: bg,
          borderRadius: 'var(--radius-xs, 2px)',
          padding: '0 3px',
          flexShrink: 0,
        }}
      >
        {entry.status[0]}
      </span>

      {/* Path */}
      <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        <span style={{ fontFamily: 'var(--font-interface)', fontSize: 12, color: 'var(--text-normal)' }}>{name}</span>
        {dir && (
          <span style={{ fontFamily: 'var(--font-interface)', fontSize: 10, color: 'var(--text-muted)', marginLeft: 4 }}>{dir}</span>
        )}
      </span>

      {/* Action button */}
      <button
        onClick={(e) => { e.stopPropagation(); onAction() }}
        title={actionTitle}
        style={{
          background: 'transparent',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-xs, 2px)',
          color: 'var(--text-muted)',
          cursor: 'pointer',
          fontFamily: 'var(--font-monospace)',
          fontSize: 12,
          lineHeight: 1,
          padding: '1px 5px',
          flexShrink: 0,
        }}
      >
        {actionLabel}
      </button>
    </div>
  )
}

// ── Utilities ─────────────────────────────────────────────────────────────────

function statusStyle(status: string): { color: string; bg: string } {
  switch (status) {
    case 'Staged':   return { color: '#27AE60', bg: 'rgba(39,174,96,0.12)' }
    case 'Added':    return { color: '#27AE60', bg: 'rgba(39,174,96,0.12)' }
    case 'Modified': return { color: '#F39C12', bg: 'rgba(243,156,18,0.12)' }
    case 'Removed':  return { color: '#E74C3C', bg: 'rgba(231,76,60,0.12)' }
    case 'Conflicted': return { color: '#E67E22', bg: 'rgba(230,126,34,0.12)' }
    case 'Renamed':  return { color: '#3498DB', bg: 'rgba(52,152,219,0.12)' }
    default:         return { color: 'var(--text-muted)', bg: 'var(--background-modifier-border)' }
  }
}

function formatRelativeDate(iso: string): string {
  try {
    const diff = Date.now() - new Date(iso).getTime()
    const s = Math.floor(diff / 1000)
    if (s < 60) return `${s}s ago`
    const m = Math.floor(s / 60)
    if (m < 60) return `${m}m ago`
    const h = Math.floor(m / 60)
    if (h < 24) return `${h}h ago`
    const d = Math.floor(h / 24)
    if (d < 30) return `${d}d ago`
    return new Date(iso).toLocaleDateString()
  } catch {
    return iso
  }
}

// ── Shared styles ─────────────────────────────────────────────────────────────

const EMPTY_STYLE: React.CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  height: '100%',
  padding: 16,
}

const MUTED_ROW: React.CSSProperties = {
  padding: '8px 12px',
  fontFamily: 'var(--font-interface)',
  fontSize: 12,
  color: 'var(--text-faint)',
}

const INLINE_BTN: React.CSSProperties = {
  background: 'transparent',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 'var(--radius-s)',
  color: 'var(--text-muted)',
  cursor: 'pointer',
  fontFamily: 'var(--font-interface)',
  fontSize: 11,
  padding: '2px 6px',
  flexShrink: 0,
}
