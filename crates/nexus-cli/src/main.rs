mod app;
mod commands;
mod output;
mod stubs;

use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "nexus", about = "Nexus IDE — headless CLI", version)]
struct Cli {
    /// Path to the forge directory (overrides NEXUS_FORGE_PATH env var)
    #[arg(long, global = true, env = "NEXUS_FORGE_PATH")]
    forge_path: Option<PathBuf>,

    /// Path to a custom config file
    #[arg(long, global = true, env = "NEXUS_CONFIG")]
    config: Option<PathBuf>,

    /// Output format: text, json, jsonl, table
    #[arg(long, global = true, default_value = "text")]
    format: String,

    /// Increase verbosity (repeat for more: -v, -vv, -vvv)
    #[arg(short, long, global = true, action = ArgAction::Count)]
    verbose: u8,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

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

    /// Canvas file operations
    Canvas(CanvasArgs),
    /// Configuration management
    Config(ConfigArgs),
    /// Bases (database) operations
    Bases(BasesArgs),

    /// AI assistant operations
    Ai(AiArgs),
    /// Process management (coming soon)
    Proc(StubArgs),
    /// Terminal management (coming soon)
    Term(StubArgs),
    /// Start MCP server (stdio mode)
    Mcp,
    /// Sync operations (coming soon)
    Sync(StubArgs),
    /// Git operations (read-only)
    Git(GitArgs),
    /// Run a script or task (coming soon)
    Run(StubArgs),
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
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
    /// Rebuild the index from files on disk
    Reindex,
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
    /// Create or open today's daily note
    Daily {
        /// Date in YYYY-MM-DD format (defaults to today)
        #[arg(long)]
        date: Option<String>,
    },
    /// Export a note to HTML
    Export {
        /// Path of the note to export
        path: String,
        /// Output file path (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<String>,
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
// AI
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct AiArgs {
    #[command(subcommand)]
    command: AiCommand,
}

#[derive(Subcommand)]
enum AiCommand {
    /// Ask a question using RAG
    Ask {
        /// The question to ask
        question: String,
    },
    /// Generate embeddings
    Embed {
        /// Embed a specific file only
        #[arg(long)]
        file: Option<String>,
    },
    /// Show AI/embedding status
    Status,
    /// Show AI configuration
    Config,
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
    /// Enable a plugin
    Enable {
        /// Plugin identifier
        plugin_id: String,
    },
    /// Disable a plugin
    Disable {
        /// Plugin identifier
        plugin_id: String,
    },
    /// View or update plugin settings
    Settings {
        /// Plugin identifier
        plugin_id: String,
        /// New settings as JSON (omit to show current settings)
        #[arg(long)]
        set: Option<String>,
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
        #[arg(short = 'n', long, default_value_t = 50)]
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
// Canvas
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct CanvasArgs {
    #[command(subcommand)]
    command: CanvasCommand,
}

#[derive(Subcommand)]
enum CanvasCommand {
    /// Create a new empty canvas file
    Create {
        /// Vault-relative path for the canvas file
        path: String,
    },
    /// Show canvas summary (nodes, edges)
    Show {
        /// Path to the canvas file
        path: String,
    },
    /// Add a node to a canvas
    AddNode {
        /// Path to the canvas file
        path: String,
        /// Node type: file, text, link, group, database, terminal
        #[arg(long = "type")]
        node_type: String,
        /// Horizontal position
        #[arg(long, default_value_t = 0.0)]
        x: f64,
        /// Vertical position
        #[arg(long, default_value_t = 0.0)]
        y: f64,
        /// Node width
        #[arg(long, default_value_t = 300.0)]
        width: f64,
        /// Node height
        #[arg(long, default_value_t = 200.0)]
        height: f64,
        /// Content (file path, text, URL, command — depends on type)
        #[arg(long)]
        content: Option<String>,
        /// Display label
        #[arg(long)]
        label: Option<String>,
    },
    /// Add an edge between two nodes
    AddEdge {
        /// Path to the canvas file
        path: String,
        /// Source node ID
        #[arg(long)]
        from: String,
        /// Target node ID
        #[arg(long)]
        to: String,
        /// Edge style: solid, dashed, dotted
        #[arg(long = "type", default_value = "solid")]
        edge_type: String,
        /// Relationship label
        #[arg(long)]
        label: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Show current configuration
    Show {
        /// Config file to show: app, workspace, mcp, ai, all
        #[arg(long, default_value = "all")]
        file: String,
    },
    /// Reset a config file to defaults
    Reset {
        /// Config file to reset: app, workspace, mcp, ai
        #[arg(long)]
        file: String,
    },
}

// ---------------------------------------------------------------------------
// Bases
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct BasesArgs {
    #[command(subcommand)]
    command: BasesCommand,
}

#[derive(Subcommand)]
enum BasesCommand {
    /// Create a new base with a schema
    Create {
        /// Path for the .bases directory
        path: String,
        /// Schema definition as JSON
        #[arg(long)]
        schema: String,
    },
    /// List all bases
    List,
    /// Show base details
    Show {
        /// Path to the .bases directory
        path: String,
    },
    /// Add a record to a base
    AddRecord {
        /// Path to the .bases directory
        path: String,
        /// Record data as JSON
        #[arg(long)]
        data: String,
    },
    /// Query records from a base
    Query {
        /// Path to the .bases directory
        path: String,
    },
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct GitArgs {
    #[command(subcommand)]
    command: GitCommand,
}

#[derive(Subcommand)]
enum GitCommand {
    /// Show repository info (branch, HEAD, dirty state)
    Info,
    /// Show file statuses (modified, staged, untracked)
    Status,
    /// Show diff for a file or all staged changes
    Diff {
        /// File path to diff (omit for staged changes)
        path: Option<String>,
    },
    /// Show blame annotations for a file
    Blame {
        /// File path to blame
        path: String,
    },
    /// Show commit log
    Log {
        /// Maximum number of entries
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// Filter to a specific file
        #[arg(short, long)]
        file: Option<String>,
    },
    /// Stage a file for commit
    Stage {
        /// File path to stage (omit with --all to stage everything)
        path: Option<String>,
        /// Stage all changes
        #[arg(short, long)]
        all: bool,
    },
    /// Unstage a file
    Unstage {
        /// File path to unstage (omit with --all to unstage everything)
        path: Option<String>,
        /// Unstage all changes
        #[arg(short, long)]
        all: bool,
    },
    /// Create a commit from staged changes
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: String,
    },
    /// Branch operations
    Branch {
        #[command(subcommand)]
        command: Option<BranchCommand>,
    },
    /// Fetch refs from a remote
    Fetch {
        /// Remote name (default: origin)
        #[arg(default_value = "origin")]
        remote: String,
    },
    /// Push a branch to a remote
    Push {
        /// Remote name (default: origin)
        #[arg(default_value = "origin")]
        remote: String,
        /// Branch to push (default: current branch)
        branch: Option<String>,
    },
    /// Pull from a remote (fetch + merge)
    Pull {
        /// Remote name (default: origin)
        #[arg(default_value = "origin")]
        remote: String,
        /// Branch to pull (default: current branch)
        branch: Option<String>,
    },
    /// Merge a branch into the current branch
    Merge {
        /// Branch name to merge
        branch: Option<String>,
        /// Abort an in-progress merge
        #[arg(long)]
        abort: bool,
    },
    /// List files with unresolved merge conflicts
    Conflicts,
    /// List configured remotes
    Remotes,
    /// Auto-commit dirty changes
    AutoCommit {
        /// Run in watch mode (loop with timer)
        #[arg(long)]
        watch: bool,
        /// Interval in seconds for watch mode (default: 1800)
        #[arg(long, default_value_t = 1800)]
        interval: u64,
        /// Debounce window in seconds (default: 5)
        #[arg(long, default_value_t = 5)]
        debounce: u64,
    },
}

/// Branch subcommands.
#[derive(Subcommand)]
enum BranchCommand {
    /// Create a new branch from HEAD
    Create {
        /// Branch name
        name: String,
    },
    /// Switch to a branch
    Switch {
        /// Branch name
        name: String,
    },
    /// Delete a branch
    Delete {
        /// Branch name
        name: String,
    },
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

    // --quiet suppresses non-error output (wired into output helpers in
    // a future milestone; accepted here so scripts can pass it today).
    let _quiet = cli.quiet;

    // --config path is accepted and forwarded to the kernel config loader
    // (wired in a future milestone when the config subsystem is complete).
    let _config_path = cli.config;

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
            ForgeCommand::Reindex => commands::forge::reindex(&mut app),
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
            ContentCommand::Daily { date } => {
                commands::content::daily(&mut app, date.as_deref())
            }
            ContentCommand::Export { path, output } => {
                commands::content::export(&mut app, &path, output.as_deref())
            }
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
            PluginCommand::Enable { plugin_id } => {
                commands::plugin::enable(&mut app, &plugin_id)
            }
            PluginCommand::Disable { plugin_id } => {
                commands::plugin::disable(&mut app, &plugin_id)
            }
            PluginCommand::Settings { plugin_id, set } => {
                commands::plugin::settings(&mut app, &plugin_id, set.as_deref())
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

        Commands::Canvas(args) => match args.command {
            CanvasCommand::Create { path } => commands::canvas::create(&mut app, &path),
            CanvasCommand::Show { path } => commands::canvas::show(&mut app, &path),
            CanvasCommand::AddNode {
                path, node_type, x, y, width, height, content, label,
            } => commands::canvas::add_node(
                &mut app, &path, &node_type, x, y, width, height,
                content.as_deref(), label.as_deref(),
            ),
            CanvasCommand::AddEdge { path, from, to, edge_type, label } => {
                commands::canvas::add_edge(&mut app, &path, &from, &to, &edge_type, label.as_deref())
            }
        },

        Commands::Config(args) => match args.command {
            ConfigCommand::Show { file } => commands::config::show(&app, &file),
            ConfigCommand::Reset { file } => commands::config::reset(&app, &file),
        },

        Commands::Bases(args) => match args.command {
            BasesCommand::Create { path, schema } => {
                commands::bases::create(&mut app, &path, &schema)
            }
            BasesCommand::List => commands::bases::list(&mut app),
            BasesCommand::Show { path } => commands::bases::show(&mut app, &path),
            BasesCommand::AddRecord { path, data } => {
                commands::bases::add_record(&mut app, &path, &data)
            }
            BasesCommand::Query { path } => commands::bases::query(&mut app, &path),
        },

        Commands::Ai(args) => match args.command {
            AiCommand::Ask { question } => commands::ai::ask(&mut app, &question),
            AiCommand::Embed { file } => commands::ai::embed(&mut app, file.as_deref()),
            AiCommand::Status => commands::ai::status(&mut app),
            AiCommand::Config => commands::ai::config(),
        },

        // Stub commands — implemented in later milestones.
        Commands::Proc(_) => stubs::not_implemented("proc"),
        Commands::Term(_) => stubs::not_implemented("term"),
        Commands::Mcp => commands::mcp::serve(&app),
        Commands::Sync(_) => stubs::not_implemented("sync"),
        Commands::Git(args) => match args.command {
            GitCommand::Info => commands::git::info(&app),
            GitCommand::Status => commands::git::status(&app),
            GitCommand::Diff { path } => commands::git::diff(&app, path.as_deref()),
            GitCommand::Blame { path } => commands::git::blame(&app, &path),
            GitCommand::Log { limit, file } => commands::git::log(&app, limit, file.as_deref()),
            GitCommand::Stage { path, all } => commands::git::stage(&app, path.as_deref(), all),
            GitCommand::Unstage { path, all } => commands::git::unstage(&app, path.as_deref(), all),
            GitCommand::Commit { message } => commands::git::commit(&app, &message),
            GitCommand::Branch { command } => commands::git::branch(&app, command),
            GitCommand::Fetch { remote } => commands::git::fetch(&app, &remote),
            GitCommand::Push { remote, branch } => commands::git::push(&app, &remote, branch.as_deref()),
            GitCommand::Pull { remote, branch } => commands::git::pull(&app, &remote, branch.as_deref()),
            GitCommand::Merge { branch, abort } => commands::git::merge(&app, branch.as_deref(), abort),
            GitCommand::Conflicts => commands::git::conflicts(&app),
            GitCommand::Remotes => commands::git::remotes(&app),
            GitCommand::AutoCommit { watch, interval, debounce } => {
                commands::git::auto_commit(&app, watch, interval, debounce)
            }
        },
        Commands::Run(_) => stubs::not_implemented("run"),

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "nexus", &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
