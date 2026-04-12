# ADR 0003: Storage Owns the File Watcher

**Date:** 2026-04-11
**Status:** Accepted

## Context

File system changes must propagate to the kernel event bus so subscribers
(plugins, CLI watch mode, future GUI) can react. PRD 03 describes a `notify`
watcher with debouncing and rename detection; PRD 01 has `FileCreated`/
`FileModified`/`FileDeleted` events. Ownership wasn't spec'd.

## Decision

`nexus-storage` owns the `notify` watcher and emits events to the kernel bus.
Rename detection (hash match on Delete+Create within the debounce window)
produces a single `FileRenamed { from, to, content_hash }` event instead of
a Delete+Create pair.

## Alternatives considered

- Kernel-owned watcher with storage as subscriber: creates a cycle because
  rename detection needs content hashing which lives in storage.
- Two independent watchers: double OS handle pressure, inconsistent state,
  debounce timers fighting. Wrong.

## Consequences

- One watcher per forge, one uniform event stream.
- `nexus-kernel` gains a `FileRenamed` event variant.
- `nexus-storage` has a compile-time dep on `nexus-kernel` (already true).
