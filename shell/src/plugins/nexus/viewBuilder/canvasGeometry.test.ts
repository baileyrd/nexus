// BL-067 Phase 2c — geometry helper unit tests.

import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  canvasHeightToRealPx,
  canvasWidthToRealPx,
  computeLayout,
  COLLAPSED_SPINE,
  dividerAt,
  dragDividerToRealPx,
  extractCanvasState,
  MAX_DOCK_FRACTION,
  regionAt,
  scaleDockHeight,
  scaleDockWidth,
  TYPICAL_WORKSPACE_HEIGHT,
  TYPICAL_WORKSPACE_WIDTH,
  type MinimalSerializedNode,
} from './canvasGeometry'

// Minimal snapshot fixture builder. Mirrors the shape `workspace
// .layoutSnapshot()` produces (split-of-tabs-of-leaves) without
// reaching into the workspace store, so the helper test stays a pure
// transform check.
function dock(
  side: 'left' | 'right' | 'bottom',
  size: number,
  collapsed: boolean,
  leafTypes: string[] = [],
): MinimalSerializedNode {
  return {
    kind: 'split',
    side,
    size,
    collapsed,
    children: [
      {
        kind: 'tabs',
        leaves: leafTypes.map((type, i) => ({
          id: `${side}-leaf-${i}`,
          viewState: { type },
        })),
      },
    ],
  }
}

function mainPane(leafTypes: string[]): MinimalSerializedNode {
  return {
    kind: 'split',
    children: [
      {
        kind: 'tabs',
        leaves: leafTypes.map((type, i) => ({
          id: `main-leaf-${i}`,
          viewState: { type },
        })),
      },
    ],
  }
}

const canvas = { width: 300, height: 200 }

test('scaleDockWidth collapses to spine when collapsed', () => {
  assert.strictEqual(scaleDockWidth(280, 300, true), COLLAPSED_SPINE)
})

test('scaleDockWidth scales linearly against TYPICAL_WORKSPACE_WIDTH', () => {
  // 280 real-px on a 1200-typical canvas → 280 * (300/1200) = 70 canvas-px.
  // Within bounds (min=16, max=300*0.35=105) so the scaled value wins.
  assert.strictEqual(scaleDockWidth(280, 300, false), 70)
})

test('scaleDockWidth clamps below the visual minimum', () => {
  // 50 real-px * (300/1200) = 12.5 → clamps up to 16 (COLLAPSED_SPINE * 2).
  assert.strictEqual(scaleDockWidth(50, 300, false), COLLAPSED_SPINE * 2)
})

test('scaleDockWidth clamps above the maximum dock fraction', () => {
  // 600 real-px * (300/1200) = 150 → clamps down to 300 * 0.35 = 105.
  assert.strictEqual(scaleDockWidth(600, 300, false), 300 * MAX_DOCK_FRACTION)
})

test('scaleDockHeight uses the typical-workspace-height ratio', () => {
  // 240 real-px * (200/800) = 60 canvas-px (within bounds).
  assert.strictEqual(scaleDockHeight(240, 200, false), 60)
})

test('canvasWidthToRealPx is the inverse of scaleDockWidth (for in-range values)', () => {
  const real = 280
  const canvasPx = scaleDockWidth(real, 300, false)
  // 70 canvas-px / (300/1200) = 280 real-px.
  assert.strictEqual(canvasWidthToRealPx(canvasPx, 300), real)
})

test('canvasHeightToRealPx is the inverse of scaleDockHeight (for in-range values)', () => {
  const real = 240
  const canvasPx = scaleDockHeight(real, 200, false)
  assert.strictEqual(canvasHeightToRealPx(canvasPx, 200), real)
})

test('computeLayout: bottom dock claims height first, top is split into 3 horizontal regions', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: false, right: false, bottom: false },
  )
  // Width split: left=70, right=70, main=160 (300-70-70).
  assert.strictEqual(layout.left.w, 70)
  assert.strictEqual(layout.right.w, 70)
  assert.strictEqual(layout.main.w, 160)
  // Heights: bottom=60 (240*200/800), top=140.
  assert.strictEqual(layout.bottom.h, 60)
  assert.strictEqual(layout.left.h, 140)
  assert.strictEqual(layout.main.h, 140)
  assert.strictEqual(layout.right.h, 140)
  // Origins line up — main starts after left, right starts after main.
  assert.strictEqual(layout.left.x, 0)
  assert.strictEqual(layout.main.x, 70)
  assert.strictEqual(layout.right.x, 230)
  assert.strictEqual(layout.bottom.x, 0)
  assert.strictEqual(layout.bottom.y, 140)
  // Bottom spans the canvas full width.
  assert.strictEqual(layout.bottom.w, 300)
})

test('computeLayout: collapsed dock shrinks to the spine', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: true, right: false, bottom: false },
  )
  assert.strictEqual(layout.left.w, COLLAPSED_SPINE)
  // Main grows to absorb the collapsed left's room.
  assert.strictEqual(layout.main.w, 300 - COLLAPSED_SPINE - 70)
})

test('regionAt returns the region containing an interior point', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: false, right: false, bottom: false },
  )
  // Interior of left dock.
  assert.strictEqual(regionAt(layout, 30, 50), 'left')
  // Interior of main pane.
  assert.strictEqual(regionAt(layout, 150, 70), 'main')
  // Interior of right dock.
  assert.strictEqual(regionAt(layout, 260, 30), 'right')
  // Interior of bottom dock.
  assert.strictEqual(regionAt(layout, 100, 170), 'bottom')
})

