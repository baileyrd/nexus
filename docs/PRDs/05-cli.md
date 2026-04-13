# Nexus CLI Subsystem PRD

**Version:** 1.0  
**Status:** Implementation-Ready  
**Target Release:** April 2026  
**Crate:** `nexus-cli` (binary: `nexus-cli`)  
**Build Tool:** Clap 4.x (command parsing), Tokio (async runtime)

---

## Executive Summary

The Nexus CLI provides complete application parity with the GUI in a headless, scriptable environment. The `nexus-cli` binary instantiates the Nexus kernel and plugin system without GUI overhead, enabling:

- Full forge and content management in terminal-native workflows
- AI-driven code generation and agent execution from the command line
- Bidirectional synchronization with the GUI via shared SQLite database
- Machine-readable output (JSON, JSONL, Table, Text) for scripting and integration
- Watch mode for file-system driven automation
- Plugin-extensible command architecture

**Scope:** All command categories from parent PRD v0.1, output formatting system, plugin CLI registration, interactive/non-interactive modes, watch mode, scripting support, shell completions, and AI commands.

---

## 1. Command Architecture

### 1.1 Binary Invocation and Top-Level Structure

The CLI binary is `nexus`.

```
nexus [GLOBAL_FLAGS] <SUBCOMMAND> [SUBCOMMAND_FLAGS] [ARGS]
```

**Global Flags** (applied before subcommand):
- `--forge-path <PATH>` — Path to forge root directory. Defaults to `$NEXUS_FORGE_PATH` env var or `~/.nexus/default`.
- `--format <FORMAT>` — Output format: `json`, `jsonl`, `text` (default), `table`. See §2.
- `--quiet` — Suppress all non-essential output; errors still printed to stderr.
- `--verbose` / `-v` — Enable debug logging; repeat for more verbosity (`-vv`, `-vvv`).
- `--no-color` — Disable ANSI color output.
- `--config <PATH>` — Path to forge config file; overrides forge-root config.

**Exit Code Conventions:**
- `0` — Success.
- `1` — Generic error.
- `2` — Invalid arguments / usage error.
- `3` — Authentication / permission denied.
- `4` — Resource not found.
- `5` — Conflict / duplicate.
- `127` — Command not found.

### 1.2 Subcommand Groups

Commands are organized by domain:

```
nexus forge <SUBCOMMAND>       # Forge management (init, open, config, status)
nexus content <SUBCOMMAND>     # Content CRUD (create, read, edit, delete, search)
nexus db <SUBCOMMAND>          # Database queries and export
nexus plugin <SUBCOMMAND>      # Plugin installation and management
nexus ai <SUBCOMMAND>          # AI queries and agent execution
nexus proc <SUBCOMMAND>        # Process manager (list, run, stop, new)
nexus term <SUBCOMMAND>        # Terminal creation and execution
nexus mcp <SUBCOMMAND>         # MCP server and connectivity
nexus sync <SUBCOMMAND>        # Synchronization and conflict resolution
nexus git <SUBCOMMAND>         # Git operations (status, commit, log, diff, push, pull)
nexus run <SCRIPT>             # Run automation script
nexus watch <GLOB>             # Watch-mode automation (§5)
```

### 1.3 Argument and Flag Conventions

- **Positional arguments** are required unless marked `[OPTIONAL]`.
- **Named flags** use long form (`--flag`) or short form (`-f`) where applicable.
- **Flag values** are space-separated: `--flag value` or `--flag=value`.
- **Variadic arguments** are marked `<ARG>...` and consume remaining args.
- **String arguments with spaces** must be quoted: `nexus content create "My Document"`.
- **Boolean flags** are toggle-only (`--recursive`, not `--recursive=true`).

---

## 2. Output Formatting System

### 2.1 Formatter Registry and Output Formats

The CLI supports four primary output formats, configurable globally (`--format`) or per-command via flag.

#### 2.1.1 Text Format (default)

Human-readable, plain-text output. Suitable for direct terminal reading.

```
Format: Multi-line prose, key-value pairs, or summary.
Color: ANSI 256-color (reds for errors, greens for success, blues for info).
Pager: Auto-paginated if output > terminal height (via `less` or system pager).
```

**Example:**
```
Forge Status: Initialized
Location: /home/user/.nexus/my-forge
Size: 128 MB (1,247 files, 342 directories)
Last Modified: 2026-04-11 14:22:17 UTC
Database Records: 1,523
Plugins Enabled: 5 (core, ai, git, mcp, sync)
```

#### 2.1.2 JSON Format

Complete structured output for machine consumption and APIs.

```
Format: Valid JSON object per command (or array for list commands).
Structure: { "status": "success", "data": {...}, "error": null, "metadata": {...} }
Streaming: Not applicable for JSON (use JSONL for streaming).
```

**Example:**
```json
{
  "status": "success",
  "data": {
    "forge_id": "abc123",
    "name": "my-forge",
    "location": "/home/user/.nexus/my-forge",
    "size_bytes": 134217728,
    "file_count": 1247,
    "db_records": 1523
  },
  "metadata": {
    "command": "nexus forge status",
    "timestamp": "2026-04-11T14:22:17Z",
    "version": "1.0"
  }
}
```

#### 2.1.3 JSONL Format

Streaming JSON (one JSON object per line). Ideal for large result sets and streaming output.

```
Format: One complete JSON object per line.
Structure: Same as JSON, but printed line-by-line.
Buffering: Lines flushed immediately to enable streaming processing.
```

**Example (search results):**
```jsonl
{"id": "doc1", "title": "Overview", "type": "document", "size": 4096}
{"id": "doc2", "title": "API Reference", "type": "document", "size": 8192}
{"id": "doc3", "title": "Examples", "type": "document", "size": 2048}
```

#### 2.1.4 Table Format

Columnar, human-readable output for lists and tabular data.

```
Format: Fixed-width columns, header row, borders (ASCII or Unicode).
Alignment: Left-aligned text, right-aligned numbers.
Truncation: Long cells truncated with "…" (respects --width or terminal width).
```

**Example:**
```
ID          Title              Type        Size      Modified
doc1        Overview           document    4 KB      2026-04-11 14:20
doc2        API Reference      document    8 KB      2026-04-11 14:19
doc3        Examples           document    2 KB      2026-04-11 14:18
```

### 2.2 Formatter Registration API

Plugins can register custom formatters via the `CliContext`:

```rust
impl CliPlugin {
    pub fn register_formatter(&self, ctx: &CliContext, format: &str, 
                              formatter: Box<dyn Formatter>) -> Result<()> {
        ctx.register_output_formatter(format, formatter)
    }
}

pub trait Formatter: Send + Sync {
    fn format(&self, data: &serde_json::Value) -> Result<String>;
    fn supports_streaming(&self) -> bool { false }
}
```

Plugins can override output for any format or register new formats (e.g., YAML, CSV, HTML).

### 2.3 Output Helpers

- **Colorization:** ANSI color codes for terminals supporting 256-color. Auto-detects NO_COLOR env var and --no-color flag.
- **Paging:** Auto-page output via system pager if output exceeds 50% of terminal height. Disable with `--no-pager`.
- **Truncation:** For tables and long text, truncate to terminal width (--width override available).
- **Progress Indicators:** Spinners for long-running operations, progress bars for known-size tasks (download, processing). Use `indicatif` crate.

---

## 3. Every Command Specification

### 3.1 Forge Management (`nexus forge ...`)

#### 3.1.1 `nexus forge init`

