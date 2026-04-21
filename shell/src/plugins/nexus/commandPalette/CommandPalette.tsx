import { useEffect, useMemo, useRef } from 'react'
import { useCommandPaletteStore } from './commandPaletteStore'
import { filterCommands } from './match'
import { getApi } from './paletteRuntime'
import type { CommandEntry } from '../../../types/plugin'

/**
 * Keyboard-first command palette overlay. Lives in the `overlay`
 * slot; renders nothing when closed. The shell-overlay container
 * already has `pointer-events: none`, so the modal sets
 * `pointer-events: auto` on its own backdrop to catch clicks.
 *
 * App.tsx's global keydown handler short-circuits when focus is on
 * an INPUT, which means our input owns ALL key semantics here —
 * including Escape (the registered `nexus.commandPalette.close`
 * keybinding never fires inside an input, so we close directly).
 */
export function CommandPalette() {
  const visible = useCommandPaletteStore((s) => s.visible)
  const query = useCommandPaletteStore((s) => s.query)
  const selectedIndex = useCommandPaletteStore((s) => s.selectedIndex)
  const setQuery = useCommandPaletteStore((s) => s.setQuery)
  const setSelectedIndex = useCommandPaletteStore((s) => s.setSelectedIndex)
  const moveSelection = useCommandPaletteStore((s) => s.moveSelection)
  const close = useCommandPaletteStore((s) => s.close)

  const inputRef = useRef<HTMLInputElement | null>(null)
  const listRef = useRef<HTMLDivElement | null>(null)

  // Pull the live command list each time we open. Late-loading
  // plugins (community ones, for instance) register commands after
  // the palette plugin's `activate` runs, so a snapshot at activate
  // time would miss them.
  const allCommands: CommandEntry[] = useMemo(() => {
    if (!visible) return []
    try {
      return getApi().commands.all()
    } catch {
      return []
    }
  }, [visible])

  const results = useMemo(
    () => filterCommands(allCommands, query),
    [allCommands, query],
  )

  // Autofocus the input every time we become visible.
  useEffect(() => {
    if (visible) {
      // requestAnimationFrame so the element is in the DOM. Calling
      // focus() in the same render tick can race with the mount.
      const id = requestAnimationFrame(() => inputRef.current?.focus())
      return () => cancelAnimationFrame(id)
    }
  }, [visible])

  // Keep the selected row visible when arrow keys push it out of
  // the scroll viewport.
  useEffect(() => {
    if (!visible) return
    const list = listRef.current
    if (!list) return
    const row = list.querySelector<HTMLDivElement>(
      `[data-row-idx="${selectedIndex}"]`,
    )
    row?.scrollIntoView({ block: 'nearest' })
  }, [selectedIndex, visible])

  if (!visible) return null

  const runCommand = (commandId: string) => {
    // Close FIRST so the palette tears down even if the command
    // throws — otherwise the user can be stuck in a frozen modal.
    close()
    void getApi().commands.execute(commandId)
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
      if (picked) runCommand(picked.command.id)
    } else if (e.key === 'Escape') {
      // stopPropagation so the App.tsx-level keydown listener
      // (and any registry-bound `escape` keybinding from another
      // plugin) doesn't see this event after we close.
      e.preventDefault()
      e.stopPropagation()
      close()
    }
  }

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    // Only close on clicks that originate on the backdrop itself —
    // clicks bubbling up from the modal shouldn't dismiss.
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
          background: 'var(--bg-raised)',
          border: '1px solid var(--line)',
          borderRadius: 'var(--r-lg)',
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
          placeholder="Type a command…"
          spellCheck={false}
          autoComplete="off"
          style={{
            background: 'transparent',
            border: 0,
            outline: 0,
            color: 'var(--fg)',
            fontFamily: 'var(--f-ui)',
            fontSize: 14,
            padding: '12px 16px',
            borderBottom: '1px solid var(--line-soft)',
          }}
        />
        <div
          ref={listRef}
          style={{
            maxHeight: 380,
            overflowY: 'auto',
          }}
        >
          {results.length === 0 ? (
            <div
              style={{
                padding: '12px 16px',
                color: 'var(--fg-dim)',
                fontFamily: 'var(--f-ui)',
                fontSize: 13,
              }}
            >
              No matching commands
            </div>
          ) : (
            results.map((r, idx) => {
              const selected = idx === selectedIndex
              return (
                <CommandRow
                  key={r.command.id}
                  index={idx}
                  command={r.command}
                  selected={selected}
                  onHover={() => setSelectedIndex(idx)}
                  onPick={() => runCommand(r.command.id)}
                />
              )
            })
          )}
        </div>
      </div>
    </div>
  )
}

interface CommandRowProps {
  index: number
  command: CommandEntry
  selected: boolean
  onHover(): void
  onPick(): void
}

function CommandRow({ index, command, selected, onHover, onPick }: CommandRowProps) {
  return (
    <div
      data-row-idx={index}
      onMouseEnter={onHover}
      onClick={onPick}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 12,
        padding: '8px 16px',
        cursor: 'pointer',
        fontFamily: 'var(--f-ui)',
        fontSize: 13,
        background: selected ? 'var(--accent-soft)' : 'transparent',
        color: selected ? 'var(--fg)' : 'var(--fg-muted)',
      }}
    >
      <div
        style={{
          flex: 1,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        <span>{command.title}</span>
        {command.category && (
          <>
            <span style={{ color: 'var(--fg-dim)', margin: '0 6px' }}>·</span>
            <span style={{ color: 'var(--fg-dim)', fontSize: '0.85em' }}>
              {command.category}
            </span>
          </>
        )}
      </div>
      {command.keybinding && (
        <span
          style={{
            background: 'var(--bg)',
            border: '1px solid var(--line-soft)',
            borderRadius: 'var(--r)',
            padding: '1px 6px',
            fontFamily: 'var(--f-mono)',
            fontSize: '0.78em',
            color: 'var(--fg-muted)',
          }}
        >
          {command.keybinding}
        </span>
      )}
    </div>
  )
}
