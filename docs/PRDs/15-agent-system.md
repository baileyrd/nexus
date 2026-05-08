# Agent System PRD — Nexus v1.0

**Version:** 1.0  
**Date:** April 2026  
**Status:** 🟢 Shipped — Substantially Complete (see [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), 2026-04-18)  
**Target Completion:** Q2 2026  
**Subsystem:** Agent System (autonomous, multi-step task execution)  

---

## Executive Summary

The Agent System provides autonomous, goal-driven task execution within Nexus. Built on the AI Engine, agents decompose user goals into step-by-step plans, execute tool actions, observe outcomes, learn from failures, and collaborate with users for approval on high-risk operations. Six built-in archetypes (Coding, Research, Refactor, Documentation, Review, Automation) serve the majority of workflows; users can define custom agents via manifest format.

**Key Design Principles:**
- **Capability-gated tool access:** Agents use the same tool registry as plugins; no special privileges.
- **User-in-the-loop:** Critical actions (file deletion, running `rm -rf`, etc.) require approval before execution.
- **Memory-driven continuity:** Agents persist decisions, context, and artifacts across invocations.
- **Observable execution:** Users see real-time step progress, tool calls, and reasoning.
- **Safe by default:** Token budgets, time limits, and destructive-action guards prevent runaway execution.

---

## 1. Agent Trait Definition

### Core Interface

```rust
#[async_trait]
pub trait Agent: Send + Sync {
    /// Agent metadata: name, version, description.
    fn metadata(&self) -> AgentMetadata;

    /// Initialize the agent for a specific goal and user context.
    async fn init(
        &mut self,
        goal: String,
        user_context: UserContext,
        memory: AgentMemory,
    ) -> Result<()>;

    /// Generate a plan (DAG of steps) from the current goal.
    /// Returns `Plan` or error if planning fails.
    async fn plan(&mut self) -> Result<Plan>;

    /// Execute a single step from the current plan.
    /// Updates internal state; returns `StepResult` (success/failure/pause).
    async fn execute_step(
        &mut self,
        step: &PlanStep,
        budget: &ExecutionBudget,
    ) -> Result<StepResult>;

    /// Process an observation (file change, terminal output, tool result, etc.).
    /// May trigger plan revision or reactive actions.
    async fn observe(&mut self, observation: Observation) -> Result<()>;

    /// Check whether execution should pause (e.g., awaiting user input).
    fn is_paused(&self) -> bool;

    /// Finalize execution: save artifacts, cleanup, return summary.
    async fn complete(&mut self) -> Result<AgentResult>;

    /// Optional: resume from a paused state after receiving user input.
    async fn resume(&mut self, user_input: UserInput) -> Result<()>;
}

pub struct AgentMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub archetype: AgentArchetype,  // Coding | Research | Refactor | Docs | Review | Automation
    pub required_tools: Vec<String>,  // e.g., ["editor", "terminal", "search"]
    pub system_prompt: String,
}

pub enum AgentArchetype {
    Coding,
    Research,
    Refactor,
    Documentation,
    Review,
    Automation,
    Custom,
}

pub struct UserContext {
    pub project_root: PathBuf,
    pub forge_path: PathBuf,
    pub user_name: String,
    pub user_preferences: serde_json::Value,  // Agent-specific config
}

pub enum StepResult {
    Success { output: String },
    Paused { reason: String, required_input: InputPrompt },
    Failed { error: String, can_retry: bool },
    Complete { result: AgentResult },
}

pub struct Plan {
    pub steps: Vec<PlanStep>,
    pub dag_edges: Vec<(usize, usize)>,  // Dependencies between steps
    pub estimated_duration: Duration,
}

pub struct PlanStep {
    pub id: usize,
    pub description: String,
    pub action: StepAction,
    pub dependencies: Vec<usize>,
    pub can_be_skipped: bool,
}

pub enum StepAction {
    ToolCall { tool: String, params: serde_json::Value },
    UserConfirmation { prompt: String, options: Vec<String> },
    Observation { pattern: String },
    Decision { branches: Vec<(String, usize)> },  // Condition -> next step
}
```

---

## 2. Agent Execution Engine

### Scheduler and Task Queue

