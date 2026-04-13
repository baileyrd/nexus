# PRD-17: Workflow System Subsystem

**Version:** 1.0  
**Status:** Implementation Ready  
**Target Release:** April 2026  
**Scope:** 500–700 lines of specification  

---

## 1. Executive Summary

The Workflow System is a new subsystem of Nexus that enables users to automate repetitive tasks by composing triggers, conditions, and actions into deterministic or AI-enhanced pipelines. It fills the gap between manual UI operations and full autonomous AI agents—a "Zapier for your local forge" that operates on files, databases, processes, git, and MCP tools.

**Positioning:**
- **Vs. Shell Scripts:** Native to Nexus data model; visual or declarative definition; rich type system; built-in error handling.
- **Vs. Agents:** Deterministic, predictable, cost-controlled; optional AI steps for enhancement, not core planning.
- **Vs. Manual Work:** Event-driven automation; repeatable, consistent; audit trail; error recovery.

---

## 2. Problem Statement

### Pain Points

1. **Repetitive Manual Tasks:** Users perform the same sequences repeatedly (e.g., "create a new note, tag it, run a search, email the result").
2. **Context Switching:** Automation requires abandoning Nexus to use shell scripts, external IFTTT services, or GitHub Actions.
3. **No Local Event Automation:** File watchers and git hooks exist in isolation; no unified trigger system.
4. **Expensive AI:** Full agents are overkill for deterministic tasks; cost and latency prohibitive for routine automation.
5. **No Template Library:** Users reinvent common patterns (daily journals, backup pipelines, tag-on-save).

### Solution

A first-class subsystem for defining, executing, testing, and sharing workflows—backed by a declarative file format, visual editor, and runtime engine.

---

## 3. Core Definitions

### Workflow
A directed acyclic graph (DAG) of **steps** triggered by **events**, gated by **conditions**, producing **outputs**. Workflows are deterministic by default; AI steps introduce controlled non-determinism.

### Trigger
An event source that initiates workflow execution. Triggers can be file system events, database changes, process state, git events, cron schedules, webhooks, or manual user invocation.

### Condition
A boolean expression evaluated at trigger time or between steps. Gates execution or branches control flow. Supports property matching, regex, time-based logic, and database queries.

### Action
An atomic operation: file I/O, database mutation, terminal command, AI call, process management, git operation, HTTP request, notification, or variable manipulation.

### Step
A named action with inputs, outputs, and error handling. Steps execute sequentially or in parallel.

### Variable
Named data available throughout workflow execution: input parameters, step outputs, environment variables, or forge context (current file, database record, user selection).

---

## 4. Workflow Definition Format

### TOML Specification (`.workflow.toml`)

```toml
# Metadata
[workflow]
name = "Daily Journal Creator"
description = "Auto-create and tag journal entry on 9 AM cron"
version = "1.0"
author = "alice@nexus.dev"
tags = ["daily", "journal", "automation"]

# Input parameters
[inputs]
journal_dir = { type = "string", default = "journal/" }
template_file = { type = "string", default = ".templates/journal.md" }

# Trigger definition
[trigger]
type = "cron"
schedule = "0 9 * * *"
timezone = "America/New_York"

# Optional: conditions that must pass for execution
[condition]
type = "time"
weekdays = [1, 2, 3, 4, 5]  # Mon-Fri only

# Steps: sequential by default; use `parallel = true` for concurrency
[[steps]]
name = "CreateFileStep"
type = "file_create"
path = "${journal_dir}/${date:YYYY-MM-DD}.md"
content = "file:${template_file}"  # Load from file
overwrite = false

[[steps]]
name = "TagStep"
type = "db_update"
query = "UPDATE notes SET tags = array_append(tags, 'journal') WHERE path = ?"
params = ["${CreateFileStep.path}"]
on_error = "log_warn"  # Continue even if tag fails

[[steps]]
name = "NotifyStep"
type = "notification"
title = "Journal Entry Created"
body = "Today's entry: ${CreateFileStep.path}"
level = "info"

# Outputs available to caller or next workflow
[outputs]
created_file = { source = "CreateFileStep.path", type = "string" }
timestamp = { source = "steps.NotifyStep.sent_at", type = "timestamp" }

# Optional: error handling policies
[error_handling]
max_retries = 3
retry_backoff = "exponential"
on_step_failure = "stop"  # or "continue", "branch_to_recovery"
recovery_step = null
```

