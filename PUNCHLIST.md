# Punch List

- [x] **Frontend → plugin event delivery — bidirectional bus.** Host pushes lifecycle events (file opened, forge switched, theme changed) to plugin subscribers. Medium. Completes the event-bus story.
- [ ] **Core-plugin (JS) path** — plugins without WASM. Big conceptual slice.
- [x] **Cleanups**: migrate palette `Mod+K` onto the keybinding dispatcher, give `workspace.settings` a default `Mod+,`, and (from this slice's leftover) maybe wire plugin-side settings reads via a WASM host import so plugins can actually use the knobs users set.
- [x] **Plugin → plugin IPC exposure** — `host::invoke_command` implemented with per-plugin backend locks to avoid re-entrancy deadlocks. `TauriIpcDispatcher` and `invoke_plugin_ipc` Tauri command + frontend wrapper added. Circular-call detection via `try_lock`.
- [x] **Host capability surface for WASM plugins** — `host::emit_event` now forwards to the Tauri frontend via `PluginEventForwarder`, so plugins can publish events mid-handler (not just via the `events` return array).
- [x] **Persisted event subscription opt-in** — Settings > Plugins tab shows each plugin's event subscriptions with toggle switches. Disabled subscriptions persisted to `subscriptions.json` and respected on reload.
- [ ] **Phase 2 PRDs 07/08** — from memory, still need the Tauri frontend scaffold work.