```rust
pub struct AgentExecutor {
    active_agents: Arc<Mutex<HashMap<String, AgentHandle>>>,
    task_queue: Arc<Mutex<VecDeque<AgentTask>>>,
    resource_monitor: ResourceMonitor,
    tool_registry: Arc<ToolRegistry>,
    max_concurrent_agents: usize,
}

pub struct AgentTask {
    pub task_id: String,
    pub agent_id: String,
    pub goal: String,
    pub user_context: UserContext,
    pub budget: ExecutionBudget,
    pub created_at: Instant,
    pub priority: u8,  // 0-255, higher = more urgent
}

pub struct ExecutionBudget {
    pub token_limit: usize,      // Total tokens for planning + execution
    pub time_limit: Duration,    // Max wall-clock time
    pub tool_call_limit: usize,  // Max tool invocations
    pub max_steps: usize,        // Max plan steps to execute
}

pub struct ResourceMonitor {
    tokens_used: Arc<AtomicUsize>,
    time_started: Instant,
    tool_calls_made: Arc<AtomicUsize>,
    steps_executed: Arc<AtomicUsize>,
}

impl AgentExecutor {
    /// Enqueue an agent task. Returns task_id.
    pub async fn enqueue(&self, task: AgentTask) -> Result<String>;

    /// Execute one step from the next queued agent.
    /// Respects budget constraints. Returns `ExecutionEvent`.
    pub async fn step(&self) -> Result<Option<ExecutionEvent>>;

    /// Run all queued agents to completion (or budget exhaustion).
    pub async fn run_all(&self) -> Result<Vec<AgentResult>>;

    /// Pause a running agent. Agent is moved to paused state.
    pub async fn pause_agent(&self, task_id: &str) -> Result<()>;

    /// Resume a paused agent after user provides input.
    pub async fn resume_agent(&self, task_id: &str, input: UserInput) -> Result<()>;

    /// Cancel a task (cleanup, save state for manual inspection).
    pub async fn cancel_agent(&self, task_id: &str) -> Result<()>;

    /// Get current execution metrics for all active agents.
    pub fn metrics(&self) -> ExecutionMetrics;
}

pub struct ExecutionEvent {
    pub task_id: String,
    pub event_type: EventType,
    pub timestamp: Instant,
    pub details: serde_json::Value,
}

pub enum EventType {
    PlanGenerated { step_count: usize },
    StepStarted { step_id: usize, description: String },
    StepCompleted { step_id: usize },
    ToolCallRequested { tool: String, params: String },
    ToolCallCompleted { tool: String, result: String },
    UserApprovalRequired { reason: String },
    PlanRevised { reason: String },
    AgentPaused { reason: String },
    AgentResumed,
    AgentCompleted { success: bool },
    BudgetExhausted { reason: String },
}
```

### Concurrent Execution Limits

- **Max concurrent agents:** 3 (configurable). Excess agents wait in the task queue.
- **Priority queue:** Tasks with higher priority are executed first.
- **Fair scheduling:** Round-robin among active agents if same priority.

---

## 3. Planning System

### LLM-Based Planning

```rust
pub struct Planner {
    ai_engine: Arc<AIEngine>,
    plan_cache: Arc<Mutex<HashMap<String, Plan>>>,
}

impl Planner {
    /// Generate a plan from a goal using the AI engine.
    /// System prompt guides the agent to decompose into clear steps.
    pub async fn plan_from_goal(
        &self,
        goal: &str,
        context: &PlanContext,
        agent_type: AgentArchetype,
    ) -> Result<Plan>;

    /// Revise a plan after a step fails.
    /// LLM re-analyzes failure and proposes alternative path.
    pub async fn revise_plan(
        &self,
        plan: &mut Plan,
        failed_step_id: usize,
        error: &str,
        max_revisions: usize,
    ) -> Result<bool>;  // true if revised, false if max revisions exhausted

    /// Request user approval for a plan before execution.
    /// Displays the plan; user can approve, reject, or request changes.
    pub async fn request_approval(
        &self,
        plan: &Plan,
        user: &UserContext,
    ) -> Result<ApprovalDecision>;
}

pub struct PlanContext {
    pub project_state: ProjectSnapshot,
    pub recent_decisions: Vec<Decision>,
    pub artifacts_produced: Vec<Artifact>,
    pub constraints: Vec<String>,  // "don't delete files", "must run tests", etc.
}

pub enum ApprovalDecision {
    Approved,
    Rejected { reason: String },
    RequestedChanges { modifications: Vec<PlanModification> },
}

pub struct PlanModification {
    pub step_id: usize,
    pub new_description: Option<String>,
    pub new_action: Option<StepAction>,
    pub insert_before: Option<usize>,
    pub remove: bool,
}

pub struct Decision {
    pub timestamp: Instant,
    pub context: String,
    pub choice: String,
    pub reasoning: String,
}

pub struct Artifact {
    pub path: PathBuf,
    pub kind: String,  // "code", "test", "doc", etc.
    pub created_by_step: usize,
}
```

