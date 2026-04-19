import { useState, useEffect, useRef, useCallback } from 'react'
import { useContextKey, useContextKeyStore } from '../../../host/ContextKeyService'
import { getRegistry } from '../../../host/shellRegistry'
import type { CommandEntry } from '../../../types/plugin'

export function CommandPaletteView() {
  const visible      = useContextKey('commandPaletteVisible') as boolean
  const [commands, setCommands] = useState<CommandEntry[]>([])
  const [query,    setQuery]    = useState('')
  const [selected, setSelected] = useState(0)
  const inputRef   = useRef<HTMLInputElement>(null)

  // Refresh command list when palette opens
  useEffect(() => {
    if (visible) {
      const reg = getRegistry()
      if (reg) setCommands(reg.commands.all())
      setTimeout(() => inputRef.current?.focus(), 0)
      setSelected(0)
    }
  }, [visible])

  const filtered = commands.filter(cmd =>
    !query ||
    cmd.title.toLowerCase().includes(query.toLowerCase()) ||
    cmd.id.toLowerCase().includes(query.toLowerCase())
  )

  const close = useCallback(() => {
    useContextKeyStore.getState().set('commandPaletteVisible', false)
    setQuery('')
    setSelected(0)
  }, [])

  const execute = useCallback((commandId: string) => {
    close()
    getRegistry()?.commands.execute(commandId)
  }, [close])

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape')    { e.preventDefault(); close() }
    else if (e.key === 'ArrowDown') { e.preventDefault(); setSelected(s => Math.min(s + 1, filtered.length - 1)) }
    else if (e.key === 'ArrowUp')   { e.preventDefault(); setSelected(s => Math.max(s - 1, 0)) }
    else if (e.key === 'Enter')     { e.preventDefault(); if (filtered[selected]) execute(filtered[selected].id) }
  }

  useEffect(() => { setSelected(0) }, [query])

  if (!visible) return null

  return (
    <div className="palette-backdrop" onClick={close} style={{ pointerEvents: 'auto' }}>
      <div className="palette-modal" onClick={e => e.stopPropagation()} onKeyDown={onKeyDown}>
        <div className="palette-input-row">
          <input
            ref={inputRef}
            className="palette-input"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Type a command..."
            autoComplete="off"
            spellCheck={false}
          />
        </div>
        <ul className="palette-list">
          {filtered.length === 0 && <li className="palette-empty">No commands found</li>}
          {filtered.map((cmd, i) => (
            <li
              key={cmd.id}
              className={`palette-item ${i === selected ? 'palette-item--selected' : ''}`}
              onClick={() => execute(cmd.id)}
              onMouseEnter={() => setSelected(i)}
            >
              <span className="palette-item-title">
                {cmd.category ? `${cmd.category}: ` : ''}{cmd.title}
              </span>
              {cmd.keybinding && (
                <span className="palette-item-keybinding">{cmd.keybinding}</span>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}