### Workflow File Locations
- User-defined: `{forge}/.workflows/` (version controlled)
- System templates: `{nexus}/workflows/templates/`
- Plugin-provided: `{nexus}/plugins/{plugin}/workflows/`

---

## 5. Trigger System

### Trigger Types

| Type | Event | Example |
|------|-------|---------|
| `file_event` | File created, modified, deleted | Watch `notes/` for new `.md` |
| `db_change` | Record inserted, updated, deleted | New tag added to database |
| `process_event` | Process started, stopped, crashed | LSP server died → restart |
| `git_event` | Commit pushed, PR opened | Auto-tag commits with `#deploy` |
| `cron` | Time-based schedule | Daily 9 AM journal |
| `manual` | User-triggered button | "Run standup generator" |
| `webhook` | HTTP POST from external | GitLab push event → auto-sync |
| `mcp_event` | MCP server event (if subscribed) | New search result available |

### Trigger Filtering and Debouncing

```toml
[trigger]
type = "file_event"
watch_dir = "notes/"
event_types = ["created", "modified"]
glob_pattern = "*.md"
ignore_pattern = ".*.swp"
debounce_ms = 500  # Coalesce rapid events
max_queue = 10     # Max pending triggers before dropping old
```

### Trigger Context Variables

Automatically available in workflow steps:

```
${trigger.file.path}      # Modified file path
${trigger.file.size}      # File size in bytes
${trigger.event.type}     # "created" | "modified" | "deleted"
${trigger.timestamp}      # When event occurred (ISO 8601)
${trigger.user}           # User who triggered (if applicable)
${trigger.mcp_result}     # Result from MCP tool (if MCP trigger)
```

---

## 6. Condition Engine

### Condition Types

```toml
# Type 1: File/Path Condition
[condition]
type = "file_exists"
path = "${journal_dir}/${date:YYYY-MM-DD}.md"

# Type 2: Property Matching
[condition]
type = "property_match"
target = "trigger.event.type"
operator = "=="
value = "modified"

# Type 3: Regex Matching
[condition]
type = "regex_match"
source = "trigger.file.path"
pattern = "notes/.*\.md$"

# Type 4: Time-Based
[condition]
type = "time_range"
after = "09:00"
before = "17:00"
weekdays = [1, 2, 3, 4, 5]

# Type 5: Database Query
[condition]
type = "db_query_result"
query = "SELECT COUNT(*) as cnt FROM notes WHERE tags @> ARRAY['urgent']"
operator = ">"
value = 0

# Type 6: Process Status
[condition]
type = "process_running"
process_name = "lsp-server"
```

### Condition Combinators

```toml
[condition]
type = "and"
conditions = [
  { type = "regex_match", source = "trigger.file.path", pattern = "notes/.*" },
  { type = "time_range", after = "09:00", before = "17:00" }
]

[condition]
type = "or"
conditions = [
  { type = "property_match", target = "trigger.event.type", operator = "==", value = "created" },
  { type = "property_match", target = "trigger.event.type", operator = "==", value = "modified" }
]
```

---

## 7. Action Types

### File Operations

```toml
# Create file
[[steps]]
type = "file_create"
path = "${output_dir}/${filename}"
content = "inline text or file:path/to/template"
permissions = "0644"
overwrite = false

# Read file
[[steps]]
type = "file_read"
path = "${input_file}"
output = "file_content"

# Write/append
[[steps]]
type = "file_write"
path = "${output_file}"
mode = "append"  # or "overwrite", "prepend"
content = "${step_output}"

# Move/copy/delete
[[steps]]
type = "file_move"
source = "${path1}"
destination = "${path2}"
```

### Database Operations