### Plan Representation

- **DAG structure:** Steps are nodes; dependencies are edges.
- **Critical path:** Planner highlights longest dependency chain.
- **Estimated duration:** Sum of step durations (inferred from history or heuristics).
- **Skippable steps:** Some steps can be skipped if conditions aren't met (e.g., "run tests only if code changed").

---

## 4. Tool Registry for Agents

### Tool Discovery and Invocation

```rust
pub struct AgentToolRegistry {
    tools: Arc<RwLock<HashMap<String, ToolSpec>>>,
    access_log: Arc<Mutex<Vec<ToolAccessRecord>>>,
}

pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Schema,  // JSON Schema
    pub output_schema: serde_json::Schema,
    pub requires_approval: bool,  // true for destructive actions
    pub estimated_duration: Duration,
    pub required_capabilities: Vec<Capability>,  // Checked against agent manifest
}

pub enum Capability {
    FileSystem { read: bool, write: bool, delete: bool },
    Terminal { execute: bool },
    Editor { read: bool, write: bool },
    Search { forge: bool, web: bool },
    Process { spawn: bool, kill: bool },
    Database { read: bool, write: bool },
    WebFetch,
    MCPHost,
}

impl AgentToolRegistry {
    /// Get all tools an agent is authorized to use.
    pub fn list_tools_for_agent(&self, agent: &AgentMetadata) -> Vec<ToolSpec>;

    /// Invoke a tool. May return `Paused` if approval is required.
    pub async fn call_tool(
        &self,
        agent_id: &str,
        tool: &str,
        params: serde_json::Value,
        budget: &ExecutionBudget,
    ) -> Result<ToolResult>;

    /// Validate tool parameters against schema.
    pub fn validate_params(spec: &ToolSpec, params: &serde_json::Value) -> Result<()>;

    /// Parse tool result. Normalize success/error responses.
    pub async fn parse_result(
        spec: &ToolSpec,
        raw: String,
    ) -> Result<ParsedToolResult>;

    /// Retry logic: exponential backoff for transient failures.
    pub async fn call_with_retry(
        &self,
        agent_id: &str,
        tool: &str,
        params: serde_json::Value,
        max_retries: usize,
        budget: &ExecutionBudget,
    ) -> Result<ToolResult>;

    /// Log tool access for audit/debugging.
    fn log_access(&self, record: ToolAccessRecord);
}

pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub parsed: serde_json::Value,
    pub duration: Duration,
    pub tokens_used: usize,
}

pub struct ToolAccessRecord {
    pub timestamp: Instant,
    pub agent_id: String,
    pub tool: String,
    pub success: bool,
    pub duration: Duration,
}

pub enum ParsedToolResult {
    Success { data: serde_json::Value },
    Warning { data: serde_json::Value, message: String },
    Error { code: String, message: String },
    Timeout,
}
```

### Error Handling

- **Tool not found:** Return error immediately; planner revises plan.
- **Tool authorization denied:** Check agent manifest; return error.
- **Transient failure (timeout, network):** Retry up to 3 times with exponential backoff.
- **Permanent failure:** Agent observes failure; planner may revise.

---

## 5. Agent Memory System

### Storage and Retrieval

```rust
pub struct AgentMemory {
    pub agent_id: String,
    pub project_root: PathBuf,
    storage: Arc<Mutex<MemoryStore>>,
}

pub enum MemoryStore {
    FileSystem { base_dir: PathBuf },  // .forge/agents/{agent_id}/
    Database { conn: Arc<rusqlite::Connection> },
}

pub struct MemorySnapshot {
    pub conversation_history: Vec<MemoryEntry>,
    pub decisions_made: Vec<Decision>,
    pub artifacts: Vec<Artifact>,
    pub context: ContextSnapshot,
    pub last_updated: Instant,
}

pub enum MemoryEntry {
    UserGoal { text: String, timestamp: Instant },
    AgentPlan { plan: Plan, timestamp: Instant },
    StepExecution { step_id: usize, result: StepResult, timestamp: Instant },
    ToolCall { tool: String, params: String, result: String, timestamp: Instant },
    UserFeedback { text: String, timestamp: Instant },
    Error { message: String, step_id: usize, timestamp: Instant },
}

pub struct ContextSnapshot {
    pub files_modified: Vec<PathBuf>,
    pub tests_run: Vec<TestResult>,
    pub build_status: Option<BuildResult>,
    pub search_queries: Vec<String>,
}

impl AgentMemory {
    /// Save current execution state to storage.
    pub async fn save_snapshot(&self, snapshot: &MemorySnapshot) -> Result<()>;

    /// Load the most recent snapshot for this agent.
    pub async fn load_latest(&self) -> Result<Option<MemorySnapshot>>;

    /// Query memory: find decisions related to a topic, recent artifacts, etc.
    pub async fn query(
        &self,
        pattern: &str,  // Simple substring or regex
        limit: usize,
    ) -> Result<Vec<MemoryEntry>>;

    /// Prune old entries (older than retention_days).
    pub async fn prune(&self, retention_days: u64) -> Result<usize>;

    /// Export memory for inspection/debugging.
    pub async fn export_markdown(&self) -> Result<String>;
}

pub struct TestResult {
    pub test_name: String,
    pub passed: bool,
    pub output: String,
    pub duration: Duration,
}

pub struct BuildResult {
    pub success: bool,
    pub output: String,
    pub warnings: usize,
    pub errors: usize,
}
```

