// shell/src/testing/typedStubs.ts
//
// V17 (repo-review-2026-06-10) — typed test-stub helpers, replacing the
// scattered `api as any` casts in colocated unit tests.
//
// `DeepPartial<T>` makes every property optional at every depth while
// keeping function signatures INTACT: a stubbed method must match the real
// signature exactly (wrong parameter lists, return types, or excess
// properties fail to compile), which is precisely the checking `as any`
// threw away. `stubPluginAPI` then performs the one unavoidable widening
// from the checked partial to the full interface in a single audited
// place, so individual tests never cast at the call site.

import type { PluginAPI } from '../types/plugin'

/**
 * Recursive `Partial` that leaves callables alone. Functions are matched
 * first so a stubbed method keeps the full (possibly generic) signature
 * of the real API rather than being decomposed property-by-property.
 */
export type DeepPartial<T> = T extends (...args: never[]) => unknown
  ? T
  : T extends ReadonlyArray<infer U>
    ? ReadonlyArray<DeepPartial<U>>
    : T extends object
      ? { [K in keyof T]?: DeepPartial<T[K]> }
      : T

/**
 * Widen a structurally-checked partial `PluginAPI` stub to the full
 * interface for handing to code under test. Tests only wire the surfaces
 * the code under test actually touches (typically `kernel.invoke`);
 * touching an un-stubbed surface throws a TypeError at the point of use —
 * a crisp test failure rather than a silent no-op.
 */
export function stubPluginAPI(stub: DeepPartial<PluginAPI>): PluginAPI {
  // The widening below is the entire point of this helper: the input was
  // fully type-checked against the real PluginAPI shape above, and a
  // partial mock is by definition not the full interface, so this is the
  // one place a test-side conversion is unavoidable.
  return stub as unknown as PluginAPI
}