**Purpose:** Initialize a new forge in a directory.

**Signature:**
```
nexus forge init [DIR] [--template <TEMPLATE>] [--name <NAME>] [--no-interactive]
```

**Arguments:**
- `DIR` — Target directory (default: current working directory). Created if not exists.

**Flags:**
- `--template <TEMPLATE>` — Template to use: `default`, `ai`, `data`, `blog` (default: `default`).
- `--name <NAME>` — Forge name (default: directory name).
- `--no-interactive` — Skip walkthrough prompts; use defaults.

**First-Run Walkthrough (interactive):**
1. Confirm directory (suggest current dir).
2. Prompt for forge name.
3. List templates; user selects one.
4. Prompt for initial plugins (core always enabled; optional: ai, git, mcp, sync).
5. Confirm and initialize.

**Output (text):**
```
✓ Forge initialized: my-forge
  Location: /path/to/my-forge
  Template: default
  Plugins: core, ai, git

Next steps:
  nexus forge open /path/to/my-forge
  nexus plugin list
  nexus content create "Welcome"
```

**Output (JSON):**
```json
{
  "status": "success",
  "data": {
    "forge_id": "uuid",
    "name": "my-forge",
    "location": "/path/to/my-forge",
    "template": "default",
    "plugins": ["core", "ai", "git"],
    "created_at": "2026-04-11T14:22:17Z"
  }
}
```

**Exit Codes:** 0 (success), 1 (I/O error), 2 (invalid args), 5 (directory exists).

---

#### 3.1.2 `nexus forge open`

**Purpose:** Open (set as active) a forge for subsequent commands.

**Signature:**
```
nexus forge open <PATH> [--remember]
```

**Arguments:**
- `PATH` — Path to forge directory.

**Flags:**
- `--remember` — Save as default forge in global config.

**Output (text):**
```
✓ Forge opened: my-forge
  Location: /path/to/my-forge
  Status: healthy
  Database: 1,523 records
```

**Exit Codes:** 0, 1 (path not found), 4 (invalid forge).

---

#### 3.1.3 `nexus forge config`

**Purpose:** Read/write forge configuration.

**Signature:**
```
nexus forge config [get|set|list] [<KEY>] [<VALUE>]
```

**Subcommands:**
- `get <KEY>` — Fetch a config value (e.g., `get ai.model`).
- `set <KEY> <VALUE>` — Set a config value. Value is TOML-parsed if needed.
- `list` — List all config keys and values (default).

**Config Keys (examples):**
- `ai.model` — LLM model to use (default: `gpt-4`).
- `ai.temperature` — Temperature (0.0–1.0, default: 0.7).
- `db.auto_backup` — Auto-backup database before mutations (default: `true`).
- `sync.enabled` — Enable sync mode (default: `false`).
- `git.auto_commit` — Auto-commit after mutations (default: `false`).

**Output (get):**
```
ai.model = "gpt-4"
```

**Output (list, table):**
```
Key                  Value
ai.model             gpt-4
ai.temperature       0.7
db.auto_backup       true
sync.enabled         false
```

**Exit Codes:** 0, 1 (I/O error), 2 (invalid key syntax), 4 (key not found).

---

#### 3.1.4 `nexus forge status`

**Purpose:** Display forge health and metadata.

**Signature:**
```
nexus forge status [--detailed]
```

**Flags:**
- `--detailed` — Include per-component status, recent errors, resource usage.

**Output (text):**
```
Forge Status: Healthy
Name: my-forge
Location: /path/to/my-forge
Database: 1,523 records, 12 MB
File Storage: 456 files, 234 MB
Plugins: 5 enabled, 0 disabled
Last Database Check: 2026-04-11 14:20:00 UTC
Last Sync: 2026-04-11 14:15:00 UTC (synced)
```

**Output (detailed, JSON):**
```json
{
  "status": "success",
  "data": {
    "health_status": "healthy",
    "forge_id": "uuid",
    "name": "my-forge",
    "location": "/path/to/my-forge",
    "db": {
      "records": 1523,
      "size_bytes": 12582912,
      "last_check_at": "2026-04-11T14:20:00Z"
    },
    "file_storage": {
      "file_count": 456,
      "dir_count": 89,
      "size_bytes": 245366784
    },
    "plugins": {
      "enabled": ["core", "ai", "git", "mcp", "sync"],
      "disabled": []
    },
    "sync": {
      "enabled": true,
      "last_sync_at": "2026-04-11T14:15:00Z",
      "status": "synced"
    }
  }
}
```

**Exit Codes:** 0, 1 (I/O error), 4 (forge not found).

---

### 3.2 Content Management (`nexus content ...`)

#### 3.2.1 `nexus content create`

**Purpose:** Create a new piece of content (document, note, snippet).

**Signature:**
```
nexus content create <TITLE> [--type <TYPE>] [--content <TEXT>|--stdin] [--tags <TAGS>] [--description <DESC>]
```

**Arguments:**
- `TITLE` — Content title.

**Flags:**
- `--type <TYPE>` — Content type: `document`, `note`, `snippet`, `code`, `test` (default: `document`).
- `--content <TEXT>` — Inline content (alternative to --stdin).
- `--stdin` — Read content from stdin (for piping).
- `--tags <TAGS>` — Comma-separated tags (e.g., `python,ai,tutorial`).
- `--description <DESC>` — Short description / summary.

**Output (text):**
```
✓ Content created: abc123
  Title: My Document
  Type: document
  Tags: python, ai
  URL: /forge/content/abc123
```

**Output (JSON):**
```json
{
  "status": "success",
  "data": {
    "id": "abc123",
    "title": "My Document",
    "type": "document",
    "tags": ["python", "ai"],
    "created_at": "2026-04-11T14:22:17Z",
    "url": "/forge/content/abc123"
  }
}
```

**Examples:**
```bash
# Inline
nexus content create "API Design" --type document --content "REST API best practices..."

# From stdin
echo "My note content" | nexus content create "Quick Note" --type note --stdin

# With tags
nexus content create "Python Tips" --tags python,performance,tips --type snippet
```

**Exit Codes:** 0, 1 (I/O error), 2 (invalid args), 5 (duplicate title).

---

#### 3.2.2 `nexus content read`

**Purpose:** Display content by ID or search.

**Signature:**
```
nexus content read <ID_OR_TITLE> [--raw]
```

**Arguments:**
- `ID_OR_TITLE` — Content ID (UUID) or exact title.

**Flags:**
- `--raw` — Output raw content (text/code only), no metadata. Useful for piping.

**Output (text, with metadata):**
```
Title: My Document
Type: document
Created: 2026-04-11 14:22:17 UTC
Modified: 2026-04-11 14:25:00 UTC
Tags: python, ai
Description: REST API best practices

--- Content ---
REST API design principles...
```

**Output (raw, --raw flag):**
```
REST API design principles...
(no metadata, newline-terminated)
```

**Exit Codes:** 0, 4 (not found).

---

#### 3.2.3 `nexus content edit`

**Purpose:** Edit content in default editor or via CLI.

**Signature:**
```
nexus content edit <ID_OR_TITLE> [--content <TEXT>] [--append] [--prepend]
```

**Arguments:**
- `ID_OR_TITLE` — Content ID or title.

**Flags:**
- `--content <TEXT>` — Replace content with TEXT.
- `--append` — Append TEXT to existing content.
- `--prepend` — Prepend TEXT to existing content.
- (no flags) — Open in `$EDITOR` (vim, nano, etc.).

