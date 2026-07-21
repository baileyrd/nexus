# nexus-cli

> Kind: bin (`nexus`) · IPC plugin id: — · CorePlugin: no · As of: 2026-05-25

## Overview

`nexus-cli` is the primary headless frontend — the `nexus` binary. It is the most feature-complete consumer of `nexus-bootstrap`: a single `clap` command tree fans out to ~30 top-level subcommands covering forge management, content CRUD, search, the knowledge graph, AI, agents, git, plugins, MCP/ACP/remote servers, collab, and more. Like every Nexus frontend it owns no domain logic of its own; each subcommand handler marshals arguments into a JSON value and routes through the single IPC boundary `context.ipc_call(plugin_id, command, args)` (or one of the typed helper wrappers in `nexus_bootstrap::storage`). The CLI never links a service crate's internals — it depends on `nexus-bootstrap` for the assembled runtime and on the protocol-server crates (`nexus-mcp`, `nexus-acp`, `nexus-remote`) only to host their stdio servers.

The runtime is assembled lazily. `main()` parses `Cli`, installs the panic-log hook, sets up tracing, resolves the forge location, and constructs an `App` (`src/app.rs`) that holds the forge location, output format, and safe-mode flag but opens nothing until first use. The first subcommand handler that needs IPC calls `App::invoker()`, which lazily builds either a local `Runtime` (via `build_cli_runtime(forge_root)`) or — for `ssh://` forge URIs — a `ReconnectingRuntime` wrapping an SSH transport. A single-worker multi-thread Tokio runtime is created alongside and used to `block_on` each async `ipc_call`, so the CLI presents a synchronous façade over the async kernel.

Forge path resolution is a three-tier fallback: the `--forge-path` flag (or its `NEXUS_FORGE_PATH` env binding) wins, then bare `NEXUS_FORGE_PATH`, then `$HOME/.nexus/default`. A resolved value containing `://` is parsed as a `nexus_remote::ForgeUri` and drives the remote-forge path (`App::new_remote`); everything else is a local filesystem root (`App::new`). This is the BL-140 Phase 2b remote-forge entry point: a local `nexus` invocation can transparently drive a headless instance on another host over SSH.

Microkernel fit: the CLI sits strictly above the kernel and is invisible to it. New user-facing capability is added by writing an IPC handler in the owning `nexus-<service>` crate and a thin subcommand here that calls it — never by reaching into a service crate directly. The handler modules are deliberately uniform (`commands::<name>::<verb>(app, …)`), so the command table in `main.rs` reads as a flat dispatch map. The command surface below is the user-facing CLI contract and is the most load-bearing part of this document.

## Position in the dependency graph

- **Direct `nexus-*` deps:** `nexus-bootstrap` (the runtime assembler — the main dependency), `nexus-kernel` (event-bus / IPC traits `Events`, `Ipc`, `EventFilter`, `NexusEvent` used by streaming handlers), `nexus-types` (constants, `plugin_ids`), `nexus-plugins` (`PluginManager`, scaffolding, manifests), `nexus-security`, `nexus-collab` + `nexus-crdt` (collab serve/join + CRDT merge driver), `nexus-mcp` / `nexus-acp` / `nexus-remote` (stdio servers + `ForgeUri`), `nexus-git`, `nexus-terminal`, `nexus-tui` (the `tui` subcommand calls `nexus_tui::run_tui()`), `nexus-templates`, `nexus-formats`, `nexus-panic-log`.
- **Notable external deps:** `clap` + `clap_complete` (command tree + shell completions), `tokio` (block-on runtime for async IPC), `rustyline` (the `ai chat` REPL line editor), `ctrlc` (SIGINT handling in `term shell`), `comfy-table` (table output format), `serde_json`, `uuid` (chat/complete session ids), `toml`, `chrono`, `anyhow`, the `tracing*` stack.
- **dev-deps:** `nexus-editor`, `nexus-storage`, `tempfile` — the `nexus-storage` dep is used only by `commands/crdt.rs::tests` (and the integration tests), which exercise `StorageEngine`/`Forge` directly, bypassing IPC. Runtime CLI code routes through `bootstrap::storage` per the `dep_invariants.rs` test.
- **What depends on it:** nothing in-tree — it is the terminal binary. It is the reference frontend the MCP server, TUI, and shell mirror.

