import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuickSwitcherStore } from './quickSwitcherStore'
import { decodeFiles, filterFiles, isAttachment, pushRecent, type FileEntry } from './fileMatch'
import { getApi } from './quickSwitcherRuntime'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const EVENT_FILE_OPEN = 'files:open'

const CONFIG_KEY_RECENTS = 'nexus.quickSwitcher.recentFiles'
const CONFIG_KEY_SHOW_EXISTING_ONLY = 'nexus.settings.quickSwitcher.showExistingOnly'
const CONFIG_KEY_SHOW_ATTACHMENTS = 'nexus.settings.quickSwitcher.showAttachments'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/** `true` when `path` already carries any file extension. */
function hasExtension(path: string): boolean {
  const base = basename(path)
  return base.includes('.') && !base.startsWith('.')
}

function openFile(relpath: string): void {
  const api = getApi()
  api.events.emit(EVENT_FILE_OPEN, { relpath, name: basename(relpath) })
  const recents = api.configuration.getValue<string[]>(CONFIG_KEY_RECENTS, [])
  api.configuration.setValue(CONFIG_KEY_RECENTS, pushRecent(recents, relpath))
}

type ResultItem =
  | { kind: 'file'; entry: FileEntry }
  | { kind: 'create'; relpath: string }

/**
 * Keyboard-first file quick-switcher overlay (C5, #358) — Ctrl+P /
 * Cmd+P, reclaimed from the command palette's alias binding. Structure
 * mirrors commandPalette/CommandPalette.tsx closely (same overlay
 * chrome, same arrow/Enter/Escape handling); the differences are the
 * data source (query_files instead of commands.all()) and the
 * create-on-Enter affordance for a query that matches nothing.
 */