```toml
[[steps]]
type = "db_query"
query = "SELECT * FROM notes WHERE tags @> ARRAY[?]"
params = ["journal"]
output = "query_result"

[[steps]]
type = "db_insert"
table = "notes"
data = { path = "${file_path}", title = "${title}", tags = ["journal"] }
output = "inserted_id"

[[steps]]
type = "db_update"
query = "UPDATE notes SET modified_at = NOW() WHERE id = ?"
params = ["${record_id}"]
output = "rows_affected"

[[steps]]
type = "db_delete"
query = "DELETE FROM notes WHERE tags @> ARRAY['temp']"
on_error = "log_warn"
```

### Terminal Commands

```toml
[[steps]]
type = "exec"
command = "git commit -m 'Auto-commit: ${date:HH:MM}'"
cwd = "${forge_root}"
output = "stdout"
capture_stderr = true
timeout_secs = 30
on_error = "stop"
```

### AI-Enhanced Steps

```toml
[[steps]]
name = "SummarizeStep"
type = "ai_prompt"
prompt = """
Summarize this text in 2-3 sentences:
${file_content}
"""
context = { 
  file = "${current_file}",
  history_lines = 10
}
model = "default"  # Use configured default or specify
max_tokens = 200
temperature = 0.5
output = "summary"
cost_budget = "0.01"  # Max $0.01 per execution
```

### Process Management

```toml
[[steps]]
type = "process_start"
name = "lsp_server"
command = "/usr/bin/rust-analyzer"
args = ["--log-level", "info"]
background = true
output = "pid"

[[steps]]
type = "process_stop"
pid = "${lsp_server.pid}"
signal = "SIGTERM"
timeout_secs = 5
```

### Git Operations

```toml
[[steps]]
type = "git_commit"
message = "${commit_message}"
files = ["${modified_file}"]
allow_empty = false

[[steps]]
type = "git_push"
remote = "origin"
branch = "${current_branch}"
force = false

[[steps]]
type = "git_tag"
tag = "v${version}"
message = "Release ${version}"
```

### HTTP Requests

```toml
[[steps]]
type = "http_request"
method = "POST"
url = "https://hooks.slack.com/services/..."
headers = { "Content-Type" = "application/json" }
body = '{"text": "Workflow completed: ${output}"}'
timeout_secs = 10
output = "response_body"
```

### Notifications

```toml
[[steps]]
type = "notification"
title = "Workflow Alert"
body = "${message}"
level = "info"  # or "warn", "error"
channels = ["system", "ui"]
```

### Variable & Control Flow

```toml
[[steps]]
type = "set_variable"
name = "computed_value"
value = "${step1.output} - ${step2.output}"

[[steps]]
type = "log"
message = "Processing: ${file_path}"
level = "debug"

[[steps]]
type = "wait"
seconds = 5

[[steps]]
type = "run_subworkflow"
workflow = "helper-workflow.workflow.toml"
inputs = { key = "${value}" }
output = "subworkflow_result"
```

---

## 8. Variable System

### Variable Scope and Types

```toml
# Input parameters (provided at workflow trigger)
[inputs]
query = { type = "string", required = true }
limit = { type = "integer", default = 10 }

# Variables scoped to workflow execution
[variables]
session_id = { type = "string", value = "${uuid()}" }
start_time = { type = "timestamp", value = "${now()}" }
```

### Variable Interpolation Syntax

```
${variable_name}              # Direct substitution
${step_name.field}            # Step output access
${trigger.event.type}         # Trigger context
${env.PATH}                   # Environment variables
${now()}                       # Functions: now, uuid, date, range
${date:YYYY-MM-DD}            # Formatted date
${forge.current_file}         # Forge context
${json(step_output, "key")}   # JSON path extraction
${regex(text, pattern, group)}# Regex extraction
```

### Type System

```toml
# Supported types
"string"       # Text
"integer"      # Int64
"float"        # Float64
"boolean"      # True/false
"timestamp"    # ISO 8601
"path"         # File path
"array"        # JSON array
"object"       # JSON object
"bytes"        # Binary data
```

### Forge Context Variables

Automatically available:

```
${forge.root}                # Forge root directory
${forge.current_file}        # File currently open in editor
${forge.current_selection}   # User selection in editor
${forge.database.url}        # Database connection info
${forge.user.name}           # Current user
${forge.user.email}          # Current user email
${forge.build_id}            # Current build identifier
```

---

## 9. Control Flow

### Sequential Execution (Default)

```toml
[[steps]]
name = "step1"
type = "file_read"
path = "input.txt"

[[steps]]
name = "step2"
type = "ai_prompt"
prompt = "Summarize: ${step1.output}"
depends_on = ["step1"]  # Explicit, but default
```

### Parallel Execution

```toml
[[steps]]
name = "parallel_fetch"
parallel = true  # Execute all steps in this array concurrently
timeout_secs = 30

  [[steps.tasks]]
  name = "fetch_api1"
  type = "http_request"
  url = "https://api1.example.com"

  [[steps.tasks]]
  name = "fetch_api2"
  type = "http_request"
  url = "https://api2.example.com"

[[steps]]
name = "merge_results"
type = "set_variable"
value = "${fetch_api1.output} + ${fetch_api2.output}"
```

### Conditional Branching

```toml
[[steps]]
name = "check_condition"
type = "condition_branch"
condition = { type = "db_query_result", query = "SELECT COUNT(*) FROM urgent_notes", operator = ">", value = 0 }

  [[steps.if_true]]
  name = "handle_urgent"
  type = "notification"
  body = "Urgent notes found!"

  [[steps.if_false]]
  name = "handle_normal"
  type = "log"
  message = "No urgent notes."
```

### Loops (For-Each)

```toml
[[steps]]
name = "process_files"
type = "for_each"
items = "${glob('notes/**/*.md')}"  # Or db query result
item_var = "current_file"
max_parallel = 4

  [[steps.body]]
  name = "process_one"
  type = "file_read"
  path = "${current_file}"

  [[steps.body]]
  name = "tag_file"
  type = "db_update"
  query = "UPDATE notes SET processed = true WHERE path = ?"
  params = ["${current_file}"]
```

### Error Handling

```toml
[[steps]]
name = "risky_operation"
type = "exec"
command = "curl https://unreliable.api"
on_error = "continue"  # or "stop", "retry", "skip_to"
max_retries = 3
retry_backoff = "exponential"
retry_initial_delay_ms = 100

[[steps]]
name = "fallback"
type = "log"
message = "API call failed, using cached data"
run_if = { step = "risky_operation", status = "failed" }
```

---

## 10. Workflow Execution Engine

### Trigger and Queue

1. **Trigger fires** → Workflow instance created with trigger context.
2. **Conditions evaluated** → If false, workflow stops.
3. **Queued for execution** with priority (default = NORMAL; user-triggered = HIGH).
4. **Executed** by scheduler based on concurrency limits.

### Execution Model

```
WorkflowInstance {
  id: UUID,
  workflow_id: String,
  state: "pending" | "running" | "paused" | "completed" | "failed" | "cancelled",
  trigger_context: Map<String, Value>,
  variables: Map<String, Value>,
  steps: [StepExecution],
  created_at: Timestamp,
  started_at: Option<Timestamp>,
  completed_at: Option<Timestamp>,
  output: Map<String, Value>,
  error: Option<Error>,
}

StepExecution {
  step_name: String,
  type: ActionType,
  state: "pending" | "running" | "completed" | "failed" | "skipped",
  started_at: Option<Timestamp>,
  duration_ms: u64,
  output: Map<String, Value>,
  error: Option<Error>,
  retry_count: u32,
}
```

### Concurrency Control

```toml
[runtime]
max_concurrent_workflows = 10
max_concurrent_per_trigger = 2    # Max 2 file_event workflows running simultaneously
max_queued = 100                  # Drop older triggers if queue exceeds
step_timeout_default_secs = 300
```

### Cancellation and Pause

```
API:
  POST /workflows/{id}/cancel
  POST /workflows/{id}/pause
  POST /workflows/{id}/resume
  
Effect:
  - Cancel: Stop current step, mark workflow failed
  - Pause: Suspend after current step, resume later
  - Resume: Continue from pause point
```

---

