// src/shell/ResizeHandle.tsx
// Drag handle for resizing sidebar and panel area.
// Attaches mousemove/mouseup to document to avoid losing the drag
// when the cursor moves faster than the element edge.

import { useCallback, useRef } from 'react'

interface Props {
  direction: 'horizontal' | 'vertical'
  onResize: (size: number) => void
}

export function ResizeHandle({ direction, onResize }: Props) {
  const startPos  = useRef(0)
  const startSize = useRef(0)

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()

    startPos.current  = direction === 'horizontal' ? e.clientX : e.clientY

    // Snapshot the current size of the adjacent panel
    const panel = (e.currentTarget as HTMLElement).previousElementSibling as HTMLElement | null
    startSize.current = panel
      ? direction === 'horizontal'
        ? panel.getBoundingClientRect().width
        : panel.getBoundingClientRect().height
      : 0

    const onMouseMove = (e: MouseEvent) => {
      const delta = direction === 'horizontal'
        ? e.clientX - startPos.current
        : e.clientY - startPos.current
      onResize(startSize.current + delta)
    }

    const onMouseUp = () => {
      document.removeEventListener('mousemove', onMouseMove)
      document.removeEventListener('mouseup',   onMouseUp)
    }

    document.addEventListener('mousemove', onMouseMove)
    document.addEventListener('mouseup',   onMouseUp)
  }, [direction, onResize])

  return (
    <div
      className={
        direction === 'horizontal'
          ? 'resize-handle resize-handle--h'
          : 'resize-handle resize-handle--v'
      }
      onMouseDown={onMouseDown}
    />
  )
}
