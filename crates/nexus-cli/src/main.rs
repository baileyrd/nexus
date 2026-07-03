mod app;
// R8 / #191 — clap argument structs + subcommand enums now live in
// `args.rs` so this file stays focused on the Cli + Commands + dispatch.
// The `use args::*;` re-export keeps the existing destructure sites
// (`Commands::Forge(args) => match args.command { ... }`) resolving the
// bare identifiers they used pre-split.
mod args;
mod commands;
mod output;

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

use args::*;

// ---------------------------------------------------------------------------
// Top-level CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "nexus",
    about = "Nexus IDE — headless CLI",
    version,
    allow_external_subcommands = true
)]
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
    /// Manage the forge trash (C3 / #356)
    Trash(TrashArgs),
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
    /// Agent tool registry (PRD-15 §4) — list catalogued tools
    Tool(ToolArgs),
    /// Comment thread operations (C74 #427) — `com.nexus.comments`
    /// list/create/reply/resolve/edit/delete, headless parity with the
    /// shell's comment pane.
    Comments(CommentsArgs),
    /// Forge-format versioning + migrations (PRD-06 §9, DG-43)
    Migrate(MigrateArgs),
    /// Skill operations (PRD-13): list and inspect `.skill.md` files
    Skill(SkillArgs),
    /// Workflow operations (PRD-16): list/show/validate `.workflow.toml` files
    Workflow(WorkflowArgs),
    /// BL-133 — send a notification through `com.nexus.notifications::send`.
    Notify(NotifyArgs),
    /// Process / saved-command management (PRD-09 §14.1)
    Proc(ProcArgs),
    /// Terminal / PTY session operations (PRD-09)
    Term(TermArgs),
    /// OS process sandbox: inspect the policy, run brokered downloads
    Sandbox(SandboxArgs),
    /// MCP (Model Context Protocol): run server or operate as host
    Mcp(McpArgs),
    /// ACP (Agent Communication Protocol — BL-145 / Hermes Feature 7):
    /// expose Nexus's agent IPC surface to external clients over a
    /// stdio JSON-RPC 2.0 server.
    Acp(AcpArgs),
    /// Live collaboration (BL-143) — relay server, peer client, and
    /// token management for the in-process WebSocket bridge that
    /// ferries CRDT ops + presence between peers.
    Collab(CollabArgs),
    /// Remote-forge server (BL-140 Phase 1) — expose the whole kernel
    /// IPC + event-bus surface over a stdio JSON-RPC 2.0 stream so a
    /// local frontend can drive this headless instance. Phase 2 (SSH
    /// transport + `ssh://` forge URIs) lands separately.
    Serve(ServeArgs),
    /// C76 — headless, long-running host for workflow triggers (cron,
    /// file/git/mcp events, digests). Blocks until Ctrl+C or SIGTERM
    /// (systemd-friendly), gracefully unloading every plugin on stop.
    /// No UI, no stdio protocol — just the trigger engines that
    /// otherwise only run inside the desktop shell.
    Daemon,
    /// Sync the forge against a git remote: fetch, pull (rebase),
    /// then push. A thin convenience wrapper over `nexus git
    /// fetch|pull|push`.
    Sync {
        /// Remote name. Defaults to `origin`.
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Branch override (defaults to the current branch).
        #[arg(long)]
        branch: Option<String>,
        /// Skip the push step (sync down only).
        #[arg(long)]
        no_push: bool,
    },
    /// Git operations (read-only)
    Git(GitArgs),
    /// Run a workflow by name — thin alias for `nexus workflow run <name>`.
    Run {
        /// Workflow name to execute (see `nexus workflow list`).
        name: String,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Import external knowledge-tool exports (Notion, …)
    Import(ImportArgs),

    /// Export forge content to external formats (Notion, …)
    Export(ExportArgs),

    /// Page template operations (list, apply)
    Template(TemplateArgs),

    /// CRDT-layer utilities (BL-074): git merge driver, state inspection.
    Crdt(CrdtArgs),

    /// Plugin-registered subcommand (`nexus <plugin-id> [args…]`)
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

// ---------------------------------------------------------------------------
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

    // BL-140 Phase 2b — detect ssh:// forge URIs (via --forge-path or
    // NEXUS_FORGE_PATH) and route through App::new_remote. Anything
    // that contains "://" is parsed as a URI; everything else falls
    // through to local-path handling.
    let resolved_forge_str = cli
        .forge_path
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| std::env::var("NEXUS_FORGE_PATH").ok());
    let mut app = match resolved_forge_str {
        Some(s) if s.contains("://") => match nexus_remote::ForgeUri::parse(&s) {
            Ok(uri) => app::App::new_remote(uri, format),
            Err(e) => {
                eprintln!("Error: invalid forge URI '{s}': {e}");
                std::process::exit(2);
            }
        },
        _ => app::App::new(default_forge_path(cli.forge_path), format),
    };
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
            ForgeCommand::Init { dir, template } => {
                commands::forge::init(&app, dir, template.as_deref())
            }
            ForgeCommand::Status => commands::forge::status(&mut app),
            ForgeCommand::Reindex => commands::forge::reindex(&mut app),
            ForgeCommand::Doctor { fix } => commands::forge::doctor(&mut app, fix),
            ForgeCommand::Import {
                source,
                dry_run,
                on_conflict,
            } => commands::forge::import(&mut app, &source, dry_run, &on_conflict),
        },

        Commands::Content(args) => match args.command {
            ContentCommand::Create {
                path,
                content,
                stdin,
            } => commands::content::create(&mut app, &path, content.as_deref(), stdin),
            ContentCommand::Update {
                path,
                content,
                stdin,
            } => commands::content::update(&mut app, &path, content.as_deref(), stdin),
            ContentCommand::List { prefix } => commands::content::list(&mut app, prefix.as_deref()),
            ContentCommand::Read { path, raw } => commands::content::read(&mut app, &path, raw),
            ContentCommand::Delete {
                path,
                force,
                permanent,
            } => commands::content::delete(&mut app, &path, force, permanent),
            ContentCommand::Search {
                query,
                limit,
                semantic,
                hybrid,
            } => {
                if semantic || hybrid {
                    commands::content::semantic_search(&mut app, &query, limit, hybrid)
                } else {
                    commands::content::search(&mut app, &query, limit)
                }
            }
            ContentCommand::Tasks {
                completed,
                all,
                file,
            } => commands::content::tasks(&mut app, completed, all, file.as_deref()),
            ContentCommand::TaskToggle { id } => commands::content::task_toggle(&mut app, id),
            ContentCommand::Links { path } => commands::content::links(&mut app, &path),
            ContentCommand::Backlinks { path } => commands::content::backlinks(&mut app, &path),
            ContentCommand::Daily { date } => commands::content::daily(&mut app, date.as_deref()),
            ContentCommand::Export { path, output } => {
                commands::content::export(&mut app, &path, output.as_deref())
            }
        },

        Commands::Trash(args) => match args.command {
            TrashCommand::List => commands::trash::list(&mut app),
            TrashCommand::Restore { trash_id } => commands::trash::restore(&mut app, &trash_id),
            TrashCommand::Empty { older_than, force } => {
                commands::trash::empty(&mut app, older_than, force)
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
            PluginCommand::Remove { id, yes } => commands::plugin::remove_shell_plugin(&id, yes),
            PluginCommand::Call {
                plugin_id,
                command,
                args,
            } => {
                let args_json = args.as_deref().unwrap_or("{}");
                commands::plugin::call(&mut app, &plugin_id, &command, args_json)
            }
            PluginCommand::Uninstall { plugin_id } => {
                commands::plugin::uninstall(&mut app, &plugin_id)
            }
            PluginCommand::Scaffold {
                plugin_type,
                id,
                name,
                author,
                output,
            } => commands::plugin::scaffold(
                &plugin_type,
                Some(&id),
                Some(&name),
                Some(&author),
                output.as_deref(),
            ),
            PluginCommand::Enable { plugin_id } => commands::plugin::enable(&mut app, &plugin_id),
            PluginCommand::Disable { plugin_id } => commands::plugin::disable(&mut app, &plugin_id),
            PluginCommand::Reset { plugin_id } => {
                commands::plugin::reset_crash(&mut app, &plugin_id)
            }
            PluginCommand::Dev { dir } => commands::plugin::dev(&dir),
            PluginCommand::Settings { plugin_id, set } => {
                commands::plugin::settings(&mut app, &plugin_id, set.as_deref())
            }
            PluginCommand::Revoke {
                plugin_id,
                capability,
            } => commands::plugin::revoke(&mut app, &plugin_id, &capability),
            PluginCommand::Verify { path, keys_dir } => {
                commands::plugin::verify(&path, keys_dir.as_deref())
            }
        },

        Commands::Watch(args) => commands::watch::run(&mut app, &args.glob),

        Commands::Logs(args) => match args.command {
            LogsCommand::Tail { level, lines } => commands::logs::tail(&app, Some(&level), lines),
            LogsCommand::Show { date } => commands::logs::show(&app, &date),
            LogsCommand::Path => commands::logs::path(&app),
            LogsCommand::List {
                plugin,
                event_type,
                since,
                limit,
            } => commands::logs::audit_list(&mut app, plugin, event_type, since, limit),
            LogsCommand::Export { start, end, format } => {
                commands::logs::audit_export(&mut app, start, end, &format)
            }
            LogsCommand::Clear { older_than } => commands::logs::audit_clear(&mut app, older_than),
        },

        Commands::Graph(args) => match args.command {
            GraphCommand::Status => commands::graph::status(&mut app),
            GraphCommand::Unresolved => commands::graph::unresolved(&mut app),
            GraphCommand::Neighbors { path, depth } => {
                commands::graph::neighbors(&mut app, &path, depth)
            }
            GraphCommand::Entity { command } => match command {
                EntityCommand::List { r#type, limit } => {
                    commands::graph::entity_list(&mut app, r#type.as_deref(), limit)
                }
                EntityCommand::Show { id } => commands::graph::entity_show(&mut app, &id),
                EntityCommand::Search {
                    query,
                    r#type,
                    limit,
                } => commands::graph::entity_search(&mut app, &query, r#type.as_deref(), limit),
                EntityCommand::Related { id, direction } => {
                    commands::graph::entity_related(&mut app, &id, &direction)
                }
                EntityCommand::Duplicates { threshold } => {
                    commands::graph::entity_duplicates(&mut app, threshold)
                }
            },
            GraphCommand::DreamCycle { command } => match command {
                DreamCycleCommand::Run {
                    phase,
                    decay_factor,
                    decay_floor,
                    review_threshold,
                    merge_threshold,
                    dry_run,
                } => commands::graph::dream_cycle_run(
                    &mut app,
                    phase.as_deref(),
                    decay_factor,
                    decay_floor,
                    review_threshold,
                    merge_threshold,
                    dry_run,
                ),
            },
        },

        Commands::Tags(args) => match args.command {
            TagsCommand::List { name } => commands::tags::list(&mut app, name.as_deref()),
        },

        Commands::Canvas(args) => match args.command {
            CanvasCommand::Create { path } => commands::canvas::create(&mut app, &path),
            CanvasCommand::Show { path } => commands::canvas::show(&mut app, &path),
            CanvasCommand::AddNode {
                path,
                node_type,
                x,
                y,
                width,
                height,
                content,
                label,
            } => commands::canvas::add_node(
                &mut app,
                &path,
                &node_type,
                x,
                y,
                width,
                height,
                content.as_deref(),
                label.as_deref(),
            ),
            CanvasCommand::AddEdge {
                path,
                from,
                to,
                edge_type,
                label,
            } => commands::canvas::add_edge(
                &mut app,
                &path,
                &from,
                &to,
                &edge_type,
                label.as_deref(),
            ),
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
            BasesCommand::Import { path, file, header } => {
                commands::bases::import(&mut app, &path, &file, header)
            }
            BasesCommand::Export { path, file } => commands::bases::export(&mut app, &path, &file),
            BasesCommand::Formula { path, record, expr } => {
                commands::bases::formula(&mut app, &path, &record, &expr)
            }
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
            AgentCommand::Run {
                goal,
                archetype,
                interactive,
                notify_after_secs,
            } => commands::agent::run(
                &mut app,
                &goal,
                archetype.as_deref(),
                interactive,
                notify_after_secs,
            ),
            AgentCommand::ListCustom => commands::agent::list_custom(&mut app),
            AgentCommand::Sessions => commands::agent::sessions(&mut app),
            AgentCommand::Show { session_id } => commands::agent::show(&mut app, &session_id),
            AgentCommand::Resume {
                session_id,
                message,
                notify_after_secs,
            } => commands::agent::resume(&mut app, &session_id, &message, notify_after_secs),
            AgentCommand::Branch {
                session_id,
                at_round,
                message,
                notify_after_secs,
            } => commands::agent::branch(
                &mut app,
                &session_id,
                at_round,
                &message,
                notify_after_secs,
            ),
            AgentCommand::Rewind {
                session_id,
                at_round,
                message,
                notify_after_secs,
            } => commands::agent::rewind(
                &mut app,
                &session_id,
                at_round,
                message.as_deref(),
                notify_after_secs,
            ),
            AgentCommand::Checkpoint {
                session_id,
                round,
                name,
            } => commands::agent::checkpoint(&mut app, &session_id, round, &name),
            AgentCommand::Checkpoints => commands::agent::checkpoints(&mut app),
            AgentCommand::CheckpointRm { name } => commands::agent::checkpoint_rm(&mut app, &name),
        },

        Commands::Tool(args) => match args.command {
            ToolCommand::List { capabilities } => commands::tool::list(&mut app, &capabilities),
        },

        Commands::Comments(args) => match args.command {
            CommentsCommand::List { path } => commands::comments::list(&mut app, &path),
            CommentsCommand::CreateThread {
                path,
                body,
                block_index,
                author,
            } => commands::comments::create_thread(
                &mut app,
                &path,
                &body,
                block_index,
                author.as_deref(),
            ),
            CommentsCommand::AddReply {
                path,
                thread_id,
                body,
                author,
            } => commands::comments::add_reply(
                &mut app,
                &path,
                &thread_id,
                &body,
                author.as_deref(),
            ),
            CommentsCommand::Resolve {
                path,
                thread_id,
                author,
            } => commands::comments::resolve(&mut app, &path, &thread_id, author.as_deref()),
            CommentsCommand::Unresolve {
                path,
                thread_id,
                author,
            } => commands::comments::unresolve(&mut app, &path, &thread_id, author.as_deref()),
            CommentsCommand::EditComment {
                path,
                thread_id,
                comment_id,
                body,
            } => commands::comments::edit_comment(&mut app, &path, &thread_id, &comment_id, &body),
            CommentsCommand::DeleteComment {
                path,
                thread_id,
                comment_id,
            } => commands::comments::delete_comment(&mut app, &path, &thread_id, &comment_id),
            CommentsCommand::DeleteThread { path, thread_id } => {
                commands::comments::delete_thread(&mut app, &path, &thread_id)
            }
        },

        Commands::Migrate(args) => match args.command {
            MigrateCommand::Scan => commands::migrate::scan(&mut app),
            MigrateCommand::Registered => commands::migrate::registered(),
        },

        Commands::Skill(args) => match args.command {
            SkillCommand::List => commands::skill::list(&mut app),
            SkillCommand::Show { id } => commands::skill::show(&mut app, &id),
            SkillCommand::Context { context } => commands::skill::context(&mut app, &context),
            SkillCommand::Triggered { text } => commands::skill::triggered(&mut app, &text),
            SkillCommand::Reload => commands::skill::reload(&mut app),
            SkillCommand::Render { id, params } => commands::skill::render(&mut app, &id, &params),
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
            ProcCommand::Reorder { slug, order } => commands::proc::reorder(&mut app, &slug, order),
            ProcCommand::History { limit, json } => commands::proc::history(&mut app, limit, json),
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
        Commands::Sandbox(args) => match args.command {
            SandboxCommand::Policy => commands::sandbox::policy(&mut app),
            SandboxCommand::Download { url, dest, cwd } => {
                commands::sandbox::download(&mut app, &url, &dest, cwd.as_deref())
            }
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
        Commands::Acp(args) => match args.command {
            AcpCommand::Serve => commands::acp::serve(&app),
        },
        Commands::Collab(args) => match args.command {
            CollabCommand::Serve {
                port,
                bind,
                token,
                save_token,
            } => commands::collab::serve(port, &bind, token, save_token),
            CollabCommand::Join {
                url,
                token,
                peer_id,
                display_name,
                save_token,
            } => commands::collab::join(&app, &url, token, peer_id, display_name, save_token),
            CollabCommand::Token { command } => match command {
                CollabTokenCommand::Set { value } => commands::collab::token_set(&value),
                CollabTokenCommand::Clear => commands::collab::token_clear(),
            },
        },
        Commands::Serve(args) => {
            if args.stdio {
                commands::serve::serve(&app)
            } else {
                Err(anyhow::anyhow!(
                    "nexus serve requires a transport flag; use --stdio (Phase 1 only supports stdio)"
                ))
            }
        }
        Commands::Daemon => commands::daemon::run(&app),
        Commands::Sync {
            remote,
            branch,
            no_push,
        } => (|| -> anyhow::Result<()> {
            commands::git::fetch(&app, &remote)?;
            commands::git::pull(&app, &remote, branch.as_deref())?;
            if !no_push {
                commands::git::push(&app, &remote, branch.as_deref())?;
            }
            Ok(())
        })(),
        Commands::Git(args) => match args.command {
            GitCommand::Info => commands::git::info(&app),
            GitCommand::Status => commands::git::status(&app),
            GitCommand::Diff { path } => commands::git::diff(&app, path.as_deref()),
            GitCommand::Blame { path } => commands::git::blame(&app, &path),
            GitCommand::Log { limit, file } => commands::git::log(&app, limit, file.as_deref()),
            GitCommand::Stage { path, all } => commands::git::stage(&app, path.as_deref(), all),
            GitCommand::Unstage { path, all } => commands::git::unstage(&app, path.as_deref(), all),
            GitCommand::StageHunk { path, hunks } => commands::git::stage_hunk(&app, &path, &hunks),
            GitCommand::UnstageHunk { path, hunks } => {
                commands::git::unstage_hunk(&app, &path, &hunks)
            }
            GitCommand::Commit { message } => commands::git::commit(&app, &message),
            GitCommand::Branch { command } => commands::git::branch(&app, command),
            GitCommand::Stash { command } => commands::git::stash(&app, command),
            GitCommand::Tag {
                name,
                message,
                delete,
                push,
            } => commands::git::tag(
                &app,
                name.as_deref(),
                message.as_deref(),
                delete.as_deref(),
                push.as_deref(),
            ),
            GitCommand::Fetch { remote } => commands::git::fetch(&app, &remote),
            GitCommand::Push { remote, branch } => {
                commands::git::push(&app, &remote, branch.as_deref())
            }
            GitCommand::Pull { remote, branch } => {
                commands::git::pull(&app, &remote, branch.as_deref())
            }
            GitCommand::Merge { branch, abort } => {
                commands::git::merge(&app, branch.as_deref(), abort)
            }
            GitCommand::Conflicts => commands::git::conflicts(&app),
            GitCommand::Remotes => commands::git::remotes(&app),
            GitCommand::AutoCommit {
                enable,
                disable,
                watch,
                interval,
                debounce,
            } => commands::git::auto_commit(&app, enable, disable, watch, interval, debounce),
            GitCommand::SetPassphrase { key } => commands::git::set_passphrase(&key),
            GitCommand::ClearPassphrase { key } => commands::git::clear_passphrase(&key),
            GitCommand::LfsStatus => commands::git::lfs_status(&mut app),
            GitCommand::Rebase { onto, abort } => {
                commands::git::rebase(&app, onto.as_deref(), abort)
            }
            GitCommand::CherryPick { commit, abort } => {
                commands::git::cherry_pick(&app, commit.as_deref(), abort)
            }
            GitCommand::AbortMerge => commands::git::abort_merge(&app),
        },
        Commands::Run { name } => commands::workflow::run(&mut app, &name),

        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "nexus", &mut std::io::stdout());
            Ok(())
        }

        Commands::Notify(args) => match args.command {
            NotifyCommand::Send {
                channel,
                source,
                severity,
                message,
                title,
            } => commands::notify::send(
                &mut app,
                channel.as_deref(),
                source.as_deref(),
                severity.as_deref(),
                &message,
                title.as_deref(),
            ),
        },

        Commands::Import(args) => match args.command {
            ImportCommand::Notion { source, dest } => {
                commands::import::notion_zip(&app, &source, dest)
            }
        },

        Commands::Export(args) => match args.command {
            ExportCommand::Notion { source, dest } => {
                commands::export::notion_dir(&app, source, &dest)
            }
        },

        Commands::Template(args) => match args.command {
            TemplateCommand::List => commands::template::list(&app),
            TemplateCommand::Apply {
                name,
                args,
                target,
                overwrite,
                dry_run,
            } => commands::template::apply(&app, &name, args, target, overwrite, dry_run),
        },

        Commands::Crdt(args) => match args.command {
            CrdtCommand::MergeDriver { base, ours, theirs } => {
                commands::crdt::merge_driver(&base, &ours, &theirs)
            }
            CrdtCommand::InstallMergeDriver { apply } => {
                commands::crdt::install_merge_driver(apply)
            }
            CrdtCommand::EnableTransport => commands::crdt::enable_transport(&mut app),
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
                ContentCommand::Update {
                    path,
                    content,
                    stdin,
                } => {
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
            "nexus",
            "content",
            "update",
            "notes/a.md",
            "--content",
            "hello",
        ])
        .expect("parse content update --content");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Update {
                    path,
                    content,
                    stdin,
                } => {
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
    fn parse_content_search_defaults_semantic_and_hybrid_to_false() {
        let cli = Cli::try_parse_from(["nexus", "content", "search", "some query"])
            .expect("parse content search");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Search {
                    query,
                    semantic,
                    hybrid,
                    ..
                } => {
                    assert_eq!(query, "some query");
                    assert!(!semantic);
                    assert!(!hybrid);
                }
                _ => panic!("expected Search"),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_content_search_accepts_semantic_and_hybrid_flags() {
        let cli = Cli::try_parse_from(["nexus", "content", "search", "q", "--semantic"])
            .expect("parse content search --semantic");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Search { semantic, .. } => assert!(semantic),
                _ => panic!("expected Search"),
            },
            _ => panic!("expected Content subcommand"),
        }

        let cli = Cli::try_parse_from(["nexus", "content", "search", "q", "--hybrid"])
            .expect("parse content search --hybrid");
        match cli.command {
            Commands::Content(args) => match args.command {
                ContentCommand::Search { hybrid, .. } => assert!(hybrid),
                _ => panic!("expected Search"),
            },
            _ => panic!("expected Content subcommand"),
        }
    }

    #[test]
    fn parse_content_search_rejects_semantic_and_hybrid_together() {
        let result =
            Cli::try_parse_from(["nexus", "content", "search", "q", "--semantic", "--hybrid"]);
        assert!(
            result.is_err(),
            "--semantic and --hybrid should be mutually exclusive"
        );
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
        assert!(
            top.contains("tags"),
            "top-level help missing 'tags':\n{top}"
        );

        // `nexus content --help` must list Update and List.
        let content = cmd
            .find_subcommand_mut("content")
            .expect("content subcommand registered")
            .render_long_help()
            .to_string();
        assert!(
            content.contains("update"),
            "content help missing 'update':\n{content}"
        );
        assert!(
            content.contains("list"),
            "content help missing 'list':\n{content}"
        );

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
