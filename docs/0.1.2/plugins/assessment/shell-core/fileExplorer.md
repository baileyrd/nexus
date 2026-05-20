# fileExplorer (core)

- **Path:** `shell/src/plugins/core/fileExplorer/`
- **Tier:** Shell Core
- **Plugin id:** `core.file-explorer`

## Architecture
- Entry point: `shell/src/plugins/core/fileExplorer/index.ts:7`
- Activation: `onStartup`
- Declared `dependsOn`: `core.filesystem-service`, `core.activity-bar`
- Modules:
  - `index.ts` — manifest + activate hook
  - `FileExplorerView.tsx` — the tree view component (213 lines)
- Persistence: none in this plugin; relies on `core.filesystem-service` for IO
- Settings owned: `fileExplorer.showHidden`, `fileExplorer.sortOrder`, `ui.fileCreationNotificationMs`
- External deps: none beyond the platform dialog + notifications surfaces

## Surface
- **Commands:** `fileExplorer.openFolder`, `fileExplorer.newFile`, `fileExplorer.newFolder`, `fileExplorer.refresh`
- **Keybindings:** `ctrl+k ctrl+o` / `cmd+k cmd+o` → `fileExplorer.openFolder`
- **Views:** none registered in 0.1.2 (the Phase 7 comment at `index.ts:59` notes the `slot:'sidebarContent'` registration was removed)
- **Events emitted:** `fileExplorer:folderOpened`, `fileExplorer:refresh`
- **Settings schema:** three keys under category `files`
- **Consumes from `@nexus/extension-api`:** `Plugin`, `PluginAPI` (uses `api.platform.dialog`, `api.input.prompt`, `api.notifications.show`, `api.configuration`, `api.commands`, `api.events`)

## Necessity
- **Verdict:** Essential (role); this implementation is superseded
- **Required for basic capabilities?** Yes — browsing markdown in the desktop shell needs a file tree. The live file tree is `nexus.files` (`catalog.ts:204`, view at `shell/src/plugins/nexus/files/FileExplorerView.tsx`); `core.file-explorer` is the legacy pre-leaf-migration variant.
- **Depended on by:** nothing in `catalog.ts`; not imported by other plugins. Its commands and settings are registered only when the plugin module is loaded, and the boot path no longer loads it.
- **Depends on (declared):** `core.filesystem-service`, `core.activity-bar`
- **What breaks if removed:** Nothing in the live shell — `nexus.files` carries the user-facing tree.

## Notes
- **Dead code in 0.1.2.** Not in the catalog. The `nexus.files` plugin reuses the same view-component name (`FileExplorerView.tsx`) and supersedes the file tree role.
- `newFile` / `newFolder` here are stubs that toast "Created: …" without touching disk — note the lack of any `fsService` call; the real CRUD lives in `nexus.files`.
- Cleanup candidate: delete the directory once the on-disk settings keys (`fileExplorer.showHidden`, `fileExplorer.sortOrder`) are confirmed to be re-registered (or migrated) by `nexus.files`.
