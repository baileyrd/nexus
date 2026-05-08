# Nexus PRD 05 — CLI (M1) Design Spec

**Version:** 1.0
**Date:** 2026-04-12
**Status:** Approved (brainstorming session output)
**Scope:** M1-scoped design for `nexus-cli` — the `nexus` binary that wires the kernel, storage, plugin, and security crates into a usable headless tool. Implements forge management, content CRUD, plugin management, watch mode, and log viewing. Deferred commands stubbed.

**Parent docs:**
- [`PRDs/05-cli.md`](../../../PRDs/05-cli.md) — full PRD
- [`2026-04-11-nexus-m1-foundation-spec.md`](2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §8 (CLI Architecture), §11 (Acceptance Tests)
- [`2026-04-11-nexus-roadmap-design.md`](2026-04-11-nexus-roadmap-design.md) — roadmap

---

## 1. Architecture Overview

Binary crate `nexus-cli` producing the `nexus` binary. Depends on all M1 library crates. Uses `clap` 4.4+ derive macros for argument parsing, `anyhow` for error handling, and helper functions for output formatting (text, json, jsonl, table).

An `App` struct owns all subsystems (storage engine, plugin manager) with lazy initialization. Each subcommand receives `&mut App` and creates subsystems on demand.

---

## 2. Crate Structure

```
crates/nexus-cli/
├── Cargo.toml
└── src/
    ├── main.rs             # entry point, clap parsing, App creation, tracing setup
    ├── app.rs              # App struct: lazy storage + plugin manager
    ├── output.rs           # OutputFormat enum, print helpers
    ├── commands/
    │   ├── mod.rs           # command dispatch
    │   ├── forge.rs         # forge init/status
    │   ├── content.rs       # content create/read/delete/search
    │   ├── plugin.rs        # plugin install/list/call/uninstall/scaffold
    │   ├── watch.rs         # watch mode
    │   └── logs.rs          # logs tail/show/path
    └── stubs.rs            # "not yet implemented" for M2+ commands
```

---

## 3. Dependencies

New workspace dependencies (root `Cargo.toml`):

| Crate | Version | Purpose |
|---|---|---|
| `clap` | 4.4+ | CLI argument parsing (derive macros) |
| `anyhow` | 1.0 | Error handling for binary crate |
| `comfy-table` | 7.0 | Table output formatting |
| `indicatif` | 0.17 | Progress bars and spinners |
| `ctrlc` | 3.4 | Ctrl+C signal handling for watch mode |

`nexus-cli` depends on: `nexus-kernel`, `nexus-security`, `nexus-storage`, `nexus-plugins`, `nexus-types`.

---

## 4. App Struct

```rust
pub struct App {
    forge_root: PathBuf,
    storage: Option<StorageEngine>,
    plugins: Option<PluginManager>,
    output: OutputFormat,
    verbose: u8,
}
```

### Lazy initialization

- `App::storage(&mut self) -> Result<&StorageEngine>` — opens `StorageEngine` on first call, caches
- `App::plugins(&mut self) -> Result<&mut PluginManager>` — creates `PluginManager` on first call, caches
- `forge init` creates the forge without calling `App::storage()` — it uses `StorageEngine::init()` directly

### Construction

```rust
impl App {
    pub fn new(forge_root: PathBuf, output: OutputFormat, verbose: u8) -> Self;
    pub fn storage(&mut self) -> anyhow::Result<&StorageEngine>;
    pub fn storage_mut(&mut self) -> anyhow::Result<&mut StorageEngine>;
    pub fn plugins(&mut self) -> anyhow::Result<&mut PluginManager>;
    pub fn output(&self) -> OutputFormat;
}
```

---

## 5. main.rs Flow

```rust
fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Set up tracing
    setup_tracing(cli.verbose, &cli.forge_path);

    // Create App
    let mut app = App::new(
        cli.forge_path.unwrap_or_else(default_forge_path),
        cli.format,
        cli.verbose,
    );

    // Dispatch command
    match cli.command {
        Commands::Forge(cmd) => commands::forge::run(&mut app, cmd),
        Commands::Content(cmd) => commands::content::run(&mut app, cmd),
        Commands::Plugin(cmd) => commands::plugin::run(&mut app, cmd),
        Commands::Watch(cmd) => commands::watch::run(&mut app, cmd),
        Commands::Logs(cmd) => commands::logs::run(&mut app, cmd),
        // Stubs
        Commands::Db(_) | Commands::Ai(_) | ... => stubs::not_implemented(&cmd_name),
    }
}
```

### Tracing setup

- Default: `warn` level to rolling log file at `<forge>/.forge/logs/nexus-YYYY-MM-DD.log`
- `-v`: `info` level
- `-vv`: `debug` level
- `-vvv`: `trace` level
- Log to both file and stderr when verbose

### Default forge path

`$NEXUS_FORGE_PATH` env var, or `~/.nexus/default`.

---

## 6. Output Helpers (output.rs)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Jsonl,
    Table,
}