## 11. AI-Enhanced Steps

### Integration with Variable System

```toml
[[steps]]
name = "classify_note"
type = "ai_prompt"
prompt = """
Classify this note as one of: [urgent, important, info, task].
Note: ${file_content}
"""
context = {
  file_path = "${current_file}",
  recent_notes = "SELECT title FROM notes ORDER BY created_at DESC LIMIT 5",
  user_preferences = "SELECT * FROM user_prefs"
}
model = "claude-3-haiku"
max_tokens = 50
temperature = 0.2
output = "classification"

# Next step uses output
[[steps]]
type = "db_update"
query = "UPDATE notes SET category = ? WHERE path = ?"
params = ["${classify_note.output}", "${current_file}"]
```

### Token and Cost Budgets

```toml
[ai_budgets]
per_workflow = { tokens = 10000, cost = "0.10" }
per_step = { tokens = 2000, cost = "0.02" }

[[steps]]
name = "expensive_summarize"
type = "ai_prompt"
prompt = "${content}"
cost_budget = "0.05"  # Override per-step budget
on_budget_exceeded = "truncate"  # or "error", "log_warn"
```

### AI Decision Branches

```toml
[[steps]]
name = "decide_action"
type = "ai_decision"
prompt = """
Should we deploy? Analyze:
- Test results: ${test_output}
- Change set: ${changes}

Respond with JSON: {"decision": "yes" | "no", "confidence": 0.0-1.0}
"""
output = "decision_result"

[[steps]]
name = "execute_decision"
type = "condition_branch"
condition = { type = "property_match", source = "decide_action.decision", operator = "==", value = "yes" }

  [[steps.if_true]]
  type = "exec"
  command = "deploy.sh"

  [[steps.if_false]]
  type = "log"
  message = "Deployment skipped by AI decision"
```

---

## 12. Built-in Workflow Templates

### 1. Daily Journal Creator
Auto-create journal entry at 9 AM, tag, and link to today's standup.

### 2. Git Commit-on-Save
Auto-commit modified files with timestamp message.

### 3. Auto-Tag New Notes
Classify and tag new notes based on content (AI-powered).

### 4. Daily Standup Generator
Extract today's commits, completed tasks, and prompt for next steps.

### 5. Link Checker
Scan all notes for broken links (regex + HTTP check), create report.

### 6. Orphan Note Finder
Find notes with no incoming links, generate removal recommendations.

### 7. Database Backup
Daily encrypted backup of forge database to S3/local storage.

### 8. Research Pipeline
Trigger: manual. Web search (MCP) → summarize → create note → tag.

### 9. PR Review Automation
Trigger: git_event (PR opened). Diff analysis → lint → comment suggestions.

### 10. Deployment Pipeline
Trigger: git tag. Run tests → build → deploy → notify Slack.

**Distribution:** Templates packaged in `{nexus}/workflows/templates/` and available via CLI:
```bash
nexus workflow template list
nexus workflow template init daily-journal
```

---

## 13. Workflow Testing

### Dry-Run Mode

```bash
nexus workflow test daily-journal.workflow.toml \
  --dry-run \
  --mock-trigger '{"type": "cron", "time": "09:00"}' \
  --verbose
```

Output:
```
✓ Step 1: CreateFileStep (file_create)
  - Would create: journal/2026-04-11.md
  - Preconditions: PASS
  - Action: SKIPPED (dry-run)
  
✓ Step 2: TagStep (db_update)
  - Query would affect: 1 rows
  - Variables available: ${CreateFileStep.path}
  
Summary: All steps would execute successfully (0 errors, 0 warnings)
```

### Step-by-Step Debugging

```bash
nexus workflow debug daily-journal.workflow.toml \
  --step-by-step \
  --mock-trigger '...'
```

Interactive mode: step through, inspect variables, modify and re-run.

### Variable Inspection

```bash
nexus workflow test daily-journal.workflow.toml \
  --inspect-variables \
  --after-step CreateFileStep
```

Output: JSON dump of all variables at breakpoint.

---

## 14. Relationship to Agents

### Workflows vs. Agents

