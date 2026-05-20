# com.nexus.terminal

- **Path:** `crates/nexus-terminal/`
- **Tier:** Core Rust
- **Bootstrap order:** 22 (penultimate — only `collab` registers after it)

## Architecture

- Entry point: `crates/nexus-terminal/src/core_plugin.rs` (`TerminalCorePlugin`).
- Bootstrap wiring: `crates/nexus-bootstrap/src/plugins/terminal.rs:21` — manifest from `IPC_HANDLERS`, `LifecycleFlags::NONE`. Bootstrap opens three SQLite stores against `<forge>/.forge/procmgr.sqlite` (saved-commands + ad-hoc history, separate tables) and `<forge>/.forge/sessions.sqlite` + `<forge>/.forge/sessions/<session_id>/scrollback.bin` (eviction-persisted scrollback for cross-session search), attaches them to the plugin, then wires:
  - `MemoryLimits::default_recommended()` (BL-061: 250 MB soft / 500 MB hard).
  - `EvictionPersister` (BL-062) that dumps scrollback to disk on LRU eviction.
  - `SqliteSessionStore` (BL-063) shared with the `cross_session_search` handler.
  - The kernel `EventBus` for streaming PTY output as events (Phase 2 WI-12).
  - Each store opens best-effort: a failure is logged and the corresponding handler family returns errors without aborting plugin load.
- Key modules: `session.rs`, `manager.rs` (in-memory session registry), `buffer.rs` (ring buffer), `ansi.rs`, `lines.rs`, `urls.rs`, `compound.rs`, `precmd.rs`, `procmgr.rs`, `memory.rs` (BL-061 poller), `persist.rs` (LRU + persister), `saved.rs` (`SqliteSavedCommandStore`), `adhoc.rs` (`SqliteAdHocStore`), `server.rs` (REPL handlers, BL-???), `external_terminal.rs` (`open_in_terminal`), `job_object.rs` (Windows process-tree termination), `ai.rs` (`suggest` handler, BL-064), `shell.rs`, `env.rs`, `profile.rs`, `handlers/`.
- Persistence (all under `<forge>/.forge/`):
  - `procmgr.sqlite` — saved commands + ad-hoc history (two tables, two `Connection`s).
  - `sessions.sqlite` — FTS5 cross-session search index for evicted scrollback (BL-063).
  - `sessions/<session_id>/scrollback.bin` — raw scrollback dumps from the eviction persister.
- Settings owned: in-crate `MemoryLimits` (defaults are hardcoded — flagged in `docs/0.1.2/settings/hardcoded-rust.md`). The forge config files at `docs/0.1.2/settings/forge-config.md` do not currently include a terminal-specific TOML.
- External dependencies of note: `portable-pty` (native PTY), Unix `libc::kill` for SIGINT/SIGTERM escalation, Windows Job Objects via `windows-sys` (PRD-09 §5.3), `rusqlite` (bundled), `tokio` (`time` feature for the BL-064 AI-suggest timeout).

## Surface

29 IPC commands (from `core_plugin.rs:278` `IPC_HANDLERS`):

- Session lifecycle: `create_session`, `close_session`, `send_input`, `send_raw_input`, `pump`, `read_output`, `read_raw_since`, `search_output`, `wait_for_pattern`, `get_session_info`, `list_sessions`, `resize`.
- Saved commands: `saved_list`, `saved_create`, `saved_update`, `saved_delete`, `saved_reorder`, `run_saved`.
- Ad-hoc history: `adhoc_list`, `adhoc_get`, `adhoc_delete`, `adhoc_promote`.
- External / cross-session / AI: `open_in_terminal`, `cross_session_search`, `suggest`.
- REPL: `repl_start`, `repl_eval`, `repl_stop`, `repl_list`.

Events: PTY byte-streams via kernel events (Phase 2 WI-12 — `with_event_bus`).

## Necessity

- **Verdict:** Optional.
- **Required for basic capabilities?** No. None of the basic-capabilities flows (open forge, browse, edit + save, search, git commit) touch a terminal session. The terminal is a discrete feature surface (panel in the shell, AI-assisted commands, saved commands).
- **Depended on by:** the shell terminal plugin and the shell's "Open in terminal" actions; the agent / AI surface (BL-064) for the `suggest` handler. No other Rust core plugin depends on the crate.
- **Depends on:** `nexus-kernel`, `nexus-plugins`, `nexus-types`. No service-crate deps.
- **What breaks if removed:** the in-shell integrated terminal panel, saved-command palette, ad-hoc history, cross-session search, "open in external terminal", REPL evaluation. The user can still run their shell outside Nexus.

## Notes

- `lib.rs:18` still describes the crate as "Phase A: PTY primitive" and says "not a core plugin yet — mirroring … `nexus-git`". That intro is stale: this **is** a core plugin (registered at `nexus-bootstrap/src/plugins/terminal.rs`, manifest built from `IPC_HANDLERS`). Same story for `nexus-git` — also a registered core plugin at this point. Worth fixing the lib doc.
- All three SQLite stores open best-effort; missing stores degrade gracefully (session IPC stays usable even when SQLite is unavailable).
- BL-064's AI-suggest path uses `tokio::time` for a timeout on the enrichment IPC but does **not** start a runtime here — it depends on the kernel-side dispatch.
- Windows uses Job Objects to atomically terminate process trees; the Unix path uses `libc::kill` for SIGINT/SIGTERM before falling back to `portable_pty`'s force-kill.
- `MemoryLimits::default_recommended` is hardcoded at 250 MB soft / 500 MB hard. Per-saved-command overrides flow through `SavedCommand.memory_limit_mb`.
