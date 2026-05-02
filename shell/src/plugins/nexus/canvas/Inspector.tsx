// Phase-4 inspector drawer. Shows editable properties for whichever
// target is currently selected — a single node or a single edge.
// Edits commit through the parent's `onUpdateNode` / `onUpdateEdge`
// hooks, which push the matching `*_update` patch + inverse so undo/
// redo covers property changes the same way it covers structure ones.
//
// Inputs commit on blur (plus Enter for the single-line fields) so a
// rapid keystroke run produces one history entry, not one per
// character. Colour + select inputs commit on change because they're
// already discrete interactions.

import { useEffect, useState } from 'react'
import type {
  CanvasBackground,
  CanvasEdge,
  CanvasEdgeType,
  CanvasNode,
} from './kernelClient'
import { useConfigValue } from '../../../stores/configStore'

const DEFAULT_COLOR_SWATCHES = ['#ef4444', '#f59e0b', '#eab308', '#22c55e', '#3b82f6', '#8b5cf6', '#ec4899']

interface Props {
  node: CanvasNode | null
  edge: CanvasEdge | null
  /** Canvas-level background when `node` + `edge` are both null and
   *  the user has opened the document inspector. Passing `undefined`
   *  suppresses the doc section entirely (selection-based UX only). */
  docBackground?: CanvasBackground | null
  showDocInspector?: boolean
  onUpdateNode: (next: CanvasNode, prev: CanvasNode) => void
  onUpdateEdge: (next: CanvasEdge, prev: CanvasEdge) => void
  onUpdateBackground?: (next: CanvasBackground | null, prev: CanvasBackground | null) => void
  onCloseDocInspector?: () => void
}

export function Inspector({
  node,
  edge,
  docBackground,
  showDocInspector,
  onUpdateNode,
  onUpdateEdge,
  onUpdateBackground,
  onCloseDocInspector,
}: Props) {
  return (
    <aside data-canvas-export-exclude="true" style={drawerStyle}>
      {node && <NodeInspector node={node} onUpdate={onUpdateNode} />}
      {edge && <EdgeInspector edge={edge} onUpdate={onUpdateEdge} />}
      {!node && !edge && showDocInspector && onUpdateBackground && (
        <DocInspector
          background={docBackground ?? null}
          onUpdate={onUpdateBackground}
          onClose={onCloseDocInspector}
        />
      )}
    </aside>
  )
}

function DocInspector({
  background,
  onUpdate,
  onClose,
}: {
  background: CanvasBackground | null
  onUpdate: (next: CanvasBackground | null, prev: CanvasBackground | null) => void
  onClose?: () => void
}) {
  const [color, setColor] = useState(background?.color ?? '#1f1f23')
  const [pattern, setPattern] = useState<string>(background?.pattern ?? '')
  useEffect(() => {
    setColor(background?.color ?? '#1f1f23')
    setPattern(background?.pattern ?? '')
  }, [background?.color, background?.pattern])

  const commit = (nextColor: string, nextPattern: string) => {
    const next: CanvasBackground | null = nextColor
      ? { color: nextColor, ...(nextPattern ? { pattern: nextPattern } : {}) }
      : null
    const a = JSON.stringify(next)
    const b = JSON.stringify(background)
    if (a === b) return
    onUpdate(next, background)
  }

  const clear = () => {
    if (!background) return
    onUpdate(null, background)
  }

  return (
    <div>
      <Header type="CANVAS" onClose={onClose} />
      <Row label="Color">
        <input
          type="color"
          value={color}
          onChange={(e) => setColor(e.target.value)}
          onBlur={() => commit(color, pattern)}
          style={{ ...inputStyle, padding: 0, height: 26, width: 60 }}
        />
        <input
          value={color}
          onChange={(e) => setColor(e.target.value)}
          onBlur={() => commit(color, pattern)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          }}
          style={{ ...inputStyle, marginLeft: 6 }}
        />
      </Row>
      <Row label="Pattern">
        <select
          value={pattern}
          onChange={(e) => {
            const next = e.target.value
            setPattern(next)
            commit(color, next)
          }}
          style={inputStyle}
        >
          <option value="">None</option>
          <option value="dots">Dots</option>
          <option value="grid">Grid</option>
          <option value="lines">Horizontal lines</option>
        </select>
      </Row>
      <Row label=" ">
        <button type="button" onClick={clear} style={clearBtnStyle}>
          Reset to theme
        </button>
      </Row>
    </div>
  )
}

