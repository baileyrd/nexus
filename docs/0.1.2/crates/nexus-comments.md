# nexus-comments

> Kind: lib · IPC plugin id: com.nexus.comments · CorePlugin: yes · Has settings: no (only a hardcoded body-size cap; no config struct) · As of: 2026-05-25

## Overview

`nexus-comments` is the **side-margin comments subsystem** (BL-050): persistent comment threads anchored to a markdown file's stable block ids (ADR 0017). The shell's margin pane reads and writes review-style threads through this crate, but the crate itself is intentionally storage-only — no UI, no event publication. Each thread is anchored to a `block_id` (a `Uuid` the editor lazily stamps into the markdown source as a `<!-- ^<uuid> -->` marker via `com.nexus.editor::stamp_block`), so the thread stays attached to a logical block rather than a line number that shifts on every edit. Re-anchoring is the editor's responsibility; this crate never parses or rewrites the markdown — it only records the `block_id` it was handed.

Persistence is **file-as-truth in spirit**: every file's threads live in a JSON sidecar at `<forge>/.forge/comments/<relpath>.json`, where `<relpath>` mirrors the forge-relative path of the source markdown file (so `a/foo.md` and `b/foo.md` get distinct sidecars and never collide). The sidecar is the source of record for thread state; the source markdown only carries the anchor markers. The store is deliberately **stateless / cache-free**: every read and write round-trips through disk, on the rationale that comment traffic is low-volume enough that a cache would only add stale-read bugs. An empty sidecar (all threads deleted) is removed rather than left as litter.

Microkernel fit: the crate is exposed as the `com.nexus.comments` core plugin (`CommentsCorePlugin`), registered by `nexus-bootstrap`. Frontends (shell margin pane, CLI, TUI, MCP) reach it only through `context.ipc_call("com.nexus.comments", ...)`, never by linking it directly — the same pattern as `com.nexus.skills` and `com.nexus.linkpreview`. The kernel never depends on this crate.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-plugins` only (for `CorePlugin`, `PluginError`, and the `define_dispatch_helpers!` macro). The crate does **not** depend on `nexus-kernel`, `nexus-storage`, or `nexus-types` — its dispatch is synchronous and self-contained.
- **Notable external deps:** `uuid` (thread / comment / block ids), `chrono` (UTC timestamps), `regex-lite` (`@mention` extraction), `serde` + `serde_json` (wire types + sidecar JSON), `thiserror` (`CommentStoreError`), `tracing`. Optional `ts-rs` + `schemars` behind the `ts-export` feature emit TypeScript + JSON Schema bindings for the IPC arg/reply types (Audit-2026-05-01 P1-3, #113). `tempfile` is a dev-dependency for tests.
- **Crates depending on it:** `nexus-bootstrap` (registers the core plugin via `crates/nexus-bootstrap/src/plugins/comments.rs`). No other crate links it directly.

## Public API surface

- **`lib.rs`** — re-exports `CommentStore`, `CommentStoreError` (from `store`) and `Comment`, `CommentFile`, `CommentId`, `Thread`, `ThreadId` (from `types`); declares `pub mod core_plugin`.
- **`types.rs`** — the wire/persistence data model:
  - `type ThreadId = Uuid` / `type CommentId = Uuid` — stable id aliases.
  - `Comment` — one reply: `id`, optional `author`, `body` (opaque markdown/plain text), `mentions: Vec<String>` (extracted at write time), `created_at`, optional `updated_at`. `#[serde(deny_unknown_fields)]`.
  - `Thread` — one thread anchored to one block: `id`, `block_id: Uuid`, `resolved: bool`, optional `resolved_at` / `resolved_by`, `created_at`, and `comments: Vec<Comment>` (oldest-first, always non-empty). `#[serde(deny_unknown_fields)]`.
  - `CommentFile` — per-file persistence container written to the sidecar: `version: u32`, `file_path: String` (forge-relative, forward-slash), `threads: Vec<Thread>`. `CommentFile::VERSION = 1`; `CommentFile::empty(file_path)` builds a fresh empty container.