export function QuickSwitcher() {
  const visible = useQuickSwitcherStore((s) => s.visible)
  const query = useQuickSwitcherStore((s) => s.query)
  const selectedIndex = useQuickSwitcherStore((s) => s.selectedIndex)
  const setQuery = useQuickSwitcherStore((s) => s.setQuery)
  const setSelectedIndex = useQuickSwitcherStore((s) => s.setSelectedIndex)
  const moveSelection = useQuickSwitcherStore((s) => s.moveSelection)
  const close = useQuickSwitcherStore((s) => s.close)

  const [files, setFiles] = useState<FileEntry[]>([])
  const [recents, setRecents] = useState<string[]>([])

  const inputRef = useRef<HTMLInputElement | null>(null)
  const listRef = useRef<HTMLDivElement | null>(null)

  // Re-fetch the file list + settings every time the switcher opens —
  // files created/renamed elsewhere since the last open must show up.
  useEffect(() => {
    if (!visible) return
    let cancelled = false
    const api = getApi()
    setRecents(api.configuration.getValue<string[]>(CONFIG_KEY_RECENTS, []))
    void api.kernel
      .invoke<unknown>(STORAGE_PLUGIN_ID, 'query_files', {})
      .then((raw) => {
        if (!cancelled) setFiles(decodeFiles(raw))
      })
      .catch(() => {
        if (!cancelled) setFiles([])
      })
    return () => {
      cancelled = true
    }
  }, [visible])

  const showAttachments = useMemo(() => {
    if (!visible) return true
    try {
      return getApi().configuration.getValue<boolean>(CONFIG_KEY_SHOW_ATTACHMENTS, true)
    } catch {
      return true
    }
  }, [visible])

  const showExistingOnly = useMemo(() => {
    if (!visible) return false
    try {
      return getApi().configuration.getValue<boolean>(CONFIG_KEY_SHOW_EXISTING_ONLY, false)
    } catch {
      return false
    }
  }, [visible])

  const visibleFiles = useMemo(
    () => (showAttachments ? files : files.filter((f) => !isAttachment(f.file_type))),
    [files, showAttachments],
  )

  const matches = useMemo(
    () => filterFiles(visibleFiles, query, recents),
    [visibleFiles, query, recents],
  )

  const trimmedQuery = query.trim()
  const canCreate =
    !showExistingOnly && trimmedQuery.length > 0 && !matches.some((m) => m.entry.path === trimmedQuery)

  const results: ResultItem[] = useMemo(() => {
    const fileItems: ResultItem[] = matches.map((m) => ({ kind: 'file', entry: m.entry }))
    if (!canCreate) return fileItems
    const relpath = hasExtension(trimmedQuery) ? trimmedQuery : `${trimmedQuery}.md`
    return [...fileItems, { kind: 'create', relpath }]
  }, [matches, canCreate, trimmedQuery])

  useEffect(() => {
    if (visible) {
      const id = requestAnimationFrame(() => inputRef.current?.focus())
      return () => cancelAnimationFrame(id)
    }
  }, [visible])

  useEffect(() => {
    if (!visible) return
    const list = listRef.current
    if (!list) return
    const row = list.querySelector<HTMLDivElement>(`[data-row-idx="${selectedIndex}"]`)
    row?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex, visible])

  if (!visible) return null

  const pick = (item: ResultItem) => {
    close()
    if (item.kind === 'file') {
      openFile(item.entry.path)
      return
    }
    const api = getApi()
    void api.kernel
      .invoke(STORAGE_PLUGIN_ID, 'create_file', { relpath: item.relpath })
      .then(() => openFile(item.relpath))
      .catch((e: unknown) => {
        api.notifications.show({ message: `Failed to create "${item.relpath}": ${String(e)}`, type: 'error' })
      })
  }

  const onInputKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveSelection(1, results.length)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveSelection(-1, results.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const picked = results[selectedIndex]
      if (picked) pick(picked)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      close()
    }
  }

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === e.currentTarget) close()
  }

  return (
    <div
      onClick={onBackdropClick}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'oklch(0 0 0 / 0.35)',
        pointerEvents: 'auto',
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'flex-start',
        paddingTop: 120,
      }}
    >
      <div
        style={{
          width: 560,
          maxWidth: '90vw',
          background: 'var(--background-secondary)',
          border: '1px solid var(--background-modifier-border)',
          borderRadius: 'var(--radius-l)',
          boxShadow: 'var(--shadow)',
          overflow: 'hidden',
          display: 'flex',
          flexDirection: 'column',
        }}
      >
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onInputKeyDown}
          placeholder="Go to file…"
          spellCheck={false}
          autoComplete="off"
          style={{
            background: 'transparent',
            border: 0,
            outline: 0,
            color: 'var(--text-normal)',
            fontFamily: 'var(--font-interface)',
            fontSize: 14,
            padding: '12px 16px',
            borderBottom: '1px solid var(--divider-color)',
          }}
        />
        <div ref={listRef} style={{ maxHeight: 380, overflowY: 'auto' }}>
          {results.length === 0 ? (
            <div
              style={{
                padding: '12px 16px',
                color: 'var(--text-faint)',
                fontFamily: 'var(--font-interface)',
                fontSize: 13,
              }}
            >
              No matching files
            </div>
          ) : (
            results.map((item, idx) => (
              <ResultRow
                key={item.kind === 'file' ? item.entry.path : `__create__${item.relpath}`}
                item={item}
                index={idx}
                selected={idx === selectedIndex}
                onHover={() => setSelectedIndex(idx)}
                onPick={() => pick(item)}
              />
            ))
          )}
        </div>
      </div>
    </div>
  )
}

function ResultRow({
  item,
  index,
  selected,
  onHover,
  onPick,
}: {
  item: ResultItem
  index: number
  selected: boolean
  onHover(): void
  onPick(): void
}) {
  const rowStyle: React.CSSProperties = {
    display: 'flex',
    alignItems: 'center',
    gap: 12,
    padding: '8px 16px',
    cursor: 'pointer',
    fontFamily: 'var(--font-interface)',
    fontSize: 13,
    background: selected ? 'var(--interactive-accent-soft)' : 'transparent',
    color: selected ? 'var(--text-normal)' : 'var(--text-muted)',
  }

  if (item.kind === 'create') {
    return (
      <div data-row-idx={index} onMouseEnter={onHover} onClick={onPick} style={rowStyle}>
        <span style={{ color: 'var(--interactive-accent)' }}>+</span>
        <span>
          Create <strong>{item.relpath}</strong>
        </span>
      </div>
    )
  }

  const name = basename(item.entry.path)
  const dir = item.entry.path.slice(0, item.entry.path.length - name.length - 1)

  return (
    <div data-row-idx={index} onMouseEnter={onHover} onClick={onPick} style={rowStyle}>
      <div style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
        <span>{name}</span>
        {dir && (
          <>
            <span style={{ color: 'var(--text-faint)', margin: '0 6px' }}>·</span>
            <span style={{ color: 'var(--text-faint)', fontSize: '0.85em' }}>{dir}</span>
          </>
        )}
      </div>
    </div>
  )
}
