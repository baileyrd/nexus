mod output;

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

    // -----------------------------------------------------------------------
    // Stub commands — implemented in later milestones
    // -----------------------------------------------------------------------

    /// Database operations (coming soon)
    Db(StubArgs),
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

    match cli.command {
        Commands::Forge(args) => match args.command {
            ForgeCommand::Init { dir } => {
                let target = dir
                    .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
                println!("forge init: {}", target.display());
            }
            ForgeCommand::Status => {
                println!("forge status");
            }
        },

        Commands::Content(args) => match args.command {
            ContentCommand::Create { path, content, stdin } => {
                println!("content create: path={path} content={content:?} stdin={stdin}");
            }
            ContentCommand::Read { path, raw } => {
                println!("content read: path={path} raw={raw}");
            }
            ContentCommand::Delete { path, force } => {
                println!("content delete: path={path} force={force}");
            }
            ContentCommand::Search { query, limit } => {
                println!("content search: query={query} limit={limit}");
            }
        },

        Commands::Plugin(args) => match args.command {
            PluginCommand::Install { dir } => {
                println!("plugin install: {}", dir.display());
            }
            PluginCommand::List => {
                println!("plugin list");
            }
            PluginCommand::Call { plugin_id, command, args } => {
                println!("plugin call: plugin={plugin_id} command={command} args={args:?}");
            }
            PluginCommand::Uninstall { plugin_id } => {
                println!("plugin uninstall: {plugin_id}");
            }
            PluginCommand::Scaffold { plugin_type, id, name, author, output } => {
                println!(
                    "plugin scaffold: type={plugin_type} id={id} name={name} author={author} output={output:?}"
                );
            }
        },

        Commands::Watch(args) => {
            println!("watch: glob={}", args.glob);
        }

        Commands::Logs(args) => match args.command {
            LogsCommand::Tail { level, lines } => {
                println!("logs tail: level={level} lines={lines}");
            }
            LogsCommand::Show { date } => {
                println!("logs show: date={date}");
            }
            LogsCommand::Path => {
                println!("logs path");
            }
        },

        // Stub commands
        Commands::Db(args) => eprintln!("db: not yet implemented (args: {:?})", args.args),
        Commands::Ai(args) => eprintln!("ai: not yet implemented (args: {:?})", args.args),
        Commands::Proc(args) => eprintln!("proc: not yet implemented (args: {:?})", args.args),
        Commands::Term(args) => eprintln!("term: not yet implemented (args: {:?})", args.args),
        Commands::Mcp(args) => eprintln!("mcp: not yet implemented (args: {:?})", args.args),
        Commands::Sync(args) => eprintln!("sync: not yet implemented (args: {:?})", args.args),
        Commands::Git(args) => eprintln!("git: not yet implemented (args: {:?})", args.args),
        Commands::Run(args) => eprintln!("run: not yet implemented (args: {:?})", args.args),
    }

    // Suppress unused-variable warning for `format` until dispatch is wired up.
    let _ = format;
}