### Storage Format

- **File system:** Markdown files under `.forge/agents/{agent_id}/`:
  - `snapshots/` — dated snapshots
  - `history.md` — append-only log
  - `artifacts/` — generated files
- **Database:** SQLite table `agent_memory` with columns: `timestamp`, `entry_type`, `content`, `metadata`.

### Memory Retention

- **Conversation history:** 100 most recent entries (or 30 days).
- **Decision log:** All decisions retained indefinitely.
- **Artifacts:** Soft-deleted (moved to `archived/`) after 90 days if unused.

---

## 6. Observation System

### Event Subscriptions

```rust
pub struct ObservationSystem {
    subscriptions: Arc<RwLock<HashMap<String, AgentSubscriptions>>>,
    event_bus: Arc<EventBus>,
}

pub struct AgentSubscriptions {
    pub agent_id: String,
    pub file_patterns: Vec<Regex>,       // Watch for changes to matching files
    pub terminal_patterns: Vec<Regex>,   // Pattern-match terminal output
    pub process_events: bool,            // Watch process exit codes
    pub build_events: bool,              // Watch build output
}

pub enum Observation {
    FileChanged { path: PathBuf, kind: FileChangeKind },
    TerminalOutput { output: String, from_command: String },
    ProcessExit { exit_code: i32, signal: Option<i32> },
    BuildResult { success: bool, output: String },
    TestResult(TestResult),
    ToolResult { tool: String, result: ToolResult },
}

pub enum FileChangeKind {
    Created,
    Modified,
    Deleted,
}

#[async_trait]
pub trait Observer: Send + Sync {
    /// Process an observation. May update agent state or trigger plan revision.
    async fn on_observation(&mut self, obs: Observation) -> Result<()>;

    /// Register reactive rules (e.g., "if test fails, run debugger").
    fn set_reactive_rule(&mut self, trigger: ObservationPattern, action: ReactiveAction);
}

pub enum ObservationPattern {
    FileMatches(String),           // Regex
    TerminalMatches(String),       // Regex (e.g., "error:")
    ProcessExitCode(i32),
    BuildFailed,
    TestFailed { test_name: String },
}

pub enum ReactiveAction {
    RevisePlan,
    SkipStep(usize),
    RerouteTo(usize),              // Jump to different step
    Pause { reason: String },
    Call { tool: String, params: serde_json::Value },
}
```

### Pattern Matching

- **File patterns:** Use gitignore-style globs or regex to match file paths.
- **Terminal output:** Regex on stderr/stdout; case-insensitive by default.
- **Process events:** Match exit code or signal.
- **Build/test events:** Parsed from structured logs (JSON or parseable output).

### Reactive Planning

- **Triggered by:** File change, failing test, compilation error, process crash.
- **Action:** Update current plan, skip steps, jump to error-handling step, or pause for user input.

---

## 7. User Collaboration Protocol

### Approval Workflow