**Output (text):**
```
✓ Content updated: abc123
  Modified: 2026-04-11 14:26:45 UTC
```

**Exit Codes:** 0, 1 (editor error), 4 (not found).

---

#### 3.2.4 `nexus content delete`

**Purpose:** Delete content.

**Signature:**
```
nexus content delete <ID_OR_TITLE> [--force]
```

**Flags:**
- `--force` — Skip confirmation prompt.

**Interactive Prompt (default):**
```
Delete "My Document"? (y/n): y
✓ Deleted: abc123
```

**Output (JSON):**
```json
{
  "status": "success",
  "data": {
    "id": "abc123",
    "title": "My Document",
    "deleted_at": "2026-04-11T14:27:00Z"
  }
}
```

**Exit Codes:** 0, 4 (not found).

---

#### 3.2.5 `nexus content search`

**Purpose:** Full-text search across content.

**Signature:**
```
nexus content search <QUERY> [--type <TYPE>] [--tag <TAG>] [--limit <N>] [--offset <N>]
```

**Arguments:**
- `QUERY` — Search term(s). Supports `AND`, `OR`, `NOT` and phrase queries (`"exact phrase"`).

**Flags:**
- `--type <TYPE>` — Filter by type (e.g., `document`, `code`).
- `--tag <TAG>` — Filter by tag (repeatable: `--tag python --tag ai`).
- `--limit <N>` — Max results (default: 50).
- `--offset <N>` — Pagination offset (default: 0).

**Output (table):**
```
ID          Title                Type        Tags              Score
abc123      API Design           document    python, rest      0.95
def456      HTTP Best Practices  snippet     web, http         0.87
ghi789      REST Client Lib      code        python, http      0.82
```

**Output (JSONL):**
```jsonl
{"id": "abc123", "title": "API Design", "type": "document", "tags": ["python", "rest"], "score": 0.95}
{"id": "def456", "title": "HTTP Best Practices", "type": "snippet", "tags": ["web", "http"], "score": 0.87}
```

**Exit Codes:** 0, 4 (no results).

---

### 3.3 Database Operations (`nexus db ...`)

#### 3.3.1 `nexus db query`

**Purpose:** Execute SQL query on forge database.

**Signature:**
```
nexus db query <SQL> [--json-output]
```

**Arguments:**
- `SQL` — SQL query string.

**Flags:**
- `--json-output` — Return results as JSON (default: table).

**Output (table):**
```
id        title                  created_at
abc123    API Design             2026-04-11 14:22:17
def456    HTTP Best Practices    2026-04-11 14:20:00
```

**Output (JSON):**
```json
{
  "status": "success",
  "data": [
    {"id": "abc123", "title": "API Design", "created_at": "2026-04-11T14:22:17Z"},
    {"id": "def456", "title": "HTTP Best Practices", "created_at": "2026-04-11T14:20:00Z"}
  ]
}
```

**Exit Codes:** 0, 1 (query error).

---

#### 3.3.2 `nexus db create-table`

**Purpose:** Create a new database table.

**Signature:**
```
nexus db create-table <NAME> [--schema <SCHEMA_JSON>]
```

**Arguments:**
- `NAME` — Table name.

**Flags:**
- `--schema <SCHEMA_JSON>` — JSON schema defining columns. Example: `{"id": "TEXT PRIMARY KEY", "name": "TEXT", "count": "INTEGER"}`.

**Output (text):**
```
✓ Table created: my_table
  Columns: 3 (id, name, count)
```

**Exit Codes:** 0, 1 (schema error), 5 (table exists).

---

#### 3.3.3 `nexus db add-record`

**Purpose:** Insert a record into a table.

**Signature:**
```
nexus db add-record <TABLE> [--fields <JSON>|--stdin]
```

**Arguments:**
- `TABLE` — Table name.

**Flags:**
- `--fields <JSON>` — JSON object with field values.
- `--stdin` — Read JSON from stdin.

**Example:**
```bash
nexus db add-record users --fields '{"id": "user123", "name": "Alice", "email": "alice@example.com"}'
```

**Output (text):**
```
✓ Record inserted
  Table: users
  Row ID: 1
```

**Exit Codes:** 0, 1 (schema error), 4 (table not found).

---

#### 3.3.4 `nexus db export`

**Purpose:** Export database or table to external format.

**Signature:**
```
nexus db export [<TABLE>] [--format <FORMAT>] [--output <FILE>]
```

**Arguments:**
- `TABLE` — Optional; if omitted, export entire database.

**Flags:**
- `--format <FORMAT>` — Export format: `sqlite`, `csv`, `json`, `jsonl` (default: `sqlite`).
- `--output <FILE>` — Write to file (default: stdout for text formats).

**Example:**
```bash
nexus db export users --format csv --output users.csv
nexus db export --format json --output backup.json
```

**Exit Codes:** 0, 1 (I/O error), 4 (table not found).

---

### 3.4 Plugin Management (`nexus plugin ...`)

#### 3.4.1 `nexus plugin install`

**Purpose:** Install a plugin from registry or local path.

**Signature:**
```
nexus plugin install <PLUGIN> [--source <URL>] [--version <VERSION>]
```

**Arguments:**
- `PLUGIN` — Plugin name or path (e.g., `nexus-ai`, `./my-plugin`).

**Flags:**
- `--source <URL>` — Registry URL (default: official Nexus registry).
- `--version <VERSION>` — Specific version to install (default: latest).

**Output (text):**
```
Installing nexus-ai...
⠙ Fetching plugin metadata...
⠹ Downloading (2.3 MB)...
✓ Plugin installed: nexus-ai v0.2.0
  Location: /path/to/plugins/nexus-ai
  Commands added: ai ask, ai chat, ai complete, agent run
```

**Exit Codes:** 0, 1 (download error), 5 (version conflict).

---

#### 3.4.2 `nexus plugin list`

**Purpose:** List installed plugins and their status.

**Signature:**
```
nexus plugin list [--detailed]
```

**Flags:**
- `--detailed` — Show version, description, and registered commands.

**Output (table):**
```
Name        Status    Version   Commands
core        enabled   1.0       forge, content, db
ai          enabled   0.2.0     ai ask, ai chat, agent run
git         enabled   0.3.1     git status, git commit, git log
mcp         disabled  0.1.0     mcp serve
sync        enabled   0.4.0     sync start, sync status
```

**Output (detailed, JSON):**
```json
{
  "status": "success",
  "data": [
    {
      "name": "core",
      "version": "1.0",
      "status": "enabled",
      "commands": ["forge", "content", "db"],
      "description": "Core forge operations"
    }
  ]
}
```

**Exit Codes:** 0.

---

#### 3.4.3 `nexus plugin enable / disable`

**Purpose:** Enable or disable a plugin.

**Signature:**
```
nexus plugin enable <PLUGIN>
nexus plugin disable <PLUGIN>
```

**Output (text):**
```
✓ Plugin enabled: nexus-ai
  Commands now available: ai ask, ai chat, agent run
```

**Exit Codes:** 0, 4 (plugin not found).

---

### 3.5 AI Operations (`nexus ai ...`)

#### 3.5.1 `nexus ai ask`

**Purpose:** Single-shot LLM query (non-interactive).

**Signature:**
```
nexus ai ask <PROMPT> [--context <FILE>] [--db-query <SQL>] [--include-file <PATH>] [--model <MODEL>] [--temperature <T>]
```

**Arguments:**
- `PROMPT` — Query text.

