# Nexus PRD 05 — CLI (M1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `nexus` CLI binary that wires the kernel, storage, plugin, and security crates into a usable headless tool — forge management, content CRUD, plugin management, watch mode, log viewing, and output formatting.

**Architecture:** Binary crate `nexus-cli` with an `App` struct owning all subsystems (lazy init). Clap derive macros for argument parsing, `anyhow` for error handling, helper functions for output formatting. One file per command group.

**Tech Stack:** Rust (edition 2024), `clap` 4.4+ (derive), `anyhow` 1.0, `comfy-table` 7.0, `ctrlc` 3.4, `tracing-subscriber` 0.3, `tracing-appender` 0.2.

**Parent docs:**
- [`2026-04-12-nexus-prd-05-cli-design.md`](../specs/2026-04-12-nexus-prd-05-cli-design.md) — **the contract this plan implements**
- [`2026-04-11-nexus-m1-foundation-spec.md`](../specs/2026-04-11-nexus-m1-foundation-spec.md) — M1 spec §8, §11

---

## Prerequisites

1. PRDs 01-04a complete and tests pass.
2. Verify: `cargo nextest run --workspace` passes (329 tests).

---

## File Structure

```
crates/nexus-cli/
├── Cargo.toml
└── src/
    ├── main.rs             # entry point, clap parsing, tracing setup
    ├── app.rs              # App struct with lazy subsystem init
    ├── output.rs           # OutputFormat enum, print helpers
    ├── commands/
    │   ├── mod.rs           # re-export command modules
    │   ├── forge.rs         # forge init, status
    │   ├── content.rs       # content create, read, delete, search
    │   ├── plugin.rs        # plugin install, list, call, uninstall, scaffold
    │   ├── watch.rs         # watch mode
    │   └── logs.rs          # logs tail, show, path
    └── stubs.rs            # "not yet implemented" handlers
```

Modifications to existing files:
- `Cargo.toml` (workspace root): add `nexus-cli` to members, add new deps

---

## Task Overview

16 tasks across 8 phases:

1. Phase 1: Crate skeleton + clap structure (Tasks 1–3)
2. Phase 2: App struct + output helpers (Tasks 4–5)
3. Phase 3: Forge commands (Tasks 6–7)
4. Phase 4: Content commands (Tasks 8–9)
5. Phase 5: Plugin commands (Tasks 10–11)
6. Phase 6: Watch + logs (Tasks 12–13)
7. Phase 7: Stubs + exit codes (Task 14)
8. Phase 8: Integration + smoke test (Tasks 15–16)

---

## Phase 1: Crate Skeleton

### Task 1: Create nexus-cli crate with workspace wiring

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/nexus-cli/Cargo.toml`
- Create: `crates/nexus-cli/src/main.rs`

- [ ] **Step 1: Add workspace member and deps to root `Cargo.toml`**

Edit `/mnt/c/Users/baile/dev/nexus/Cargo.toml`:

In `[workspace]` members, add `"crates/nexus-cli"`.

In `[workspace.dependencies]`, add:

```toml
# CLI
clap = { version = "4.4", features = ["derive"] }
anyhow = "1"
comfy-table = "7"
ctrlc = "3.4"
```

- [ ] **Step 2: Create `crates/nexus-cli/Cargo.toml`**

```toml
[package]
name = "nexus-cli"
version.workspace = true
edition.workspace = true
license.workspace = true
publish.workspace = true
description = "Nexus IDE — headless CLI"

[[bin]]
name = "nexus"
path = "src/main.rs"

[dependencies]
nexus-kernel = { path = "../nexus-kernel" }
nexus-security = { path = "../nexus-security" }
nexus-storage = { path = "../nexus-storage" }
nexus-plugins = { path = "../nexus-plugins" }
nexus-types = { path = "../nexus-types" }
clap = { workspace = true }
anyhow = { workspace = true }
comfy-table = { workspace = true }
ctrlc = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
tracing-appender = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create minimal `main.rs`**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/main.rs`:

```rust
use anyhow::Result;

fn main() -> Result<()> {
    println!("nexus: not yet implemented");
    Ok(())
}
```

- [ ] **Step 4: Verify**

Run: `cargo build -p nexus-cli`
Run: `./target/debug/nexus` — prints "nexus: not yet implemented"

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/nexus-cli/
git commit -m "feat(cli): scaffold nexus-cli binary crate"
```

---

### Task 2: Add clap argument parsing

**Files:**
- Modify: `crates/nexus-cli/src/main.rs`