## Public API surface / module layout

This is a binary, so there is no library API. Module layout:

- `src/main.rs` (~2460 lines) — the entire `clap` derive tree (`Cli` + every `*Args`/`*Command` enum), `default_forge_path`, the `main()` dispatch table mapping each parsed subcommand to a `commands::<mod>::<fn>` call, and the top-level error handler. Also hosts the argument-parsing unit tests.
- `src/app.rs` — `App` (central lazy-init state) and `ForgeLocation` (`Local(PathBuf)` | `Remote(ForgeUri)`). Owns `invoker()`/`runtime()` (local vs. remote), the Tokio runtime, the `PluginManager` (lazy), and the BL-113 protocol-host contribution-wiring helpers (`wire_/unwire_{dap,lsp,mcp,acp}_contributions[_for_plugin]`) used around `plugin enable/disable`.
- `src/output.rs` — `OutputFormat` (`Text`/`Json`/`Jsonl`/`Table`), `from_str`, `use_color` (honours `--no-color`, `NO_COLOR`, TTY detection), and the `print_success` / `print_list` / `print_value` helpers (table rendering via `comfy-table`).
- `src/commands/mod.rs` — re-exports the 31 handler modules. One module per top-level subcommand group; each exposes free functions taking `&mut App` (or `&App` for read-only / server commands).

Representative handler modules: `ai.rs` (RAG/embed/status/config + the `chat` rustyline REPL + `complete`, both driving the shared `stream_and_collect` bus pump), `agent.rs` (plan/run with the BL-132 interactive approval loop), `serve.rs` / `mcp.rs` / `acp.rs` (stdio servers), `collab.rs` (relay serve/join + keyring token mgmt), `plugin.rs` (install/list/scaffold/enable/disable/verify/grant/revoke + external-subcommand dispatch), `git.rs` (full git surface), `crdt.rs` (git merge driver), `desktop.rs` (spawns the `nexus-shell` binary), `tui.rs` (one-liner into `nexus_tui::run_tui`), `term.rs` (PTY run/shell), plus the file/graph/db/bases/workflow/skill/template/proc/notify/logs/forge/content handlers.

## Command surface

Global flags (apply to every subcommand): `--forge-path <PATH>` (env `NEXUS_FORGE_PATH`), `--config <PATH>` (env `NEXUS_CONFIG`; accepted, wiring deferred), `--format <text|json|jsonl|table>` (default `text`), `-v/--verbose` (repeatable → WARN/INFO/DEBUG/TRACE), `-q/--quiet` (accepted; wiring deferred), `--no-color`, `--safe-mode` (env `NEXUS_SAFE_MODE`; skip community plugins at load).

Unless noted, handlers route through `context.ipc_call(plugin_id, …)`. Plugin ids referenced below are the `com.nexus.*` ids resolved via `nexus_types::plugin_ids`.

**Frontends / launchers**
- `tui` — calls `nexus_tui::run_tui()` directly (no IPC).
- `desktop [args…]` — spawns the `nexus-shell` binary as a subprocess, forwards args, propagates exit code. Resolution order: `$NEXUS_SHELL_BIN` → exe sibling → `$PATH`. Not built in the Cargo workspace.

**Forge / workspace** (`com.nexus.storage`)
- `forge init [dir] [--template os]` — init a forge (local-only path; `os` lays out the BL-054 folder scaffold). `forge status`, `forge reindex`, `forge doctor [--fix]` (file-vs-index drift, BL-137), `forge import <source> [--dry-run] [--on-conflict skip|overwrite|rename]` (BL-083).