**Flags:**
- `--context <FILE>` — Prepend file content to prompt (e.g., for problem context).
- `--db-query <SQL>` — Include query results in prompt context.
- `--include-file <PATH>` — Include file content in context (repeatable).
- `--model <MODEL>` — Override default LLM model.
- `--temperature <T>` — Temperature (0.0–1.0).

**Output (text):**
```
Query: "How do I implement a REST API in Rust?"

Response:
To implement a REST API in Rust, use the Actix-web framework...
(full response follows)

Metadata:
  Model: gpt-4
  Tokens: 342 (in), 891 (out)
  Duration: 2.3s
```

**Output (JSON):**
```json
{
  "status": "success",
  "data": {
    "query": "How do I implement a REST API in Rust?",
    "response": "To implement a REST API in Rust...",
    "metadata": {
      "model": "gpt-4",
      "tokens": {"in": 342, "out": 891},
      "duration_ms": 2300
    }
  }
}
```

**Example:**
```bash
nexus ai ask "Optimize this function" --include-file main.rs --temperature 0.5
```

**Exit Codes:** 0, 1 (API error), 3 (auth failed).

---

#### 3.5.2 `nexus ai chat`

**Purpose:** Interactive multi-turn conversation with LLM.

**Signature:**
```
nexus ai chat [--context <FILE>] [--model <MODEL>]
```

**Flags:**
- Same as `nexus ai ask`.

**Interactive REPL:**
```
Nexus AI Chat (exit with 'quit' or Ctrl+D)
Model: gpt-4 | Temperature: 0.7

You> How do I implement a REST API?
Claude> To implement a REST API in Rust...

You> Can you show me example code?
Claude> Here's a minimal example using Actix-web...

You> quit
```

**Features:**
- Command history (via readline; `up/down` arrow keys).
- Context persistence across turns.
- Syntax highlighting for code blocks (if terminal supports it).
- Auto-completion of commands (e.g., `/help`, `/clear`, `/save <file>`).

**Built-in Commands:**
- `/help` — Show help.
- `/clear` — Clear conversation history.
- `/save <FILE>` — Save conversation to file.
- `/model <MODEL>` — Switch model mid-conversation.
- `/context <FILE>` — Add context file.

**Exit Codes:** 0 (normal exit), 1 (API error).

---

#### 3.5.3 `nexus ai complete`

**Purpose:** Code/text completion.

**Signature:**
```
nexus ai complete <FILE> [--line <N>] [--col <N>] [--context <NUM_LINES>]
```

**Arguments:**
- `FILE` — File path to complete in.

**Flags:**
- `--line <N>` — Line number for cursor (default: end of file).
- `--col <N>` — Column number (default: end of line).
- `--context <NUM_LINES>` — Lines of context before cursor (default: 10).

**Output (text):**
```
Completion suggestions for main.rs:42:15

1. pub async fn handle_request(
2. pub fn handle_request(
3. pub fn handler_request(

Selected: 1
Result:
pub async fn handle_request(req: HttpRequest) -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().body("Hello"))
}
```

**Exit Codes:** 0, 4 (file not found).

---

#### 3.5.4 `nexus agent run`

**Purpose:** Execute an AI agent for multi-step task automation.

**Signature:**
```
nexus agent run <AGENT> [--goal <GOAL>] [--max-steps <N>] [--verbose-trace]
```

**Arguments:**
- `AGENT` — Agent name/path.

**Flags:**
- `--goal <GOAL>` — Override agent goal.
- `--max-steps <N>` — Max steps before abort (default: 20).
- `--verbose-trace` — Print detailed step trace (planning, reasoning, actions).

**Output (text, progress):**
```
Running agent: code-generator
Goal: Generate REST API boilerplate for Rust

⠙ Step 1: Analyze requirements...
   → Parsed: HTTP server, async, JSON
⠹ Step 2: Generate scaffolding...
   → Created: src/main.rs, src/models.rs, Cargo.toml
⠸ Step 3: Implement endpoints...
   → Added: GET /items, POST /items, DELETE /items/:id
✓ Agent complete: 3 steps

Generated files:
  src/main.rs (234 lines)
  src/models.rs (89 lines)
  Cargo.toml (15 lines)
```

**Output (verbose, JSON):**
```json
{
  "status": "success",
  "data": {
    "agent": "code-generator",
    "goal": "Generate REST API boilerplate for Rust",
    "steps": 3,
    "trace": [
      {
        "step": 1,
        "action": "analyze_requirements",
        "result": "Parsed: HTTP server, async, JSON"
      }
    ],
    "artifacts": ["src/main.rs", "src/models.rs", "Cargo.toml"]
  }
}
```

**Exit Codes:** 0, 1 (agent error), 6 (max steps exceeded).

---

### 3.6 Process Manager (`nexus proc ...`)

#### 3.6.1 `nexus proc list`

**Purpose:** List running processes / background tasks.

**Signature:**
```
nexus proc list [--status <STATUS>]
```

**Flags:**
- `--status <STATUS>` — Filter: `running`, `idle`, `error` (default: all).

**Output (table):**
```
PID    Name              Status      Memory  CPU    Started
1234   ai-codegen        running     124 MB  45%    2026-04-11 14:20
5678   sync-worker       idle        32 MB   0%     2026-04-11 14:15
9012   git-push          running     64 MB   12%    2026-04-11 14:25
```

**Exit Codes:** 0.

---

#### 3.6.2 `nexus proc run`

**Purpose:** Run a task in the background.

**Signature:**
```
nexus proc run <NAME> [--cmd <COMMAND>] [--detach]
```

**Arguments:**
- `NAME` — Process name.

**Flags:**
- `--cmd <COMMAND>` — Command to run (shell syntax).
- `--detach` — Don't wait for completion; return immediately.

**Output (text):**
```
✓ Process started: my-task
  PID: 1234
  Detached: true
  Log: /path/to/logs/my-task.log

Check status: nexus proc list
View logs: tail -f /path/to/logs/my-task.log
```

**Exit Codes:** 0, 1 (command error).

---

#### 3.6.3 `nexus proc stop`

**Purpose:** Stop a running process.

**Signature:**
```
nexus proc stop <PID_OR_NAME> [--force]
```

**Flags:**
- `--force` — Send SIGKILL instead of SIGTERM.

**Exit Codes:** 0, 4 (process not found).

---

### 3.7 Terminal Operations (`nexus term ...`)

#### 3.7.1 `nexus term create`

**Purpose:** Create a persistent terminal session.

**Signature:**
```
nexus term create <NAME> [--shell <SHELL>]
```

**Arguments:**
- `NAME` — Session name.

**Flags:**
- `--shell <SHELL>` — Shell type: `bash`, `zsh`, `fish` (default: system shell).

**Output (text):**
```
✓ Terminal session created: dev-term
  Session ID: term_abc123
  Shell: bash
  
Attach: nexus term exec dev-term
```

**Exit Codes:** 0, 5 (session exists).

---

#### 3.7.2 `nexus term exec`

**Purpose:** Execute command(s) in a terminal session.

**Signature:**
```
nexus term exec <NAME> [--cmd <COMMAND>] [--interactive]
```

**Flags:**
- `--cmd <COMMAND>` — Command to execute (non-interactive).
- `--interactive` — Attach interactively (stdin/stdout).

**Exit Codes:** 0, 4 (session not found), 1 (command error).

---

#### 3.7.3 `nexus term list`

**Purpose:** List active terminal sessions.

