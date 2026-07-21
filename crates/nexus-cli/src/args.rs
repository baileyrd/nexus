// R8 / #191 — clap argument structs + subcommand enums lifted out of
// `main.rs` (which exceeded 2,500 LoC). Every type below is consumed by
// the `Commands` enum in `main.rs` via tuple-newtype variants
// (`Commands::Forge(args)`) and pattern-destructured in the dispatch
// match (`match args.command { ... }`). Struct and enum visibility plus
// the per-field visibility are `pub(crate)` so the existing destructure
// sites in `main.rs` keep compiling without `commands::*` re-exports.
//
// Imports mirror what `main.rs` had before the split:

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

// Forge
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct ForgeArgs {
    #[command(subcommand)]
    pub(crate) command: ForgeCommand,
}

#[derive(Subcommand)]
pub(crate) enum ForgeCommand {
    /// Initialise a new forge
    Init {
        /// Directory in which to create the forge (defaults to current dir)
        dir: Option<PathBuf>,
        /// Optional scaffold template. `os` lays out the
        /// raw / wiki / output / projects / ops / personal / archive
        /// folders + a memory-map `CLAUDE.md` per BL-054 Phase 1.
        #[arg(long, value_parser = ["os"])]
        template: Option<String>,
    },
    /// Show forge status
    Status,
    /// Rebuild the index from files on disk
    Reindex,
    /// Walk the forge and report files-vs-index drift (BL-137).
    ///
    /// Read-only by default: prints files on disk that the index
    /// doesn't know about, indexed files that have been deleted from
    /// disk, and entries where the on-disk `mtime` disagrees with the
    /// indexed `modified_at`. Pass `--fix` to invoke `rebuild_index`
    /// when drift is detected.
    Doctor {
        /// After reporting, rebuild the index if any drift was found.
        #[arg(long)]
        fix: bool,
    },
    /// Import another forge into this one (BL-083). Walks the
    /// source, hashes every file, classifies as copy / skip / conflict,
    /// then either reports the plan (`--dry-run`) or applies it.
    Import {
        /// Source forge directory to import.
        source: PathBuf,
        /// Report what would happen without touching the destination.
        #[arg(long)]
        dry_run: bool,
        /// Conflict-resolution strategy.
        #[arg(long, default_value = "skip", value_parser = ["skip", "overwrite", "rename"])]
        on_conflict: String,
    },
}

// ---------------------------------------------------------------------------
// Trash (C3 / #356)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct TrashArgs {
    #[command(subcommand)]
    pub(crate) command: TrashCommand,
}