| Aspect | Workflow | Agent |
|--------|----------|-------|
| **Planning** | Predefined DAG | Autonomous goal decomposition |
| **Determinism** | Deterministic (optional AI steps) | Non-deterministic (reasoning) |
| **Cost** | Predictable (fixed steps) | Variable (depends on planning depth) |
| **Latency** | Low (no reasoning overhead) | High (reasoning time) |
| **Use Case** | Routine automation | Complex reasoning, exploration |

### Workflows Invoking Agents

```toml
[[steps]]
name = "agent_research"
type = "run_agent"
agent = "research-assistant"
goal = "Find best Rust web frameworks for our use case"
context = {
  project_type = "microservice",
  performance_req = "< 100ms p99"
}
max_steps = 5
output = "agent_result"
```

### Agents Triggering Workflows

```python
# In agent code:
nexus.workflows.trigger(
  workflow="email-summary",
  inputs={"subject": "Research Complete", "body": agent_output}
)
```

---

## 15. Relationship to Process Manager

### Processes as Workflow Actions

Workflows can start/stop/monitor processes (defined in Process Manager subsystem). Difference:
- **Processes:** Long-lived background tasks with independent lifecycle.
- **Workflow steps:** Short-lived actions that complete and pass data to next step.

```toml
[[steps]]
type = "process_start"
name = "background_sync"
command = "/opt/nexus/sync-service"
background = true
output = "pid"

[[steps]]
type = "process_wait"
pid = "${background_sync.pid}"
timeout_secs = 60
output = "exit_code"
```

---

## 16. Plugin Integration

### Custom Trigger Types

Plugins register new trigger types:

```rust
// In plugin:
nexus.workflows.register_trigger_type(
  "custom_webhook",
  TriggerDescriptor {
    schema: json_schema,
    validate: fn(config) -> Result<()>,
    on_trigger: fn(config, nexus) -> TriggerContext,
  }
);
```

### Custom Action Types

```rust
nexus.workflows.register_action_type(
  "call_external_api",
  ActionDescriptor {
    schema: json_schema,
    execute: async fn(config, variables, nexus) -> Result<Map<String, Value>>,
  }
);
```

### Plugin-Provided Templates

```
plugins/
  my-plugin/
    workflows/
      setup-project.workflow.toml
      daily-sync.workflow.toml
```

Discovered and available via `nexus workflow template list`.

---

## 17. Security Model

### Capability Requirements

Workflows operate under principle of least privilege. Dangerous actions require user approval:

```toml
[[steps]]
type = "exec"
command = "rm -rf /"
dangerous = true
requires_approval = true
```

### Approval Workflow

1. Workflow definition includes `requires_approval = true`.
2. User runs workflow → UI prompts before executing dangerous steps.
3. User reviews action, approves, executes.
4. Audit logged: who approved, when, which action.

### Sandboxing Community Workflows

Community-shared workflows (from gallery) run in limited mode:
- File I/O restricted to `{forge}/.community-workflows/`
- No exec unless whitelisted commands.
- No database mutations.
- AI steps capped at low token limits.

Users can opt-in to unrestricted execution by moving workflow to personal directory.

---

## 18. Performance Targets

| Metric | Target |
|--------|--------|
| **Trigger evaluation latency** | < 50 ms (median) |
| **Step execution overhead** | < 10 ms per step |
| **Max concurrent workflows** | 10 (configurable) |
| **Memory per active workflow** | < 5 MB |
| **Workflow startup time** | < 100 ms |
| **Database query in workflow** | < 500 ms (p95) |
| **AI step (haiku model)** | < 2 s (p95) |

### Optimization Strategies
- Lazy variable evaluation (only interpolate used variables).
- Step result caching (avoid re-computation if inputs unchanged).
- Concurrent step execution where possible.
- Trigger coalescing (debounce rapid file events).

---

## 19. Workflow Editor UX

### Visual Workflow Builder

- **Left panel:** Action library (triggers, conditions, actions, templates).
- **Center canvas:** Node-graph view with drag-and-drop.
  - Nodes represent steps.
  - Edges represent data flow and dependencies.
  - Right-click → add condition, parallel steps, error handlers.
