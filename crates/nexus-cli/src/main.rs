mod app;
mod commands;
mod output;
mod stubs;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "nexus", about = "Nexus IDE — headless CLI", version, allow_external_subcommands = true)]
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

    /// Safe mode: skip every community (non-core) plugin at load time.
    /// Equivalent to setting `NEXUS_SAFE_MODE=1`. Useful for recovering
    /// from a misbehaving community plugin without hand-editing manifests.
    #[arg(long, global = true, env = "NEXUS_SAFE_MODE")]
    safe_mode: bool,

    #[command(subcommand)]
    command: Commands,
}

// ---------------------------------------------------------------------------
// Top-level subcommands
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
enum Commands {
    /// Launch the terminal UI
    Tui,
    /// Launch the desktop shell (nexus-shell). Any arguments after `desktop`
    /// are forwarded to the shell binary.
    Desktop {
        /// Passthrough args forwarded to `nexus-shell`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
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
    /// Tag operations
    Tags(TagsArgs),

    // -----------------------------------------------------------------------
    // Stub commands — implemented in later milestones
    // -----------------------------------------------------------------------

    /// Canvas file operations
    Canvas(CanvasArgs),
    /// Configuration management
    Config(ConfigArgs),
    /// Bases (database) operations
    Bases(BasesArgs),
    /// Database engine operations — wraps `com.nexus.database` IPC handlers
    /// (PRD-10). Lower-level twin of `bases` (which works at the filesystem
    /// layer); `db` works on raw records / formulas via `ipc_call`.
    Db {
        #[command(subcommand)]
        cmd: commands::db::DbCommand,
    },

    /// AI assistant operations
    Ai(AiArgs),
    /// Agent operations (PRD-15): plan + execute tool-calling loops
    Agent(AgentArgs),
    /// Skill operations (PRD-13): list and inspect `.skill.md` files
    Skill(SkillArgs),
    /// Workflow operations (PRD-16): list/show/validate `.workflow.toml` files
    Workflow(WorkflowArgs),
    /// Process / saved-command management (PRD-09 §14.1)
    Proc(ProcArgs),
    /// Terminal / PTY session operations (PRD-09)
    Term(TermArgs),
    /// MCP (Model Context Protocol): run server or operate as host
    Mcp(McpArgs),
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

    /// Import external knowledge-tool exports (Notion, …)
    Import(ImportArgs),

    /// Plugin-registered subcommand (`nexus <plugin-id> [args…]`)
    #[command(external_subcommand)]
    External(Vec<OsString>),
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
    /// Update (overwrite) an existing content node
    Update {
        /// Path of the node to update
        path: String,
        /// Inline content body
        #[arg(long)]
        content: Option<String>,
        /// Read body from stdin
        #[arg(long)]
        stdin: bool,
    },
    /// List content nodes (optionally filtered by path prefix)
    List {
        /// Only include paths that start with this prefix
        #[arg(long)]
        prefix: Option<String>,
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
// Tags
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct TagsArgs {
    #[command(subcommand)]
    command: TagsCommand,
}

#[derive(Subcommand)]
enum TagsCommand {
    /// List tag occurrences across the forge (optionally filtered by name)
    List {
        /// Filter to a specific tag name (omit to list every tag occurrence)
        #[arg(long)]
        name: Option<String>,
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
    /// Multi-turn streaming chat REPL (BL-010)
    Chat {
        /// Forge-relative file whose contents seed the conversation
        /// as the first user message.
        #[arg(long)]
        context: Option<String>,
        /// Override the model (e.g. `claude-opus-4-7`). Defaults to
        /// whatever `ai config` resolves.
        #[arg(long)]
        model: Option<String>,
        /// Resume an existing session id; new uuid is generated when
        /// omitted.
        #[arg(long)]
        session: Option<String>,
        /// System prompt prepended to the conversation.
        #[arg(long)]
        system: Option<String>,
    },
    /// Headless single-shot completion (BL-011)
    Complete {
        /// Forge-relative file to read. The text up to the requested
        /// position is sent as the prompt.
        file: String,
        /// 1-based line number; defaults to the last line.
        #[arg(long)]
        line: Option<usize>,
        /// 1-based column on the chosen line; defaults to end-of-line.
        #[arg(long)]
        col: Option<usize>,
        /// Number of lines of leading context to retain. Defaults to
        /// the whole file (use a smaller value to bound the prompt).
        #[arg(long = "context")]
        context_lines: Option<usize>,
    },
}

// ---------------------------------------------------------------------------
// Agent (PRD-15)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct AgentArgs {
    #[command(subcommand)]
    command: AgentCommand,
}

#[derive(Subcommand)]
enum AgentCommand {
    /// Produce a plan for a goal without executing it
    Plan {
        /// Natural-language goal
        goal: String,
        /// Archetype — writer / coder / researcher / general (default)
        #[arg(long)]
        archetype: Option<String>,
    },
    /// Plan + execute a goal end-to-end
    Run {
        /// Natural-language goal
        goal: String,
        /// Archetype — writer / coder / researcher / general (default)
        #[arg(long)]
        archetype: Option<String>,
    },
    /// Execute a preset plan from a JSON file produced by `plan`
    RunPlan {
        /// Path to the plan JSON file
        file: PathBuf,
    },
}

// ---------------------------------------------------------------------------
// Proc (PRD-09 §14.1)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct ProcArgs {
    #[command(subcommand)]
    command: ProcCommand,
}

#[derive(Subcommand)]
enum ProcCommand {
    /// List every saved command (sidebar order, nulls last)
    List,
    /// Show full record for one saved command
    Show {
        /// Slug
        slug: String,
    },
    /// Create a new saved command
    Add {
        /// Human-readable label (also used to derive the slug)
        name: String,
        /// Full shell command to run
        command: String,
        /// Shell binary; defaults to /bin/sh
        #[arg(long)]
        shell: Option<String>,
        /// Working directory
        #[arg(long, value_name = "DIR")]
        cwd: Option<String>,
    },
    /// Delete a saved command
    Delete {
        /// Slug
        slug: String,
    },
    /// Set `sidebar_order` for a saved command (omit `--order` to clear)
    Reorder {
        /// Slug
        slug: String,
        /// New sidebar_order; omit to clear the override
        #[arg(long)]
        order: Option<i32>,
    },
}

// ---------------------------------------------------------------------------
// Skill (PRD-13)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct SkillArgs {
    #[command(subcommand)]
    command: SkillCommand,
}

#[derive(Subcommand)]
enum SkillCommand {
    /// List every loaded skill
    List,
    /// Show full frontmatter + body for one skill
    Show {
        /// Skill id
        id: String,
    },
    /// List skills whose applicable_contexts match
    Context {
        /// Context id (e.g. ai-chat, editor, pull-request)
        context: String,
    },
    /// List skills whose triggers substring-match the given text
    Triggered {
        /// Free-form text to match against each skill's trigger phrases
        text: String,
    },
    /// Re-scan the `.forge/skills/` directory
    Reload,
    /// Render a skill's body with parameter substitution
    Render {
        /// Skill id
        id: String,
        /// Parameter override(s) in `key=value` form (repeatable)
        #[arg(long = "param", short = 'p', value_name = "KEY=VALUE")]
        params: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// MCP (PRD-14)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct McpArgs {
    #[command(subcommand)]
    command: McpCommand,
}

#[derive(Subcommand)]
enum McpCommand {
    /// Start Nexus MCP server on stdio (exposes forge ops to external MCP clients)
    Serve,
    /// List external MCP servers configured in `.forge/mcp.toml`
    Servers,
    /// List tools exposed by one external MCP server
    Tools {
        /// Server name as declared in `.forge/mcp.toml`
        server: String,
    },
    /// Invoke a tool on an external MCP server
    Call {
        /// Server name as declared in `.forge/mcp.toml`
        server: String,
        /// Tool name
        tool: String,
        /// JSON object of tool arguments (defaults to `{}`)
        #[arg(long, default_value = "{}")]
        arguments: String,
    },
}

// ---------------------------------------------------------------------------
// Workflow (PRD-16)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct WorkflowArgs {
    #[command(subcommand)]
    command: WorkflowCommand,
}

#[derive(Subcommand)]
enum WorkflowCommand {
    /// List every loaded workflow
    List,
    /// Show full metadata for one workflow
    Show {
        /// Workflow name (as declared in `[workflow].name`)
        name: String,
    },
    /// Execute a workflow end-to-end
    Run {
        /// Workflow name (as declared in `[workflow].name`)
        name: String,
    },
    /// Re-scan the `.workflows/` directory
    Reload,
    /// Validate a `.workflow.toml` file without loading it into the registry
    Validate {
        /// Path to the workflow file
        file: PathBuf,
    },
    /// Built-in workflow templates (BL-028f)
    Template(WorkflowTemplateArgs),
}

#[derive(Parser)]
struct WorkflowTemplateArgs {
    #[command(subcommand)]
    command: WorkflowTemplateCommand,
}

#[derive(Subcommand)]
enum WorkflowTemplateCommand {
    /// List every built-in template
    List,
    /// Print one template's TOML body to stdout
    Show {
        /// Template slug (kebab-case, e.g. `daily-journal`)
        slug: String,
    },
    /// Instantiate a template into the forge's `.workflows/` directory
    Init {
        /// Template slug
        slug: String,
        /// Optional filename override (basename only, no path separators)
        #[arg(long)]
        as_file: Option<String>,
        /// Overwrite an existing file at the target path
        #[arg(long)]
        overwrite: bool,
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
    /// Install a plugin. If `plugin` is an existing local directory, the
    /// kernel loads it from disk (legacy behavior). Otherwise the argument
    /// is treated as a marketplace plugin id and the command prints a stub
    /// message pointing at Phase 5 WI-44.
    Install {
        /// Local plugin directory path, or marketplace plugin id
        /// (e.g. `community.hello-world`).
        plugin: String,
    },
    /// List installed plugins. Default lists kernel plugins from the forge;
    /// with `--shell`, enumerates shell plugins under `~/.nexus-shell/plugins/`.
    List {
        /// List shell plugins from `~/.nexus-shell/plugins/` instead of
        /// kernel plugins.
        #[arg(long)]
        shell: bool,
    },
    /// Remove a shell plugin by id — deletes `~/.nexus-shell/plugins/<id>/`.
    /// For kernel plugins, use `plugin uninstall`.
    Remove {
        /// Plugin id (directory name under `~/.nexus-shell/plugins/`).
        id: String,
        /// Skip confirmation prompt.
        #[arg(short = 'y', long)]
        yes: bool,
    },
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
    /// Scaffold a new plugin project from a built-in template.
    ///
    /// Templates:
    ///   script     — sandboxed JS/TS community plugin (default, modern path).
    ///                Emits plugin.json, index.ts, package.json, tsconfig.json,
    ///                README.md. Run `pnpm install && pnpm build` inside the
    ///                output directory to produce `index.js`.
    ///   core       — WASM plugin, maximum trust, no capability gates (legacy).
    ///   community  — WASM plugin, capability-gated (legacy).
    Scaffold {
        /// Template to scaffold from: `script` (default), `core`, or
        /// `community`. `--type` is accepted as an alias for back-compat.
        #[arg(long = "template", alias = "type", default_value = "script")]
        plugin_type: String,
        /// Plugin identifier (reverse-DNS form, e.g. `com.example.hello`).
        #[arg(long)]
        id: String,
        /// Human-readable plugin name.
        #[arg(long)]
        name: String,
        /// Author name or e-mail.
        #[arg(long, default_value = "Unknown")]
        author: String,
        /// Output directory (defaults to `./<id>/`).
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
    /// Reset a plugin's crash counter (F-8.2.1). Quarantined plugins
    /// (crashed ≥ `max_crashes` times) skip load until this is called.
    Reset {
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
    /// Query records from a base (with optional filters and sorting)
    Query {
        /// Path to the .bases directory
        path: String,
        /// Filter expression (e.g., "status = Done"). Repeatable.
        #[arg(long)]
        filter: Vec<String>,
        /// Sort expression (e.g., "priority desc"). Repeatable.
        #[arg(long)]
        sort: Vec<String>,
        /// Maximum number of records to return
        #[arg(long)]
        limit: Option<u32>,
        /// Number of records to skip
        #[arg(long)]
        offset: Option<u32>,
    },
    /// Import records from a CSV file
    Import {
        /// Path to the .bases directory
        path: String,
        /// CSV file to import from
        #[arg(long)]
        file: String,
        /// Whether the CSV has a header row
        #[arg(long, default_value = "true")]
        header: bool,
    },
    /// Export records to a CSV file
    Export {
        /// Path to the .bases directory
        path: String,
        /// CSV file to export to
        #[arg(long)]
        file: String,
    },
    /// Evaluate a formula against a record
    Formula {
        /// Path to the .bases directory
        path: String,
        /// Record ID
        #[arg(long)]
        record: String,
        /// Formula expression (e.g., 'if(prop("status") == "done", 1, 0)')
        #[arg(long)]
        expr: String,
    },
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct ImportArgs {
    #[command(subcommand)]
    command: ImportCommand,
}

#[derive(Subcommand)]
enum ImportCommand {
    /// Import a Notion markdown-export zip
    Notion {
        /// Path to the Notion zip export
        #[arg(long = "source")]
        source: PathBuf,
        /// Forge-relative destination directory (default: "Imported from Notion")
        #[arg(long = "dest")]
        dest: Option<PathBuf>,
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
// Term (PRD-09 §3.7)
// ---------------------------------------------------------------------------

#[derive(Parser)]
struct TermArgs {
    #[command(subcommand)]
    command: TermCommand,
}

#[derive(Subcommand)]
enum TermCommand {
    /// Print the default shell nexus-terminal would pick on this host.
    Env,
    /// Run a command in a PTY shell, stream ANSI-stripped output to
    /// stdout, and exit with the child's status code.
    Run {
        /// The command string passed to `sh -c`. Wrap multi-word
        /// commands in shell quoting as usual.
        cmd: String,
        /// Wall-clock budget in seconds. On overshoot the session is
        /// shut down and the CLI exits 124 (GNU `timeout` convention).
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },
    /// Attach the current terminal to a fresh PTY shell. Output is
    /// ANSI-stripped and printed line-by-line; stdin forwarding lands
    /// in the future daemon-backed terminal surface. Useful as a
    /// manual verification path — run it, watch the shell banner
    /// appear, Ctrl-C to exit.
    Shell,
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
    // Install the local panic hook before anything that could panic
    // (argument parsing, tracing setup). Entries land in
    // ~/.nexus-shell/logs/panic.log. See docs/planning/PHASE-5-IMPLEMENTATION-PLAN.md §4.
    nexus_panic_log::install("nexus");

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
    app.set_safe_mode(cli.safe_mode);
    if cli.safe_mode {
        tracing::warn!(audit = true, "safe mode: community plugins will be skipped");
    }

    let result = match cli.command {
        Commands::Tui => commands::tui::run(),

        Commands::Desktop { args } => match commands::desktop::launch(&args) {
            Ok(code) => std::process::exit(code),
            Err(e) => Err(e),
        },

        Commands::Forge(args) => match args.command {
            ForgeCommand::Init { dir } => commands::forge::init(&app, dir),
            ForgeCommand::Status => commands::forge::status(&mut app),
            ForgeCommand::Reindex => commands::forge::reindex(&mut app),
        },

        Commands::Content(args) => match args.command {
            ContentCommand::Create { path, content, stdin } => {
                commands::content::create(&mut app, &path, content.as_deref(), stdin)
            }
            ContentCommand::Update { path, content, stdin } => {
                commands::content::update(&mut app, &path, content.as_deref(), stdin)
            }
            ContentCommand::List { prefix } => {
                commands::content::list(&mut app, prefix.as_deref())
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
            PluginCommand::Install { plugin } => {
                commands::plugin::install_dispatch(&mut app, &plugin)
            }
            PluginCommand::List { shell } => {
                if shell {
                    commands::plugin::list_shell_plugins()
                } else {
                    commands::plugin::list(&mut app)
                }
            }
            PluginCommand::Remove { id, yes } => {
                commands::plugin::remove_shell_plugin(&id, yes)
            }
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
            PluginCommand::Reset { plugin_id } => {
                commands::plugin::reset_crash(&mut app, &plugin_id)
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

        Commands::Tags(args) => match args.command {
            TagsCommand::List { name } => commands::tags::list(&mut app, name.as_deref()),
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
            ConfigCommand::Show { file } => commands::config::show(&mut app, &file),
            ConfigCommand::Reset { file } => commands::config::reset(&mut app, &file),
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
            BasesCommand::Query {
                path,
                filter,
                sort,
                limit,
                offset,
            } => commands::bases::query(&mut app, &path, &filter, &sort, limit, offset),
            BasesCommand::Import {
                path,
                file,
                header,
            } => commands::bases::import(&mut app, &path, &file, header),
            BasesCommand::Export { path, file } => {
                commands::bases::export(&mut app, &path, &file)
            }
            BasesCommand::Formula {
                path,
                record,
                expr,
            } => commands::bases::formula(&mut app, &path, &record, &expr),
        },

        Commands::Db { cmd } => commands::db::run(&mut app, cmd),

        Commands::Ai(args) => match args.command {
            AiCommand::Ask { question } => commands::ai::ask(&mut app, &question),
            AiCommand::Embed { file } => commands::ai::embed(&mut app, file.as_deref()),
            AiCommand::Status => commands::ai::status(&mut app),
            AiCommand::Config => commands::ai::config(&mut app),
            AiCommand::Chat {
                context,
                model,
                session,
                system,
            } => commands::ai::chat(
                &mut app,
                context.as_deref(),
                model.as_deref(),
                session.as_deref(),
                system.as_deref(),
            ),
            AiCommand::Complete {
                file,
                line,
                col,
                context_lines,
            } => commands::ai::complete(&mut app, &file, line, col, context_lines),
        },

        Commands::Agent(args) => match args.command {
            AgentCommand::Plan { goal, archetype } => {
                commands::agent::plan(&mut app, &goal, archetype.as_deref())
            }
            AgentCommand::Run { goal, archetype } => {
                commands::agent::run(&mut app, &goal, archetype.as_deref())
            }
            AgentCommand::RunPlan { file } => {
                commands::agent::run_plan(&mut app, &file.to_string_lossy())
            }
        },

        Commands::Skill(args) => match args.command {
            SkillCommand::List => commands::skill::list(&mut app),
            SkillCommand::Show { id } => commands::skill::show(&mut app, &id),
            SkillCommand::Context { context } => commands::skill::context(&mut app, &context),
            SkillCommand::Triggered { text } => commands::skill::triggered(&mut app, &text),
            SkillCommand::Reload => commands::skill::reload(&mut app),
            SkillCommand::Render { id, params } => {
                commands::skill::render(&mut app, &id, &params)
            }
        },

        Commands::Workflow(args) => match args.command {
            WorkflowCommand::List => commands::workflow::list(&mut app),
            WorkflowCommand::Show { name } => commands::workflow::show(&mut app, &name),
            WorkflowCommand::Run { name } => commands::workflow::run(&mut app, &name),
            WorkflowCommand::Reload => commands::workflow::reload(&mut app),
            WorkflowCommand::Validate { file } => {
                commands::workflow::validate(&mut app, &file.to_string_lossy())
            }
            WorkflowCommand::Template(targs) => match targs.command {
                WorkflowTemplateCommand::List => commands::workflow::template_list(&mut app),
                WorkflowTemplateCommand::Show { slug } => {
                    commands::workflow::template_show(&mut app, &slug)
                }
                WorkflowTemplateCommand::Init {
                    slug,
                    as_file,
                    overwrite,
                } => commands::workflow::template_init(
                    &mut app,
                    &slug,
                    as_file.as_deref(),
                    overwrite,
                ),
            },
        },

        Commands::Proc(args) => match args.command {
            ProcCommand::List => commands::proc::list(&mut app),
            ProcCommand::Show { slug } => commands::proc::show(&mut app, &slug),
            ProcCommand::Add {
                name,
                command,
                shell,
                cwd,
            } => commands::proc::add(&mut app, &name, &command, shell.as_deref(), cwd.as_deref()),
            ProcCommand::Delete { slug } => commands::proc::delete(&mut app, &slug),
            ProcCommand::Reorder { slug, order } => {
                commands::proc::reorder(&mut app, &slug, order)
            }
        },

        // Stub commands — implemented in later milestones.
        Commands::Term(args) => match args.command {
            TermCommand::Env => commands::term::env(),
            TermCommand::Run { cmd, timeout } => match commands::term::run(&cmd, timeout) {
                Ok(code) => std::process::exit(code),
                Err(e) => Err(e),
            },
            TermCommand::Shell => match commands::term::shell() {
                Ok(code) => std::process::exit(code),
                Err(e) => Err(e),
            },
        },
        Commands::Mcp(args) => match args.command {
            McpCommand::Serve => commands::mcp::serve(&app),
            McpCommand::Servers => commands::mcp::host_servers(&mut app),
            McpCommand::Tools { server } => commands::mcp::host_tools(&mut app, &server),
            McpCommand::Call {
                server,
                tool,
                arguments,
            } => commands::mcp::host_call(&mut app, &server, &tool, &arguments),
        },
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

        Commands::Import(args) => match args.command {
            ImportCommand::Notion { source, dest } => {
                commands::import::notion_zip(&app, &source, dest)
            }
        },

        // Dispatch to a plugin-registered CLI subcommand: `nexus <subcommand> [args…]`
        Commands::External(raw_args) => {
            let subcommand = raw_args
                .first()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let rest: Vec<String> = raw_args[1..]
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            commands::plugin::dispatch_external(&mut app, &subcommand, rest)
        }
    };

    if let Err(err) = result {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    //! Argument-parsing tests for the MCP-parity subcommands (WI-40).
    //!
    //! These exercise only `clap` wiring — they do not invoke a runtime.
    use super::*;

    #[test]
    fn parse_content_update_with_stdin_flag() {
        let cli = Cli::try_parse_from(["nexus", "content", "update", "foo.md", "--stdin"])
            .expect("parse content update --stdin");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Update { path, content, stdin } => {
                    assert_eq!(path, "foo.md");
                    assert!(content.is_none());
                    assert!(stdin);
                }
                other => panic!("expected Update, got {:?}", std::mem::discriminant(&other)),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_content_update_with_content_flag() {
        let cli = Cli::try_parse_from([
            "nexus", "content", "update", "notes/a.md", "--content", "hello",
        ])
        .expect("parse content update --content");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Update { path, content, stdin } => {
                    assert_eq!(path, "notes/a.md");
                    assert_eq!(content.as_deref(), Some("hello"));
                    assert!(!stdin);
                }
                _ => panic!("expected Update"),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_content_list_default_prefix_is_none() {
        let cli = Cli::try_parse_from(["nexus", "content", "list"]).expect("parse content list");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::List { prefix } => assert!(prefix.is_none()),
                _ => panic!("expected List"),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_content_list_with_prefix() {
        let cli = Cli::try_parse_from(["nexus", "content", "list", "--prefix", "notes/"])
            .expect("parse content list --prefix");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::List { prefix } => assert_eq!(prefix.as_deref(), Some("notes/")),
                _ => panic!("expected List"),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_tags_list_without_filter() {
        let cli = Cli::try_parse_from(["nexus", "tags", "list"]).expect("parse tags list");
        match cli.command {
            Commands::Tags(args) => match args.command {
                TagsCommand::List { name } => assert!(name.is_none()),
            },
            _ => panic!("expected Tags subcommand"),
        }
    }

    #[test]
    fn help_lists_new_mcp_parity_subcommands() {
        // Smoke check that the three new subcommands (WI-40) are discoverable
        // through `--help`. We inspect the rendered help strings instead of
        // spawning the binary because integration tests run inside a sandbox
        // that forbids `./target/debug/nexus`.
        let mut cmd = Cli::command();

        // Top-level: `nexus tags` subtree must exist.
        let top = cmd.render_long_help().to_string();
        assert!(top.contains("tags"), "top-level help missing 'tags':\n{top}");

        // `nexus content --help` must list Update and List.
        let content = cmd
            .find_subcommand_mut("content")
            .expect("content subcommand registered")
            .render_long_help()
            .to_string();
        assert!(content.contains("update"), "content help missing 'update':\n{content}");
        assert!(content.contains("list"), "content help missing 'list':\n{content}");

        // `nexus tags --help` must list list.
        let tags = cmd
            .find_subcommand_mut("tags")
            .expect("tags subcommand registered")
            .render_long_help()
            .to_string();
        assert!(tags.contains("list"), "tags help missing 'list':\n{tags}");
    }

    #[test]
    fn parse_tags_list_with_name_filter() {
        let cli = Cli::try_parse_from(["nexus", "tags", "list", "--name", "project"])
            .expect("parse tags list --name");
        match cli.command {
            Commands::Tags(args) => match args.command {
                TagsCommand::List { name } => assert_eq!(name.as_deref(), Some("project")),
            },
            _ => panic!("expected Tags subcommand"),
        }
    }
}