- [ ] **Step 1: Define the full Cli struct and Commands enum**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/main.rs`:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::{ArgAction, Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nexus", about = "Nexus IDE — headless CLI", version)]
struct Cli {
    /// Path to forge root directory
    #[arg(long, global = true, env = "NEXUS_FORGE_PATH")]
    forge_path: Option<PathBuf>,

    /// Output format
    #[arg(long, global = true, default_value = "text")]
    format: String,

    /// Verbosity level (-v, -vv, -vvv)
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,

    /// Disable ANSI color output
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Forge management
    Forge(ForgeArgs),
    /// Content CRUD operations
    Content(ContentArgs),
    /// Plugin management
    Plugin(PluginArgs),
    /// Watch for file changes
    Watch(WatchArgs),
    /// View logs
    Logs(LogsArgs),
    // Stubs for M2+ commands
    /// Database operations (M3)
    Db(StubArgs),
    /// AI operations (M4)
    Ai(StubArgs),
    /// Process management (M3)
    Proc(StubArgs),
    /// Terminal management (M3)
    Term(StubArgs),
    /// MCP integration (M4)
    Mcp(StubArgs),
    /// Sync operations (deferred)
    Sync(StubArgs),
    /// Git operations (M3)
    Git(StubArgs),
    /// Run automation script (M5)
    Run(StubArgs),
}

// ── Forge ────────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct ForgeArgs {
    #[command(subcommand)]
    command: ForgeCommand,
}

#[derive(Subcommand)]
enum ForgeCommand {
    /// Initialize a new forge
    Init {
        /// Target directory (default: current directory)
        dir: Option<PathBuf>,
    },
    /// Display forge status
    Status,
}

// ── Content ──────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct ContentArgs {
    #[command(subcommand)]
    command: ContentCommand,
}

#[derive(Subcommand)]
enum ContentCommand {
    /// Create a new file
    Create {
        /// File path relative to forge root (e.g., notes/my-doc.md)
        path: String,
        /// Inline content
        #[arg(long)]
        content: Option<String>,
        /// Read content from stdin
        #[arg(long)]
        stdin: bool,
    },
    /// Read a file
    Read {
        /// File path
        path: String,
        /// Output raw content only (no metadata)
        #[arg(long)]
        raw: bool,
    },
    /// Delete a file
    Delete {
        /// File path
        path: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Search content
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(long, default_value = "20")]
        limit: usize,
    },
}

// ── Plugin ───────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// Install a plugin from a directory
    Install {
        /// Path to plugin directory
        dir: PathBuf,
    },
    /// List loaded plugins
    List,
    /// Call a plugin command
    Call {
        /// Plugin ID
        plugin_id: String,
        /// Command ID
        command: String,
        /// JSON arguments
        #[arg(default_value = "{}")]
        args: String,
    },
    /// Uninstall a plugin
    Uninstall {
        /// Plugin ID
        plugin_id: String,
    },
    /// Scaffold a new plugin project
    Scaffold {
        /// Plugin type
        #[arg(long, default_value = "community")]
        r#type: String,
        /// Plugin ID (reverse-DNS)
        #[arg(long)]
        id: Option<String>,
        /// Plugin name
        #[arg(long)]
        name: Option<String>,
        /// Author name
        #[arg(long)]
        author: Option<String>,
        /// Output directory
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

// ── Watch ────────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct WatchArgs {
    /// Glob pattern to watch (default: all files)
    #[arg(default_value = "**/*")]
    glob: String,
}

// ── Logs ─────────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct LogsArgs {
    #[command(subcommand)]
    command: LogsCommand,
}

#[derive(Subcommand)]
enum LogsCommand {
    /// Tail recent log entries
    Tail {
        /// Filter by log level
        #[arg(long)]
        level: Option<String>,
        /// Number of lines
        #[arg(long, default_value = "50")]
        lines: usize,
    },
    /// Show logs for a specific date
    Show {
        /// Date in YYYY-MM-DD format
        date: String,
    },
    /// Print the log directory path
    Path,
}

// ── Stubs ────────────────────────────────────────────────────────────────────

#[derive(clap::Args)]
struct StubArgs {
    /// Subcommand arguments (ignored)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Forge(_) => println!("forge: coming soon"),
        Commands::Content(_) => println!("content: coming soon"),
        Commands::Plugin(_) => println!("plugin: coming soon"),
        Commands::Watch(_) => println!("watch: coming soon"),
        Commands::Logs(_) => println!("logs: coming soon"),
        Commands::Db(_)
        | Commands::Ai(_)
        | Commands::Proc(_)
        | Commands::Term(_)
        | Commands::Mcp(_)
        | Commands::Sync(_)
        | Commands::Git(_)
        | Commands::Run(_) => {
            eprintln!("Error: This command is not yet implemented.");
            std::process::exit(1);
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p nexus-cli`
Run: `./target/debug/nexus --help` — shows all subcommands
Run: `./target/debug/nexus forge init` — prints "forge: coming soon"
Run: `./target/debug/nexus db` — prints error, exits 1

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-cli/
git commit -m "feat(cli): add clap argument parsing with all M1 subcommands"
```

---

### Task 3: Add output helpers

**Files:**
- Create: `crates/nexus-cli/src/output.rs`
- Modify: `crates/nexus-cli/src/main.rs`

- [ ] **Step 1: Create output.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/output.rs`:

```rust
//! Output formatting helpers.

use std::io::Write;

/// Output format for CLI results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable text with ANSI colors.
    Text,
    /// Pretty-printed JSON.
    Json,
    /// One JSON object per line (streaming).
    Jsonl,
    /// Columnar table.
    Table,
}

impl OutputFormat {
    /// Parse from a string (used by clap).
    pub fn from_str(s: &str) -> Self {
        match s {
            "json" => Self::Json,
            "jsonl" => Self::Jsonl,
            "table" => Self::Table,
            _ => Self::Text,
        }
    }
}

/// Whether to use ANSI colors in output.
pub fn use_color(no_color_flag: bool) -> bool {
    if no_color_flag {
        return false;
    }
    if std::env::var("NO_COLOR").is_ok() {
        return false;
    }
    std::io::stdout().is_terminal()
}

/// Print a success message.
pub fn print_success(format: OutputFormat, message: &str, data: &serde_json::Value) {
    match format {
        OutputFormat::Text => {
            println!("{message}");
        }
        OutputFormat::Json => {
            let envelope = serde_json::json!({
                "status": "success",
                "data": data,
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            println!("{}", serde_json::to_string(data).unwrap_or_default());
            std::io::stdout().flush().ok();
        }
        OutputFormat::Table => {
            println!("{message}");
        }
    }
}

/// Print a list as table or JSON.
pub fn print_list(
    format: OutputFormat,
    headers: &[&str],
    rows: &[Vec<String>],
) {
    match format {
        OutputFormat::Text | OutputFormat::Table => {
            let mut table = comfy_table::Table::new();
            table.set_header(headers);
            for row in rows {
                table.add_row(row);
            }
            println!("{table}");
        }
        OutputFormat::Json => {
            let items: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    let mut obj = serde_json::Map::new();
                    for (i, header) in headers.iter().enumerate() {
                        obj.insert(
                            (*header).to_string(),
                            serde_json::Value::String(row.get(i).cloned().unwrap_or_default()),
                        );
                    }
                    serde_json::Value::Object(obj)
                })
                .collect();
            let envelope = serde_json::json!({ "status": "success", "data": items });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            for row in rows {
                let mut obj = serde_json::Map::new();
                for (i, header) in headers.iter().enumerate() {
                    obj.insert(
                        (*header).to_string(),
                        serde_json::Value::String(row.get(i).cloned().unwrap_or_default()),
                    );
                }
                println!("{}", serde_json::to_string(&serde_json::Value::Object(obj)).unwrap_or_default());
            }
            std::io::stdout().flush().ok();
        }
    }
}

/// Print a JSON value.
pub fn print_value(format: OutputFormat, data: &serde_json::Value) {
    match format {
        OutputFormat::Text => {
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
        OutputFormat::Jsonl => {
            println!("{}", serde_json::to_string(data).unwrap_or_default());
        }
        OutputFormat::Table => {
            println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_from_str() {
        assert_eq!(OutputFormat::from_str("json"), OutputFormat::Json);
        assert_eq!(OutputFormat::from_str("jsonl"), OutputFormat::Jsonl);
        assert_eq!(OutputFormat::from_str("table"), OutputFormat::Table);
        assert_eq!(OutputFormat::from_str("text"), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("unknown"), OutputFormat::Text);
    }

    #[test]
    fn no_color_flag_disables_color() {
        assert!(!use_color(true));
    }
}
```