**Content nodes** (`com.nexus.storage`, mostly via `bootstrap::storage` typed helpers)
- `content create|update <path> [--content … | --stdin]`, `content list [--prefix …]`, `content read <path> [--raw]`, `content delete <path> [-f]`, `content search <query> [-l] [--semantic | --hybrid]`, `content tasks [--completed] [--all] [--file …]`, `content task-toggle <id>`, `content links|backlinks <path>`, `content daily [--date]`, `content export <path> [-o]`.
  - `--semantic`/`--hybrid` (C78 #431; mutually exclusive) bypass lexical FTS and call `com.nexus.ai::semantic_search` directly (embedding similarity, or `hybrid: true` for RRF fusion with Tantivy BM25) — same handler the shell's "Search by Meaning" uses. Requires an AI embedding provider (`nexus ai config`).

**Knowledge graph** (`com.nexus.storage`)
- `graph status|unresolved`, `graph neighbors <path> [-d]`, `graph entity list|show|search|related|duplicates …` (BL-128 personal entity graph), `graph dream-cycle run [--phase dedup|decay|enrich|infer] [--decay-factor] [--decay-floor] [--review-threshold] [--merge-threshold] [--dry-run]` (BL-129; enrich/infer require an AI provider).
- `tags list [--name …]`.

**AI** (`com.nexus.ai`)
- `ai ask <question>` (RAG), `ai embed [--file …]` (index one/all files), `ai status`, `ai config`.
- `ai chat [--context FILE] [--model …] [--session …] [--system …]` — multi-turn streaming REPL (rustyline). Subscribes to the `com.nexus.ai.stream_*` bus prefix, invokes `stream_chat`, prints chunk events live. Slash commands: `/help /clear /save <file> /model <name> /context <file> /system <prompt> /quit|/exit`. Ctrl-C exits 130; Ctrl-D/`/quit` exit 0.
- `ai complete <file> [--line] [--col] [--context …]` — headless single-shot completion (`mode=complete`), shares the same stream pump as `chat`.
- `ai runtime list [--status …] [--limit …]`, `ai runtime get <task_id>`, `ai runtime cancel <task_id> [--reason …]`, `ai runtime pool-stats`, `ai runtime triggers` (C79 #432) — observe/control `com.nexus.ai.runtime` background tasks (agent delegate fan-out, async workflow steps); pure frontend wiring over the pre-existing observe/control handlers (`crates/nexus-cli/src/commands/ai_runtime.rs`).

**Agent** (`com.nexus.agent`)
- `agent plan <goal> [--archetype]`, `agent run <goal> [--archetype] [--interactive] [--notify-after-secs 30]`, `agent list-custom`. `--interactive` subscribes to `com.nexus.agent.round_proposed`, prompts y/n on stderr, and replies via `round_decide` (BL-132). The notify threshold dispatches a notification through `com.nexus.notifications` when a run overruns (BL-133).
- Session tree (RFC 0008): `agent sessions` (list stored sessions with `↳` fork markers), `agent show <id>` (assembled transcript), `agent resume <id> <message>` (fork at the tip), `agent branch <id> <round> <message>` (fork at a round), `agent rewind <id> <round> [message]` (non-destructive redo). All route through the `session_resume` / `session_branch` / `session_rewind` / `session_list` / `session_get` handlers.
- Checkpoints (RFC 0008): `agent checkpoint <id> <round> <name>` (bookmark a `(session, round)`), `agent checkpoints` (list), `agent checkpoint-rm <name>` (remove). Route through `session_checkpoint` / `session_checkpoints` / `session_checkpoint_delete`.
- `tool list [--capability ID]…` — list catalogued agent tools.

**Comments** (`com.nexus.comments`) — C74 (#427)
- `comments list <path>`, `comments create-thread <path> <body> [--block-index N] [--author]`, `comments add-reply <path> <thread-id> <body> [--author]`, `comments resolve|unresolve <path> <thread-id> [--author]`, `comments edit-comment <path> <thread-id> <comment-id> <body>`, `comments delete-comment <path> <thread-id> <comment-id>`, `comments delete-thread <path> <thread-id>`. Headless parity with the shell's comment pane — a non-destructive annotation channel distinct from editing the note body. `create-thread` also reaches `com.nexus.editor` (`open` → `get_tree` → `stamp_block`) to resolve `--block-index` (0-based into the file's top-level blocks, default 0) to a stable anchor, since a headless caller has no editor selection to offer.

**Database / bases**
- `bases create|list|show|add-record|query|import|export|formula …` — filesystem-layer `.bases` operations.
- `db <cmd>` — lower-level twin wrapping `com.nexus.database` IPC handlers (raw records / formulas).

**Git** (`com.nexus.git`)
- `git info|status|diff|blame|log|stage|unstage|stage-hunk|unstage-hunk|commit|branch|stash|tag|fetch|push|pull|merge|conflicts|remotes|auto-commit|set-passphrase|clear-passphrase|lfs-status|rebase|cherry-pick|abort-merge`. `set-passphrase`/`clear-passphrase` cache SSH key passphrases in the OS keyring (BL-090).
- `sync [--remote origin] [--branch] [--no-push]` — convenience wrapper: `git fetch` → `git pull` → `git push`.

**Plugins** (`PluginManager`, not IPC, plus shell-plugin filesystem ops)
- `plugin install <dir|id>` (local dir → kernel load; bare id → Phase-5 marketplace stub, exits 2), `plugin list [--shell]` (kernel plugins, or `~/.nexus-shell/plugins/`), `plugin remove <id> [-y]` (delete shell plugin dir), `plugin call <id> <command> [--args JSON]`, `plugin uninstall <id>`, `plugin enable|disable <id>` (also wire/unwire DAP/LSP/MCP/ACP contributions, BL-113), `plugin reset <id>` (clear crash quarantine), `plugin settings <id> [--set JSON]`, `plugin grant <id> <capability> [--yes]` (C82 #435, prompts unless `--yes`) and `plugin revoke <id> <capability>` (BL-096), `plugin verify <path> [--keys-dir]` (manifest signature, BL-099).
- `plugin scaffold --template script|core|community --id … --name … [--author] [-o]` — `script` (default; sandboxed JS/TS) emits plugin.json/index.ts/package.json/tsconfig.json/README.md; `core`/`community` emit a WASM Cargo project. `--type` is a back-compat alias for `--template`.

**Protocol servers** (host the respective stdio servers; build a fresh local `Runtime`)
- `mcp serve` — `nexus_mcp::NexusMcpServer` on stdio. `mcp servers|tools <server>|call <server> <tool> [--arguments JSON]` — drive the MCP host via `com.nexus.mcp`.
- `acp serve` — `nexus_acp::AcpServer` on stdio (BL-145; allow-listed `agent/*` verbs).
- `serve --stdio` — `nexus_remote::RemoteServer` on stdio: exposes the whole kernel IPC + event-bus surface (BL-140 Phase 1). Without `--stdio` it errors.

**Collab** (`com.nexus.collab` / `nexus-collab`)
- `collab serve [--port 7700] [--bind 0.0.0.0] [--token] [--save-token]` (WebSocket relay), `collab join <ws-url> [--token] [--peer-id] [--display-name] [--save-token]` (bridge local bus to a relay), `collab token set <value>|clear` (keyring `nexus.collab.token`).

**Skills / workflows / templates / migrate / proc / term / notify / logs / canvas / config / import / export / crdt**
- `skill list|show|context|triggered|reload|render` (`com.nexus.skills`).
- `workflow list|show|run|reload|validate|template {list|show|init}` (`com.nexus.workflow`); `run <name>` is a top-level alias for `workflow run`.
- `template list|apply <name> [--arg k=v]… [--target] [--overwrite] [--dry-run]` (`nexus-templates`).
- `migrate scan|registered` (forge-format versioning).
- `proc list|show|add|delete|reorder|history` (saved commands / process manager).
- `term env|run <cmd> [--timeout 30]|shell` (`nexus-terminal` PTY; `shell` installs a `ctrlc` handler; `run` exits with the child code or 124 on timeout).
- `notify send <message> [--channel|--source] [--severity] [--title]` (`com.nexus.notifications`; defaults `--source cli` when neither flag is given).
- `logs tail|show|path` (file logs) and `logs list|export|clear` (persisted audit log, `com.nexus.security`).
- `canvas create|show|add-node|add-edge`, `config show|reset`, `import notion`, `export notion`.
- `crdt merge-driver --base --ours --theirs` (git merge driver; runs without a forge), `crdt install-merge-driver [--apply]`, `crdt enable-transport` (BL-007 git-CRDT setup).

**Meta**
- `completions <shell>` — emit shell completions via `clap_complete` (no runtime).
- `<plugin-subcommand> [args…]` — `external_subcommand`: loads all community plugins and forwards to whichever registered a `[[registrations.cli_subcommand]]`; ANSI-stripped error lists available subcommands.

## IPC handlers

None. `nexus-cli` registers no IPC handlers and is not a plugin — it is exclusively an IPC *caller*. Every domain operation is dispatched through `context.ipc_call(...)` (directly or via the typed `nexus_bootstrap::storage` helpers) or, for community-plugin lifecycle, through `PluginManager` methods. The only first-class servers it stands up (`mcp/acp/serve`) are owned by `nexus-mcp`/`nexus-acp`/`nexus-remote`; the CLI just builds the runtime and hands over the context.

## Capabilities

The CLI does not declare or check capabilities itself. The invoker context returned by `build_cli_runtime` is bootstrap-granted with the full local capability set (the CLI is a trusted first-party frontend), so capability gating happens inside the service plugins behind the IPC boundary, not here. `--safe-mode` / `NEXUS_SAFE_MODE` flips `App::set_safe_mode`, which is passed into `PluginManagerConfig` so community plugins are skipped at load. `plugin grant`/`plugin revoke` mutate a plugin's granted-capability set (persisted to `granted_caps.json`, `revoke` also live-effective on the running plugin) via the plugin manager; `plugin verify` checks a manifest signature against the trusted-key ring (`~/.nexus/keys` or `--keys-dir`). Forge selection is the only ambient privilege the CLI controls (`--forge-path` / `NEXUS_FORGE_PATH`).

## Settings / Config

- **Forge path:** `--forge-path` (env-bound to `NEXUS_FORGE_PATH`) → `NEXUS_FORGE_PATH` → `$HOME/.nexus/default`. An `ssh://` value routes to the remote runtime.
- **`--config <PATH>` / `NEXUS_CONFIG`:** parsed and accepted but its forwarding into the kernel config loader is deferred (held in `main()` as `_config_path`).
- The CLI reads service config indirectly through IPC (`ai config`, `config show`, `mcp servers` reading `.forge/mcp.toml`, `git auto-commit --enable/--disable` writing `.forge/app.toml`); it owns no top-level TOML file of its own.
- **Shell completions:** `nexus completions <shell>` renders to stdout via `clap_complete::generate` for any `clap_complete::Shell` value.
- **Output:** `--format` selects the `OutputFormat`; `--no-color` / `NO_COLOR` / non-TTY disable color.

## Events

The CLI is mostly request/response, but several handlers subscribe to the kernel event bus (via `context.subscribe(EventFilter::CustomPrefix(...))`) to stream live output — these require a *local* runtime (`App::runtime()`):

- `ai chat` / `ai complete` — subscribe to `com.nexus.ai.stream_*`, filter by `session_id`, and pump `com.nexus.ai.stream_chunk` payloads to stdout until `com.nexus.ai.stream_done` (`stream_and_collect`).
- `agent run --interactive` — subscribe to `com.nexus.agent.round_proposed`, prompt for approval, reply via `round_decide`.
- `watch` — subscribe to `com.nexus.storage.*` and print filesystem-change events.
- `collab join` — subscribe to the CRDT-op + collab topic prefixes to bridge the local bus to a relay.

All other subcommands do no event subscription.

## Internals & notable implementation details

- **Lazy bootstrap:** `App` holds only the forge location, format, and safe-mode flag at construction. `invoker()` builds the runtime (or `ReconnectingRuntime` for remote) on first use; `runtime()` additionally exposes the kernel/context/event-bus and errors for remote forges ("requires a local forge"). Both share one multi-thread Tokio runtime built with one worker thread and `KERNEL_BLOCKING_POOL_SIZE` blocking threads.
- **Sync-over-async:** every handler calls `rt.block_on(invoker.ipc_call(...))` with an IPC timeout constant (`IPC_TIMEOUT_NORMAL`/`_LONG`), presenting a synchronous CLI over the async kernel.
- **Remote forge (BL-140):** `App::new_remote(ForgeUri)` defers the SSH child spawn until first `invoker()`; the transport is wrapped in `SshConnectionFactory` + `ReconnectingRuntime` so a dropped connection transparently re-spawns the remote `nexus serve --stdio` on the next call. The placeholder local `forge_root` is `<remote>`.
- **Server subcommands** (`mcp/acp/serve`) bypass `App`'s lazy runtime and build a dedicated `build_cli_runtime`, destructure the `Runtime`, move `context` into an `Arc`, and block the multi-thread Tokio runtime on the server's stdin/stdout duplex; the kernel handle is held alive until the loop exits.
- **Panic log:** `nexus_panic_log::install("nexus")` runs first thing in `main()`, before argument parsing, writing crashes to `~/.nexus-shell/logs/panic.log`.
- **SIGINT / exit codes:** `ai chat` exits 130 on Ctrl-C; `term shell` installs a `ctrlc` handler; `term run` propagates the child's status or exits 124 on timeout; `plugin install <id>` exits 2 (marketplace stub); the top-level handler prints `Error: {err:#}` to stderr and exits 1 on any handler `Err`.
- **Output formatting:** `output.rs` wraps every structured result in a `{ status, message?, data }` envelope for `json`/`jsonl`, renders `comfy-table` for `table`, and falls back to plain lines / pretty JSON for `text`.
- **External-subcommand safety:** `dispatch_external` strips ANSI escapes from plugin-supplied ids/descriptions before printing them (operator-controlled manifests are not necessarily operator-authored; issue #85).

## Tests

- **`src/main.rs` unit tests** — `clap` wiring only (no runtime): parse `content update --stdin`/`--content`, `content list [--prefix]`, `tags list [--name]`, plus a `render_long_help` smoke check that the WI-40 MCP-parity subcommands are discoverable.
- **`src/app.rs` tests** — the BL-113 contribution-wiring helpers are no-ops when no community plugins are installed / the plugin is unknown (DAP/LSP/MCP variants), against a `StorageEngine::init` temp forge.
- **`src/output.rs` tests** — `OutputFormat::from_str` round-trips; `--no-color` forces color off.
- **`src/commands/ai.rs` tests** — `slice_prompt` edge cases (whole file, line/col clipping, multibyte-safe col, context-line cap, overshoot clamp) and `build_chat_args` wire-shape.
- **`tests/cli-integration.rs`** — storage round-trips via `StorageEngine` and via `build_cli_runtime` + `bootstrap::storage` helpers (content update/list-prefix/tags), plus plugin scaffold (community + script layout assertions) and WASM load/dispatch (skipped if the fixture isn't built).
- **`tests/prd-05-smoke.rs`** — the M1 walking-skeleton lifecycle (forge init → create/read/search/query/delete), the plugin lifecycle, and scaffold validity.
- Note: end-to-end binary spawning is deliberately avoided (the test sandbox forbids executing `./target/debug/nexus`); coverage exercises the underlying IPC helpers and `clap` parse trees instead.

## Gaps / uncertainties

- `--quiet` and `--config` are parsed but their effects are not yet wired (`main()` binds them to `_quiet` / `_config_path`); documented as deferred in-code.
- The `nexus serve` help text mentions future `--port` (WebSocket) / `--unix-socket` transports; only `--stdio` exists today.
- `plugin install <marketplace-id>` is a Phase-5 stub (exits 2); only local-directory installs work.
- Several subcommand-group enums are commented "stub — implemented in later milestones" in `main.rs`, but their handlers are in fact implemented; the comments are stale and do not reliably indicate unimplemented surface.
