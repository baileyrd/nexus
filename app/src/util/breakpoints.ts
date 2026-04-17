import { useEffect, useRef, useState } from "react";

/**
 * Responsive breakpoints (PRD-07 §12.1).
 *
 * Values are the minimum window width (px) at which each named tier
 * becomes active. A width of 900px resolves to `md` because it's `>=768`
 * and `<1024`.
 */
export const BREAKPOINTS = {
  xs: 0,
  sm: 480,
  md: 768,
  lg: 1024,
  xl: 1440,
  xxl: 1920,
} as const;

export type BreakpointName = keyof typeof BREAKPOINTS;

const ORDER: BreakpointName[] = ["xs", "sm", "md", "lg", "xl", "xxl"];

export function breakpointFor(width: number): BreakpointName {
  let current: BreakpointName = "xs";
  for (const name of ORDER) {
    if (width >= BREAKPOINTS[name]) current = name;
  }
  return current;
}

/** True when `a` is strictly smaller than `b` on the breakpoint ladder. */
export function lt(a: BreakpointName, b: BreakpointName): boolean {
  return ORDER.indexOf(a) < ORDER.indexOf(b);
}

/**
 * Subscribe to the current window-width breakpoint. Returns the active
 * name (e.g. `"md"`) and the raw width so callers can drive both CSS
 * data-attributes and threshold-sensitive effects from a single source.
 *
 * SSR-safe: falls back to `lg` (1024px) when `window` is undefined.
 */
export function useBreakpoint(): { name: BreakpointName; width: number } {
  const [state, setState] = useState(() => {
    if (typeof window === "undefined") {
      return { name: "lg" as BreakpointName, width: BREAKPOINTS.lg };
    }
    const width = window.innerWidth;
    return { name: breakpointFor(width), width };
  });

  useEffect(() => {
    if (typeof window === "undefined") return;
    const onResize = () => {
      const width = window.innerWidth;
      setState((prev) => {
        const name = breakpointFor(width);
        if (name === prev.name && width === prev.width) return prev;
        return { name, width };
      });
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return state;
}

/**
 * Fire `onCross` when the breakpoint transitions downward across
 * `threshold` (e.g. from `lg` → `md`). Does not fire on upward crossings
 * so manual user expansions are not undone when the window grows again.
 *
 * Used by `WorkspaceView` to auto-collapse side panels at narrow widths
 * while leaving user-intent-driven state alone on widen.
 */
export function useBreakpointDownCross(
  current: BreakpointName,
  threshold: BreakpointName,
  onCross: () => void,
) {
  const wasAbove = useRef(!lt(current, threshold));
  useEffect(() => {
    const nowBelow = lt(current, threshold);
    if (wasAbove.current && nowBelow) {
      onCross();
    }
    wasAbove.current = !nowBelow;
  }, [current, threshold, onCross]);
}