- **Right panel:** Step configuration (TOML or form-based).
- **Bottom:** Variable inspector, debug output.

**Shortcuts:**
- `Cmd/Ctrl+S` → Save workflow.
- `Cmd/Ctrl+T` → Test/dry-run.
- `Cmd/Ctrl+D` → Toggle debug view.

### Linear Step Editor

Alternative: text-based TOML editor with:
- Autocomplete for action types and variables.
- Inline type hints and documentation.
- Real-time TOML validation.
- Variable picker (Cmd/Ctrl+Shift+V).

### Test Run Button

```
[Test Run] dropdown
  ├─ Test with mock trigger
  ├─ Test with real trigger
  ├─ Dry-run (no side effects)
  ├─ Debug mode (step-by-step)
  └─ View last execution
```

---

## 20. Workflow Browser and Management

### Workflow List View

- **Columns:** Name, enabled/disabled, trigger type, last run, last result, actions.
- **Sorting:** By name, last run, success rate.
- **Filtering:** By tag, trigger type, status.
- **Actions:** Run, edit, delete, duplicate, export, share.

### Execution History

```
Workflow: daily-journal
├─ 2026-04-11 09:00:15 ✓ Completed in 245 ms
│  └─ Outputs: { "created_file": "journal/2026-04-11.md" }
├─ 2026-04-10 09:00:08 ✓ Completed in 189 ms
├─ 2026-04-09 09:00:22 ✗ Failed: Step "TagStep" timed out
│  └─ Error: Database query exceeded 30s timeout
```

### Workflow Status Indicators

- ✓ Active: Enabled, triggered recently.
- ⏸ Idle: Enabled, not triggered recently.
- ✗ Error: Last execution failed.
- ⚙ Running: Currently executing.

---

## 21. CLI Workflow Commands

```bash
# List workflows
nexus workflow list [--tag journal] [--trigger-type cron]

# Run workflow (with optional inputs)
nexus workflow run daily-journal [--input "key=value"]

# Create new workflow
nexus workflow create my-workflow [--from-template daily-journal]

# Edit workflow (opens editor)
nexus workflow edit daily-journal

# Enable/disable
nexus workflow enable daily-journal
nexus workflow disable daily-journal

# View history
nexus workflow history daily-journal [--limit 20] [--json]

# Test workflow
nexus workflow test daily-journal [--dry-run] [--mock-trigger '...'] [--verbose]

# Delete workflow
nexus workflow delete daily-journal [--confirm]

# Export workflow
nexus workflow export daily-journal --output ./daily-journal.workflow.toml

# Import workflow
nexus workflow import ./daily-journal.workflow.toml [--tag "community"]
```

**Output format:**
- Human-readable (default): Colorized table/tree.
- `--json`: Parseable JSON for scripting.
- `--quiet`: Minimal output (exit code only).

---

## 22. Workflow Sharing and Community Gallery

### Export/Import

```bash
# Export with metadata
nexus workflow export daily-journal --include-metadata

# Imports to {forge}/.workflows/
nexus workflow import ./daily-journal.workflow.toml

# Auto-detect dependencies (plugins, templates)
nexus workflow import ./complex-workflow.workflow.toml --resolve-deps
```

### Community Gallery

- **Central registry:** `workflows.nexus.community` (community-hosted).
- **Browse:** Web UI or CLI `nexus workflow gallery search "journal"`.
- **Install:** `nexus workflow gallery install daily-journal --from "@user/daily-journal"`.
- **Publish:** `nexus workflow publish daily-journal --tag community --license MIT`.

### Metadata in Workflow

```toml
[workflow]
name = "Daily Journal"
version = "1.2.0"
license = "MIT"
author = "alice@nexus.dev"
homepage = "https://github.com/alice/daily-journal"
description = "Creates and tags a journal entry at 9 AM daily"
keywords = ["daily", "journal", "automation", "ai-powered"]
dependencies = ["ai-engine>=1.0"]
requires_approval = false
```

---

## 23. Implementation Roadmap