```rust
pub struct UserCollaboration {
    ui_channel: Arc<Mutex<UIChannel>>,
    approval_timeout: Duration,
}

pub enum UIChannel {
    Interactive { stdin: BufReader<Stdin>, stdout: BufWriter<Stdout> },
    WebSocket { ws: WebSocketConnection },
    REST { api_endpoint: String },
}

pub enum UserInteraction {
    Confirmation { prompt: String },
    ChoiceSelection { prompt: String, options: Vec<String> },
    FreeformInput { prompt: String, validator: Option<Regex> },
    ApprovalGate { action: String, reason: String, can_modify: bool },
}

pub struct InputPrompt {
    pub id: String,
    pub prompt_type: UserInteraction,
    pub timeout: Duration,
    pub created_at: Instant,
}

pub enum UserInput {
    Confirmed,
    Rejected { reason: String },
    Selected { index: usize },
    FreeformAnswer { text: String },
    Modified { modifications: Vec<PlanModification> },
    Canceled,
}

impl UserCollaboration {
    /// Request user confirmation for a high-risk action.
    /// Blocks until user responds or timeout.
    pub async fn request_approval(
        &self,
        action: &str,
        reason: &str,
        can_modify: bool,
    ) -> Result<UserInput>;

    /// Request user to choose from options.
    pub async fn request_choice(
        &self,
        prompt: &str,
        options: Vec<String>,
    ) -> Result<usize>;

    /// Request freeform input (e.g., function name, file path).
    pub async fn request_input(
        &self,
        prompt: &str,
        validator: Option<&Regex>,
    ) -> Result<String>;

    /// Send progress update to UI (non-blocking).
    pub async fn report_progress(
        &self,
        step_num: usize,
        step_desc: &str,
        percent_complete: u8,
    );

    /// Display final summary.
    pub async fn show_result(&self, result: &AgentResult);
}
```

### Approval Gate Examples

| Action | Requires Approval | Risk Level |
|--------|-------------------|-----------|
| Read file | No | Low |
| Create file | No | Low |
| Modify file | Yes (show diff) | Medium |
| Delete file | Yes (with confirmation) | High |
| Run `rm -rf` | Yes (explicit) | Critical |
| Run untrusted subprocess | Yes | High |
| Modify `.forge/` | Yes | High |
| Deploy to production | Yes | Critical |

### Progress Reporting

- **Real-time:** Show current step, tool calls in progress, file changes.
- **Estimated time:** Based on historical step durations.
- **Progress bar:** Percent complete, current/total steps.

---

## 8. Built-In Agent Implementations

### 8.1 Coding Agent

**Purpose:** Write, refactor, debug, test code autonomously.

**System Prompt:**
```
You are an expert Rust developer. Your goal is to write high-quality, well-tested code.
Always consider performance, safety, and maintainability. When writing code:
1. Ask for clarification if the requirements are ambiguous.
2. Write tests before implementation (TDD).
3. Review your own code for bugs.
4. Run the linter and address warnings.
5. If tests fail, debug and fix the implementation.
```

**Available Tools:** Editor, Terminal, File System, Search, Process Manager, AI Engine

**Planning Strategy:**
1. Understand requirements (ask user or infer from tests/issues).
2. Locate existing code and identify change points.
3. Write tests for new functionality.
4. Implement changes.
5. Run tests; if fail, debug and iterate.
6. Lint; address warnings.
7. Commit with clear message.

**Example Workflow:**
```
Goal: "Add async file I/O to the editor module"

Plan:
  1. Search for "editor" in codebase
  2. Read editor.rs to understand current impl
  3. Write test for async read_file()
  4. Implement async read_file() using tokio::fs
  5. Run tests
  6. Run linter
  7. Commit with message
```

---

### 8.2 Research Agent

**Purpose:** Gather and synthesize information from web, MCP servers, and internal knowledge.

**System Prompt:**
```
You are a research analyst. Your goal is to find accurate, relevant information.
When researching:
1. Use multiple sources (web, MCP servers, docs).
2. Cross-check facts from different sources.
3. Cite sources.
4. Highlight conflicting information and resolve discrepancies.
5. Summarize findings in clear markdown.
```

**Available Tools:** Web Fetch, Search (Forge + Web), MCP Host, Editor

**Planning Strategy:**
1. Decompose research question into sub-questions.
2. Search for relevant documents/links.
3. Fetch and parse web content.
4. Query MCP servers for additional context.
5. Synthesize findings into a report.
6. Save report to `.forge/research/`.

---

### 8.3 Refactor Agent

**Purpose:** Systematic codebase changes with impact analysis (rename, extract, consolidate).

**System Prompt:**
```
You are an expert code refactorer. Your goal is to improve code structure without changing behavior.
Always:
1. Analyze impact of the change (which files/functions will be affected?).
2. Run tests before and after to ensure behavior is unchanged.
3. Provide a detailed impact report.
4. Create a git commit with a clear message.
```

**Available Tools:** Editor, Terminal, File System, Search, Process Manager

**Planning Strategy:**
1. Identify the refactoring goal (e.g., "extract Config struct").
2. Search for all usages of the target.
3. Plan the changes (impact analysis).
4. Request user approval for high-impact changes.
5. Modify files in dependency order.
6. Run tests to verify no breakage.
7. Commit.

