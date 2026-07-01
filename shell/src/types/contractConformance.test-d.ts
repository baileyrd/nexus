/**
 * Compile-only conformance test for the common plugin contract (#187).
 *
 * In-process twin of
 * `packages/nexus-extension-api/src/contractConformance.test-d.ts`:
 * asserts the shell's live `PluginAPI` structurally satisfies the
 * package's `NexusPluginContext`. Participates in
 * `pnpm --filter nexus-shell typecheck` (tsconfig includes `src` and
 * only excludes `*.test.ts(x)`); no runtime.
 *
 * A `tsc` failure here means either a `PluginAPI` edit dropped below
 * the common contract or a contract edit outgrew this tier. Fix the
 * shape; do not delete the assertion.
 */

import type { NexusPluginContext } from '@nexus/extension-api'

import type { PluginAPI } from './plugin'

// `A extends B` mirrors structural assignability — the same check the
// compiler performs where a `PluginAPI` value is passed to code typed
// against the common contract.
type Extends<A, B> = A extends B ? true : false
type Assert<T extends true> = T

// ─── The headline assertion (#187) ──────────────────────────────────────────

export type InProcessTierConforms = Assert<
  Extends<PluginAPI, NexusPluginContext>
>

// ─── Per-namespace probes ────────────────────────────────────────────────────
// When the headline assertion breaks, these narrow the blast radius to
// the offending namespace instead of one opaque compiler error. Keep in
// lockstep with the members of `NexusPluginContext`.

declare const api: PluginAPI
declare function expectType<T>(value: T): void

expectType<NexusPluginContext['pluginId']>(api.pluginId)
expectType<NexusPluginContext['commands']>(api.commands)
expectType<NexusPluginContext['kernel']>(api.kernel)
expectType<NexusPluginContext['platform']>(api.platform)
expectType<NexusPluginContext['events']>(api.events)
expectType<NexusPluginContext['storage']>(api.storage)
expectType<NexusPluginContext['notifications']>(api.notifications)
expectType<NexusPluginContext['context']>(api.context)
expectType<NexusPluginContext['input']>(api.input)
expectType<NexusPluginContext['uri']>(api.uri)
expectType<NexusPluginContext['activityBar']>(api.activityBar)
expectType<NexusPluginContext['statusBar']>(api.statusBar)
