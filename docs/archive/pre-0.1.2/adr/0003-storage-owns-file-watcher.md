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

---

## Addendum 2026-05-12 — `FileRenamed` lives in `nexus-storage`, not `nexus-kernel`

> Appended without editing the body of the original decision. The
> *decision* — single watcher owned by storage, single uniform event
> stream — still holds. Only the filing claim under Consequences is
> superseded.

When the storage watcher landed, `FileRenamed` was filed under
`nexus-storage::watcher::StorageEvent`, not as a variant on a
kernel-owned event enum as Consequences §2 of the original ADR
implies. The current authoritative location is:

- **Enum definition:** [`crates/nexus-storage/src/watcher.rs:48`](../../crates/nexus-storage/src/watcher.rs#L48) — `StorageEvent::FileRenamed { from, to, content_hash }`.
- **Emit site:** [`crates/nexus-storage/src/watcher.rs:427`](../../crates/nexus-storage/src/watcher.rs#L427) (rename-as-pair detection in the watcher debounce loop).
- **Dispatch:** [`crates/nexus-storage/src/core_plugin.rs:1515`](../../crates/nexus-storage/src/core_plugin.rs#L1515) — translates `StorageEvent::FileRenamed` into the kernel-bus `com.nexus.storage.file_renamed` topic.

`nexus-kernel` doesn't own a `FileRenamed` enum variant. The kernel
bus carries the event as a topic-string payload, decoupling the
generic bus from storage-specific event shapes — consistent with the
file-as-truth invariant where `nexus-storage` owns the forge.

Surfaced by [DG-22](../roadmap/DOC-GAPS.md#dg-22--adr-0003-says-filerenamed-lives-in-nexus-kernel-it-doesnt) in the 2026-05-12 doc-traceability audit.