---

### 8.4 Documentation Agent

**Purpose:** Generate/update documentation from code and project context.

**System Prompt:**
```
You are a technical writer. Your goal is to create clear, accurate documentation.
When writing docs:
1. Analyze the code to understand what it does.
2. Write clear explanations with examples.
3. Keep docs up-to-date with code changes.
4. Use markdown and diagrams.
5. Make docs discoverable and searchable.
```

**Available Tools:** Editor, File System, Search, AI Engine

**Planning Strategy:**
1. Identify missing or outdated docs.
2. Analyze the code/feature.
3. Write documentation with examples.
4. Request user review.
5. Commit.

---

### 8.5 Review Agent

**Purpose:** Code/architecture/spec review with actionable feedback.

**System Prompt:**
```
You are a senior code reviewer. Your goal is to provide constructive feedback.
When reviewing:
1. Check for correctness, performance, security issues.
2. Suggest improvements with reasoning.
3. Highlight patterns and best practices.
4. Be specific and actionable.
5. Respect the author's intent.
```

**Available Tools:** Editor, File System, Search, Terminal (for lint/test runs), AI Engine

**Planning Strategy:**
1. Receive code/PR for review.
2. Read the code and understand intent.
3. Run linter and tests.
4. Analyze for issues.
5. Write detailed review comment with suggestions.
6. Save review to `.forge/reviews/`.

---

### 8.6 Automation Agent

**Purpose:** Scripted workflows, CI/CD, forge maintenance.

**System Prompt:**
```
You are a DevOps/automation engineer. Your goal is to create reliable, maintainable automation.
When creating automations:
1. Make scripts idempotent.
2. Handle errors gracefully.
3. Log important events.
4. Test the script before deploying.
5. Document the automation.
```

**Available Tools:** Terminal, Process Manager, File System, Editor, Database

**Planning Strategy:**
1. Understand the workflow.
2. Decompose into steps.
3. Write bash/script that implements each step.
4. Test the script.
5. Deploy (add to CI/CD or `.forge/automation/`).

---

## 9. Custom Agent Definition Format

### Manifest (TOML)

```toml
[agent]
name = "MyCustomAgent"
version = "1.0.0"
description = "Analyze code quality and suggest refactorings."
archetype = "Custom"

[execution]
max_steps = 50
token_budget = 10000
time_limit_secs = 300
requires_approval_for = ["file_delete", "process_execute", "database_write"]

[tools]
# Whitelist of tools this agent can use
allowed = [
    "editor:read",
    "editor:write",
    "search:forge",
    "terminal:execute",
    "ai_engine:call",
]
denied = [
    "database:write",
    "process:kill",
]

[memory]
storage = "filesystem"  # or "database"
retention_days = 90
max_entries = 1000

[system_prompt]
text = """
You are a code quality analyst. Your goal is to identify patterns that could be improved.
Always explain your reasoning. Ask for clarification if needed.
"""
```

### Directory Structure

```
.forge/agents/my-custom-agent/
  SKILL.md              # Agent definition (alternative to TOML)
  system_prompt.txt
  snapshots/
    2026-04-11T10-30.md
  artifacts/
    analysis_2026-04-11.md
  history.md
```

### SKILL.md Format (Alternative)

```markdown
# My Custom Agent v1.0

## Description
Analyzes code quality and suggests refactorings.

## Tools
- editor:read, editor:write
- search:forge
- terminal:execute

## Execution Limits
- Max steps: 50
- Token budget: 10000
- Time limit: 5 min

## System Prompt
You are a code quality analyst...
```

---

## 10. Agent-to-Agent Communication

### Delegation Pattern

```rust
pub struct AgentOrchestrator {
    executor: Arc<AgentExecutor>,
    agents: Arc<HashMap<String, Arc<dyn Agent>>>,
}

impl AgentOrchestrator {
    /// One agent delegates a subtask to another.
    pub async fn delegate(
        &self,
        from_agent: &str,
        to_agent: &str,
        goal: String,
    ) -> Result<DelegationResult>;

    /// Run multiple agents in parallel.
    pub async fn parallel(
        &self,
        agents: Vec<(&str, String)>,  // (agent_id, goal)
    ) -> Result<Vec<AgentResult>>;

    /// Run agents in sequence (output of one feeds into next).
    pub async fn pipeline(
        &self,
        agents: Vec<(&str, String)>,
    ) -> Result<Vec<AgentResult>>;
}

pub struct DelegationResult {
    pub success: bool,
    pub result: AgentResult,
    pub artifacts: Vec<Artifact>,
}
```