function NodeInspector({
  node,
  onUpdate,
}: {
  node: CanvasNode
  onUpdate: (next: CanvasNode, prev: CanvasNode) => void
}) {
  // Local echoes of the text inputs so typing stays responsive even
  // though the doc-level commit only happens on blur. `node.id` in the
  // dep list resets the fields when selection moves to a different
  // node without confusing a mid-edit user.
  const [label, setLabel] = useState(node.label ?? '')
  const [text, setText] = useState(node.text ?? '')
  useEffect(() => {
    setLabel(node.label ?? '')
    setText(node.text ?? '')
  }, [node.id, node.label, node.text])

  const commitLabel = () => {
    if ((node.label ?? '') === label) return
    onUpdate({ ...node, label: label || undefined }, node)
  }
  const commitText = () => {
    if ((node.text ?? '') === text) return
    onUpdate({ ...node, text }, node)
  }
  const commitColor = (color: string) => {
    const normalized = color || undefined
    if (node.color === normalized) return
    onUpdate({ ...node, color: normalized }, node)
  }

  return (
    <div>
      <Header type={`${node.type.toUpperCase()} NODE`} />

      <Row label="Label">
        <input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          onBlur={commitLabel}
          onKeyDown={(e) => {
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          }}
          style={inputStyle}
        />
      </Row>

      {node.type === 'text' && (
        <Row label="Text" align="start">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onBlur={commitText}
            rows={4}
            style={{ ...inputStyle, resize: 'vertical', fontFamily: 'inherit' }}
          />
        </Row>
      )}

      <Row label="Color">
        <ColorPicker value={node.color} onChange={commitColor} />
      </Row>

      <Row label="Size">
        <div style={{ color: 'var(--text-muted)', fontSize: 11 }}>
          {Math.round(node.width)} × {Math.round(node.height)}
        </div>
      </Row>

      <Row label="Position">
        <div style={{ color: 'var(--text-muted)', fontSize: 11 }}>
          {Math.round(node.x)}, {Math.round(node.y)}
        </div>
      </Row>
    </div>
  )
}

function EdgeInspector({
  edge,
  onUpdate,
}: {
  edge: CanvasEdge
  onUpdate: (next: CanvasEdge, prev: CanvasEdge) => void
}) {
  const [label, setLabel] = useState(edge.label ?? '')
  useEffect(() => {
    setLabel(edge.label ?? '')
  }, [edge.id, edge.label])

  const commitLabel = () => {
    if ((edge.label ?? '') === label) return
    onUpdate({ ...edge, label: label || undefined }, edge)
  }
  const commitColor = (color: string) => {
    const normalized = color || undefined
    if (edge.color === normalized) return
    onUpdate({ ...edge, color: normalized }, edge)
  }
  const commitType = (type: CanvasEdgeType) => {
    if ((edge.type ?? 'solid') === type) return
    onUpdate({ ...edge, type }, edge)
  }

  return (
    <div>
      <Header type="EDGE" />

      <Row label="Label">
        <input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          onBlur={commitLabel}
          onKeyDown={(e) => {
            if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
          }}
          style={inputStyle}
        />
      </Row>

      <Row label="Color">
        <ColorPicker value={edge.color} onChange={commitColor} />
      </Row>

      <Row label="Style">
        <select
          value={edge.type ?? 'solid'}
          onChange={(e) => commitType(e.target.value as CanvasEdgeType)}
          style={inputStyle}
        >
          <option value="solid">Solid</option>
          <option value="dashed">Dashed</option>
          <option value="dotted">Dotted</option>
        </select>
      </Row>
    </div>
  )
}

