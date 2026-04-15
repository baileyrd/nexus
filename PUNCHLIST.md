# Punch List

- [x] **Frontend → plugin event delivery — bidirectional bus.** Host pushes lifecycle events (file opened, forge switched, theme changed) to plugin subscribers. Medium. Completes the event-bus story.
- [ ] **Core-plugin (JS) path** — plugins without WASM. Big conceptual slice.
- [x] **Cleanups**: migrate palette `Mod+K` onto the keybinding dispatcher, give `workspace.settings` a default `Mod+,`, and (from this slice's leftover) maybe wire plugin-side settings reads via a WASM host import so plugins can actually use the knobs users set.
- [ ] **Plugin → plugin IPC exposure** — you have `dispatch_ipc_checked` on the loader but no Tauri command/UI for it. Lets plugins call each other.
- [ ] **Host capability surface for WASM plugins** — `ctx.publish_event` inside a handler (currently they can only emit events by returning an `events` array).
- [ ] **Persisted event subscription opt-in** — surface event subscribers in the Settings > Plugins tab so users can see/toggle them.
- [ ] **Phase 2 PRDs 07/08** — from memory, still need the Tauri frontend scaffold work.