### Phase 1 (Week 1–2)
- Workflow definition parser (TOML format).
- Trigger system (file_event, cron, manual).
- Basic action types (file_create, file_read, file_write, log).
- Execution engine (sequential steps, error handling).
- CLI: `nexus workflow run/list/create/edit`.

### Phase 2 (Week 3–4)
- Condition engine (property_match, regex_match, time-based).
- Control flow (if/else, for-each, parallel steps).
- Database operations (db_query, db_insert, db_update, db_delete).
- Variable interpolation (${...} syntax, type system).
- Workflow testing (dry-run, debug mode).

### Phase 3 (Week 5–6)
- AI-enhanced steps (ai_prompt, ai_decision).
- Advanced trigger types (git_event, process_event, webhook, mcp_event).
- HTTP actions, notifications, process management.
- Workflow browser UI.
- Built-in templates (5 templates).

### Phase 4 (Week 7–8)
- Visual workflow editor (linear and node-graph modes).
- Workflow execution view (real-time progress).
- Additional templates (5 more).
- Plugin integration (custom triggers, actions).
- Community gallery and publishing.

---

## 24. Acceptance Criteria

### Functional

- [x] Workflows can be defined in TOML format, persisted to disk, and loaded.
- [x] Trigger system evaluates events and initiates workflow execution.
- [x] Condition engine gates execution and branches control flow.
- [x] All 12 action types execute correctly with proper error handling.
- [x] Variable interpolation works for all scopes (inputs, step outputs, forge context).
- [x] Control flow (sequential, parallel, conditional, loops) executes as expected.
- [x] AI steps integrate with variable system and respect token/cost budgets.
- [x] Workflows can be tested via dry-run and step-by-step debug.
- [x] CLI provides full workflow management (list, run, edit, create, delete, history).
- [x] At least 5 built-in templates are functional and discoverable.

### Non-Functional

- [x] Trigger evaluation latency < 50 ms.
- [x] Step execution overhead < 10 ms.
- [x] Max 10 concurrent workflows without degradation.
- [x] Workflow definitions version-controlled in `{forge}/.workflows/`.
- [x] Execution history logged to database and queryable.
- [x] Error messages are clear and actionable.

### Integration

- [x] Workflows integrate with all existing Nexus subsystems (Process Manager, AI Engine, Database, File System, Git).
- [x] Plugins can register custom triggers and actions.
- [x] Agents can trigger workflows; workflows can invoke agents.

---

## 25. Dependencies and Risks

### Dependencies

- **AI Engine:** For `ai_prompt` and `ai_decision` steps.
- **Process Manager:** For process start/stop/status actions.
- **Git Subsystem:** For git operations.
- **Database:** For db_query, db_insert, etc.
- **MCP Layer:** For MCP event triggers and calls.

### Risks

| Risk | Mitigation |
|------|-----------|
| **Cost overrun from AI steps** | Token budgets per workflow/step; cost tracking; alerts. |
| **Uncontrolled concurrency** | Max concurrent workflows limit; queue management. |
| **Infinite loops** | Max iteration limit on for-each; timeout on steps. |
| **Data loss from exec actions** | User approval for dangerous actions; audit log. |
| **Trigger thrashing** | Debounce and rate limiting. |

---

## 26. Success Metrics

- **Adoption:** 50%+ of Nexus users create at least one workflow in first month.
- **Execution:** 10,000+ workflows executed per day across user base.
- **Reliability:** 99.5% workflow success rate (excluding user-caused errors).
- **Performance:** Trigger latency p50 < 30 ms, p95 < 100 ms.
- **Community:** 50+ shared templates in community gallery by month 2.

---

## 27. References and Related Docs

- [PRD-01: Core Architecture](01-core-architecture.md)
- [PRD-02: Process Manager](02-process-manager.md)
- [PRD-08: AI Engine](08-ai-engine.md)
- [PRD-11: Database & Knowledge Store](11-database-knowledge-store.md)
- [Nexus CLI Spec](../specs/cli.md)
- [TOML Format Reference](https://toml.io/en/v1.0.0)

---

**Document Version:** 1.0  
**Last Updated:** April 2026  
**Status:** Implementation Ready
