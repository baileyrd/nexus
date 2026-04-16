# Punch List

- [x] **Frontend → plugin event delivery — bidirectional bus.** Host pushes lifecycle events (file opened, forge switched, theme changed) to plugin subscribers. Medium. Completes the event-bus story.
- [x] **Core-plugin (JS) path** — `[script]` manifest section, `PluginBackend::Script` variant, frontend `scriptRuntime.ts` with Blob URL dynamic import, `nexusContext.ts` host API wrapper. `read_plugin_script` Tauri command. `hello-js` demo plugin. Frontend contribution bridge detects `runtime: "script"` and dispatches locally.
- [x] **Cleanups**: migrate palette `Mod+K` onto the keybinding dispatcher, give `workspace.settings` a default `Mod+,`, and (from this slice's leftover) maybe wire plugin-side settings reads via a WASM host import so plugins can actually use the knobs users set.
- [x] **Plugin → plugin IPC exposure** — `host::invoke_command` implemented with per-plugin backend locks to avoid re-entrancy deadlocks. `TauriIpcDispatcher` and `invoke_plugin_ipc` Tauri command + frontend wrapper added. Circular-call detection via `try_lock`.
- [x] **Host capability surface for WASM plugins** — `host::emit_event` now forwards to the Tauri frontend via `PluginEventForwarder`, so plugins can publish events mid-handler (not just via the `events` return array).
- [x] **Persisted event subscription opt-in** — Settings > Plugins tab shows each plugin's event subscriptions with toggle switches. Disabled subscriptions persisted to `subscriptions.json` and respected on reload.
- [x] **Phase 2 PRDs 07/08 — editor bootstrap.** CodeMirror 6 editor surface replaces read-only `<pre>` FileViewer. Markdown syntax highlighting, Mod+S save via `write_forge_file`, dirty-state indicator, outline scroll-to-heading integration, theme bridge via CSS variables.