- **`store.rs`** — `CommentStore`, a forge-rooted store (`comments_root = <forge>/.forge/comments`), cheap to construct, no I/O until first call. Methods: `new`, `load`, `save`, `list_threads`, `create_thread`, `add_reply`, `set_resolved`, `delete_thread`, `delete_comment`, `edit_comment`. `CommentStoreError` enumerates `InvalidFilePath`, `Io`, `Malformed { path, source }`, `ThreadNotFound`, `CommentNotFound`, `LastCommentInThread`. Private helpers: `sidecar_path`, `normalize_relpath`, `extract_mentions`.
- **`core_plugin.rs`** — `CommentsCorePlugin` (`CorePlugin` impl), `PLUGIN_ID`, the seven `HANDLER_*` id constants, and `IPC_HANDLERS: &[(&str, u32)]` (SD-06 single source of truth for command/id pairs consumed by bootstrap). Public IPC arg structs: `FilePathArg`, `CreateThreadArgs`, `AddReplyArgs`, `SetResolvedArgs`, `DeleteThreadArgs`, `DeleteCommentArgs`, `EditCommentArgs`.

## IPC handlers

All handlers are synchronous; ids are append-only (SD-06). Returns are JSON-serialized forms of the listed type. `delete_*` return `{}`.

| Id | Command | Args | Returns | Capability | Description |
|---:|---------|------|---------|-----------|-------------|
| 1 | `list` | `{ file_path }` | `Vec<Thread>` | none declared | Load all threads anchored in the file's sidecar (empty array if no sidecar). |
| 2 | `create_thread` | `{ file_path, block_id, body, author? }` | `Thread` | none declared | Create a new thread anchored to `block_id`, seeded with one comment. Body ≤ 64 KiB. |
| 3 | `add_reply` | `{ file_path, thread_id, body, author? }` | `Comment` | none declared | Append a reply to an existing thread. Body ≤ 64 KiB. Errors `ThreadNotFound`. |
| 4 | `set_resolved` | `{ file_path, thread_id, resolved, author? }` | `Thread` | none declared | Toggle the thread's `resolved` flag; stamps/clears `resolved_at` + `resolved_by`. |
| 5 | `delete_thread` | `{ file_path, thread_id }` | `{}` | none declared | Remove a thread outright. Errors `ThreadNotFound`. |
| 6 | `delete_comment` | `{ file_path, thread_id, comment_id }` | `{}` | none declared | Remove one comment; refuses the last comment in a thread (`LastCommentInThread`). |
| 7 | `edit_comment` | `{ file_path, thread_id, comment_id, body }` | `Comment` | none declared | Edit a comment body in place; updates `updated_at`, re-extracts mentions. Body ≤ 64 KiB. |

Each handler deserializes its typed arg struct (all `#[serde(deny_unknown_fields)]`), then maps `CommentStoreError` to `PluginError` via `map_store_err` (string-formatted). `dispatch` rejects unknown handler ids with `"unknown handler id {n}"`. `define_dispatch_helpers!()` supplies `exec_err`. Bootstrap also registers `.v1` aliases for every command (ADR 0021) via `with_v1_aliases`, so e.g. `list.v1` resolves to the same handler.

## Capabilities

**None declared and none checked.** The bootstrap manifest (`core_manifest_with_ipc` with `LifecycleFlags::NONE`) registers only the seven IPC commands plus their `.v1` aliases — it carries no `[[capabilities]]` block, and the dispatch code performs no capability checks. The store performs sidecar I/O directly via `std::fs` against `<forge>/.forge/comments/`; it does **not** route writes through `fs.write` or `nexus-storage`. The `docs/0.1.2/ipc-handlers.md` note ("All forge-local thread store mutations; downstream `fs.write` gated") refers to the fact that mutations are confined to the forge-local comments directory, not to a capability gate inside this crate. Path safety is enforced by `normalize_relpath` (rejects empty, absolute, `..`, root, prefix, and non-UTF-8 segments), which is the de-facto sandbox boundary in lieu of a capability.

## Settings / Config