**Signature:**
```
nexus term list
```

**Output (table):**
```
Name        ID           Shell   Created
dev-term    term_abc123  bash    2026-04-11 14:20
build       term_def456  zsh     2026-04-11 14:18
```

**Exit Codes:** 0.

---

### 3.8 MCP Server (`nexus mcp ...`)

#### 3.8.1 `nexus mcp serve`

**Purpose:** Start Nexus as an MCP server (for use by Claude, IDEs, etc.).

**Signature:**
```
nexus mcp serve [--port <PORT>] [--stdio]
```

**Flags:**
- `--port <PORT>` — HTTP/WebSocket port (default: 9000).
- `--stdio` — Use stdio transport instead of HTTP (for IDE integration).

**Output (text):**
```
✓ MCP server started
  Transport: stdio
  Capabilities: read, write, execute, query
  
Waiting for connections...
(Ctrl+C to stop)
```

**Exit Codes:** 0, 1 (port in use).

---

#### 3.8.2 `nexus mcp connect`

**Purpose:** Connect to a remote MCP server.

**Signature:**
```
nexus mcp connect <URL> [--auth <TOKEN>]
```

**Arguments:**
- `URL` — Server URL (e.g., `ws://localhost:9000`).

**Flags:**
- `--auth <TOKEN>` — Bearer token for authentication.

**Output (text):**
```
✓ Connected to MCP server
  URL: ws://localhost:9000
  Status: active
```

**Exit Codes:** 0, 1 (connection error).

---

### 3.9 Synchronization (`nexus sync ...`)

#### 3.9.1 `nexus sync start`

**Purpose:** Initiate bidirectional sync with remote.

**Signature:**
```
nexus sync start [--remote <URL>] [--direction <DIR>]
```

**Flags:**
- `--remote <URL>` — Remote sync endpoint (default: configured remote).
- `--direction <DIR>` — `pull`, `push`, `bidirectional` (default: `bidirectional`).

**Output (text, with progress):**
```
Starting sync: bidirectional
⠙ Pulling changes from remote...
  ✓ Fetched 5 documents, 2 deletions
⠹ Pushing local changes...
  ✓ Sent 3 documents, 1 deletion
✓ Sync complete
  Local: 1,523 records
  Remote: 1,523 records (in sync)
```

**Exit Codes:** 0, 1 (sync error), 6 (unresolved conflicts).

---

#### 3.9.2 `nexus sync status`

**Purpose:** Show sync state and pending changes.

**Signature:**
```
nexus sync status [--detailed]
```

**Output (text):**
```
Sync Status: synced
Last sync: 2026-04-11 14:15:00 UTC
Local changes: 0
Remote changes: 0
Conflicts: 0
```

**Exit Codes:** 0.

---

#### 3.9.3 `nexus sync resolve`

**Purpose:** Resolve sync conflicts interactively.

**Signature:**
```
nexus sync resolve [--strategy <STRATEGY>]
```

**Flags:**
- `--strategy <STRATEGY>` — Auto-resolution: `local`, `remote`, `manual` (default: `manual`).

**Interactive Conflict Resolution:**
```
Conflict 1/3: Document "API Design"
  Local version: modified 2026-04-11 14:20
  Remote version: modified 2026-04-11 14:18
  
  Options:
  1. Keep local
  2. Use remote
  3. Merge (manual)
  4. Skip
  
  Choice: 1
  ✓ Resolved: using local version
```

**Exit Codes:** 0, 6 (unresolved conflicts remain).

---

### 3.10 Git Operations (`nexus git ...`)

#### 3.10.1 `nexus git status`

**Purpose:** Show forge git status.

**Signature:**
```
nexus git status
```

**Output (text):**
```
On branch main
Modified files:
  M content/api-design.md
  M src/main.rs
  
Untracked:
  ?? new-feature.rs
```

**Exit Codes:** 0.

---

#### 3.10.2 `nexus git commit`

**Purpose:** Commit changes with message.

**Signature:**
```
nexus git commit [--message <MSG>] [--all]
```

**Flags:**
- `--message <MSG>` — Commit message.
- `--all` — Stage all changes before commit.

**Output (text):**
```
✓ Committed
  Hash: abc123def456
  Author: user@example.com
  Message: "Update API documentation"
```

**Exit Codes:** 0, 1 (no changes).

---

#### 3.10.3 `nexus git log`

**Purpose:** Show commit history.

**Signature:**
```
nexus git log [--limit <N>] [--format <FMT>]
```

**Flags:**
- `--limit <N>` — Max commits (default: 20).
- `--format <FMT>` — Format: `oneline`, `short`, `full` (default: `oneline`).

**Output (text, oneline):**
```
abc123d (HEAD -> main) Update API documentation
def456g Add REST endpoints
ghi789j Initial commit
```

**Exit Codes:** 0.

---

#### 3.10.4 `nexus git diff`

**Purpose:** Show changes between commits or working tree.

**Signature:**
```
nexus git diff [<REF1> [<REF2>]]
```

**Arguments:**
- `REF1`, `REF2` — Commit refs; defaults to `HEAD` vs working tree.

**Output (text, unified diff):**
```
diff --git a/content/api-design.md b/content/api-design.md
index abc123..def456 100644
--- a/content/api-design.md
+++ b/content/api-design.md
@@ -10,3 +10,5 @@ REST API principles:
 - Error handling
 - Documentation
+- Performance optimization
+- Security best practices
```

**Exit Codes:** 0.

---

#### 3.10.5 `nexus git push / pull`

**Purpose:** Sync with remote repository.

**Signature:**
```
nexus git push [--remote <NAME>] [--branch <BRANCH>]
nexus git pull [--remote <NAME>] [--branch <BRANCH>]
```

**Flags:**
- `--remote <NAME>` — Remote name (default: `origin`).
- `--branch <BRANCH>` — Branch (default: current branch).

**Output (text):**
```
✓ Pushed to origin/main
  4 commits, 12 files changed
```

**Exit Codes:** 0, 1 (network error), 6 (merge conflicts).

---

#### 3.10.6 `nexus git branch`

**Purpose:** List or create branches.

**Signature:**
```
nexus git branch [<NAME>] [--delete] [--list]
```

**Arguments:**
- `NAME` — Branch name (creates if not exists).

**Flags:**
- `--delete` — Delete branch.
- `--list` — List branches (default).

**Output (list):**
```
* main
  feature/api-v2
  bugfix/sync-issue
```

**Exit Codes:** 0, 4 (branch not found).

---

### 3.11 Automation (`nexus run` and `nexus watch`)

#### 3.11.1 `nexus run`

**Purpose:** Execute automation script.

**Signature:**
```
nexus run <SCRIPT> [--var <KEY>=<VALUE>]...
```

**Arguments:**
- `SCRIPT` — Path to script file (shell, Python, or Nexus DSL).

**Flags:**
- `--var <KEY>=<VALUE>` — Pass variables to script (repeatable).

**Script Support:**
- Bash/Zsh: Executed directly.
- Python: Run via `python`.
- Nexus DSL (`.nexus` files): Interpreted by Nexus runtime.

**Example Script (`build.sh`):**
```bash
#!/bin/bash
nexus content create "Build Report" --type document --content "$(date)"
nexus db add-record build_logs --fields "{\"status\": \"success\"}"
echo "Build completed"
```

**Invocation:**
```bash
nexus run ./build.sh
```

