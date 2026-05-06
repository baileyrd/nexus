// WCAG 2.1 contrast-ratio utilities.
// All functions are pure and dependency-free.

/** Parse #rgb or #rrggbb → [r, g, b] in [0, 255]. Returns null for non-hex. */
export function parseHex(color: string): [number, number, number] | null {
  const s = color.trim()
  if (/^#[0-9a-fA-F]{3}$/.test(s)) {
    return [
      parseInt(s[1] + s[1], 16),
      parseInt(s[2] + s[2], 16),
      parseInt(s[3] + s[3], 16),
    ]
  }
  if (/^#[0-9a-fA-F]{6}$/.test(s)) {
    return [
      parseInt(s.slice(1, 3), 16),
      parseInt(s.slice(3, 5), 16),
      parseInt(s.slice(5, 7), 16),
    ]
  }
  return null
}

/** Linearise a single sRGB channel value (0–255) per WCAG 2.1. */
function linearise(c: number): number {
  const v = c / 255
  return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
}

/** WCAG 2.1 relative luminance (0..1) from [r, g, b] in [0, 255]. */
export function relativeLuminance(r: number, g: number, b: number): number {
  return 0.2126 * linearise(r) + 0.7152 * linearise(g) + 0.0722 * linearise(b)
}

/**
 * WCAG 2.1 contrast ratio between two CSS colour strings.
 * Returns null if either value is not a parseable hex colour (#rgb / #rrggbb).
 * A ratio ≥ 4.5 passes AA; ≥ 7 passes AAA.
 */
export function contrastRatio(fg: string, bg: string): number | null {
  const fgRgb = parseHex(fg)
  const bgRgb = parseHex(bg)
  if (!fgRgb || !bgRgb) return null
  const l1 = relativeLuminance(...fgRgb)
  const l2 = relativeLuminance(...bgRgb)
  const lighter = Math.max(l1, l2)
  const darker  = Math.min(l1, l2)
  return (lighter + 0.05) / (darker + 0.05)
}