No config struct and no TOML file. The only tunable is a hardcoded constant, `MAX_COMMENT_BODY_BYTES = 64 * 1024` (64 KiB) in `core_plugin.rs`, enforced by `check_body_size` on `create_thread`, `add_reply`, and `edit_comment` (issue #85) to bound per-file sidecar JSON growth and watcher-reload cost.

**Sidecar format + path scheme:**
- Path: `<forge>/.forge/comments/<relpath>.json`, where `<relpath>` is the forge-relative markdown path with `.json` appended to the **file** component (e.g. `notes/foo.md` → `.forge/comments/notes/foo.md.json`). Appending to the file component lets a file and a directory share a stem without collision. The `comments_root` directory is created lazily on first write (`create_dir_all` of the sidecar's parent).
- Format: a JSON object serialized from `CommentFile` (pretty-printed) — `{ version, file_path, threads: [...] }`, schema `version = 1`. `file_path` is stored redundantly so a misplaced sidecar can be recovered. Optional/empty fields (`author`, `updated_at`, `mentions`, `resolved_at`, `resolved_by`) are skipped when empty/`None`.
- On save, when `threads` is empty the sidecar file is **deleted** (NotFound treated as success), so an empty sidecar never lingers.

## Events

**None.** The crate publishes and subscribes to no events; the module docs note the shell may instead subscribe to file-watcher events on `.forge/comments/` for live cross-window sync.

## Internals & notable implementation details

- **Data model & ordering.** A `Thread` always holds at least one `Comment` — `create_thread` seeds the thread and its first comment in a single `save`. Threads and comments are stored oldest-first. New ids use `Uuid::new_v4()` (the doc comment on `Comment.id` aspirationally describes uuid v7 / time-ordered, but the code generates v4).
- **block_id anchoring.** A thread stores the `block_id` it was created with and never mutates it. Re-anchoring on edits is the editor's job (it maintains the `<!-- ^<uuid> -->` markers); this crate has no awareness of file content or edits and does not reconcile against the source markdown.
- **Authorship & timestamps.** `author` is optional throughout (CLI without a configured git identity passes `None`). `created_at` is stamped on write; `edit_comment` sets `updated_at` and re-runs mention extraction; `set_resolved` stamps `resolved_at` + `resolved_by` only on the false→true transition and clears them on true→false (a redundant set to the same value is a no-op that still re-saves a snapshot).
- **Mention extraction.** `extract_mentions` uses `regex-lite` pattern `(?:^|[^\w])@([A-Za-z0-9_-]{1,32})` to pull `@name` tokens (1–32 chars), de-duplicated in first-seen order, deliberately skipping addresses like `foo@example.com`. Mentions are stored explicitly on each comment so callers needn't re-scan bodies.
- **Sync / atomicity.** Reads and writes are stateless round-trips through `std::fs::read` / `std::fs::write`; there is **no cache, no locking, and no atomic temp-file-rename** — `save` writes the sidecar in place. Concurrent writers to the same file could race. The crate relies on low comment volume and the shell coordinating writes.
- **Path validation.** `normalize_relpath` collapses `.` (CurDir) segments, joins with `/`, and rejects empty input, absolute paths, `..`/root/prefix components, and non-UTF-8 segments — the security boundary keeping all sidecars inside `<forge>/.forge/comments/`.
- **Error mapping.** Store errors are surfaced as `PluginError` via their `Display` string; the missing-sidecar case is *not* an error — `load` returns `CommentFile::empty` so callers treat "no comments" and "no file" identically.

## Tests

No `tests/` directory; all tests are inline `#[cfg(test)]` modules (using `tempfile::TempDir` for an isolated forge root).

- **`store.rs` tests** (store layer): missing-sidecar-returns-empty; create→list roundtrip (block_id, author, body, unresolved); reply append; reply-to-unknown-thread error; resolve/unresolve round trip (stamps + clears); delete-thread removes the now-empty sidecar; delete-thread not-found error; delete a non-last comment; refuse to delete the last comment; edit updates body + `updated_at` + mentions; nested paths don't collide (`a/foo.md` vs `b/foo.md`); reject absolute / parent-traversal / empty paths; malformed-sidecar surfaces `Malformed`; mention de-dupe + email skip; save round-trips via disk; `normalize_relpath` collapses CurDir segments.
- **`core_plugin.rs` tests** (IPC dispatch layer): list-empty returns `[]`; create→list via IPC (round-trips `block_id`); add-reply via IPC (comment count grows to 2); set_resolved true then false (checks `resolved` + `resolved_by`); delete_thread via IPC; unknown-handler id errors; edit_comment via IPC (body + `updated_at`); invalid path surfaces as a `PluginError` ("invalid file path").

Coverage is thorough for the store CRUD/validation surface and the IPC happy paths + key error paths. Not exercised: the `MAX_COMMENT_BODY_BYTES` over-limit rejection, and the `delete_comment` IPC handler (id 6) is covered only at the store layer, not via `dispatch`.
