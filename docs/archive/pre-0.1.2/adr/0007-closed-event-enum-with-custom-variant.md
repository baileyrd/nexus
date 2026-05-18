# ADR 0007: Closed Event Enum with Custom Variant

**Date:** 2026-04-11
**Status:** Accepted

## Context

The `NexusEvent` enum must carry events from all 17 PRDs plus plugin-emitted
signals. A monolithic enum scales poorly if plugins need to emit their own
types; an open trait-object system loses pattern-matching exhaustiveness.

## Decision

Closed enum for kernel-owned events (one variant per subsystem concept, added
per phase). Single `NexusEvent::Custom { type_id, emitting_plugin, payload }`
variant for plugin-emitted signals. Plugins cannot emit kernel events.
`type_id` must start with the emitting plugin's id (reverse-DNS namespace);
enforced by the kernel. Bounded broadcast channel, capacity 2048, with
`Lagged(n)` on slow subscribers.

## Alternatives considered

- Open trait-object events: loses exhaustive pattern matching.
- All events via `Custom`: no compile-time help for kernel-side subsystems.

## Consequences

- Each phase adds kernel events by editing `nexus-kernel`, forcing explicit
  cross-phase coordination via compile errors.
- Plugin events are type-unsafe at the payload level (JSON blob); plugin
  authors deserialize at the boundary.
- Anti-spoofing is enforced by construction: the kernel sets
  `emitting_plugin` from the calling plugin's identity.
