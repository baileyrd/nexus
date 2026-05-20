# com.nexus.comments

- **Path:** `crates/nexus-comments/`
- **Tier:** Core Rust
- **Bootstrap order:** 14

## Architecture
- Entry point: `crates/nexus-comments/src/core_plugin.rs` (`CommentsCorePlugin`). Modules: `store`, `types`.
- Stateless wrapper around `CommentStore` — every dispatch reads/writes the JSON sidecar fresh from disk; no caching, no in-memory state.
- BL-050. Threads are anchored to stable block ids (`Uuid`) per ADR 0017; the editor (`com.nexus.editor::stamp_block`) is responsible for materialising the `<!-- ^<uuid> -->` markers in markdown source.
- File-as-truth: the JSON sidecar at `<forge>/.forge/comments/<relpath>.json` is the source of record.
- Registered with `LifecycleFlags::NONE` (`crates/nexus-bootstrap/src/plugins/comments.rs:26`).

## Persistence
- `<forge>/.forge/comments/<relpath>.json` — one JSON file per source markdown file, mirroring the forge tree (`src/store.rs:62`).
- 64 KiB per-comment-body size cap enforced at the IPC layer (`MAX_COMMENT_BODY_BYTES`, `core_plugin.rs:268`).

## Settings owned
- None. No config struct, no `.forge/` config file.

## External dependencies of note
- `serde_json`, `uuid`, `chrono`, `regex-lite`, `thiserror`. Pure-Rust, no native libs.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:57`):

| Id | Command | Returns |
|---:|---------|---------|
| 1 | `list` | `Vec<Thread>` for a file |
| 2 | `create_thread` | New `Thread` anchored to a `block_id` |
| 3 | `add_reply` | Appended `Comment` |
| 4 | `set_resolved` | `Thread` with flipped resolved flag + optional resolver |
| 5 | `delete_thread` | `{}` |
| 6 | `delete_comment` | `{}` |
| 7 | `edit_comment` | Updated `Comment` |

No event publication, no kernel-bus subscriptions, no IPC outcalls. Shell side-margin pane (`shell-nexus` `comments`) drives it via `ipc_call`.

## Necessity
- **Verdict:** Optional
- **Required for basic capabilities?** No — opening, browsing, editing, searching, and committing markdown does not require comment threads.
- **Depended on by:** shell-nexus `comments` plugin only. Nothing in the kernel or other core plugins requires it.
- **Depends on:** `com.nexus.editor::stamp_block` to materialise block ids (logically; not a hard runtime coupling — comments will store a thread against any UUID the caller supplies).
- **What breaks if removed:** the side-margin comments UI and its persistence. The markdown file itself remains intact — block-id stamps are inert HTML comments.

## Notes
- Implementation is small (4 source files) and clean. Path-traversal guard at the store layer rejects absolute / escaping paths (test at `core_plugin.rs:518`).
- No `category` field surfaces in the registration manifest; manifest comes from `core_manifest_with_ipc` which provides only `(id, name, ipc_handlers)`.
