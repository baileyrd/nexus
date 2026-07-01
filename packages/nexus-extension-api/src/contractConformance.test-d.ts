/**
 * Compile-only conformance test for the common plugin contract (#187).
 *
 * Locks the V9 gating signal from `CONTRACT_STATUS.md`: the sandbox
 * runtime's context shape must structurally satisfy
 * `NexusPluginContext`. The in-process twin lives at
 * `shell/src/types/contractConformance.test-d.ts` (it needs the
 * shell-side `PluginAPI`, which this package cannot import).
 *
 * No runtime — a `tsc` failure here means a contract edit broke one of
 * the tiers. Fix the contract (or the runtime shape); do not delete
 * the assertion. Same compile-only convention as
 * `./sandbox/types.test-d.ts`.
 */

import type { MaybePromise, NexusPluginContext } from './index';
import type { SandboxedPluginContext } from './sandbox/context';

// `A extends B` mirrors structural assignability — the same check the
// compiler performs where a `SandboxedPluginContext` value is passed to
// code typed against the common contract.
type Extends<A, B> = A extends B ? true : false;
type Assert<T extends true> = T;

// ─── The headline assertion (#187) ──────────────────────────────────────────

export type SandboxTierConforms = Assert<
  Extends<SandboxedPluginContext, NexusPluginContext>
>;

// ─── Per-namespace probes ────────────────────────────────────────────────────
// When the headline assertion breaks, these narrow the blast radius to
// the offending namespace instead of one opaque compiler error. Keep in
// lockstep with the members of `NexusPluginContext`.

declare const ctx: SandboxedPluginContext;
declare function expectType<T>(value: T): void;

expectType<NexusPluginContext['pluginId']>(ctx.pluginId);
expectType<NexusPluginContext['commands']>(ctx.commands);
expectType<NexusPluginContext['kernel']>(ctx.kernel);
expectType<NexusPluginContext['platform']>(ctx.platform);
expectType<NexusPluginContext['events']>(ctx.events);
expectType<NexusPluginContext['storage']>(ctx.storage);
expectType<NexusPluginContext['notifications']>(ctx.notifications);
expectType<NexusPluginContext['context']>(ctx.context);
expectType<NexusPluginContext['input']>(ctx.input);
expectType<NexusPluginContext['uri']>(ctx.uri);
expectType<NexusPluginContext['activityBar']>(ctx.activityBar);
expectType<NexusPluginContext['statusBar']>(ctx.statusBar);

// ─── MaybePromise sanity ─────────────────────────────────────────────────────
// Both a plain value and a promise inhabit the union — the property the
// whole sync/async bridge rests on.

expectType<MaybePromise<number>>(1);
expectType<MaybePromise<number>>(Promise.resolve(1));
