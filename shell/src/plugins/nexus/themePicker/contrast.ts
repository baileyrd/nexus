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

// ── HSL utilities (used by hue-lock in the theme builder) ────────────────────

/** Convert hex colour → [h°, s%, l%]. Returns null for non-hex. */
export function hexToHsl(color: string): [number, number, number] | null {
  const rgb = parseHex(color)
  if (!rgb) return null
  const [r, g, b] = rgb.map((v) => v / 255) as [number, number, number]
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const l = (max + min) / 2
  if (max === min) return [0, 0, l * 100]
  const d = max - min
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  let h: number
  if (max === r)      h = ((g - b) / d + (g < b ? 6 : 0)) / 6
  else if (max === g) h = ((b - r) / d + 2) / 6
  else                h = ((r - g) / d + 4) / 6
  return [h * 360, s * 100, l * 100]
}

/** Convert [h°, s%, l%] → 6-digit hex string. */
export function hslToHex(h: number, s: number, l: number): string {
  const hh = ((h % 360) + 360) % 360
  const ss = Math.max(0, Math.min(100, s)) / 100
  const ll = Math.max(0, Math.min(100, l)) / 100
  const c  = (1 - Math.abs(2 * ll - 1)) * ss
  const x  = c * (1 - Math.abs((hh / 60) % 2 - 1))
  const m  = ll - c / 2
  let r = 0, g = 0, b = 0
  if      (hh < 60)  { r = c; g = x }
  else if (hh < 120) { r = x; g = c }
  else if (hh < 180) {        g = c; b = x }
  else if (hh < 240) {        g = x; b = c }
  else if (hh < 300) { r = x;        b = c }
  else               { r = c;        b = x }
  const hex = (v: number) => Math.round((v + m) * 255).toString(16).padStart(2, '0')
  return `#${hex(r)}${hex(g)}${hex(b)}`
}

/**
 * Apply the hue + saturation delta from (prev → next) to target, keeping
 * target's lightness unchanged. Returns null if any argument is non-hex.
 * Used by the dual-mode hue-lock to keep paired colours in harmony.
 */
export function applyHueSatDelta(
  prev: string,
  next: string,
  target: string,
): string | null {
  const pH = hexToHsl(prev)
  const nH = hexToHsl(next)
  const tH = hexToHsl(target)
  if (!pH || !nH || !tH) return null
  return hslToHex(
    tH[0] + (nH[0] - pH[0]),
    Math.max(0, Math.min(100, tH[1] + (nH[1] - pH[1]))),
    tH[2],
  )
}