#[derive(Subcommand)]
pub(crate) enum TrashCommand {
    /// List trashed entries, newest first
    List,
    /// Restore a trashed entry to its original path
    Restore {
        /// Bucket id (from `nexus trash list` or the delete output)
        trash_id: String,
    },
    /// Permanently delete trashed entries
    Empty {
        /// Only remove entries older than this many days
        #[arg(long, value_name = "DAYS")]
        older_than: Option<u64>,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct ContentArgs {
    #[command(subcommand)]
    pub(crate) command: ContentCommand,
}

#[derive(Subcommand)]
pub(crate) enum ContentCommand {
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
    /// Delete a content node (moves to the forge trash by default)
    Delete {
        /// Path of the node to delete
        path: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
        /// Bypass the trash and delete permanently (pre-C3 behaviour)
        #[arg(long)]
        permanent: bool,
    },
    /// Search content nodes
    Search {
        /// Full-text query
        query: String,
        /// Maximum number of results
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// #375 — skip this many ranked hits before taking the page of
        /// `--limit`, for paging through results.
        #[arg(long, default_value_t = 0)]
        offset: usize,
        /// #375 — sort order: "relevance" (default), "mtime-desc", or
        /// "mtime-asc".
        #[arg(long, default_value = "relevance")]
        sort: String,
        /// #375 — only include blocks whose file mtime is on or after
        /// this Unix-seconds timestamp.
        #[arg(long)]
        mtime_after: Option<i64>,
        /// #375 — only include blocks whose file mtime is on or before
        /// this Unix-seconds timestamp.
        #[arg(long)]
        mtime_before: Option<i64>,
        /// Vector-only semantic search (embedding similarity) instead
        /// of lexical FTS. Requires an AI embedding provider
        /// (`nexus ai config`). Mutually exclusive with `--hybrid`.
        #[arg(long, conflicts_with = "hybrid")]
        semantic: bool,
        /// Hybrid search: RRF fusion of lexical FTS (BM25) and vector
        /// similarity (C78 #431). Requires an AI embedding provider.
        /// Mutually exclusive with `--semantic`.
        #[arg(long, conflicts_with = "semantic")]
        hybrid: bool,
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
pub(crate) struct GraphArgs {
    #[command(subcommand)]
    pub(crate) command: GraphCommand,
}

#[derive(Subcommand)]
pub(crate) enum GraphCommand {
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
    /// Personal entity graph operations (BL-128)
    Entity {
        #[command(subcommand)]
        command: EntityCommand,
    },
    /// Dream Cycle — BL-129 entity-graph maintenance.
    #[command(name = "dream-cycle")]
    DreamCycle {
        #[command(subcommand)]
        command: DreamCycleCommand,
    },
}

#[derive(Subcommand)]
pub(crate) enum DreamCycleCommand {
    /// Run one or every supported maintenance phase: `dedup` (with
    /// auto-merge above `--merge-threshold`), `decay`, `enrich`
    /// (LLM-expanded descriptions), and `infer` (LLM-proposed new
    /// relations). The latter two require a configured AI provider
    /// and surface per-entity failures in the report.
    Run {
        /// Restrict to a single phase. Omit to run every phase in
        /// the spec order: dedup → decay → enrich → infer.
        #[arg(long, value_parser = ["dedup", "decay", "enrich", "infer"])]
        phase: Option<String>,
        /// Multiplicative decay factor in `(0.0, 1.0]`. Defaults to
        /// `0.95` server-side when omitted.
        #[arg(long)]
        decay_factor: Option<f32>,
        /// Lower bound for relation confidence. Defaults to `0.10`
        /// server-side when omitted.
        #[arg(long)]
        decay_floor: Option<f32>,
        /// Surface-for-review threshold for the dedup phase. Pairs at
        /// or above this value (and below `--merge-threshold`) are
        /// reported but not merged. Defaults to `0.92`.
        #[arg(long)]
        review_threshold: Option<f32>,
        /// Auto-merge threshold for the dedup phase. Pairs at or above
        /// this value are silently merged (lex-smaller id survives).
        /// Defaults to `0.97`.
        #[arg(long)]
        merge_threshold: Option<f32>,
        /// Compute counts but skip every write (no decay rewrites, no
        /// auto-merges). Surfaced counts still reflect what *would*
        /// have changed.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum EntityCommand {
    /// List entities (optionally filtered by `--type`)
    List {
        /// Filter to a single canonical entity_type.
        #[arg(long)]
        r#type: Option<String>,
        /// Maximum hits to return.
        #[arg(short, long, default_value_t = 50)]
        limit: u32,
    },
    /// Show one entity by canonical id or alias.
    Show {
        /// Canonical id or one of the entity's aliases.
        id: String,
    },
    /// Substring search across entity ids / aliases / descriptions.
    Search {
        /// Query string. Empty string returns the first `--limit` entities.
        query: String,
        /// Optional `entity_type` filter.
        #[arg(long)]
        r#type: Option<String>,
        /// Maximum hits to return.
        #[arg(short, long, default_value_t = 10)]
        limit: u32,
    },
    /// Show outgoing / incoming / both relations for one entity.
    Related {
        /// Canonical id or one of the entity's aliases.
        id: String,
        /// One of `outgoing`, `incoming`, `both`.
        #[arg(long, default_value = "both")]
        direction: String,
    },
    /// List same-type entity pairs with Jaccard similarity ≥ threshold
    /// (BL-129's Dream-Cycle dedup seed).
    Duplicates {
        /// Minimum similarity in `[0.0, 1.0]`. Defaults to 0.92.
        #[arg(long, default_value_t = 0.92)]
        threshold: f32,
    },
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct TagsArgs {
    #[command(subcommand)]
    pub(crate) command: TagsCommand,
}

#[derive(Subcommand)]
pub(crate) enum TagsCommand {
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
pub(crate) struct AiArgs {
    #[command(subcommand)]
    pub(crate) command: AiCommand,
}

#[derive(Subcommand)]
pub(crate) enum AiCommand {
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
    /// Export a persisted chat session as markdown (#384)
    Export {
        /// Session id to export; omit for the legacy single-session file.
        id: Option<String>,
        /// Write the markdown to this path instead of stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Observe/control `com.nexus.ai.runtime` background tasks (C79 #432):
    /// agent delegate fan-out and async workflow steps. No backend work
    /// needed — the observe/control IPC handlers already exist; this is
    /// the first frontend to reach them.
    Runtime {
        #[command(subcommand)]
        command: AiRuntimeCommand,
    },
}

#[derive(Subcommand)]
pub(crate) enum AiRuntimeCommand {
    /// List background tasks, optionally filtered by status.
    List {
        /// Only show tasks in this status (queued, running, paused,
        /// cancelled, completed, failed). Omit to show every status.
        #[arg(long)]
        status: Option<String>,
        /// Cap the number of rows returned.
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Show full detail for one task.
    Get {
        /// Task id (uuid), as printed by `list`.
        task_id: String,
    },
    /// Request cancellation of a queued or running task.
    Cancel {
        /// Task id (uuid), as printed by `list`.
        task_id: String,
        /// Human-readable reason, captured in the task's `Cancelled` event.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Show current worker-pool utilization (queued/running/max workers).
    PoolStats,
    /// List registered ambient triggers.
    Triggers,
}

// ---------------------------------------------------------------------------
// Migrate (PRD-06 §9 — DG-43)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct MigrateArgs {
    #[command(subcommand)]
    pub(crate) command: MigrateCommand,
}

#[derive(Subcommand)]
pub(crate) enum MigrateCommand {
    /// Walk the forge and print the count of files at each format
    /// version. Files without a `version:` frontmatter key are
    /// tallied under `1.0` (the implicit default).
    Scan,
    /// Print the migrations registered for this build. Empty until
    /// a forge-format-breaking change ships.
    Registered,
}

// ---------------------------------------------------------------------------
// Tool registry (PRD-15 §4 — DG-32)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct ToolArgs {
    #[command(subcommand)]
    pub(crate) command: ToolCommand,
}

#[derive(Subcommand)]
pub(crate) enum ToolCommand {
    /// List agent tools registered in the process-global catalogue.
    /// Optional `--capability` filters by what the agent holds.
    List {
        /// Restrict to tools the agent could call given these
        /// capabilities. Repeat to add more. Example:
        /// `--capability fs.read --capability search.forge`.
        #[arg(long = "capability", value_name = "ID")]
        capabilities: Vec<String>,
    },
}

// ---------------------------------------------------------------------------
// Comments (C74 #427)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct CommentsArgs {
    #[command(subcommand)]
    pub(crate) command: CommentsCommand,
}

#[derive(Subcommand)]
pub(crate) enum CommentsCommand {
    /// List every comment thread on a note, with full reply history.
    List {
        /// Forge-relative path of the markdown file.
        path: String,
    },
    /// Start a new comment thread anchored to a top-level block.
    #[command(name = "create-thread")]
    CreateThread {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Body text of the first comment in the thread.
        body: String,
        /// 0-based index into the file's top-level blocks (default 0
        /// — the file's first block). A headless CLI session has no
        /// editor selection, so this is the only anchor it can offer.
        #[arg(long)]
        block_index: Option<u32>,
        /// Optional author display name.
        #[arg(long)]
        author: Option<String>,
    },
    /// Append a reply to an existing comment thread.
    #[command(name = "add-reply")]
    AddReply {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread to append to.
        thread_id: String,
        /// Reply body.
        body: String,
        /// Optional author display name.
        #[arg(long)]
        author: Option<String>,
    },
    /// Mark a comment thread resolved.
    Resolve {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread to mark.
        thread_id: String,
        /// Author of the resolution flip (best-effort).
        #[arg(long)]
        author: Option<String>,
    },
    /// Mark a comment thread unresolved.
    Unresolve {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread to mark.
        thread_id: String,
        /// Author of the resolution flip (best-effort).
        #[arg(long)]
        author: Option<String>,
    },
    /// Edit an existing comment's body in place.
    #[command(name = "edit-comment")]
    EditComment {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread containing the comment.
        thread_id: String,
        /// Comment to edit.
        comment_id: String,
        /// New body text.
        body: String,
    },
    /// Delete a single comment from a thread.
    #[command(name = "delete-comment")]
    DeleteComment {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread containing the comment.
        thread_id: String,
        /// Comment to delete.
        comment_id: String,
    },
    /// Delete an entire comment thread, including all its replies.
    #[command(name = "delete-thread")]
    DeleteThread {
        /// Forge-relative path of the markdown file.
        path: String,
        /// Thread to delete.
        thread_id: String,
    },
}

// ---------------------------------------------------------------------------
// Agent (PRD-15)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct AgentArgs {
    #[command(subcommand)]
    pub(crate) command: AgentCommand,
}

// ---------------------------------------------------------------------------
// Notify (BL-133)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct NotifyArgs {
    #[command(subcommand)]
    pub(crate) command: NotifyCommand,
}

#[derive(Subcommand)]
pub(crate) enum NotifyCommand {
    /// Send a notification through `com.nexus.notifications::send`.
    ///
    /// BL-135 — either `--channel` (override path; bypass the
    /// router) or `--source` (router path; consults
    /// `<forge>/.forge/notifications.toml`) must be supplied. When
    /// neither is supplied the CLI defaults `--source cli` so a
    /// bare `nexus notify send "msg"` invocation routes through the
    /// `[sources.cli]` block.
    Send {
        /// Explicit target channel — `desktop` | `discord` | `telegram` | `email`.
        /// Bypasses the BL-135 router. Cannot be used together with `--source`.
        #[arg(long, conflicts_with = "source")]
        channel: Option<String>,
        /// BL-135 source tag — feeds the router to pick channels
        /// from `notifications.toml`.
        #[arg(long)]
        source: Option<String>,
        /// Optional severity (`debug` / `info` / `warn` / `error`).
        /// Defaults to `info` server-side.
        #[arg(long)]
        severity: Option<String>,
        /// Message body.
        message: String,
        /// Optional title — transports that need a header fall back
        /// to `"Nexus"` when omitted.
        #[arg(long)]
        title: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum AgentCommand {
    /// Produce a plan for a goal without executing it
    Plan {
        /// Natural-language goal
        goal: String,
        /// Archetype — writer / coder / researcher / general (default)
        #[arg(long)]
        archetype: Option<String>,
    },
    /// Run a session against a goal end-to-end. Without
    /// `--interactive`, auto-approves every tool call (legacy
    /// behaviour). With `--interactive`, drives the BL-132 approval
    /// flow — subscribes to `com.nexus.agent.round_proposed` events
    /// and prompts y/n on stderr for any round whose tool calls
    /// flag `requires_approval = true`.
    Run {
        /// Natural-language goal
        goal: String,
        /// Archetype — writer / coder / researcher / general (default)
        #[arg(long)]
        archetype: Option<String>,
        /// BL-132 — prompt for approval on rounds whose tool calls
        /// are flagged `requires_approval = true`. Without this
        /// flag, every round auto-approves (pre-BL-132 default).
        #[arg(long)]
        interactive: bool,
        /// BL-133 follow-up — dispatch a desktop notification when
        /// the session takes longer than this. `0` disables the
        /// auto-notify path. Default 30s.
        #[arg(long, default_value_t = 30)]
        notify_after_secs: u64,
    },
    /// List custom agents defined in `<forge>/.forge/agents/*/agent.toml`
    /// (PRD-15 §9 — DG-36).
    ListCustom,
    /// List stored agent sessions (id, outcome, goal; fork linkage).
    /// RFC 0008 (Phase 5.4).
    Sessions,
    /// Show a stored session's full transcript by id. A forked session
    /// (resume / branch / rewind) is shown with its inherited rounds
    /// assembled in. RFC 0008.
    Show {
        /// Session id (as listed by `agent sessions`).
        session_id: String,
    },
    /// Resume a finished session with a follow-up message — forks a child
    /// at the parent's tip, seeded with its full transcript. RFC 0008.
    Resume {
        /// Session id to resume.
        session_id: String,
        /// Follow-up message that drives the continued run.
        message: String,
        /// Dispatch a desktop notification when the run takes longer than
        /// this many seconds. `0` disables. Default 30s.
        #[arg(long, default_value_t = 30)]
        notify_after_secs: u64,
    },
    /// Branch a parallel line from an earlier round of a session. RFC 0008.
    Branch {
        /// Session id to branch from.
        session_id: String,
        /// Round to fork from (1-based, inclusive); new rounds continue after.
        at_round: u32,
        /// Message that drives the new branch.
        message: String,
        #[arg(long, default_value_t = 30)]
        notify_after_secs: u64,
    },
    /// Non-destructively re-run a session from an earlier round — the
    /// original line is preserved. RFC 0008.
    Rewind {
        /// Session id to rewind.
        session_id: String,
        /// Round to fork from (1-based, inclusive).
        at_round: u32,
        /// Optional new message; omit to simply redo from that round.
        message: Option<String>,
        #[arg(long, default_value_t = 30)]
        notify_after_secs: u64,
    },
    /// Name a `(session, round)` location as a checkpoint — a stable handle
    /// for later navigation. RFC 0008.
    Checkpoint {
        /// Session id to bookmark.
        session_id: String,
        /// Round within the session (1-based, inclusive).
        round: u32,
        /// Unique checkpoint name.
        name: String,
    },
    /// List named checkpoints. RFC 0008.
    Checkpoints,
    /// Remove a named checkpoint. RFC 0008.
    CheckpointRm {
        /// Checkpoint name to remove.
        name: String,
    },
}

// ---------------------------------------------------------------------------
// Proc (PRD-09 §14.1)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct ProcArgs {
    #[command(subcommand)]
    pub(crate) command: ProcCommand,
}

#[derive(Subcommand)]
pub(crate) enum ProcCommand {
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
    /// Show recent ad-hoc command history (BL-060)
    History {
        /// Maximum number of rows
        #[arg(long, default_value_t = 100)]
        limit: u32,
        /// Emit raw JSON instead of the table view
        #[arg(long)]
        json: bool,
    },
}

// ---------------------------------------------------------------------------
// Skill (PRD-13)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct SkillArgs {
    #[command(subcommand)]
    pub(crate) command: SkillCommand,
}

#[derive(Subcommand)]
pub(crate) enum SkillCommand {
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
pub(crate) struct McpArgs {
    #[command(subcommand)]
    pub(crate) command: McpCommand,
}

#[derive(Subcommand)]
pub(crate) enum McpCommand {
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
// ACP (BL-145 / Hermes Feature 7)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct AcpArgs {
    #[command(subcommand)]
    pub(crate) command: AcpCommand,
}

#[derive(Subcommand)]
pub(crate) enum AcpCommand {
    /// Start the inbound ACP JSON-RPC 2.0 server on stdio. Exposes a
    /// fixed allow-list of `com.nexus.agent` IPC verbs (`agent/run`,
    /// `agent/list`, `agent/get`) to a Hermes-compatible parent
    /// process. Pure proxy — every method dispatches through the
    /// kernel's `ipc_call` boundary.
    Serve,
}

// ---------------------------------------------------------------------------
// Collab (BL-143)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct CollabArgs {
    #[command(subcommand)]
    pub(crate) command: CollabCommand,
}

#[derive(Subcommand)]
pub(crate) enum CollabCommand {
    /// Run a WebSocket relay that ferries CRDT-op + presence envelopes
    /// between connected peers.
    Serve {
        /// TCP port to bind on. Defaults to 7700.
        #[arg(long, default_value_t = crate::commands::collab::DEFAULT_SERVE_PORT)]
        port: u16,
        /// Interface to bind on. Defaults to `0.0.0.0` (every IPv4
        /// interface). Use `127.0.0.1` to restrict to loopback or a
        /// specific IP to constrain access to a chosen interface.
        #[arg(long, default_value = crate::commands::collab::DEFAULT_BIND_ADDRESS)]
        bind: String,
        /// Shared secret. Without this flag, falls back to the
        /// keyring entry written by `nexus collab token set`.
        #[arg(long)]
        token: Option<String>,
        /// Save the supplied `--token` into the keyring so future
        /// invocations don't need it.
        #[arg(long)]
        save_token: bool,
    },
    /// Connect to a relay and bridge the local kernel event bus.
    Join {
        /// `ws://host:port[?token=…]` URL announced by `serve`.
        url: String,
        /// Override the token in the URL (or the keyring fallback).
        #[arg(long)]
        token: Option<String>,
        /// Peer id to register on the relay. Defaults to `$USER`.
        #[arg(long)]
        peer_id: Option<String>,
        /// Human-readable name shown in the peers panel. Defaults to
        /// the title-cased peer id.
        #[arg(long)]
        display_name: Option<String>,
        /// Save the resolved token into the keyring so future
        /// invocations don't need `--token` or `?token=`.
        #[arg(long)]
        save_token: bool,
    },
    /// Manage the keyring entry that stores the relay's shared secret.
    Token {
        #[command(subcommand)]
        command: CollabTokenCommand,
    },
}

#[derive(Subcommand)]
pub(crate) enum CollabTokenCommand {
    /// Store `<value>` in the keyring under `nexus.collab.token`.
    Set {
        /// The shared secret to remember.
        value: String,
    },
    /// Delete the stored token. A missing entry is not an error.
    Clear,
}

// ---------------------------------------------------------------------------
// Remote-forge server (BL-140)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct ServeArgs {
    /// Read JSON-RPC frames from stdin and write responses to stdout.
    /// Required in Phase 1; future phases add `--port` for WebSocket /
    /// `--unix-socket` for a local UDS transport.
    #[arg(long)]
    pub(crate) stdio: bool,
}

// ---------------------------------------------------------------------------
// Workflow (PRD-16)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct WorkflowArgs {
    #[command(subcommand)]
    pub(crate) command: WorkflowCommand,
}

#[derive(Subcommand)]
pub(crate) enum WorkflowCommand {
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
pub(crate) struct WorkflowTemplateArgs {
    #[command(subcommand)]
    pub(crate) command: WorkflowTemplateCommand,
}

#[derive(Subcommand)]
pub(crate) enum WorkflowTemplateCommand {
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
pub(crate) struct PluginArgs {
    #[command(subcommand)]
    pub(crate) command: PluginCommand,
}

#[derive(Subcommand)]
pub(crate) enum PluginCommand {
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
    /// C80 — run a long-lived plugin-development session: loads every
    /// plugin under `dir` (same one-subdirectory-per-plugin layout as
    /// `.forge/plugins/`), then watches for `.wasm` changes and
    /// hot-reloads the affected plugin until Ctrl+C.
    Dev {
        /// Directory to load plugins from and watch (e.g. a scratch dir
        /// containing the one plugin you're iterating on, or
        /// `.forge/plugins/` to live-reload everything installed there).
        dir: PathBuf,
    },
    /// View or update plugin settings
    Settings {
        /// Plugin identifier
        plugin_id: String,
        /// New settings as JSON (omit to show current settings)
        #[arg(long)]
        set: Option<String>,
    },
    /// Grant a HIGH-risk capability to a plugin (C82 #435), pairing this
    /// verb with `revoke`. Persists install-time consent to
    /// `<plugin>/granted_caps.json`, pinned to the plugin's currently
    /// loaded version; takes effect on the plugin's next load or
    /// hot-reload. Capability strings use the dotted kernel form (e.g.
    /// `fs.write.external`, `process.spawn`, `net.http`). Prompts for
    /// interactive confirmation unless `--yes` is passed — HIGH-risk
    /// grants are exactly the kind of unattended-execution risk the
    /// shell's capability-consent UI exists to surface.
    Grant {
        /// Plugin identifier (e.g. `com.nexus.agent`).
        plugin_id: String,
        /// Capability to grant (dotted form, e.g. `process.spawn`).
        capability: String,
        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Revoke a previously-granted HIGH-risk capability for a plugin
    /// (BL-096). Live-mutates the running plugin's wired context, then
    /// persists to `<plugin>/granted_caps.json`. Capability strings use
    /// the dotted kernel form (e.g. `fs.write.external`, `process.spawn`,
    /// `net.http`).
    Revoke {
        /// Plugin identifier (e.g. `com.nexus.agent`).
        plugin_id: String,
        /// Capability to revoke (dotted form, e.g. `process.spawn`).
        capability: String,
    },
    /// Verify a plugin's manifest signature (BL-099) without
    /// loading the plugin. Reads the trusted-key ring from
    /// `~/.nexus/keys/` (or `--keys-dir` if supplied).
    Verify {
        /// Path to the plugin directory (containing `manifest.toml`).
        path: PathBuf,
        /// Optional override for the trusted-key directory; defaults
        /// to `~/.nexus/keys`.
        #[arg(long)]
        keys_dir: Option<PathBuf>,
    },
}

// ---------------------------------------------------------------------------
// Watch
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct WatchArgs {
    /// Glob pattern to watch (default: "**/*")
    #[arg(default_value = "**/*")]
    pub(crate) glob: String,
}

// ---------------------------------------------------------------------------
// Events (C85 / #438)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct EventsArgs {
    #[command(subcommand)]
    pub(crate) command: EventsCommand,
}

#[derive(Subcommand)]
pub(crate) enum EventsCommand {
    /// Tail the kernel event bus and print every matching event
    Tail {
        /// Event filter: "" or "*" for everything, a bare kernel variant
        /// name (e.g. "PluginLoaded"), a "prefix.*" custom-event prefix
        /// (e.g. "com.nexus.storage.*"), or an exact custom `type_id`.
        #[arg(long, default_value = "*")]
        filter: String,
    },
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct LogsArgs {
    #[command(subcommand)]
    pub(crate) command: LogsCommand,
}

#[derive(Subcommand)]
pub(crate) enum LogsCommand {
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
    /// Query the persisted audit log (newest first)
    List {
        /// Restrict to one plugin id
        #[arg(long)]
        plugin: Option<String>,
        /// Restrict to one event type (e.g. capability_denied)
        #[arg(long = "type")]
        event_type: Option<String>,
        /// Only entries on or after this ISO date / datetime (UTC, e.g. 2026-05-01)
        #[arg(long)]
        since: Option<String>,
        /// Max rows to return
        #[arg(short = 'n', long, default_value_t = 100)]
        limit: u32,
    },
    /// Export audit entries to stdout
    Export {
        /// ISO start date (UTC). Inclusive.
        #[arg(long)]
        start: Option<String>,
        /// ISO end date (UTC). Exclusive.
        #[arg(long)]
        end: Option<String>,
        /// Output format
        #[arg(long, default_value = "jsonl")]
        format: String,
    },
    /// Delete audit entries older than N days
    Clear {
        /// Delete entries older than this many days (defaults to 90)
        #[arg(long = "older-than", default_value_t = 90)]
        older_than: u32,
    },
}

// ---------------------------------------------------------------------------
// Canvas
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct CanvasArgs {
    #[command(subcommand)]
    pub(crate) command: CanvasCommand,
}

#[derive(Subcommand)]
pub(crate) enum CanvasCommand {
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
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Subcommand)]
pub(crate) enum ConfigCommand {
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
pub(crate) struct BasesArgs {
    #[command(subcommand)]
    pub(crate) command: BasesCommand,
}

#[derive(Subcommand)]
pub(crate) enum BasesCommand {
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
pub(crate) struct ImportArgs {
    #[command(subcommand)]
    pub(crate) command: ImportCommand,
}

#[derive(Subcommand)]
pub(crate) enum ImportCommand {
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

#[derive(Parser)]
pub(crate) struct TemplateArgs {
    #[command(subcommand)]
    pub(crate) command: TemplateCommand,
}

#[derive(Subcommand)]
pub(crate) enum TemplateCommand {
    /// List every template available in the active forge.
    List,
    /// Render a template and write the result to the forge.
    Apply {
        /// Template name (from `template list`)
        name: String,
        /// Set a parameter: `--arg key=value` (repeatable)
        #[arg(long = "arg", action = ArgAction::Append)]
        args: Vec<String>,
        /// Override the template's `target_path`. Forge-relative.
        #[arg(long = "target")]
        target: Option<PathBuf>,
        /// Overwrite the destination if it exists.
        #[arg(long = "overwrite")]
        overwrite: bool,
        /// Print what would be written without touching disk.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
}

#[derive(Parser)]
pub(crate) struct ExportArgs {
    #[command(subcommand)]
    pub(crate) command: ExportCommand,
}

#[derive(Subcommand)]
pub(crate) enum ExportCommand {
    /// Export to a Notion-compatible folder tree (re-importable into Notion)
    Notion {
        /// Forge-relative subdirectory to export (default: forge root)
        #[arg(long = "source")]
        source: Option<PathBuf>,
        /// Output directory (created if missing)
        #[arg(long = "dest")]
        dest: PathBuf,
    },
}

// ---------------------------------------------------------------------------
// Crdt (BL-074)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct CrdtArgs {
    #[command(subcommand)]
    pub(crate) command: CrdtCommand,
}

#[derive(Subcommand)]
pub(crate) enum CrdtCommand {
    /// Three-way git merge driver for `.forge/.editor/crdt/<sha>.json`
    /// state files. Reads `--ours` and `--theirs` envelopes, takes
    /// the idempotent union of their op logs, writes the merged
    /// envelope back to `--ours` (the destination git expects).
    /// `--base` is read for diagnostics only — the union is
    /// independent of the merge base because every op carries its
    /// own causality witness.
    ///
    /// To register: run `nexus crdt install-merge-driver` or set up
    /// manually with `git config merge.nexus-crdt.driver "nexus crdt
    /// merge-driver --base %O --ours %A --theirs %B"` and add
    /// `.forge/.editor/crdt/* merge=nexus-crdt` to `.gitattributes`.
    MergeDriver {
        /// Path to the merge base file (informational; may not exist
        /// if the file was added on both branches).
        #[arg(long = "base")]
        base: PathBuf,
        /// Path to "our" side of the merge — also the destination
        /// the merged envelope is written to.
        #[arg(long = "ours")]
        ours: PathBuf,
        /// Path to "their" side of the merge.
        #[arg(long = "theirs")]
        theirs: PathBuf,
    },
    /// Print the recommended `.gitattributes` line and the
    /// `git config` invocation for registering the merge driver in
    /// the current repository, plus a one-line setup script. Use
    /// `--apply` to actually run the configuration changes.
    InstallMergeDriver {
        /// Apply the changes (write `.gitattributes` if missing the
        /// rule, run `git config`). Without this flag, only prints
        /// the commands.
        #[arg(long = "apply")]
        apply: bool,
    },
    /// One-shot enabler for the BL-007 git-CRDT transport on an
    /// existing forge. Writes a default `.forge/.gitignore` if
    /// missing (so the rebuildable indexes / per-machine SQLite
    /// stores stay out of git, while `.forge/.editor/crdt/*.json`
    /// rides through), then registers the merge driver via
    /// `install-merge-driver --apply`. Both steps are idempotent.
    EnableTransport,
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct GitArgs {
    #[command(subcommand)]
    pub(crate) command: GitCommand,
}

#[derive(Subcommand)]
pub(crate) enum GitCommand {
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
    /// Stage specific hunks within a file
    #[command(name = "stage-hunk")]
    StageHunk {
        /// Forge-relative path of the file
        path: String,
        /// 0-based hunk index to stage (repeat for multiple hunks)
        #[arg(required = true, num_args = 1..)]
        hunks: Vec<usize>,
    },
    /// Unstage specific hunks within a file
    #[command(name = "unstage-hunk")]
    UnstageHunk {
        /// Forge-relative path of the file
        path: String,
        /// 0-based hunk index to unstage (repeat for multiple hunks)
        #[arg(required = true, num_args = 1..)]
        hunks: Vec<usize>,
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
    /// List, create, delete, or push tags
    Tag {
        /// Tag name to create (omit to list all tags)
        name: Option<String>,
        /// Create an annotated tag with this message (requires <name>)
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// Delete a local tag by name
        #[arg(short = 'd', long, value_name = "NAME")]
        delete: Option<String>,
        /// Push all local tags to this remote
        #[arg(long, value_name = "REMOTE")]
        push: Option<String>,
    },
    /// Stash uncommitted changes
    Stash {
        #[command(subcommand)]
        command: Option<StashCommand>,
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
        /// Enable background auto-commit for this forge (writes to .forge/app.toml)
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        /// Disable background auto-commit for this forge (writes to .forge/app.toml)
        #[arg(long, conflicts_with = "enable")]
        disable: bool,
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
    /// Cache an SSH key passphrase in the OS keyring (BL-090)
    #[command(name = "set-passphrase")]
    SetPassphrase {
        /// Key file basename (e.g. `id_ed25519`, `id_rsa`); read from `~/.ssh/<key>`
        key: String,
    },
    /// Remove a cached SSH passphrase from the OS keyring
    #[command(name = "clear-passphrase")]
    ClearPassphrase {
        /// Key file basename whose passphrase should be removed
        key: String,
    },
    /// Show Git-LFS state — tracked patterns, pointer-only files,
    /// and locally-materialised files (BL-091).
    #[command(name = "lfs-status")]
    LfsStatus,
    /// Rebase the current branch onto another branch (BL-088).
    /// Conflicts pause the rebase; resolve and commit manually
    /// or invoke `nexus git rebase --abort`.
    Rebase {
        /// Branch to rebase onto (e.g. `main`).
        onto: Option<String>,
        /// Abort an in-progress rebase, restoring pre-rebase state.
        #[arg(long, conflicts_with = "onto")]
        abort: bool,
    },
    /// Cherry-pick a single commit onto HEAD (BL-088). Conflicts
    /// pause the operation; resolve manually or
    /// `nexus git cherry-pick --abort`.
    #[command(name = "cherry-pick")]
    CherryPick {
        /// Commit hash to apply (full or short form accepted).
        commit: Option<String>,
        /// Abort an in-progress cherry-pick.
        #[arg(long, conflicts_with = "commit")]
        abort: bool,
    },
    /// Abort an in-progress merge, restoring pre-merge HEAD (BL-084).
    #[command(name = "abort-merge")]
    AbortMerge,
}

/// Branch subcommands.
#[derive(Subcommand)]
pub(crate) enum BranchCommand {
    /// Create a new branch from HEAD
    Create {
        /// Branch name
        name: String,
    },
    /// Switch to a branch
    Switch {
        /// Branch name
        name: String,
        /// Stash uncommitted changes before switching (no auto-pop)
        #[arg(long)]
        stash: bool,
    },
    /// Delete a branch
    Delete {
        /// Branch name
        name: String,
    },
}

/// Stash subcommands.
#[derive(Subcommand)]
pub(crate) enum StashCommand {
    /// List all stash entries
    List,
    /// Apply the most-recent (or specified) stash and remove it from the stack
    Pop {
        /// 0-based stash index (default: 0 = most recent)
        #[arg(default_value_t = 0)]
        index: usize,
    },
    /// Discard a stash entry without applying it
    Drop {
        /// 0-based stash index (default: 0 = most recent)
        #[arg(default_value_t = 0)]
        index: usize,
    },
}

// ---------------------------------------------------------------------------
// Stub — used for not-yet-implemented command groups
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct StubArgs {
    /// Subcommand and arguments (not yet implemented)
    #[arg(trailing_var_arg = true)]
    pub(crate) args: Vec<String>,
}

// ---------------------------------------------------------------------------
// Term (PRD-09 §3.7)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct TermArgs {
    #[command(subcommand)]
    pub(crate) command: TermCommand,
}

// ---------------------------------------------------------------------------
// Sandbox (Phase 4 — OS process sandbox)
// ---------------------------------------------------------------------------

#[derive(Parser)]
pub(crate) struct SandboxArgs {
    #[command(subcommand)]
    pub(crate) command: SandboxCommand,
}

#[derive(Subcommand)]
pub(crate) enum SandboxCommand {
    /// Print the active OS-sandbox config (`.forge/sandbox.toml`): process
    /// confinement mode, writable roots, network access, download allowlist.
    Policy,
    /// Perform a brokered, allowlisted download into a sandbox writable root.
    Download {
        /// Source URL (https + on the sandbox allowlist).
        url: String,
        /// Destination path (inside a sandbox writable root).
        dest: String,
        /// Working dir for resolving writable roots (default: dest's parent).
        #[arg(long)]
        cwd: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum TermCommand {
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