test('regionAt returns null outside the canvas bounds', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: false, right: false, bottom: false },
  )
  assert.strictEqual(regionAt(layout, -5, 100), null)
  assert.strictEqual(regionAt(layout, 100, 250), null)
})

test('dividerAt detects vertical left / right dividers within the hit zone', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: false, right: false, bottom: false },
  )
  // Left divider sits at x=70.
  assert.strictEqual(dividerAt(layout, 70, 50), 'left')
  assert.strictEqual(dividerAt(layout, 72, 50), 'left')
  assert.strictEqual(dividerAt(layout, 68, 50), 'left')
  // Just outside the hit zone.
  assert.strictEqual(dividerAt(layout, 75, 50), null)
  // Right divider sits at x=230.
  assert.strictEqual(dividerAt(layout, 230, 50), 'right')
})

test('dividerAt detects the horizontal bottom divider', () => {
  const layout = computeLayout(
    canvas,
    { left: 280, right: 280, bottom: 240 },
    { left: false, right: false, bottom: false },
  )
  // Bottom divider at y=140.
  assert.strictEqual(dividerAt(layout, 100, 140), 'bottom')
  assert.strictEqual(dividerAt(layout, 100, 142), 'bottom')
  assert.strictEqual(dividerAt(layout, 100, 138), 'bottom')
  assert.strictEqual(dividerAt(layout, 100, 145), null)
})

test('dragDividerToRealPx maps left-divider drag to inverse-scaled real width', () => {
  // Pointer at x=80 on a 300-wide canvas → canvas-px width = 80
  // → real-px = 80 / (300/1200) = 320.
  assert.strictEqual(dragDividerToRealPx('left', canvas, 80, 100), 320)
})

test('dragDividerToRealPx maps right-divider drag (canvas.width - x)', () => {
  // Pointer at x=220 → right canvas-px = 80 → real-px = 320.
  assert.strictEqual(dragDividerToRealPx('right', canvas, 220, 100), 320)
})

test('dragDividerToRealPx clamps negative deltas to zero before scaling', () => {
  // Pointer beyond the canvas left edge — left dock can't be negative.
  assert.strictEqual(dragDividerToRealPx('left', canvas, -10, 100), 0)
})

test('extractCanvasState pulls dock sizes + collapsed flags from the snapshot', () => {
  const snap = {
    main: mainPane(['markdown']),
    left: dock('left', 280, false, ['file-explorer']),
    right: dock('right', 320, true, ['outline']),
    bottom: dock('bottom', 240, false, ['terminal']),
  }
  const state = extractCanvasState(snap, 'main-leaf-0')
  assert.deepStrictEqual(state.sizes, { left: 280, right: 320, bottom: 240 })
  assert.deepStrictEqual(state.collapsed, { left: false, right: true, bottom: false })
})

test('extractCanvasState collects leaves into the right region with active flag', () => {
  const snap = {
    main: mainPane(['markdown']),
    left: dock('left', 280, false, ['file-explorer']),
    right: dock('right', 320, false, ['outline', 'backlinks']),
    bottom: dock('bottom', 240, false, []),
  }
  const state = extractCanvasState(snap, 'right-leaf-1')
  assert.strictEqual(state.regions.left.length, 1)
  assert.strictEqual(state.regions.left[0]?.type, 'file-explorer')
  assert.strictEqual(state.regions.left[0]?.active, false)
  assert.strictEqual(state.regions.main.length, 1)
  assert.strictEqual(state.regions.main[0]?.type, 'markdown')
  assert.strictEqual(state.regions.right.length, 2)
  assert.strictEqual(state.regions.right[1]?.type, 'backlinks')
  assert.strictEqual(state.regions.right[1]?.active, true, 'active leaf flagged')
  assert.strictEqual(state.regions.bottom.length, 0)
})

test('extractCanvasState walks past root + floating wrappers', () => {
  const snap = {
    main: { kind: 'root', child: mainPane(['markdown']) } as MinimalSerializedNode,
    left: dock('left', 280, false, ['file-explorer']),
    right: dock('right', 320, false, []),
    bottom: dock('bottom', 240, false, []),
  }
  const state = extractCanvasState(snap, null)
  assert.strictEqual(state.regions.main.length, 1)
  assert.strictEqual(state.regions.main[0]?.type, 'markdown')
})

test('extractCanvasState treats missing dock as collapsed-zero', () => {
  const snap = {
    main: mainPane(['markdown']),
    left: dock('left', 280, false, ['file-explorer']),
    right: dock('right', 320, false, []),
    // bottom omitted
  }
  const state = extractCanvasState(snap, null)
  assert.strictEqual(state.sizes.bottom, 0)
  assert.strictEqual(state.collapsed.bottom, true)
  assert.strictEqual(state.regions.bottom.length, 0)
})

test('TYPICAL_WORKSPACE_WIDTH and HEIGHT are unchanged across the test suite', () => {
  // Pin the constants so a tweak doesn't silently change the visual
  // calibration without flagging the test mismatch.
  assert.strictEqual(TYPICAL_WORKSPACE_WIDTH, 1200)
  assert.strictEqual(TYPICAL_WORKSPACE_HEIGHT, 800)
})
