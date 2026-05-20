# fileSystemService

- **Path:** `shell/src/plugins/core/fileSystemService/`
- **Tier:** Shell Core
- **Plugin id:** `core.filesystem-service`

## Architecture
- Entry point: `shell/src/plugins/core/fileSystemService/index.ts:70`
- Activation: `onStartup`
- Modules:
  - `index.ts` — defines `FilesystemService` class + registers it as the `fsService` internal service
- The class is a thin adapter over `api.platform.fs` (the sanctioned WI-25 Phase 2b surface), with one exception: `watch` still imports `watch` from `@tauri-apps/plugin-fs` because `api.platform.fs` has no watch equivalent yet (orchestrator allowlist noted in the file header).
- Persistence: none — pure passthrough; the underlying capability-gated IO is performed by the Tauri host
- Settings owned: none
- External deps: `@tauri-apps/plugin-fs` (`watch` only)

## Surface
- **Internal services registered:** `fsService` — an instance of `FilesystemService` with methods `read`, `write`, `list`, `exists`, `mkdir`, `delete`, `rename`, `watch`
- **Commands / keybindings / views / settings:** none
- **Consumes from `@nexus/extension-api`:** `FileEntry`, `FsEvent` types; uses `api.platform.fs` (`readText`, `writeText`, `readDir`, `exists`, `mkdir`, `remove`, `rename`)

## Necessity
- **Verdict:** Essential
- **Required for basic capabilities?** Yes — opening a forge, browsing the markdown tree, reading and writing files, and watching for external changes all funnel through `fsService`. Without it the shell has no normalised file IO and every plugin would have to re-implement capability-gated `api.platform.fs` calls itself.
- **Depended on by:** declared `dependsOn` from `core.file-explorer` (legacy) and from the live `nexus.files` plugin via the `fsService` lookup. Indirect consumers include any plugin needing change notifications.
- **Depends on:** `api.platform.fs` (the kernel-mediated capability surface) and the Tauri `plugin-fs` `watch` primitive
- **What breaks if removed:** File reads/writes, directory listings, and `watch` notifications break for every plugin that uses `fsService`; the live file tree (`nexus.files`) loses its IO backbone.

## Notes
- The only direct Tauri-API import outside `api.platform.fs` is the `watch` call. The header documents this as the WI-23 allowlist exception; closing the gap requires adding `watch` to `api.platform.fs`.
- The local `RawWatchEvent` narrowing is defensive: the published SDK type for the Tauri watcher payload is loose (`{ type?: unknown; paths?: unknown }`), so we coerce to the `FsEvent.kind` enum here.