pub fn print_success(format: OutputFormat, message: &str, data: &serde_json::Value);
pub fn print_error(format: OutputFormat, error: &anyhow::Error);
pub fn print_list(format: OutputFormat, headers: &[&str], rows: &[Vec<String>]);
pub fn print_value(format: OutputFormat, data: &serde_json::Value);
```

### Format behaviors

- **Text**: ANSI-colored. Green checkmark for success, red for errors. Respects `--no-color` flag and `NO_COLOR` env var. Detects TTY via `std::io::stdout().is_terminal()`.
- **Json**: `serde_json::to_string_pretty` wrapped in `{ "status": "success", "data": ... }` envelope.
- **Jsonl**: One JSON object per line for list commands, flushed immediately.
- **Table**: `comfy_table::Table` with headers and aligned columns.

---

## 7. M1 Commands

### 7.1 Forge commands (`nexus forge`)

**`forge init [DIR]`**
- Creates a new forge via `StorageEngine::init(dir)`
- Prints forge location and next-steps guidance
- Exit 0 on success, 5 if directory is non-empty

**`forge status`**
- Opens storage, queries file count via `query_files`
- Lists loaded plugins via plugin manager
- Prints summary (name, location, file count, plugin count)

### 7.2 Content commands (`nexus content`)

**`content create <PATH> [--content <TEXT> | --stdin]`**
- Writes file via `StorageEngine::write_file`
- Content from `--content` flag, stdin, or empty default
- Prints file metadata

**`content read <PATH> [--raw]`**
- Reads via `StorageEngine::read_file`
- `--raw`: print content only (no metadata)
- Default: print with file info header

**`content delete <PATH> [--force]`**
- Deletes via `StorageEngine::delete_file`
- `--force` skips confirmation prompt
- Default: prompts "Delete <path>? [y/N]"

**`content search <QUERY> [--limit N]`**
- Searches via `StorageEngine::search(query, limit)`
- Prints results as list (path, score, excerpt)
- Default limit: 20

### 7.3 Plugin commands (`nexus plugin`)

**`plugin install <DIR>`**
- Loads plugin via `PluginManager::load(dir)`
- Prints plugin info (id, version, capabilities)

**`plugin list`**
- Lists all loaded plugins via `PluginManager::list()`
- Table output: id, name, version, status, trust_level

**`plugin call <PLUGIN_ID> <COMMAND> [ARGS_JSON]`**
- Dispatches IPC call via `PluginManager::dispatch_ipc`
- Args are JSON string (default `{}`)
- Prints JSON result

**`plugin uninstall <PLUGIN_ID>`**
- Unloads via `PluginManager::unload`

**`plugin scaffold [--type core|community] [--id ID] [--name NAME] [--author AUTHOR]`**
- Calls `nexus_plugins::scaffold()` with prompted or flag-provided config
- Prints generated file list

### 7.4 Watch mode (`nexus watch`)

**`watch [GLOB]`**
- Gets watcher receiver from `StorageEngine::watch_changes()`
- Prints events as they arrive (path, event type, hash)
- Ctrl+C to stop (via `ctrlc` crate)
- Default glob: `**/*` (all files)

### 7.5 Log commands (`nexus logs`)

**`logs tail [--level LEVEL] [--lines N]`**
- Reads the current day's log file
- Filters by level if `--level` specified
- Default: last 50 lines

**`logs show <DATE>`**
- Reads log file for a specific date (YYYY-MM-DD)

**`logs path`**
- Prints the log directory path

### 7.6 Stubs

`db`, `ai`, `proc`, `term`, `mcp`, `sync`, `git`, `run` — print "Not yet implemented (planned for MN)" to stderr and exit with code 1.

---

## 8. Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Generic error |
| 2 | Invalid arguments / usage error (clap handles this) |
| 3 | Permission denied |
| 4 | Resource not found |
| 5 | Conflict / duplicate |

The `main()` function maps `anyhow::Error` to appropriate exit codes by downcasting to subsystem error types.

---

## 9. Clap Derive Structure

```rust
#[derive(Parser)]
#[command(name = "nexus", about = "Nexus IDE — headless CLI")]
pub struct Cli {
    #[arg(long, global = true)]
    pub forge_path: Option<PathBuf>,

    #[arg(long, global = true, default_value = "text")]
    pub format: OutputFormat,

    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Forge(ForgeCommand),
    Content(ContentCommand),
    Plugin(PluginCommand),
    Watch(WatchCommand),
    Logs(LogsCommand),
    // Stubs
    Db(StubCommand),
    Ai(StubCommand),
    Proc(StubCommand),
    Term(StubCommand),
    Mcp(StubCommand),
    Sync(StubCommand),
    Git(StubCommand),
    Run(StubCommand),
}
```

Each command group has its own enum with subcommands (e.g., `ForgeCommand` has `Init`, `Status`).

---

## 10. Deferred from M1

| Item | Rationale | Revisit |
|---|---|---|
| Interactive walkthrough (`forge init`) | Headless sufficient for M1 | v0.2 |
| Shell completions | Nice-to-have | v0.2 |
| Pager auto-engagement | `\| less` works | v0.2 |
| `content edit` with `$EDITOR` | External process management | v0.2 |
| `forge open --remember` | Single-forge use in M1 | v0.2 |
| `forge config` get/set | Direct TOML editing sufficient | v0.2 |
| Plugin formatter registration | No plugin consumers | When needed |
| Progress bars (indicatif) | Adds dep, minimal UX gain for CLI | v0.2 |
| `--quiet` flag | Low priority | v0.2 |