**Output (text):**
```
Executing: ./build.sh

Build completed

✓ Script completed successfully
  Duration: 0.8s
  Exit code: 0
```

**Exit Codes:** 0 (success), 1 (script error), 2 (invalid script).

---

#### 3.11.2 `nexus watch`

**Purpose:** Watch files/directories and execute command on changes.

**Signature:**
```
nexus watch <GLOB> [--exec <COMMAND>] [--debounce <MS>] [--parallel]
```

**Arguments:**
- `GLOB` — File glob pattern (e.g., `src/**/*.rs`, `*.md`).

**Flags:**
- `--exec <COMMAND>` — Command to execute on change.
- `--debounce <MS>` — Debounce delay in milliseconds (default: 500).
- `--parallel` — Run multiple commands in parallel (default: serial).

**Command Substitution:**
- `{file}` — Changed file path.
- `{event}` — Event type: `create`, `modify`, `delete`.
- `{timestamp}` — ISO 8601 timestamp.

**Example:**
```bash
# Compile Rust on source changes
nexus watch 'src/**/*.rs' --exec 'cargo build 2>&1 | nexus content create "Build: {timestamp}"'

# Format Python files
nexus watch '**/*.py' --exec 'black {file}'

# Run tests on doc changes
nexus watch 'docs/**/*.md' --exec 'nexus ai ask "Test this: {file}"'
```

**Output (text, with progress):**
```
Watching: src/**/*.rs
Debounce: 500ms
Command: cargo build

✓ Ready
2026-04-11 14:30:15 [modify] src/main.rs
⠙ Executing command...
   cargo build
   Compiling nexus v0.1.0
   Finished release [optimized] target(s) in 1.23s
✓ Command completed (exit code: 0)

2026-04-11 14:30:28 [create] src/utils.rs
⠙ Executing command...
```

**Exit Codes:** 0 (Ctrl+C), 1 (command error).

---

## 4. Plugin CLI Registration

### 4.1 CliContext API

Plugins register CLI commands via the `CliContext`, passed to `Plugin::initialize_cli()`:

```rust
pub struct CliContext {
    // Register a subcommand group
    pub fn register_command_group(
        &self,
        group: &str,
        description: &str,
    ) -> Result<CommandBuilder>;
    
    // Register a single command
    pub fn register_command(
        &self,
        group: &str,
        name: &str,
        handler: Box<dyn CliCommandHandler>,
        description: &str,
    ) -> Result<()>;
    
    // Register output formatter
    pub fn register_output_formatter(
        &self,
        format: &str,
        formatter: Box<dyn Formatter>,
    ) -> Result<()>;
    
    // Access forge/kernel
    pub fn forge(&self) -> &Forge;
    pub fn kernel(&self) -> &Kernel;
}

pub trait CliCommandHandler: Send + Sync {
    async fn execute(
        &self,
        args: &[String],
        ctx: &CommandContext,
    ) -> Result<CliOutput>;
}

pub struct CommandContext {
    pub forge_path: PathBuf,
    pub format: OutputFormat,
    pub quiet: bool,
    pub verbose_level: u32,
}

pub enum CliOutput {
    Text(String),
    Json(serde_json::Value),
    Jsonl(Vec<serde_json::Value>),
    Raw(Vec<u8>),
}
```

### 4.2 Plugin Initialization Example

```rust
impl Plugin for MyPlugin {
    fn name(&self) -> &str { "my-plugin" }
    
    async fn initialize_cli(&self, ctx: &CliContext) -> Result<()> {
        // Register command group
        let group = ctx.register_command_group(
            "myplugin",
            "My Plugin Commands"
        )?;
        
        // Register individual command
        ctx.register_command(
            "myplugin",
            "do-thing",
            Box::new(MyCommandHandler),
            "Do a thing"
        )?;
        
        Ok(())
    }
}

struct MyCommandHandler;

#[async_trait]
impl CliCommandHandler for MyCommandHandler {
    async fn execute(&self, args: &[String], ctx: &CommandContext) -> Result<CliOutput> {
        // Parse args with clap (or custom)
        // Execute logic
        Ok(CliOutput::Json(json!({
            "status": "success",
            "data": { ... }
        })))
    }
}
```

### 4.3 Community Plugin CLI Support

Community plugins can register their own CLI commands by implementing the `CliPlugin` trait. The CLI loader automatically discovers and registers them:

1. Plugin publishes crate with `CliPlugin` impl.
2. User runs `nexus plugin install my-plugin`.
3. CLI loader calls `initialize_cli()` at startup.
4. Commands become available as `nexus myplugin <subcommand>`.

**Restriction:** Community plugins CANNOT override core commands (e.g., `nexus content`) without explicit allowlist.

---

## 5. Interactive vs Non-Interactive Modes

### 5.1 TTY Detection

The CLI detects TTY via `isatty()` on stdin:

- **Interactive (TTY detected):** Prompts enabled, colored output, pager support, readline history.
- **Non-interactive (pipe/redirect):** Prompts disabled, plain output, no pager, no history.

**Override:**
- `--interactive` — Force interactive mode.
- `--non-interactive` — Force non-interactive mode.
- `--quiet` — Suppress prompts; use defaults or error.

### 5.2 Prompt Fallback

If TTY is not detected and a prompt is required:

```rust
fn prompt_user(question: &str, default: Option<&str>) -> Result<String> {
    if is_tty() {
        // Interactive: show prompt
    } else if let Some(def) = default {
        // Non-interactive: use default
        eprintln!("{} (using default: {})", question, def);
        Ok(def.to_string())
    } else {
        // Error: cannot proceed
        Err(Error::NoTtyAndNoDefault)
    }
}
```

### 5.3 Confirmation Prompts

Confirmations (delete, overwrite, etc.) are skipped in non-interactive mode if `--force` is passed:

```bash
# Interactive
nexus content delete "My Doc"
# Prompt: Delete "My Doc"? (y/n): 

# Non-interactive with --force
nexus content delete "My Doc" --force
# No prompt; deletes immediately

# Non-interactive without --force
nexus content delete "My Doc"
# Error: confirmation required in non-interactive mode
```

### 5.4 Progress Indicators

- **Interactive:** Spinners (`⠙`, `⠹`, `⠸`, `⠼`, `⠴`, `⠦`, `⠧`, `⠇`, `⠏`) and progress bars.
- **Non-interactive:** Plain status messages (e.g., `[1/10] Processing...`).

---

## 6. Watch Mode Implementation

### 6.1 Core Logic

Watch mode monitors file system events via `notify` crate and executes a command on change:

```rust
pub async fn watch(
    glob: &str,
    exec: &str,
    debounce_ms: u64,
    parallel: bool,
) -> Result<()> {
    let mut watcher = notify::watcher()?;
    let (tx, mut rx) = tokio::sync::mpsc::channel(100);
    
    // Debounce: collect events, wait, then execute
    let mut pending_files = HashSet::new();
    
    loop {
        tokio::select! {
            Some((file, event)) = rx.recv() => {
                pending_files.insert((file, event));
                // Start debounce timer
            }
            _ = tokio::time::sleep(Duration::from_millis(debounce_ms)) => {
                for (file, event) in pending_files.drain() {
                    let cmd = exec
                        .replace("{file}", &file)
                        .replace("{event}", &event.to_string())
                        .replace("{timestamp}", &chrono::Local::now().to_rfc3339());
                    
                    execute_command(&cmd, parallel).await?;
                }
            }
        }
    }
}
```

### 6.2 Command Substitution

