# editorArea (core)

- **Path:** `shell/src/plugins/core/editorArea/`
- **Tier:** Shell Core
- **Plugin id:** `core.editor-area`

## Architecture
- Entry point: `shell/src/plugins/core/editorArea/index.ts:7`
- Activation: `onStartup`
- Modules:
  - `index.ts` — manifest + activate hook (commands, keybindings, context-key wiring)
  - `editorStore.ts` — Zustand tab store (`tabs`, `activeTabId`, `openTab`, `closeTab`, `setActiveTab`, `pinTab`)
  - `EditorAreaView.tsx` — tab strip + content surface (no longer mounted by `SlotRegistry`, see Phase 7 comment at `index.ts:30`)
  - `MarkdownDoc.tsx` — markdown renderer + heading extractor; exports the `Heading` type
- Persistence: in-memory tab list
- Settings owned: none
- External deps: none beyond shell-internal types

## Surface
- **Commands:** `editor.closeTab`, `editor.closeAllTabs`, `editor.nextTab`, `editor.previousTab`, `editor.pinTab` (all category `Editor`)
- **Keybindings:** `ctrl+w` close, `ctrl+tab` / `ctrl+shift+tab` cycle, all gated on `when: editorFocus`
- **Views:** none (Phase 7 removed the `slot: 'editorArea'` registration)
- **Context keys set:** `editorFocus`, `editorHasTabs`
- **Consumes from `@nexus/extension-api`:** `Plugin`, `PluginAPI` types only

## Necessity
- **Verdict:** Essential (role); this implementation is superseded
- **Required for basic capabilities?** Yes — editing markdown requires *an* editor area. The active editing surface in 0.1.2 is `nexus.editor` (`catalog.ts:210`) backed by CodeMirror; this directory is the retired pre-leaf-migration surface.
- **Depended on by (live shell):** `shell/src/stores/docStore.ts:6` still imports the `Heading` type from `core/editorArea/MarkdownDoc`; nothing imports the plugin module itself.
- **Depends on:** nothing
- **What breaks if removed:** Remove the plugin (`index.ts`, `EditorAreaView.tsx`, `editorStore.ts`) and nothing in the live shell breaks. Removing `MarkdownDoc.tsx` would break `docStore`'s `Heading` import — that type belongs in `types/` and should be moved before deleting the rest.

## Notes
- **Largely dead code.** Not registered in `catalog.ts`. The Phase 7 comment at `index.ts:30` documents that the slot registration was removed; the commands are still registered but their `useEditorStore` is a separate store from `nexus.editor`'s `editorStore` (`shell/src/plugins/nexus/editor/editorStore.ts`), so they would only operate on tabs nobody opens.
- `MarkdownDoc.tsx` (443 lines) is the largest file in the directory and is the only artefact with a live external consumer (`docStore.ts`). The `nexus.editor` markdown render path has its own implementation (`shell/src/plugins/nexus/editor/markdownRender.test.ts` notes the parallel test).
- Cleanup candidate: relocate `Heading` to a shared types module, then delete the rest.
