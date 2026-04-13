mod app;
mod commands;
mod output;
mod stubs;

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "nexus", about = "Nexus IDE — headless CLI", version)]
struct Cli {
    /// Path to the forge directory (overrides NEXUS_FORGE_PATH env var)
    #[arg(long, global = true, env = "NEXUS_FORGE_PATH")]
    forge_path: Option<PathBuf>,

    /// Output format: text, json, jsonl, table
    #[arg(long, global = true, default_value = "text")]
    format: String,

    /// Increase verbosity (repeat for more: -v, -vv, -vvv)
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,

    /// Disable color output
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

// ---------------------------------------------------------------------------
// Top-level subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
enum Commands {
    /// Manage the forge (workspace)
    Forge(ForgeArgs),
    /// Manage content nodes
    Content(ContentArgs),
    /// Manage plugins
    Plugin(PluginArgs),
    /// Watch filesystem for changes
    Watch(WatchArgs),
    /// View logs
    Logs(LogsArgs),

    /// Knowledge graph operations
    Graph(GraphArgs),

    // -----------------------------------------------------------------------
    // Stub commands — implemented in later milestones
    // -----------------------------------------------------------------------

    /// AI assistant operations (coming soon)
    Ai(StubArgs),
    /// Process management (coming soon)
    Proc(StubArgs),
    /// Terminal management (coming soon)
    Term(StubArgs),
    /// Model Context Protocol operations (coming soon)
    Mcp(StubArgs),
    /// Sync operations (coming soon)
    Sync(StubArgs),
    /// Git operations (coming soon)
    Git(StubArgs),
    /// Run a script or task (coming soon)
    Run(StubArgs),
}

// ---------------------------------------------------------------------------
// Forge
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct ForgeArgs {
    #[command(subcommand)]
    command: ForgeCommand,
}

#[derive(Subcommand)]
enum ForgeCommand {
    /// Initialise a new forge
    Init {
        /// Directory in which to create the forge (defaults to current dir)
        dir: Option<PathBuf>,
    },
    /// Show forge status
    Status,
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct ContentArgs {
    #[command(subcommand)]
    command: ContentCommand,
}

#[derive(Subcommand)]
enum ContentCommand {
    /// Create a new content node
    Create {
        /// Path of the node to create
        path: String,
        /// Inline content body
        #[arg(long)]
        content: Option<String>,
        /// Read body from stdin
        #[arg(long)]
        stdin: bool,
    },
    /// Read a content node
    Read {
        /// Path of the node to read
        path: String,
        /// Emit raw body without metadata
        #[arg(long)]
        raw: bool,
    },
    /// Delete a content node
    Delete {
        /// Path of the node to delete
        path: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
    /// Search content nodes
    Search {
        /// Full-text query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },
    /// List tasks across the forge
    Tasks {
        /// Show only completed tasks
        #[arg(long)]
        completed: bool,
        /// Show all tasks (completed and pending)
        #[arg(long)]
        all: bool,
        /// Filter to tasks in a specific file
        #[arg(long)]
        file: Option<String>,
    },
    /// Toggle a task's completion state
    TaskToggle {
        /// Task ID to toggle
        id: u64,
    },
    /// Show outgoing links from a file
    Links {
        /// Path of the file
        path: String,
    },
    /// Show files that link to this file
    Backlinks {
        /// Path of the file
        path: String,
    },
}

// ---------------------------------------------------------------------------
// Graph
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct GraphArgs {
    #[command(subcommand)]
    command: GraphCommand,
}

#[derive(Subcommand)]
enum GraphCommand {
    /// Show knowledge graph statistics
    Status,
    /// List unresolved (broken) links
    Unresolved,
    /// Show files within N hops of a file
    Neighbors {
        /// Path of the file
        path: String,
        /// Maximum traversal depth
        #[arg(short, long, default_value_t = 1)]
        depth: usize,
    },
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// Install a plugin from a directory
    Install {
        /// Path to the plugin directory
        dir: PathBuf,
    },
    /// List installed plugins
    List,
    /// Call a plugin command
    Call {
        /// Plugin identifier
        plugin_id: String,
        /// Command to invoke on the plugin
        command: String,
        /// Arguments to pass to the command (JSON)
        #[arg(long)]
        args: Option<String>,
    },
    /// Uninstall a plugin
    Uninstall {
        /// Plugin identifier
        plugin_id: String,
    },
    /// Scaffold a new plugin
    Scaffold {
        /// Plugin type (e.g. wasm, native)
        #[arg(long = "type")]
        plugin_type: String,
        /// Plugin identifier
        #[arg(long)]
        id: String,
        /// Human-readable plugin name
        #[arg(long)]
        name: String,
        /// Author name
        #[arg(long)]
        author: String,
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Watch
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct WatchArgs {
    /// Glob pattern to watch (default: "**/*")
    #[arg(default_value = "**/*")]
    glob: String,
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct LogsArgs {
    #[command(subcommand)]
    command: LogsCommand,
}

#[derive(Subcommand)]
enum LogsCommand {
    /// Stream the most recent log entries
    Tail {
        /// Minimum log level to show (trace, debug, info, warn, error)
        #[arg(short, long, default_value = "info")]
        level: String,
        /// Number of historical lines to show before streaming
        #[arg(short, long, default_value_t = 50)]
        lines: usize,
    },
    /// Show logs for a specific date (YYYY-MM-DD)
    Show {
        /// Date in YYYY-MM-DD format
        date: String,
    },
    /// Print path to the log directory
    Path,
}

// ---------------------------------------------------------------------------
// Stub — used for not-yet-implemented command groups
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct StubArgs {
    /// Subcommand and arguments (not yet implemented)
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Resolve the forge root from CLI flag, env var, or default location.
fn default_forge_path(override_path: Option<PathBuf>) -> PathBuf {
    if let Some(p) = override_path {
        return p;
    }
    if let Ok(p) = std::env::var("NEXUS_FORGE_PATH") {
        return PathBuf::from(p);
    }
    // Fall back to $HOME/.nexus/default
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".nexus").join("default"))
        .unwrap_or_else(|| PathBuf::from(".nexus/default"))
}

fn main() {
    let cli = Cli::parse();

    let format = output::OutputFormat::from_str(&cli.format);

    // Initialise tracing at the requested verbosity level.
    let level = match cli.verbose {
        0 => tracing::Level::WARN,
        1 => tracing::Level::INFO,
        2 => tracing::Level::DEBUG,
        _ => tracing::Level::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_ansi(output::use_color(cli.no_color))
        .init();

    let forge_root = default_forge_path(cli.forge_path);
    let mut app = app::App::new(forge_root, format);

    let result = match cli.command {
        Commands::Forge(args) => match args.command {
            ForgeCommand::Init { dir } => commands::forge::init(&app, dir),
            ForgeCommand::Status => commands::forge::status(&mut app),
        },

        Commands::Content(args) => match args.command {
            ContentCommand::Create { path, content, stdin } => {
                commands::content::create(&mut app, &path, content.as_deref(), stdin)
            }
            ContentCommand::Read { path, raw } => commands::content::read(&mut app, &path, raw),
            ContentCommand::Delete { path, force } => {
                commands::content::delete(&mut app, &path, force)
            }
            ContentCommand::Search { query, limit } => {
                commands::content::search(&mut app, &query, limit)
            }
            ContentCommand::Tasks { completed, all, file } => {
                commands::content::tasks(&mut app, completed, all, file.as_deref())
            }
            ContentCommand::TaskToggle { id } => {
                commands::content::task_toggle(&mut app, id)
            }
            ContentCommand::Links { path } => commands::content::links(&mut app, &path),
            ContentCommand::Backlinks { path } => commands::content::backlinks(&mut app, &path),
        },

        Commands::Plugin(args) => match args.command {
            PluginCommand::Install { dir } => commands::plugin::install(&mut app, &dir),
            PluginCommand::List => commands::plugin::list(&mut app),
            PluginCommand::Call { plugin_id, command, args } => {
                let args_json = args.as_deref().unwrap_or("{}");
                commands::plugin::call(&mut app, &plugin_id, &command, args_json)
            }
            PluginCommand::Uninstall { plugin_id } => {
                commands::plugin::uninstall(&mut app, &plugin_id)
            }
            PluginCommand::Scaffold { plugin_type, id, name, author, output } => {
                commands::plugin::scaffold(
                    &plugin_type,
                    Some(&id),
                    Some(&name),
                    Some(&author),
                    output.as_deref(),
                )
            }
        },

        Commands::Watch(args) => commands::watch::run(&mut app, &args.glob),

        Commands::Logs(args) => match args.command {
            LogsCommand::Tail { level, lines } => {
                commands::logs::tail(&app, Some(&level), lines)
            }
            LogsCommand::Show { date } => commands::logs::show(&app, &date),
            LogsCommand::Path => commands::logs::path(&app),
        },

        Commands::Graph(args) => match args.command {
            GraphCommand::Status => commands::graph::status(&mut app),
            GraphCommand::Unresolved => commands::graph::unresolved(&mut app),
            GraphCommand::Neighbors { path, depth } => {
                commands::graph::neighbors(&mut app, &path, depth)
            }
        },

        // Stub commands — implemented in later milestones.
        Commands::Ai(_) => stubs::not_implemented("ai"),
        Commands::Proc(_) => stubs::not_implemented("proc"),
        Commands::Term(_) => stubs::not_implemented("term"),
        Commands::Mcp(_) => stubs::not_implemented("mcp"),
        Commands::Sync(_) => stubs::not_implemented("sync"),
        Commands::Git(_) => stubs::not_implemented("git"),
        Commands::Run(_) => stubs::not_implemented("run"),
    };

    if let Err(err) = result {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