### Orchestration Examples

**Parallel:**
```
Refactor Agent: "Rename Config struct"
Documentation Agent: "Update docs for Config"
Review Agent: "Review changes"
→ All run simultaneously
```

**Pipeline:**
```
Research Agent: "Find best Rust async library"
  ↓ (output: recommendation)
Coding Agent: "Integrate tokio into project"
  ↓ (output: updated code)
Review Agent: "Review integration"
```

### Limits

- **Max delegation depth:** 3 levels (prevent infinite recursion).
- **Parallel limit:** 3 agents per orchestration.

---

## 11. Safety and Guardrails

### Destructive Action Prevention

| Action | Guard |
|--------|-------|
| `rm -rf /` | Blocked (absolute path starting with `/`) |
| Delete file | Requires approval + confirmation dialog |
| Kill process | Requires approval + process name verification |
| Drop database table | Requires approval + table name confirmation |
| Modify `.forge/` | Requires approval |
| Run untrusted script | Requires approval + sandbox (if custom agent) |

### Token/Cost Budgets

- **Default token budget per agent run:** 10,000 tokens.
- **Overage handling:** Agent pauses and reports budget exhaustion; user can extend.
- **Cost tracking:** Log tokens used by agent and tool.

### Sandboxing for Community Agents

- **Custom agents (WASM):** Run in WebAssembly VM with restricted system access.
- **Capability-gated tools:** Agents declare required tools in manifest; execution engine validates.
- **No file system access by default:** Agents must explicitly request file read/write via tool calls.

### Approval Required Actions

```rust
pub const APPROVAL_REQUIRED_ACTIONS: &[&str] = &[
    "file:delete",
    "file:move",
    "directory:delete",
    "process:execute",  // If command contains dangerous patterns
    "database:delete",
    "database:modify",
    "forge:modify",
];
```

---

## 12. Agent Debugging

### Step-by-Step Replay

```rust
pub struct AgentDebugger {
    session_id: String,
    history: Vec<DebugFrame>,
}

pub struct DebugFrame {
    pub step_id: usize,
    pub step: PlanStep,
    pub tool_calls: Vec<(String, serde_json::Value)>,  // tool, params
    pub observations: Vec<Observation>,
    pub agent_state: serde_json::Value,
    pub timestamp: Instant,
    pub duration: Duration,
}

impl AgentDebugger {
    /// Load a past agent run.
    pub async fn load_session(session_id: &str) -> Result<Self>;

    /// Replay from a specific step.
    pub async fn replay_from(
        &self,
        step_id: usize,
        modifications: Option<Vec<PlanModification>>,
    ) -> Result<AgentResult>;

    /// Inspect agent state at a specific frame.
    pub fn inspect_frame(&self, frame_num: usize) -> DebugFrame;

    /// Export trace as JSON for analysis.
    pub fn export_trace(&self) -> String;
}
```

### Inspector UI

- **Step tree:** Collapsible list of executed steps.
- **Tool call log:** Tool, params, result, duration.
- **Observations:** Events that triggered reactive actions.
- **Agent state:** Snapshots at each step.
- **Execution timeline:** Duration and dependencies.

---

## 13. Performance Targets

| Metric | Target |
|--------|--------|
| Plan generation latency | < 5 seconds |
| Step execution latency | < 30 seconds (avg) |
| Memory per active agent | < 50 MB |
| Tool call overhead | < 500 ms |
| Max steps per agent run | 100 (configurable) |
| Max concurrent agents | 3 (configurable) |
| Agent resume latency | < 2 seconds |

---

## 14. Versioning

### Agent Definition Versioning

```toml
[agent]
version = "1.0.0"
nexus_min_version = "1.0.0"
```

### Backward Compatibility

- **Memory migration:** When agent definition changes, memory is versioned and migrated if possible.
- **Tool deprecation:** Deprecated tools trigger warnings; agent can mark fallback tools.
- **Manifest versioning:** `.forge/agents/{agent_id}/VERSION` tracks last-used manifest version.

---

## 15. Agent Invocation

### Command Palette

```
> Agent: Run Coding Agent
> Agent: Run Research Agent
> Agent: List Active Agents
> Agent: Cancel Agent
```

### Chat Commands

```
@coding "Write a function to parse JSON"
@research "Find best practices for Rust error handling"
@refactor "Rename Config to AppConfig"
```

### Keyboard Shortcuts

- **Ctrl+Shift+A:** Open agent selector.
- **Ctrl+Shift+R:** Resume paused agent.
- **Ctrl+Shift+X:** Cancel running agent.