function Header({ type, onClose }: { type: string; onClose?: () => void }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        fontSize: 10,
        letterSpacing: 0.8,
        color: 'var(--text-muted)',
        fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
        marginBottom: 12,
      }}
    >
      <span style={{ flex: 1 }}>{type}</span>
      {onClose && (
        <button
          type="button"
          onClick={onClose}
          style={{
            background: 'transparent',
            border: 'none',
            color: 'var(--text-muted)',
            fontSize: 14,
            cursor: 'pointer',
            lineHeight: 1,
            padding: 0,
          }}
          aria-label="Close"
        >
          ×
        </button>
      )}
    </div>
  )
}

function Row({
  label,
  children,
  align = 'center',
}: {
  label: string
  children: React.ReactNode
  align?: 'center' | 'start'
}) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '72px 1fr',
        alignItems: align,
        gap: 8,
        marginBottom: 10,
      }}
    >
      <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>{label}</div>
      <div>{children}</div>
    </div>
  )
}

/** Small swatch row + "clear" button. Custom colour is keyed through
 *  the native picker so we stay on platform styles. */
function ColorPicker({
  value,
  onChange,
}: {
  value: string | undefined
  onChange: (next: string) => void
}) {
  const swatches = useConfigValue('canvas.colorSwatches', DEFAULT_COLOR_SWATCHES) as string[]
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexWrap: 'wrap' }}>
      {swatches.map((c) => (
        <button
          key={c}
          type="button"
          onClick={() => onChange(c)}
          aria-label={`Set colour ${c}`}
          style={{
            width: 18,
            height: 18,
            borderRadius: 9,
            background: c,
            border:
              value?.toLowerCase() === c
                ? '2px solid var(--text-normal)'
                : '1px solid var(--divider-color)',
            padding: 0,
            cursor: 'pointer',
          }}
        />
      ))}
      <input
        type="color"
        value={value ?? '#808080'}
        onChange={(e) => onChange(e.target.value)}
        style={{
          width: 22,
          height: 22,
          padding: 0,
          border: 'none',
          background: 'transparent',
          cursor: 'pointer',
        }}
      />
      {value && (
        <button
          type="button"
          onClick={() => onChange('')}
          style={{
            fontSize: 10,
            background: 'transparent',
            color: 'var(--text-muted)',
            border: 'none',
            cursor: 'pointer',
            padding: 0,
          }}
        >
          clear
        </button>
      )}
    </div>
  )
}

const drawerStyle: React.CSSProperties = {
  position: 'absolute',
  top: 12,
  right: 12,
  width: 260,
  maxHeight: 'calc(100% - 24px)',
  overflowY: 'auto',
  padding: 14,
  borderRadius: 8,
  background: 'var(--background-secondary)',
  border: '1px solid var(--divider-color)',
  boxShadow: '0 4px 16px rgba(0, 0, 0, 0.35)',
  color: 'var(--text-normal)',
  fontSize: 12,
  fontFamily: 'var(--font-family, system-ui, sans-serif)',
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  padding: '4px 6px',
  background: 'var(--bg-muted)',
  border: '1px solid var(--divider-color)',
  borderRadius: 4,
  color: 'var(--text-normal)',
  fontSize: 12,
  boxSizing: 'border-box',
}

const clearBtnStyle: React.CSSProperties = {
  padding: '3px 10px',
  background: 'var(--bg-muted)',
  color: 'var(--text-normal)',
  border: '1px solid var(--divider-color)',
  borderRadius: 4,
  fontSize: 11,
  cursor: 'pointer',
  fontFamily: 'inherit',
}