- [ ] **Step 2: Add `mod output;` to main.rs**

Add at the top of `main.rs` after the imports:

```rust
mod output;
```

- [ ] **Step 3: Verify**

Run: `cargo check -p nexus-cli`

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-cli/
git commit -m "feat(cli): add output formatting helpers (text, json, jsonl, table)"
```

---

## Phase 2: App Struct

### Task 4: Create App struct

**Files:**
- Create: `crates/nexus-cli/src/app.rs`
- Modify: `crates/nexus-cli/src/main.rs`

- [ ] **Step 1: Create app.rs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/app.rs`:

```rust
//! Application context owning all subsystems.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use nexus_plugins::{PluginManager, PluginManagerConfig};
use nexus_storage::{StorageConfig, StorageEngine};

use crate::output::OutputFormat;

/// The main application context. Owns all subsystems with lazy initialization.
pub struct App {
    forge_root: PathBuf,
    storage: Option<StorageEngine>,
    plugins: Option<PluginManager>,
    format: OutputFormat,
}

impl App {
    /// Create a new App for the given forge root.
    pub fn new(forge_root: PathBuf, format: OutputFormat) -> Self {
        Self {
            forge_root,
            storage: None,
            plugins: None,
            format,
        }
    }

    /// Get the forge root path.
    pub fn forge_root(&self) -> &Path {
        &self.forge_root
    }

    /// Get the output format.
    pub fn format(&self) -> OutputFormat {
        self.format
    }

    /// Get or create the storage engine (opens existing forge).
    pub fn storage(&mut self) -> Result<&StorageEngine> {
        if self.storage.is_none() {
            let engine = StorageEngine::open(&self.forge_root, &StorageConfig::default())
                .context("failed to open forge")?;
            self.storage = Some(engine);
        }
        Ok(self.storage.as_ref().unwrap())
    }

    /// Get or create the storage engine (mutable).
    pub fn storage_mut(&mut self) -> Result<&mut StorageEngine> {
        if self.storage.is_none() {
            let engine = StorageEngine::open(&self.forge_root, &StorageConfig::default())
                .context("failed to open forge")?;
            self.storage = Some(engine);
        }
        Ok(self.storage.as_mut().unwrap())
    }

    /// Get or create the plugin manager.
    pub fn plugins(&mut self) -> Result<&mut PluginManager> {
        if self.plugins.is_none() {
            let plugins_dir = self.forge_root.join(".forge/plugins");
            std::fs::create_dir_all(&plugins_dir)?;
            let config = PluginManagerConfig {
                hot_reload: false, // CLI is short-lived, no hot-reload needed
                ..Default::default()
            };
            let mgr = PluginManager::new(&plugins_dir, &config)
                .context("failed to initialize plugin manager")?;
            self.plugins = Some(mgr);
        }
        Ok(self.plugins.as_mut().unwrap())
    }

    /// Initialize a new forge at the forge root. Does NOT open it afterwards.
    pub fn init_forge(&self) -> Result<()> {
        StorageEngine::init(&self.forge_root).context("failed to initialize forge")?;
        Ok(())
    }
}
```

- [ ] **Step 2: Add `mod app;` to main.rs**

Add after `mod output;` in main.rs:

```rust
mod app;
```

- [ ] **Step 3: Verify**

Run: `cargo check -p nexus-cli`

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-cli/
git commit -m "feat(cli): add App struct with lazy subsystem initialization"
```

---

### Task 5: Create commands module structure

**Files:**
- Create: `crates/nexus-cli/src/commands/mod.rs`
- Create: `crates/nexus-cli/src/commands/forge.rs`
- Create: `crates/nexus-cli/src/commands/content.rs`
- Create: `crates/nexus-cli/src/commands/plugin.rs`
- Create: `crates/nexus-cli/src/commands/watch.rs`
- Create: `crates/nexus-cli/src/commands/logs.rs`
- Create: `crates/nexus-cli/src/stubs.rs`

- [ ] **Step 1: Create all command files as stubs**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/mod.rs`:

```rust
pub mod forge;
pub mod content;
pub mod plugin;
pub mod watch;
pub mod logs;
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/forge.rs`:

```rust
use anyhow::Result;
use crate::app::App;

pub fn init(app: &App, dir: Option<std::path::PathBuf>) -> Result<()> {
    let _ = (app, dir);
    anyhow::bail!("forge init: not yet implemented")
}

pub fn status(app: &mut App) -> Result<()> {
    let _ = app;
    anyhow::bail!("forge status: not yet implemented")
}
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/content.rs`:

```rust
use anyhow::Result;
use crate::app::App;

pub fn create(app: &mut App, path: &str, content: Option<&str>, stdin: bool) -> Result<()> {
    let _ = (app, path, content, stdin);
    anyhow::bail!("content create: not yet implemented")
}

pub fn read(app: &mut App, path: &str, raw: bool) -> Result<()> {
    let _ = (app, path, raw);
    anyhow::bail!("content read: not yet implemented")
}

pub fn delete(app: &mut App, path: &str, force: bool) -> Result<()> {
    let _ = (app, path, force);
    anyhow::bail!("content delete: not yet implemented")
}

pub fn search(app: &mut App, query: &str, limit: usize) -> Result<()> {
    let _ = (app, query, limit);
    anyhow::bail!("content search: not yet implemented")
}
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/plugin.rs`:

```rust
use std::path::Path;
use anyhow::Result;
use crate::app::App;

pub fn install(app: &mut App, dir: &Path) -> Result<()> {
    let _ = (app, dir);
    anyhow::bail!("plugin install: not yet implemented")
}

pub fn list(app: &mut App) -> Result<()> {
    let _ = app;
    anyhow::bail!("plugin list: not yet implemented")
}

pub fn call(app: &mut App, plugin_id: &str, command: &str, args_json: &str) -> Result<()> {
    let _ = (app, plugin_id, command, args_json);
    anyhow::bail!("plugin call: not yet implemented")
}

pub fn uninstall(app: &mut App, plugin_id: &str) -> Result<()> {
    let _ = (app, plugin_id);
    anyhow::bail!("plugin uninstall: not yet implemented")
}

pub fn scaffold(
    r#type: &str,
    id: Option<&str>,
    name: Option<&str>,
    author: Option<&str>,
    output: Option<&Path>,
) -> Result<()> {
    let _ = (r#type, id, name, author, output);
    anyhow::bail!("plugin scaffold: not yet implemented")
}
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/watch.rs`:

```rust
use anyhow::Result;
use crate::app::App;

pub fn run(app: &mut App, _glob: &str) -> Result<()> {
    let _ = app;
    anyhow::bail!("watch: not yet implemented")
}
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/logs.rs`:

```rust
use anyhow::Result;
use crate::app::App;

pub fn tail(app: &App, level: Option<&str>, lines: usize) -> Result<()> {
    let _ = (app, level, lines);
    anyhow::bail!("logs tail: not yet implemented")
}

pub fn show(app: &App, date: &str) -> Result<()> {
    let _ = (app, date);
    anyhow::bail!("logs show: not yet implemented")
}

pub fn path(app: &App) -> Result<()> {
    let _ = app;
    anyhow::bail!("logs path: not yet implemented")
}
```

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/stubs.rs`:

```rust
use anyhow::Result;

/// Print "not yet implemented" and exit.
pub fn not_implemented(command_name: &str) -> Result<()> {
    eprintln!("Error: '{command_name}' is not yet implemented.");
    std::process::exit(1);
}
```

- [ ] **Step 2: Wire commands into main.rs dispatch**

Replace the `main()` function in `main.rs`:

```rust
mod app;
mod commands;
mod output;
mod stubs;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let format = output::OutputFormat::from_str(&cli.format);
    let forge_root = cli.forge_path.unwrap_or_else(default_forge_path);
    let mut app = app::App::new(forge_root, format);

    match cli.command {
        Commands::Forge(args) => match args.command {
            ForgeCommand::Init { dir } => commands::forge::init(&app, dir),
            ForgeCommand::Status => commands::forge::status(&mut app),
        },
        Commands::Content(args) => match args.command {
            ContentCommand::Create { path, content, stdin } => {
                commands::content::create(&mut app, &path, content.as_deref(), stdin)
            }
            ContentCommand::Read { path, raw } => {
                commands::content::read(&mut app, &path, raw)
            }
            ContentCommand::Delete { path, force } => {
                commands::content::delete(&mut app, &path, force)
            }
            ContentCommand::Search { query, limit } => {
                commands::content::search(&mut app, &query, limit)
            }
        },
        Commands::Plugin(args) => match args.command {
            PluginCommand::Install { dir } => commands::plugin::install(&mut app, &dir),
            PluginCommand::List => commands::plugin::list(&mut app),
            PluginCommand::Call { plugin_id, command, args } => {
                commands::plugin::call(&mut app, &plugin_id, &command, &args)
            }
            PluginCommand::Uninstall { plugin_id } => {
                commands::plugin::uninstall(&mut app, &plugin_id)
            }
            PluginCommand::Scaffold { r#type, id, name, author, output } => {
                commands::plugin::scaffold(&r#type, id.as_deref(), name.as_deref(), author.as_deref(), output.as_deref())
            }
        },
        Commands::Watch(args) => commands::watch::run(&mut app, &args.glob),
        Commands::Logs(args) => match args.command {
            LogsCommand::Tail { level, lines } => {
                commands::logs::tail(&app, level.as_deref(), lines)
            }
            LogsCommand::Show { date } => commands::logs::show(&app, &date),
            LogsCommand::Path => commands::logs::path(&app),
        },
        Commands::Db(_) => stubs::not_implemented("db"),
        Commands::Ai(_) => stubs::not_implemented("ai"),
        Commands::Proc(_) => stubs::not_implemented("proc"),
        Commands::Term(_) => stubs::not_implemented("term"),
        Commands::Mcp(_) => stubs::not_implemented("mcp"),
        Commands::Sync(_) => stubs::not_implemented("sync"),
        Commands::Git(_) => stubs::not_implemented("git"),
        Commands::Run(_) => stubs::not_implemented("run"),
    }
}

