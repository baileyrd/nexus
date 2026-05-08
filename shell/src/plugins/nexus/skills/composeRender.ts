// shell/src/plugins/nexus/skills/composeRender.ts
//
// AIG-01 follow-up — annotate the merged composed-skill body so the
// user can see at a glance which ancestor contributed each section.
//
// The kernel writes the merged body as a sequence of
// `## Skill: <name> [<id>]\n<body>` blocks separated by blank lines.
// We slice on those markers and tag each span with the originating
// fragment id; the renderer then tints the gutter so a layered
// composition reads like a stack of contributions instead of one flat
// blob.

export interface FragmentSpan {
  /** Fragment id this span belongs to, or `null` for content that
   *  precedes the first heading (shouldn't happen with the kernel's
   *  current writer, but kept defensive). */
  fragmentId: string | null
  text: string
  /** True for the `## Skill: …` heading line itself, false for the
   *  body that follows it. The renderer styles them differently. */
  isHeading: boolean
}

export interface FragmentRef {
  id: string
  name: string
}

/**
 * Walk the merged body, locating each fragment's heading line and
 * carving out (heading, body) span pairs. Falls back to a single
 * unattributed span when the body doesn't match the expected shape
 * (so a kernel format change degrades to "no highlights" rather
 * than a render crash).
 */
export function splitMergedBody(
  merged: string,
  fragments: readonly FragmentRef[],
): FragmentSpan[] {
  if (fragments.length === 0 || merged.length === 0) {
    return [{ fragmentId: null, text: merged, isHeading: false }]
  }

  const positions: Array<{
    fragmentId: string
    headingStart: number
    headingEnd: number
  }> = []
  let cursor = 0
  for (const f of fragments) {
    const heading = `## Skill: ${f.name} [${f.id}]`
    const idx = merged.indexOf(heading, cursor)
    if (idx === -1) continue
    const lineEnd = merged.indexOf('\n', idx)
    const headingEnd = lineEnd === -1 ? merged.length : lineEnd + 1
    positions.push({ fragmentId: f.id, headingStart: idx, headingEnd })
    cursor = headingEnd
  }

  if (positions.length === 0) {
    return [{ fragmentId: null, text: merged, isHeading: false }]
  }

  const spans: FragmentSpan[] = []
  if (positions[0].headingStart > 0) {
    spans.push({
      fragmentId: null,
      text: merged.slice(0, positions[0].headingStart),
      isHeading: false,
    })
  }
  for (let i = 0; i < positions.length; i++) {
    const cur = positions[i]
    const next = positions[i + 1]
    spans.push({
      fragmentId: cur.fragmentId,
      text: merged.slice(cur.headingStart, cur.headingEnd),
      isHeading: true,
    })
    const bodyEnd = next ? next.headingStart : merged.length
    if (bodyEnd > cur.headingEnd) {
      spans.push({
        fragmentId: cur.fragmentId,
        text: merged.slice(cur.headingEnd, bodyEnd),
        isHeading: false,
      })
    }
  }
  return spans
}

/**
 * Deterministic 8-step palette indexed by fragment position. We use
 * HSL hues spaced around the wheel so adjacent fragments contrast
 * cleanly even on light themes; saturation/lightness are chosen for
 * a low-contrast tint (gutter accent + faint background wash, not a
 * full repaint).
 */
const PALETTE_STEPS = 8

export function fragmentTint(index: number): { border: string; background: string } {
  const hue = ((index % PALETTE_STEPS) * 360) / PALETTE_STEPS
  return {
    border: `hsl(${hue} 60% 55%)`,
    background: `hsl(${hue} 60% 55% / 0.07)`,
  }
}