- `{file}` — Full path of changed file.
- `{event}` — Event type: `create`, `modify`, `delete`, `rename`.
- `{timestamp}` — ISO 8601 timestamp of event.

Example:
```bash
nexus watch 'src/**/*.rs' --exec 'echo "File {file} was {event} at {timestamp}"'
```

### 6.3 Execution Modes

- **Serial (default):** Commands execute sequentially; next command waits for previous to finish.
- **Parallel (`--parallel`):** Commands execute concurrently; all background commands run in parallel.

---

## 7. Scripting Support

### 7.1 Output Formats for Piping

Commands output machine-readable formats for scripting:

```bash
# Get content as raw text (no metadata)
nexus content read "API Design" --raw | head -20

# Parse JSON output
nexus forge status --format json | jq '.data.db_records'

# Streaming with JSONL
nexus content search "api" --format jsonl | while read -r line; do
  ID=$(echo "$line" | jq -r '.id')
  TITLE=$(echo "$line" | jq -r '.title')
  echo "Processing: $ID - $TITLE"
done

# Create document from piped content
cat README.md | nexus content create "Project README" --stdin --type document
```

### 7.2 Exit Code Conventions

- `0` — Success.
- `1` — Generic error (checked via `$?` in scripts).
- `2` — Usage error.
- `3` — Permission denied.
- `4` — Resource not found.
- `5` — Conflict (e.g., duplicate).
- `127` — Command not found.

Scripts check exit codes:

```bash
nexus content create "Test" || {
  echo "Failed to create content (exit $?)"
  exit 1
}
```

### 7.3 Signal Handling

CLI respects POSIX signals:

- **SIGINT (Ctrl+C):** Gracefully shutdown; save state if needed.
- **SIGTERM:** Clean termination.
- **SIGHUP:** Reload config (if long-running).

Watch mode gracefully stops on Ctrl+C:

```bash
nexus watch 'src/**/*.rs' --exec 'cargo build'
# Ctrl+C → Finish current command, then exit
```

---

## 8. Shell Completions

### 8.1 Supported Shells

Completions generated for: bash, zsh, fish, PowerShell.

### 8.2 Installation

```bash
# Bash
nexus --generate-completions bash > ~/.bash_completions
echo 'source ~/.bash_completions' >> ~/.bashrc

# Zsh
nexus --generate-completions zsh > ~/.zfunc/_nexus
# Add ~/.zfunc to $fpath

# Fish
nexus --generate-completions fish > ~/.config/fish/completions/nexus.fish

# PowerShell
nexus --generate-completions powershell | Out-File $PROFILE
```

### 8.3 Completion Features

- **Command names:** `nexus <TAB>` → lists subcommands.
- **Subcommand flags:** `nexus content create --<TAB>` → lists available flags.
- **Forge paths:** `--forge-path <TAB>` → lists recent forges or ~/.*forge directories.
- **Content IDs/titles:** `nexus content read <TAB>` → lists recent content (from db).
- **Plugin names:** `nexus plugin install <TAB>` → lists available plugins from registry.
- **Branches:** `nexus git branch <TAB>` → lists local branches.

---

## 9. Configuration via CLI

### 9.1 `nexus forge config` Command

```bash
# List all config
nexus forge config list

# Get a value
nexus forge config get ai.model

# Set a value
nexus forge config set ai.temperature 0.5
nexus forge config set db.auto_backup false

# Set nested values
nexus forge config set sync.remote.url "https://sync.example.com"
```

### 9.2 Config File Hierarchy

Precedence (highest to lowest):

1. **CLI arguments:** `--format json`, `--forge-path /custom`
2. **Environment variables:** `NEXUS_FORMAT=json`, `NEXUS_FORGE_PATH=/custom`
3. **Forge config file:** `<forge-root>/.nexus/config.toml`
4. **Global config:** `~/.nexus/config.toml`
5. **Defaults:** Built-in defaults.

### 9.3 Config File Format (TOML)

```toml
[ai]
model = "gpt-4"
temperature = 0.7
max_tokens = 2000

[db]
auto_backup = true
backup_retention_days = 30

[sync]
enabled = true
remote = "https://sync.example.com"
frequency = "every 5 minutes"

[git]
auto_commit = false
commit_template = "docs: update via CLI"

[cli]
color = true
pager = true
width = 120
```

---

## 10. AI CLI Commands Detail

### 10.1 `nexus ai ask` — Multi-Turn Capability

The `--db-query` flag enables context from database results:

```bash
# Ask with SQL context
nexus ai ask "Summarize my recent documents" \
  --db-query "SELECT title, content FROM documents ORDER BY created_at DESC LIMIT 5"

# Output includes query results prepended to prompt
# Query Results:
# 1. "API Design" - 2026-04-11
# 2. "REST Patterns" - 2026-04-11
# ...
# (LLM response follows)
```

### 10.2 `nexus ai chat` — Session Management

REPL maintains history file:

```
~/.nexus/ai-chat-history.json
```

History persists across sessions. `/clear` resets in-memory state but preserves history file.

### 10.3 `nexus ai complete` — Code Context

Completion pulls surrounding code context automatically:

```bash
# Given main.rs with cursor at line 42:
nexus ai complete main.rs --line 42 --col 15 --context 20

# Sends 20 lines before cursor to LLM for context
# Returns top-N completion suggestions
```

### 10.4 `nexus agent run` — Artifact Capture

Agent output automatically captured to forge:

```bash
nexus agent run code-generator --goal "REST API for products"

# Output files created by agent are automatically:
# 1. Added to forge content (as documents/code snippets)
# 2. Committed to git (if git plugin enabled)
# 3. Synced to remote (if sync enabled)
```

---

## 11. Kernel Initialization

### 11.1 Startup Flow

1. Parse CLI args (global flags).
2. Initialize minimal kernel (no GUI).
3. Load core plugin only (unless `--plugins all`).
4. Load forge from `--forge-path`.
5. Load additional plugins from forge config.
6. Execute command.
7. Shutdown cleanly.

### 11.2 Startup Time Optimization

- **Lazy plugin loading:** Only load plugins needed for command.
- **Database lazy open:** Open SQLite only on first DB access.
- **Parallel initialization:** Initialize independent subsystems in parallel.
- **Target:** Startup < 500ms (from argv to command execution).

### 11.3 Minimal Plugin Loading

By default, only `core` plugin loads. Other plugins load on-demand or if configured:

```rust
// In kernel.rs
async fn initialize_plugins(&self, requested: &[&str]) -> Result<()> {
    // Always load core
    self.load_plugin("core").await?;
    
    // Load if requested
    for name in requested {
        self.load_plugin(name).await?;
    }
}
```

---

## 12. Error Handling

### 12.1 Error Output Format

**Text (default):**
```
Error: Invalid forge path
  Path: /nonexistent
  Reason: Directory does not exist

Run 'nexus help' for usage info.
Exit code: 4
```

**JSON:**
```json
{
  "status": "error",
  "error": {
    "code": 4,
    "message": "Invalid forge path",
    "details": {
      "path": "/nonexistent",
      "reason": "Directory does not exist"
    }
  }
}
```

### 12.2 Error Code Taxonomy

| Code | Meaning |
|------|---------|
| 1 | Generic I/O or runtime error |
| 2 | Invalid arguments or usage error |
| 3 | Permission denied / authentication failed |
| 4 | Resource not found |
| 5 | Conflict (duplicate, constraint violation) |
| 6 | Timeout or limit exceeded |
| 127 | Command not found |

