// src/plugins/core/terminal/TerminalView.tsx
// Simple textarea-based terminal placeholder.
// Replace with xterm.js + Tauri shell plugin for a real PTY.

import { useEffect, useRef } from 'react'
import { useTerminalStore } from './terminalStore'
import { useConfigValue } from '../../../stores/configStore'

export function TerminalView() {
  const { lines, input, addLine, setInput } = useTerminalStore()
  const bottomRef = useRef<HTMLDivElement>(null)
  const fontSize = useConfigValue('terminal.fontSize', 13)
  const fontFamily = useConfigValue('terminal.fontFamily', 'monospace')

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [lines])

  const handleKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key !== 'Enter') return
    const cmd = input.trim()
    if (!cmd) return

    addLine(`$ ${cmd}`, 'input')
    setInput('')

    // Simple built-in commands — real terminal would use Tauri shell plugin
    if (cmd === 'clear') {
      useTerminalStore.getState().clear()
    } else if (cmd === 'help') {
      addLine('Available: clear, help, echo <text>', 'output')
    } else if (cmd.startsWith('echo ')) {
      addLine(cmd.slice(5), 'output')
    } else {
      addLine(`Command not found: ${cmd.split(' ')[0]}`, 'error')
      addLine('(Real shell execution requires tauri-plugin-shell)', 'system')
    }
  }

  return (
    <div className="terminal" style={{ fontSize, fontFamily: String(fontFamily) }}>
      <div className="terminal-output">
        {lines.map(line => (
          <div key={line.id} className={`terminal-line terminal-line--${line.type}`}>
            {line.text}
          </div>
        ))}
        <div ref={bottomRef} />
      </div>
      <div className="terminal-input-row">
        <span className="terminal-prompt">$</span>
        <input
          className="terminal-input"
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          spellCheck={false}
          autoComplete="off"
          placeholder="Enter command..."
        />
      </div>
    </div>
  )
}