### Right-Click Context Menu

- On file: "Review this file" (Review Agent)
- On directory: "Analyze code quality" (Custom agent)
- On terminal: "Debug error" (Coding Agent)

---

## 16. Agent Progress UI

### Real-Time Progress Panel

```
╔════════════════════════════════════════╗
║ Coding Agent: "Add async I/O"          ║
╠════════════════════════════════════════╣
║ Step 2/7: Implement async read_file()  ║
║ ████████░░░░░░░░░░░░░░░░░░░░  28%     ║
║ Est. time: 45 seconds (18s elapsed)    ║
║                                        ║
║ Recent tool calls:                     ║
║  ✓ editor:read (3.2s)                  ║
║  ⟳ terminal:execute (running...)       ║
║                                        ║
║ File changes:                          ║
║  M editor.rs                           ║
║  + editor_async_test.rs                ║
╚════════════════════════════════════════╝
```

### File Changes View

- **Real-time diffs:** Show file modifications as they happen.
- **Artifact explorer:** Browse generated code, docs, test results.

---

## 17. Agent Approval Flow

### High-Risk Action Dialog

```
╔═══════════════════════════════════════════════════╗
║ ⚠️  Action Requires Approval                      ║
╠═══════════════════════════════════════════════════╣
║ Action: Delete file                               ║
║ File: src/deprecated.rs                           ║
║ Reason: Cleanup: removing deprecated module       ║
║                                                   ║
║ Preview:                                          ║
║   [Show diff / Show file content]                 ║
║                                                   ║
║ [✓ Approve] [✗ Reject] [Modify plan]            ║
╚═══════════════════════════════════════════════════╝
```

### Modify Plan UI

- Display current plan.
- Allow user to:
  - Skip steps.
  - Reorder steps.
  - Add constraints (e.g., "don't delete anything").
  - Override tool parameters.

---

## 18. Agent History

### Session Browser

```
Recent Agent Runs:
┌─────────────────────────────────────────┐
│ 2026-04-11 14:32                        │
│ Coding Agent: "Add async file I/O"      │
│ Status: ✓ Completed (3min 24sec)        │
│ Files changed: 3                        │
│ Tests: 5 passed                         │
│ Artifacts: editor_async.rs, tests       │
└─────────────────────────────────────────┘
│ 2026-04-11 13:15                        │
│ Research Agent: "JWT best practices"    │
│ Status: ✓ Completed (1min 12sec)        │
│ Artifacts: research_report.md           │
└─────────────────────────────────────────┘
```

### Session Details View

- **Goal:** What the user asked for.
- **Plan:** Steps executed.
- **Tool calls:** Complete log with parameters and results.
- **Files modified:** Diffs.
- **Artifacts:** Generated files.
- **Decisions:** User choices made.
- **Errors:** Any failures and how they were resolved.
- **Re-run:** Option to re-run with modifications.

---

## Acceptance Criteria

- [x] Agent trait and execution engine implemented.
- [x] Planning system with DAG and LLM-based decomposition.
- [x] Tool registry for capability-gated access.
- [x] Memory system (FS or DB storage) with query/prune.
- [x] Observation system with pattern matching and reactive rules.
- [x] User collaboration protocol (approval gates, progress reporting).
- [x] Spec for all 6 built-in agents.
- [x] Custom agent manifest format and validator.
- [x] Agent-to-agent delegation (orchestrator).
- [x] Safety guardrails and destructive action prevention.
- [x] Debugger with replay and trace export.
- [x] Performance targets defined and testable.
- [x] UI mockups for invocation, progress, approval, history.

## Dependencies

- **AI Engine (subsystem 07):** LLM-based planning.
- **Tool Registry (subsystem 04):** Tool discovery and invocation.
- **Forge (subsystem 01):** Memory storage, artifact persistence.
- **Editor (subsystem 02):** File read/write.
- **Terminal (subsystem 03):** Process execution, output observation.
- **Web Module:** MCP host, web fetch.

## Timeline

- **Week 1:** Agent trait, execution engine, tool registry.
- **Week 2:** Planning system, memory system.
- **Week 3:** Observation system, user collaboration protocol.
- **Week 4:** Built-in agents, custom agent manifest.
- **Week 5:** Agent orchestration, safety guardrails, debugger.
- **Week 6:** UI implementation (progress, approval, history).
- **Week 7:** Integration tests, performance tuning, documentation.

---

**Version:** 1.0  
**Status:** Ready for Implementation  
**Last Updated:** April 2026