### 12.3 Debug Mode

`--verbose` / `-v` (repeatable) enables debug logging:

```bash
nexus content create "Test" -vv
# Output includes timing, debug traces, stack traces on errors
```

**Verbosity levels:**
- `-v` — Info (command timing, status).
- `-vv` — Debug (plugin loading, db queries).
- `-vvv` — Trace (detailed execution flow).

---

## 13. Performance Targets

- **CLI startup:** < 500ms (args to ready).
- **Command execution:** < 1s for database ops, < 5s for AI operations.
- **Output streaming:** Flush every 1MB or 100ms, whichever comes first.
- **Watch debounce:** 500ms default.
- **Help text generation:** < 100ms.

---

## 14. Testing Strategy

### 14.1 CLI Integration Tests

```rust
#[tokio::test]
async fn test_content_create() {
    let forge = TestForge::new().await;
    let result = forge.exec("content create \"Test\" --type document").await;
    assert!(result.is_ok());
    assert_json!(result.json()["data"]["title"], "Test");
}
```


### 14.2 Snapshot Testing

Output format testing via snapshots:

```bash
# First run: create snapshot
nexus forge status --format json > /tmp/status.json.snap

# Subsequent runs: compare
nexus forge status --format json | diff - /tmp/status.json.snap
```

### 14.3 Cross-Platform Testing

- Linux (primary).
- macOS (Clap + signal handling compatibility).
- Windows (PowerShell completions, path handling).

---

## 15. First-Run Experience

### 15.1 `nexus forge init` Walkthrough

```
Welcome to Nexus CLI!

This wizard will help you initialize a new forge.

1. Directory
   Current: /home/user/my-project
   Use this? (y/n): y

2. Forge Name
   Suggested: my-project
   Enter name [my-project]: my-awesome-project

3. Template
   1. default (general-purpose)
   2. ai (AI-native)
   3. data (data science)
   4. blog (blogging platform)
   
   Select [1]: 2

4. Plugins
   Enable core? (y/n): y [auto-yes]
   Enable ai? (y/n): y
   Enable git? (y/n): y
   Enable mcp? (y/n): n
   Enable sync? (y/n): n

✓ Forge initialized at: /home/user/my-project
  Template: ai
  Plugins: core, ai, git

Next steps:
  nexus forge open /home/user/my-project --remember
  nexus content create "Welcome"
  nexus ai ask "How do I get started?"
```

### 15.2 Suggested Next Commands

After init, show contextual help:

```
To get started:
  • Create your first document:
    nexus content create "My First Note"
  
  • Ask an AI question:
    nexus ai ask "What is a REST API?"
  
  • Learn more:
    nexus help
    nexus <command> --help
```

---

## 16. Help System

### 16.1 `nexus help`

Top-level help lists all command groups:

```
Nexus CLI v1.0

USAGE:
    nexus [OPTIONS] <COMMAND> [ARGS]

COMMANDS:
    forge       Forge management (init, open, config, status)
    content     Content CRUD (create, read, edit, delete, search)
    db          Database operations
    plugin      Plugin management
    ai          AI operations
    proc        Process management
    term        Terminal operations
    mcp         MCP server
    sync        Synchronization
    git         Git integration
    run         Execute automation script
    watch       Watch files and execute commands
    help        Show this message

OPTIONS:
    --forge-path <PATH>    Forge root directory
    --format <FORMAT>      Output format: json, jsonl, text, table
    --quiet                Suppress non-essential output
    --verbose, -v          Enable debug logging
    --no-color             Disable ANSI colors
    --config <PATH>        Path to config file
    --help, -h             Print help
    --version, -V          Print version

Use 'nexus <COMMAND> --help' for more information on a command.
```

### 16.2 `nexus <COMMAND> --help`

Command-specific help:

```
nexus content create --help

Create a new piece of content

USAGE:
    nexus content create <TITLE> [OPTIONS]

ARGUMENTS:
    <TITLE>    Content title

OPTIONS:
    --type <TYPE>              Content type [default: document]
    --content <TEXT>           Inline content
    --stdin                    Read content from stdin
    --tags <TAGS>              Comma-separated tags
    --description <DESC>       Short description
    --quiet                    Suppress output
    --format <FORMAT>          Output format [default: text]
    --help, -h                 Print help

EXAMPLES:
    # Create inline
    nexus content create "My Note" --content "This is a note" --tags note,quick
    
    # From stdin
    cat README.md | nexus content create "Project README" --stdin
    
    # JSON output
    nexus content create "Test" --format json
```

### 16.3 Man Page Generation

Generate man pages from clap:

```bash
nexus --generate-man-pages > docs/man/
```

---

## 17. REPL Mode (Optional, Design-Level)

### 17.1 Conceptual Design

Interactive REPL for exploratory forging:

```
$ nexus-repl

Welcome to Nexus REPL v1.0
Type 'help' for commands, 'quit' to exit

nexus> content list
[1] "API Design" (document, 4 KB)
[2] "REST Patterns" (snippet, 2 KB)

nexus> content read 1
Title: API Design
Content: ...

nexus> ai ask "Summarize this content"
(processes recent content in context)
...

nexus> quit
```

**Features:**
- Command history (readline).
- Auto-completion of commands and content IDs.
- Context persistence (recent content in scope).
- Shorthand syntax (`list`, `read 1` instead of full command).

**Status:** Optional for v1.0; defer if time-constrained.

---

## 18. Acceptance Criteria

### Must-Have (v1.0)

- [ ] All command categories implemented (§3).
- [ ] Output formatting system working (§2).
- [ ] Plugin CLI registration API functional (§4).
- [ ] Watch mode functional (§5).
- [ ] Shell completions for bash/zsh (§8).
- [ ] AI commands (ask, chat, complete, agent run) working (§3.5, §10).
- [ ] Configuration system functional (§9).
- [ ] Error codes and help system (§12, §16).
- [ ] Startup < 500ms.
- [ ] Integration tests covering main commands (§14).

### Nice-to-Have

- [ ] REPL mode (§17).
- [ ] PowerShell/Fish completions.
- [ ] Dynamic completion for database records.
- [ ] Man page generation.
- [ ] Streaming progress indicators (spinner, progress bar).

### Scope Exclusions

- GUI (handled by separate crate).
- Network sync protocol details (assumed `nexus sync` uses Kernel API).
- Plugin development documentation (separate plugin guide).

---

## 19. Dependencies

- `clap` 4.x — CLI argument parsing, help generation.
- `tokio` — Async runtime.
- `serde_json` — JSON serialization.
- `serde` — Serialization framework.
- `colored` — ANSI coloring.
- `indicatif` — Progress indicators.
- `notify` — File system watching.
- `rustyline` — Readline (for REPL, optional).
- `toml` — Configuration parsing.

---

## 20. Version History & Release Plan

**v1.0 (April 2026):**
- Initial CLI with all command categories.
- Output formatting, plugin registration.
- Watch mode, scripting support.
- Shell completions (bash, zsh).
- ~600 lines of PRD, ~10k LOC in nexus-cli crate.

**v1.1 (May 2026):**
- REPL mode.
- PowerShell/Fish completions.
- Dynamic completions (db record names).

**v2.0 (future):**
- Plugin CLI extensions.
- Remote MCP client.
- Advanced agent orchestration.

---

**End of PRD**

*This document is implementation-ready for the Nexus CLI v1.0 release. All command signatures, output formats, and behavior are specified to support direct development without further design meetings.*