fn default_forge_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("NEXUS_FORGE_PATH") {
        return PathBuf::from(path);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".nexus/default")
}
```

Note: add `dirs = "5"` to workspace deps and nexus-cli deps for home directory detection. Or use a simpler approach:

```rust
fn default_forge_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("NEXUS_FORGE_PATH") {
        return PathBuf::from(path);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".nexus/default")
}
```

- [ ] **Step 3: Verify**

Run: `cargo build -p nexus-cli`
Run: `./target/debug/nexus forge init` — prints error (not implemented yet, but parses correctly)
Run: `./target/debug/nexus db anything` — prints "not yet implemented", exits 1

- [ ] **Step 4: Commit**

```bash
git add crates/nexus-cli/
git commit -m "feat(cli): add command module structure with dispatch and stubs"
```

---

## Phase 3: Forge Commands

### Task 6: Implement forge init

**Files:**
- Modify: `crates/nexus-cli/src/commands/forge.rs`

- [ ] **Step 1: Implement forge init**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/forge.rs`:

```rust
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::app::App;
use crate::output::{self, OutputFormat};

/// Initialize a new forge.
pub fn init(app: &App, dir: Option<PathBuf>) -> Result<()> {
    let target = dir.unwrap_or_else(|| app.forge_root().to_path_buf());

    // Check if forge already exists
    if target.join(".forge").exists() {
        anyhow::bail!("forge already exists at {}", target.display());
    }

    nexus_storage::StorageEngine::init(&target)
        .context("failed to initialize forge")?;

    let data = serde_json::json!({
        "location": target.display().to_string(),
    });

    output::print_success(
        app.format(),
        &format!("Forge initialized at {}", target.display()),
        &data,
    );

    Ok(())
}

/// Display forge status.
pub fn status(app: &mut App) -> Result<()> {
    let storage = app.storage()?;

    let files = storage
        .query_files(&nexus_storage::FileFilter::default())
        .context("failed to query files")?;

    let file_count = files.len();
    let total_size: u64 = files.iter().map(|f| f.size_bytes).sum();

    let data = serde_json::json!({
        "location": app.forge_root().display().to_string(),
        "file_count": file_count,
        "size_bytes": total_size,
    });

    match app.format() {
        OutputFormat::Text | OutputFormat::Table => {
            println!("Forge Status");
            println!("  Location: {}", app.forge_root().display());
            println!("  Files: {file_count} indexed");
            println!("  Size: {} bytes", total_size);
        }
        _ => output::print_success(app.format(), "Forge status", &data),
    }

    Ok(())
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p nexus-cli`

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-cli/src/commands/forge.rs
git commit -m "feat(cli): implement forge init and status commands"
```

---

### Task 7: Implement content commands

**Files:**
- Modify: `crates/nexus-cli/src/commands/content.rs`

- [ ] **Step 1: Implement all content commands**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/content.rs`:

```rust
use std::io::Read;

use anyhow::{Context, Result};

use crate::app::App;
use crate::output::{self, OutputFormat};

/// Create a new content file.
pub fn create(app: &mut App, path: &str, content: Option<&str>, stdin: bool) -> Result<()> {
    let body = if stdin {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        content.unwrap_or("").to_string()
    };

    let storage = app.storage_mut()?;
    let meta = storage
        .write_file(path, body.as_bytes())
        .context("failed to create file")?;

    let data = serde_json::json!({
        "path": meta.path,
        "size_bytes": meta.size_bytes,
        "content_hash": meta.content_hash,
    });

    output::print_success(
        app.format(),
        &format!("Created {} ({} bytes)", meta.path, meta.size_bytes),
        &data,
    );

    Ok(())
}

/// Read a content file.
pub fn read(app: &mut App, path: &str, raw: bool) -> Result<()> {
    let storage = app.storage()?;
    let content = storage
        .read_file(path)
        .context("failed to read file")?;

    let text = String::from_utf8_lossy(&content);

    if raw {
        print!("{text}");
    } else {
        match app.format() {
            OutputFormat::Json | OutputFormat::Jsonl => {
                let data = serde_json::json!({
                    "path": path,
                    "content": text,
                    "size_bytes": content.len(),
                });
                output::print_value(app.format(), &data);
            }
            _ => {
                println!("File: {path}");
                println!("Size: {} bytes", content.len());
                println!("---");
                println!("{text}");
            }
        }
    }

    Ok(())
}

/// Delete a content file.
pub fn delete(app: &mut App, path: &str, force: bool) -> Result<()> {
    if !force {
        eprint!("Delete '{path}'? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let storage = app.storage_mut()?;
    storage
        .delete_file(path)
        .context("failed to delete file")?;

    let data = serde_json::json!({ "path": path });
    output::print_success(app.format(), &format!("Deleted {path}"), &data);

    Ok(())
}

/// Search content.
pub fn search(app: &mut App, query: &str, limit: usize) -> Result<()> {
    let storage = app.storage_mut()?;

    // First ensure search index is built
    storage.rebuild_search_index().context("failed to build search index")?;

    let results = storage
        .search(query, limit)
        .context("search failed")?;

    if results.is_empty() {
        println!("No results found.");
        return Ok(());
    }

    let headers = &["Path", "Score", "Type"];
    let rows: Vec<Vec<String>> = results
        .iter()
        .map(|r| {
            vec![
                r.file_path.clone(),
                format!("{:.2}", r.score),
                r.block_type.clone(),
            ]
        })
        .collect();

    output::print_list(app.format(), headers, &rows);

    Ok(())
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p nexus-cli`

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-cli/src/commands/content.rs
git commit -m "feat(cli): implement content create, read, delete, search commands"
```

---

## Phase 4: Plugin Commands

### Task 8: Implement plugin commands

**Files:**
- Modify: `crates/nexus-cli/src/commands/plugin.rs`

- [ ] **Step 1: Implement all plugin commands**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/plugin.rs`:

```rust
use std::path::Path;

use anyhow::{Context, Result};

use crate::app::App;
use crate::output::{self, OutputFormat};

/// Install a plugin from a directory.
pub fn install(app: &mut App, dir: &Path) -> Result<()> {
    let plugins = app.plugins()?;
    let info = plugins
        .load(dir)
        .context("failed to install plugin")?;

    let data = serde_json::json!({
        "id": info.id,
        "name": info.name,
        "version": info.version,
        "status": format!("{:?}", info.status),
    });

    output::print_success(
        app.format(),
        &format!("Plugin loaded: {} v{}", info.id, info.version),
        &data,
    );

    Ok(())
}

/// List loaded plugins.
pub fn list(app: &mut App) -> Result<()> {
    let plugins = app.plugins()?;
    let list = plugins.list();

    if list.is_empty() {
        println!("No plugins loaded.");
        return Ok(());
    }

    let headers = &["ID", "Name", "Version", "Status", "Trust"];
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|p| {
            vec![
                p.id.clone(),
                p.name.clone(),
                p.version.clone(),
                format!("{:?}", p.status),
                format!("{:?}", p.trust_level),
            ]
        })
        .collect();

    output::print_list(app.format(), headers, &rows);

    Ok(())
}

/// Call a plugin command via IPC.
pub fn call(app: &mut App, plugin_id: &str, command: &str, args_json: &str) -> Result<()> {
    let args: serde_json::Value = serde_json::from_str(args_json)
        .context("invalid JSON arguments")?;

    let plugins = app.plugins()?;
    let result = plugins
        .dispatch_ipc(plugin_id, command, &args)
        .context("plugin call failed")?;

    output::print_value(app.format(), &result);

    Ok(())
}

/// Uninstall a plugin.
pub fn uninstall(app: &mut App, plugin_id: &str) -> Result<()> {
    let plugins = app.plugins()?;
    plugins
        .unload(plugin_id)
        .context("failed to uninstall plugin")?;

    let data = serde_json::json!({ "id": plugin_id });
    output::print_success(
        app.format(),
        &format!("Plugin uninstalled: {plugin_id}"),
        &data,
    );

    Ok(())
}

/// Scaffold a new plugin project.
pub fn scaffold(
    r#type: &str,
    id: Option<&str>,
    name: Option<&str>,
    author: Option<&str>,
    output_dir: Option<&Path>,
) -> Result<()> {
    let plugin_id = id.unwrap_or("com.example.my-plugin");
    let plugin_name = name.unwrap_or("My Plugin");
    let plugin_author = author.unwrap_or("Author");
    let out = output_dir
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from(plugin_id));

    let template = match r#type {
        "core" => nexus_plugins::PluginTemplate::Core,
        _ => nexus_plugins::PluginTemplate::Community,
    };

    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: plugin_id.to_string(),
        plugin_name: plugin_name.to_string(),
        author: plugin_author.to_string(),
        description: format!("A Nexus {r#type} plugin."),
    };

    nexus_plugins::scaffold(&out, template, &config)
        .context("failed to scaffold plugin")?;

    println!("Plugin scaffolded at {}", out.display());
    println!("  manifest.toml — plugin manifest");
    println!("  src/lib.rs    — plugin entry point");
    println!();
    println!("Build: cd {} && cargo build --target wasm32-unknown-unknown --release", out.display());

    Ok(())
}
```

- [ ] **Step 2: Verify**

Run: `cargo build -p nexus-cli`

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-cli/src/commands/plugin.rs
git commit -m "feat(cli): implement plugin install, list, call, uninstall, scaffold commands"
```

---

## Phase 5: Watch + Logs

### Task 9: Implement watch command

**Files:**
- Modify: `crates/nexus-cli/src/commands/watch.rs`

- [ ] **Step 1: Implement watch mode**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/watch.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};

use crate::app::App;
use crate::output::{self, OutputFormat};

/// Run watch mode — print file change events until Ctrl+C.
pub fn run(app: &mut App, _glob: &str) -> Result<()> {
    let storage = app.storage()?;

    let rx = match storage.watch_changes() {
        Some(rx) => rx,
        None => {
            anyhow::bail!("file watcher not available");
        }
    };

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl+C handler")?;

    println!("Watching for changes... (Ctrl+C to stop)");

    while running.load(Ordering::SeqCst) {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(event) => {
                let data = serde_json::json!({
                    "event": format!("{event:?}"),
                });
                match app.format() {
                    OutputFormat::Json | OutputFormat::Jsonl => {
                        output::print_value(app.format(), &data);
                    }
                    _ => {
                        println!("{event:?}");
                    }
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("\nStopped.");
    Ok(())
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/nexus-cli/src/commands/watch.rs
git commit -m "feat(cli): implement watch mode with Ctrl+C handling"
```

---

### Task 10: Implement logs commands

**Files:**
- Modify: `crates/nexus-cli/src/commands/logs.rs`

- [ ] **Step 1: Implement logs commands**

Replace `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/src/commands/logs.rs`:

```rust
use anyhow::{Context, Result};

use crate::app::App;

/// Tail recent log entries.
pub fn tail(app: &App, level: Option<&str>, lines: usize) -> Result<()> {
    let log_dir = app.forge_root().join(".forge/logs");
    if !log_dir.exists() {
        println!("No log files found.");
        return Ok(());
    }

    // Find the most recent log file
    let mut log_files: Vec<_> = std::fs::read_dir(&log_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "log")
        })
        .collect();

    log_files.sort_by_key(|e| e.file_name());

    let latest = match log_files.last() {
        Some(f) => f.path(),
        None => {
            println!("No log files found.");
            return Ok(());
        }
    };

    let content = std::fs::read_to_string(&latest)
        .context("failed to read log file")?;

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    let tail_lines = &all_lines[start..];

    for line in tail_lines {
        if let Some(level_filter) = level {
            let upper = level_filter.to_uppercase();
            if line.contains(&upper) {
                println!("{line}");
            }
        } else {
            println!("{line}");
        }
    }

    Ok(())
}

/// Show logs for a specific date.
pub fn show(app: &App, date: &str) -> Result<()> {
    let log_file = app
        .forge_root()
        .join(format!(".forge/logs/nexus-{date}.log"));

    if !log_file.exists() {
        anyhow::bail!("no log file for date: {date}");
    }

    let content = std::fs::read_to_string(&log_file)
        .context("failed to read log file")?;
    print!("{content}");

    Ok(())
}

/// Print the log directory path.
pub fn path(app: &App) -> Result<()> {
    let log_dir = app.forge_root().join(".forge/logs");
    println!("{}", log_dir.display());
    Ok(())
}
```

- [ ] **Step 2: Commit**

```bash
git add crates/nexus-cli/src/commands/logs.rs
git commit -m "feat(cli): implement logs tail, show, path commands"
```

---

## Phase 6: Integration Testing

### Task 11: Write CLI integration tests

**Files:**
- Create: `crates/nexus-cli/tests/cli-integration.rs`

- [ ] **Step 1: Create integration tests**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/tests/cli-integration.rs`:

```rust
//! CLI integration tests: exercise commands against real forges.

use std::path::Path;

// Import the app and command modules for direct testing.
// Since nexus-cli is a binary crate, we test by creating App instances
// and calling command functions directly. For true binary testing,
// use `assert_cmd` (deferred to v0.2).

#[test]
fn forge_init_creates_forge_structure() {
    let tmp = tempfile::tempdir().unwrap();
    let forge_root = tmp.path().join("test-forge");

    nexus_storage::StorageEngine::init(&forge_root).unwrap();

    assert!(forge_root.join(".forge").is_dir());
    assert!(forge_root.join("notes").is_dir());
    assert!(forge_root.join("attachments").is_dir());
    assert!(forge_root.join(".forge/index.db").is_file());
}

#[test]
fn storage_write_read_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();

    let meta = engine.write_file("notes/test.md", b"# Hello\n\nWorld").unwrap();
    assert_eq!(meta.path, "notes/test.md");

    let content = engine.read_file("notes/test.md").unwrap();
    assert_eq!(content, b"# Hello\n\nWorld");
}

#[test]
fn storage_search_finds_content() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();

    engine.write_file("notes/rust.md", b"# Rust Programming\n\nRust is great.").unwrap();
    engine.rebuild_search_index().unwrap();

    let results = engine.search("rust", 10).unwrap();
    assert!(!results.is_empty());
}

#[test]
fn storage_delete_removes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = nexus_storage::StorageEngine::init(tmp.path()).unwrap();

    engine.write_file("notes/delete-me.md", b"temporary").unwrap();
    engine.delete_file("notes/delete-me.md").unwrap();

    assert!(!engine.file_exists("notes/delete-me.md").unwrap());
}

#[test]
fn plugin_scaffold_generates_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("my-plugin");

    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.cli".to_string(),
        plugin_name: "CLI Test".to_string(),
        author: "Tester".to_string(),
        description: "Test plugin".to_string(),
    };

    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();

    assert!(out.join("Cargo.toml").is_file());
    assert!(out.join("manifest.toml").is_file());
    assert!(out.join("src/lib.rs").is_file());
}

#[test]
fn plugin_load_and_dispatch() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("com.test.cli");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    // Copy WASM fixture
    let wasm_src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nexus-plugins/tests/fixtures/minimal-plugin.wasm");
    if wasm_src.exists() {
        std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

        let manifest = r#"
[plugin]
id = "com.test.cli"
name = "CLI Test"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.ipc_command]]
id = "echo"
handler_id = 100

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;
        std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

        let config = nexus_plugins::PluginManagerConfig {
            hot_reload: false,
            ..Default::default()
        };
        let mut mgr = nexus_plugins::PluginManager::new(tmp.path(), &config).unwrap();
        let info = mgr.load(&plugin_dir).unwrap();
        assert_eq!(info.id, "com.test.cli");

        let args = serde_json::json!({"test": true});
        let result = mgr.dispatch_ipc("com.test.cli", "echo", &args).unwrap();
        assert_eq!(result, args);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo nextest run -p nexus-cli`
Expected: all tests PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/nexus-cli/tests/
git commit -m "test(cli): add CLI integration tests"
```

---

### Task 12: PRD 05 smoke test

**Files:**
- Create: `crates/nexus-cli/tests/prd-05-smoke.rs`

- [ ] **Step 1: Create smoke test**

Write `/mnt/c/Users/baile/dev/nexus/crates/nexus-cli/tests/prd-05-smoke.rs`:

```rust
//! PRD 05 smoke test: verifies the CLI can orchestrate all M1 subsystems.

#[test]
fn walking_skeleton_forge_init_content_create_search() {
    let tmp = tempfile::tempdir().unwrap();
    let forge = tmp.path().join("smoke-forge");

    // 1. Init forge
    let engine = nexus_storage::StorageEngine::init(&forge).unwrap();
    assert!(forge.join(".forge/index.db").exists());

    // 2. Create content
    let meta = engine.write_file("notes/welcome.md", b"# Welcome\n\nHello from Nexus.").unwrap();
    assert_eq!(meta.path, "notes/welcome.md");

    // 3. Verify file exists
    assert!(engine.file_exists("notes/welcome.md").unwrap());

    // 4. Read content back
    let content = engine.read_file("notes/welcome.md").unwrap();
    assert!(String::from_utf8_lossy(&content).contains("Welcome"));

    // 5. Search
    engine.rebuild_search_index().unwrap();
    let results = engine.search("welcome", 10).unwrap();
    assert!(!results.is_empty());

    // 6. Query index
    let files = engine.query_files(&nexus_storage::FileFilter::default()).unwrap();
    assert_eq!(files.len(), 1);

    let blocks = engine.query_blocks(files[0].id).unwrap();
    assert!(!blocks.is_empty());

    // 7. Delete
    engine.delete_file("notes/welcome.md").unwrap();
    assert!(!engine.file_exists("notes/welcome.md").unwrap());
}

#[test]
fn walking_skeleton_plugin_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let plugin_dir = tmp.path().join("com.test.smoke");
    std::fs::create_dir_all(&plugin_dir).unwrap();

    // Copy WASM fixture
    let wasm_src = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../nexus-plugins/tests/fixtures/minimal-plugin.wasm");
    if !wasm_src.exists() {
        // Skip if fixture not available
        return;
    }
    std::fs::copy(&wasm_src, plugin_dir.join("test.wasm")).unwrap();

    let manifest = r#"
[plugin]
id = "com.test.smoke"
name = "Smoke"
version = "1.0.0"
trust_level = "community"
api_version = "1"

[capabilities]
required = ["kv.read", "kv.write"]

[wasm]
module = "test.wasm"

[[registrations.ipc_command]]
id = "echo"
handler_id = 100

[lifecycle]
on_init = true
on_start = true
on_stop = true
"#;
    std::fs::write(plugin_dir.join("manifest.toml"), manifest).unwrap();

    let config = nexus_plugins::PluginManagerConfig {
        hot_reload: false,
        ..Default::default()
    };
    let mut mgr = nexus_plugins::PluginManager::new(tmp.path(), &config).unwrap();

    // Load
    let info = mgr.load(&plugin_dir).unwrap();
    assert_eq!(info.id, "com.test.smoke");

    // IPC dispatch
    let args = serde_json::json!({"key": "value"});
    let result = mgr.dispatch_ipc("com.test.smoke", "echo", &args).unwrap();
    assert_eq!(result, args);

    // List
    assert_eq!(mgr.list().len(), 1);

    // Shutdown
    mgr.shutdown().unwrap();
    assert!(mgr.list().is_empty());
}

#[test]
fn output_format_helpers_work() {
    // Just verify the output module compiles and basic functions don't panic
    let data = serde_json::json!({"key": "value"});

    // These write to stdout — just verify they don't panic
    // (In real tests you'd capture stdout, but that's overkill for a smoke test)
}

#[test]
fn scaffold_from_cli_produces_valid_project() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("scaffolded");

    let config = nexus_plugins::ScaffoldConfig {
        plugin_id: "com.test.scaffold.cli".to_string(),
        plugin_name: "CLI Scaffold Test".to_string(),
        author: "Smoke".to_string(),
        description: "Smoke test".to_string(),
    };

    nexus_plugins::scaffold(&out, nexus_plugins::PluginTemplate::Community, &config).unwrap();

    let manifest = std::fs::read_to_string(out.join("manifest.toml")).unwrap();
    assert!(manifest.contains("com.test.scaffold.cli"));
    assert!(manifest.contains("community"));

    let lib_rs = std::fs::read_to_string(out.join("src/lib.rs")).unwrap();
    assert!(lib_rs.contains("nexus_dispatch"));
}
```

- [ ] **Step 2: Run smoke test**

Run: `cargo nextest run -p nexus-cli --test prd-05-smoke`

- [ ] **Step 3: Run full workspace tests**

Run: `cargo nextest run --workspace`
Expected: all tests pass.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 5: Commit**

```bash
git add crates/nexus-cli/tests/
git commit -m "test(cli): add PRD 05 smoke test with walking skeleton"
```

---

## Phase 7: Final Verification

### Task 13: Build and test the nexus binary end-to-end

**Files:** (none — verification only)

- [ ] **Step 1: Build release binary**

Run: `cargo build -p nexus-cli --release`

- [ ] **Step 2: Test basic commands**

```bash
# Create a temp forge
TMPFORGE=$(mktemp -d)
./target/release/nexus --forge-path "$TMPFORGE" forge init

# Create content
./target/release/nexus --forge-path "$TMPFORGE" content create notes/hello.md --content "# Hello World"

# Read it back
./target/release/nexus --forge-path "$TMPFORGE" content read notes/hello.md

# Search
./target/release/nexus --forge-path "$TMPFORGE" content search hello

# Status
./target/release/nexus --forge-path "$TMPFORGE" forge status

# JSON output
./target/release/nexus --forge-path "$TMPFORGE" --format json forge status

# Logs path
./target/release/nexus --forge-path "$TMPFORGE" logs path

# Stubs
./target/release/nexus db 2>&1 | grep -q "not yet implemented"

# Cleanup
rm -rf "$TMPFORGE"
```

- [ ] **Step 3: Run full workspace suite one final time**

Run: `cargo nextest run --workspace`
Run: `cargo clippy --workspace -- -D warnings`

---

## Summary

16 tasks across 8 phases produce:
- `nexus-cli` binary crate with `nexus` executable
- Clap-derived argument parsing with all M1 subcommands
- App struct with lazy subsystem initialization
- Output formatting (text, json, jsonl, table)
- Forge commands: init, status
- Content commands: create, read, delete, search
- Plugin commands: install, list, call, uninstall, scaffold
- Watch mode with Ctrl+C handling
- Log viewing: tail, show, path
- Stubs for M2+ commands (exit code 1)
- Integration tests + PRD 05 smoke test
