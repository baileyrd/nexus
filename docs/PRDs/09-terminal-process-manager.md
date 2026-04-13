# PRD: Terminal & Process Manager Subsystem

**Nexus: Rust-based, AI-native Developer Knowledge Environment**  
Version 1.0 | April 2026 | Status: Implementation-Ready

---

## Overview

The Terminal & Process Manager is a **programmable plugin surface** and first-class workspace citizen. Developers and AI agents read from, write to, and orchestrate terminal sessions. The subsystem unifies:
- **Terminal (PTY):** Portable, multi-session PTY with ANSI/256-color/TrueColor support
- **Process Manager (nexusforge.procmgr):** Persistent, governed management of long-running processes with lifecycle state machine, signal hierarchy, memory tracking, and auto-restart

Together, they enable AI to understand developer intent, propose commands, execute workflows, and learn from output patterns.

---

## 1. PTY Architecture

### 1.1 Portable-PTY Integration

**Dependency:** `portable-pty` (Rust cross-platform PTY library)

**Architecture:**
- Single PTY instance per named terminal session
- PTY spawned on session creation, size synchronized with UI viewport
- Terminal file descriptor held open for session lifetime
- CLOEXEC flag on PTY to prevent child process inheritance issues

**PTY Lifecycle:**
```
CreateSession
  → Detect shell
  → Allocate PTY (pty_system.openpty())
  → Spawn process (pty_system.spawn_child())
  → Attach reader/writer threads
  → Begin output capture loop
  → Return session handle
```

### 1.2 Shell Detection Algorithm

**Priority order (checked on session creation):**

1. **Explicit override:** User specifies shell in session config → use it
2. **Environment:** `SHELL` env var → validate executable exists
3. **User database:** `/etc/passwd` entry (Unix) → extract login shell
4. **Fallback:** `/bin/bash` (Unix) or `cmd.exe` (Windows)

**Validation:** For each candidate, verify file exists and is executable. Skip if not.

**Windows specifics:**
- Detect PowerShell vs cmd.exe via registry or `pwsh.exe --version`
- Support Windows Terminal ConPTY for enhanced color/mouse support

### 1.3 Shell Profile Loading

**Problem:** Login shells load `.bashrc`, `.zshrc`, etc.; non-login shells don't. Need both interactive features (aliases, functions) and env vars.

**Solution:**
- Explicitly source profile on session init:
  ```bash
  # For bash: source ~/.bashrc if interactive
  # For zsh: source ~/.zshrc
  # Capture env: set | grep KEY=
  ```
- In process manager: pre-commands run *after* profile load to inherit user environment

**Implementation:**
- Send `source ~/.bashrc; env` as initial shell command
- Parse output to build baseline environment
- Merge with Nexus-injected vars (see §8)

### 1.4 TERM Environment Variable Strategy

**Default:** `TERM=xterm-256color`

**Rationale:** Supports 256-color ANSI sequences (sufficient for most dev tools), widely compatible.

**User override:** Allow in session settings for tools requiring specific TERM (e.g., `TERM=screen-256color` for tmux compatibility).

**Verification:** Query `infocmp $TERM` to confirm terminfo database supports color depth.

---

## 2. Terminal Session Management

### 2.1 Session Creation & Lifecycle

**Session object** (persisted in SQLite):
```
sessions (
  id TEXT PRIMARY KEY,
  name TEXT,                   -- user-friendly: "build", "dev", "ai-shell"
  slug TEXT UNIQUE,            -- URL-safe identifier
  shell TEXT,                  -- /bin/bash, /bin/zsh, cmd.exe, pwsh.exe
  working_dir TEXT,            -- cwd for spawned shell
  created_at INTEGER,          -- Unix timestamp
  last_accessed_at INTEGER,    -- for LRU eviction
  is_active BOOLEAN,           -- true if PTY alive
  buffer_size_bytes INTEGER    -- ring buffer capacity
)
```

**Session state transitions:**
```
Created → Initialized (PTY spawned) → Active (reading input/output)
         ↓
       Closed (PTY freed, session kept for history)
         ↓
       Reopened (new PTY allocated, same history)
```

### 2.2 Persistence Across Restarts

**On shutdown:**
- Save session metadata to SQLite (name, slug, working_dir, shell, buffer)
- Write ring buffer to disk: `~/.nexus/sessions/{session_id}/scrollback.bin` (binary, ANSI-inclusive)
- Record last position in buffer

**On startup:**
- Load session list from database
- Do NOT auto-spawn PTY; wait for user to click/focus session tab
- Lazy-load scrollback buffer on first tab click
- Restore PTY with same shell/working_dir

**Session state serialization:**
```json
{
  "id": "sess_abc123",
  "name": "build",
  "slug": "build",
  "shell": "/bin/bash",
  "working_dir": "/home/user/project",
  "buffer_path": "~/.nexus/sessions/sess_abc123/scrollback.bin",
  "buffer_offset": 1024,
  "is_active": false
}
```

### 2.3 Max Sessions Limit

**Hard limit:** 50 active sessions per workspace

**Eviction policy (LRU):**
- Track `last_accessed_at` for each session
- When creating new session and at limit: close oldest unused session
- Preserve history on disk (scrollback.bin); allow reopening
- Warn user before eviction

---

## 3. Output Ring Buffer

### 3.1 Buffer Sizing

**Per-process buffer:**
- Default: 10 MB (10_000_000 bytes)
- Configurable per saved command (user can increase for long-running builds)

**Global cap:** 500 MB across all active processes
- Monitor total memory; pause new process output if exceeded
- Implement backpressure: read thread blocks until buffer space freed

**Memory efficient storage:**
- Store lines as variable-length records: `[u32 len][u8* data][u32 timestamp]`
- ANSI sequences stored inline (not stripped)
- Deduplicate repeated identical lines (common with spinners, progress bars)

### 3.2 ANSI Parsing & Storage

**Storage strategy:** Store raw output (with ANSI codes); parse on render.

**Why:** Preserves exact terminal state; allows re-rendering at different color depths or font sizes.

**ANSI parsing library:** `anyhow` crate or custom parser
- Track state machine: current color, attributes (bold, underline, etc.)
- Detect SGR sequences (e.g., `\x1b[31m` for red)
- Handle 256-color (`\x1b[38;5;Nm`) and TrueColor (`\x1b[38;2;R;G;Bm`)
- Parse control sequences (cursor move, erase, etc.) but ignore for text search

**Storage format (per line):**
```
Line {
  timestamp: u64,
  raw_bytes: Vec<u8>,        // includes ANSI codes
  text_only: String,         // stripped codes, for search/FTS
  length: usize,             // byte length
}
```

### 3.3 Efficient Text Search Over Buffer

**Search implementation:**
- **Exact match:** Linear scan over `text_only` field (ANSI-stripped)
- **Regex:** Compile regex once; apply to each line's `text_only`
- **FTS5 (future):** Index lines to SQLite FTS table; enable complex queries

**Current (v1.0):**
```rust
fn search_output(buffer: &RingBuffer, query: &str, is_regex: bool) -> Vec<usize> {
  let lines = buffer.lines();
  if is_regex {
    let re = Regex::new(query).unwrap();
    lines.iter()
      .enumerate()
      .filter(|(_, line)| re.is_match(&line.text_only))
      .map(|(idx, _)| idx)
      .collect()
  } else {
    lines.iter()
      .enumerate()
      .filter(|(_, line)| line.text_only.contains(query))
      .map(|(idx, _)| idx)
      .collect()
  }
}
```

**Performance target:** < 100 ms for 100k-line buffer on modern hardware.

### 3.4 Memory Pressure Handling

**Monitoring:** Background task polls total buffer memory every 1s.

**Actions on pressure:**
1. **Soft limit (75%):** Log warning, notify user in UI
2. **Hard limit (100%):** Pause process output, buffer read thread blocks
3. **Eviction:** Drop oldest 10% of buffer; emit `ProcessOutputDropped` event
4. **User action:** Clear buffer via UI button, resume process

---

## 4. Process Lifecycle State Machine

### 4.1 State Diagram

```
┌─────────┐
│ Stopped │─────────────────────────┐
└────┬────┘                         │
     │ user clicks "Run"            │
     ├──→ PreCommand (execute pre-commands)
         │ all succeed?
         ├─ yes ──→ ┌────────┐
         │          │Starting│
         └─ no  ──→ └────┬───┘
                        │ spawn process
                        ↓
                    ┌────────┐
                    │ Running│
                    └────┬───┘
                         │
            ┌────────────┼────────────┐
            │            │            │
       exit 0         exit !0    user closes
            │            │            │
            ↓            ↓            ↓
        Stopped     ┌────────┐    Stopped
                    │Crashed │
                    └───┬────┘
                        │
                  auto_restart?
                   /          \
                 yes          no
                  │             │
                  ↓             ↓
            Restarting      Stopped
                  │
              [delay]
                  │
                  ↓
            Running
```

### 4.2 State Definitions & Transitions

| State | Definition | Transitions |
|-------|-----------|------------|
| **Stopped** | No process running; may have history | → PreCommand (user run), → Deleted |
| **PreCommand** | Executing pre-commands in sequence | → Starting (success), → Stopped (failure) |
| **Starting** | Process spawned, initial handshake | → Running (PTY ready), → Crashed (PTY fail) |
| **Running** | Process active, reading output | → Stopped (clean exit), → Crashed (abnormal exit) |
| **Crashed** | Process exited abnormally | → Restarting (if auto_restart=true), → Stopped (manual reset or auto_restart=false) |
| **Restarting** | Waiting for auto-restart delay | → Running (delay elapsed, new spawn), → Stopped (user cancels) |

### 4.3 Timeout & Error Handling

**Pre-command execution:**
- **Timeout per step:** 30s (user-configurable)
- **On timeout:** Kill step process (SIGTERM), emit `ProcessStepTimedOut`, transition to Stopped
- **On failure:** Record error, emit `ProcessPreCommandFailed`, transition to Stopped

**Process startup:**
- **Timeout:** 5s waiting for PTY ready signal (first output or response to input)
- **On timeout:** Assume shell ready; proceed to Running
- **On error:** Emit `ProcessStartFailed`, transition to Crashed

**Auto-restart delays:**
- **First restart:** 2s
- **Second restart:** 5s
- **Third+ restarts:** 10s (exponential backoff, capped)
- **Max retries:** 10 (user-configurable); after this, transition to Stopped

### 4.4 Pre-Command Execution Pipeline

**Purpose:** Run setup commands before main process (e.g., source virtual env, change directory, export vars).

**Execution:**
1. Iterate over `pre_commands` list in order
2. For each command:
   - Spawn in same shell as main process (inherit profile)
   - Set timeout (30s default)
   - Capture stdout/stderr to process output buffer
   - If exit code != 0: emit `ProcessPreCommandFailed`, abort sequence, transition to Stopped
3. If all succeed: proceed to Starting state

**Example:**
```yaml
command: pytest
pre_commands:
  - "cd /home/user/project"
  - "source venv/bin/activate"
  - "export CI=1"
```

**What if pre-command fails?**
- User sees error output in process panel
- "Run" button remains active; user can fix issue and retry
- Process never reaches Running state

---

## 5. Signal Handling & Process Termination

### 5.1 Unix Signal Escalation

**Hierarchy (5s between each):**

1. **SIGINT (Ctrl+C)** — 5s grace period
   - Most graceful; allows cleanup
   - Sent to process group: `kill(-pgid, SIGINT)`
   
2. **SIGTERM** — 5s grace period
   - Process group: `kill(-pgid, SIGTERM)`
   
3. **SIGKILL** — Forceful termination
   - No grace period
   - Process group: `kill(-pgid, SIGKILL)`

**Implementation:**
```rust
async fn escalate_kill(pid: Pid, start_time: Instant) {
  match elapsed {
    0..5s => {
      signal::kill(Pid::from_raw(-pid.as_raw()), Signal::SIGINT)?;
      emit_event(ProcessSignalSent { pid, signal: "SIGINT" });
    }
    5s..10s => {
      signal::kill(Pid::from_raw(-pid.as_raw()), Signal::SIGTERM)?;
      emit_event(ProcessSignalSent { pid, signal: "SIGTERM" });
    }
    10s..∞ => {
      signal::kill(Pid::from_raw(-pid.as_raw()), Signal::SIGKILL)?;
      emit_event(ProcessSignalSent { pid, signal: "SIGKILL" });
    }
  }
}
```

**Process group setup:** On process spawn, call `setsid()` to create new session (isolated from parent). Child processes inherit session; kill by negative PID kills whole tree.

### 5.2 Child Process Tree Killing

**Problem:** Parent process spawns children; killing parent doesn't kill children.

**Solution (Unix):**
- Parent calls `setsid()` on startup → creates new session
- Kill using `-pgid` (negative PID) → sends signal to entire process group
- Shell spawned processes inherit session by default

**Verification:**
```bash
ps -o sid= -p $PID  # get session ID
ps -o sid= | grep $SID  # list all processes in session
```

**Implementation:**
```rust
let child = Command::new("bash")
  .process_group(0)  // Create new process group
  .spawn()?;
let pgid = Pid::from_raw(child.id() as i32);

// Later, kill entire tree:
signal::kill(Pid::from_raw(-pgid.as_raw()), Signal::SIGTERM)?;
```

### 5.3 Windows Job Objects

**Windows alternative to process groups:**

**Job object lifecycle:**
1. Create job: `CreateJobObject(NULL, NULL)`
2. Assign process: `AssignProcessToJobObject(hJob, hProcess)`
3. Set limits: `SetInformationJobObject(hJob, JobObjectBasicLimitInformation, ...)`
4. Kill all in job: `TerminateJobObject(hJob, exit_code)`

**Advantage:** Automatic child process assignment; no inheritance issues.

**Implementation (winapi):**
```rust
let job = unsafe {
  winapi::um::jobapi2::CreateJobObjectA(std::ptr::null_mut(), std::ptr::null())
};
unsafe {
  winapi::um::jobapi2::AssignProcessToJobObject(job, process_handle);
  winapi::um::jobapi2::TerminateJobObject(job, 1);
}
```

### 5.4 Memory-Efficient Signal Delivery

**Polling thread** (background):
- Every 100ms, check processes awaiting termination
- Apply escalation logic (current time vs. start_kill_time)
- Send appropriate signal

**Alternative:** Use OS event-driven API (epoll on Linux, IOCP on Windows) but polling simpler for v1.0.

---

## 6. URL Auto-Detection & Handling

### 6.1 URL Detection Regex Patterns

**Comprehensive regex (matches http/https, file://, localhost):**

```regex
(?:https?|file)://[^\s\)\]\}]*
|localhost:\d+[^\s\)\]\}]*
|file://[^\s\)\]\}]*
```

**Patterns by category:**

| Category | Regex | Example |
|----------|-------|---------|
| HTTP/HTTPS | `https?://[^\s\)]+` | `http://example.com:3000/api` |
| Localhost | `localhost:(\d+)` | `localhost:8080` |
| File paths | `file://[^\s]+` | `file:///home/user/app.js:42` |
| Port mapping | `(\d{4,5})/` | `3000/` (requires manual click) |

**Refinements:**
- Exclude trailing punctuation (`.`, `,`, `;`, `)`, `]`)
- Handle URL-encoded characters (`%20`, etc.)
- Detect ANSI color codes *before* URL boundaries

### 6.2 Localhost Port Mapping

**Problem:** Many dev tools print `http://localhost:3000` but browser can't resolve.

**Solution:** Map to `http://127.0.0.1:3000` (loopback IP).

**Implementation:**
```rust
fn resolve_url(url: &str) -> String {
  url.replace("localhost:", "127.0.0.1:")
}
```

**In UI:** Detected URLs rendered as clickable links; onClick → open in browser (platform-specific: `open`, `xdg-open`, `start`).

### 6.3 Clickable URL Rendering

**Storage:** Tag each detected URL in buffer with position and resolved target.

**Render:**
```
Output line: "Server started at http://localhost:3000"
Parsed:      "Server started at <a href="http://127.0.0.1:3000">http://localhost:3000</a>"
```

**Performance:** Run URL detection on output lines incrementally (not entire buffer at once).

---

## 7. Memory Monitoring

### 7.1 Per-Process Memory Tracking

**Platform APIs:**

**Unix (/proc/pid/status):**
```bash
cat /proc/1234/status | grep VmRSS
# Output: VmRSS:        12345 kB
```

**Parse RSS (resident set size) in kB; convert to bytes.**

**Windows (GetProcessMemoryInfo):**
```rust
let mut pmc: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
GetProcessMemoryInfo(hProcess, &mut pmc as *mut _, std::mem::size_of_val(&pmc) as u32)?;
let rss_bytes = pmc.WorkingSetSize;
```

### 7.2 Polling Interval & Strategy

**Polling frequency:** Every 1s per active process.

**Implementation:**
```rust
tokio::spawn(async move {
  let mut interval = tokio::time::interval(Duration::from_secs(1));
  loop {
    interval.tick().await;
    if let Ok(memory) = get_process_memory(pid) {
      emit_event(ProcessMemoryUpdate { pid, memory_bytes: memory });
    }
  }
});
```

**Store history:** Keep 60-second rolling window of memory samples.

### 7.3 Memory Limit Enforcement & Alerting

**Soft limits (warning):**
- 250 MB per process → emit warning event, display yellow indicator in UI

**Hard limits (kill):**
- 500 MB per process → emit alert, kill process, transition to Crashed
- Configurable per saved command

**User experience:**
1. Process exceeds 250 MB → yellow badge with memory indicator
2. Process exceeds 500 MB → red badge, auto-kill, show crash notification
3. User can adjust limit in command editor

**Example config:**
```yaml
command: docker build
memory_limit_mb: 1000  # Allow 1 GB for resource-heavy builds
```

---

## 8. Environment Variable System

### 8.1 Resolution Algorithm (Precedence)

**Order (highest to lowest priority):**

1. **Per-command env_vars** (user-defined overrides)
   ```yaml
   env_vars:
     DEBUG: "1"
     API_KEY: "${NEXUS_API_KEY}"  # interpolation support
   ```

2. **.env file** (project root)
   - Files checked in order: `.env.local`, `.env.development`, `.env`
   - Parsed line-by-line; `KEY=value` format
   - Support quotes: `KEY="value with spaces"`

3. **Shell environment** (inherited from login shell)
   - Source shell profile (bashrc/zshrc) during session init
   - Export PATH, HOME, USER, etc.

4. **Nexus-injected vars** (lowest priority, allows override)
   - `NEXUS_WORKSPACE`: current workspace path
   - `NEXUS_PROJECT`: current project folder
   - `NEXUS_SESSION_ID`: session identifier
   - `NEXUS_THEME`: "light" or "dark" (if relevant)

**Resolution code:**
```rust
fn resolve_env(
  command_env: &HashMap<String, String>,
  env_file_path: &Path,
  shell_env: &HashMap<String, String>,
  nexus_injected: &HashMap<String, String>,
) -> HashMap<String, String> {
  let mut result = nexus_injected.clone();
  result.extend(shell_env.clone());
  
  if let Ok(parsed) = parse_env_file(env_file_path) {
    result.extend(parsed);
  }
  
  result.extend(command_env.clone());
  
  // Interpolate variables
  interpolate_vars(result)
}
```

### 8.2 .env File Parsing

**Supported formats:**

```
# Comments start with #
KEY1=value1
KEY2="value with spaces"
KEY3='single quotes'
KEY4=${HOME}/relative/path
KEY5=$HOME/also/works

# Multiline (if supported in v1.0)
# KEY6="line1\
# line2"
```

**File selection order (per-command):**
- If `env_file` specified in command config → use only that
- Otherwise, search working directory for: `.env.local` → `.env.development` → `.env`

**Parser implementation:**
```rust
fn parse_env_file(path: &Path) -> Result<HashMap<String, String>> {
  let contents = std::fs::read_to_string(path)?;
  let mut env = HashMap::new();
  
  for line in contents.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
    
    if let Some((key, value)) = trimmed.split_once('=') {
      let value = value
        .trim_matches('"')
        .trim_matches('\'');
      env.insert(key.to_string(), value.to_string());
    }
  }
  
  Ok(env)
}
```

### 8.3 Variable Interpolation & Secret Masking

**Interpolation:** Support `${VAR}` and `$VAR` syntax.

```rust
fn interpolate_vars(env: &HashMap<String, String>) -> HashMap<String, String> {
  let mut result = env.clone();
  let max_iterations = 10; // Prevent infinite loops
  
  for _ in 0..max_iterations {
    let before = result.clone();
    
    for (key, value) in result.iter_mut() {
      let re = Regex::new(r"\$\{?(\w+)\}?").unwrap();
      *value = re.replace_all(value, |caps: &Captures| {
        let var_name = &caps[1];
        result.get(var_name)
          .map(|s| s.as_str())
          .unwrap_or(&caps[0])
      }).to_string();
    }
    
    if result == before { break; }
  }
  
  result
}
```

**Secret masking in UI:**
- Detect keys matching patterns: `*API*`, `*KEY*`, `*SECRET*`, `*TOKEN*`, `*PASSWORD*`
- In process panel & logs, replace value with `[REDACTED]`
- Never log full environment to disk

---

## 9. Compound Command Splitting & Execution

### 9.1 Parser for Operator Chains

**Supported operators:**
- `&&` (AND) — next command runs only if previous exit code = 0
- `||` (OR) — next command runs only if previous exit code ≠ 0
- `;` (SEQ) — next command always runs, regardless of previous exit code

**Parser output:** List of (operator, command) tuples.

```rust
#[derive(Debug)]
struct CommandStep {
  operator: Operator,  // AND, OR, SEQ
  command: String,
}

#[derive(Debug, Clone, Copy)]
enum Operator {
  And,  // &&
  Or,   // ||
  Seq,  // ;
}

fn parse_command_chain(input: &str) -> Vec<CommandStep> {
  let mut steps = vec![];
  let parts = Regex::new(r"(&&|\|\||;)")
    .unwrap()
    .split(input);
  
  let mut operator = Operator::Seq; // First step has no operator
  for (i, part) in parts.enumerate() {
    if i % 2 == 0 {
      // Command
      steps.push(CommandStep {
        operator,
        command: part.trim().to_string(),
      });
    } else {
      // Operator
      operator = match part {
        "&&" => Operator::And,
        "||" => Operator::Or,
        ";" => Operator::Seq,
        _ => unreachable!(),
      };
    }
  }
  
  steps
}
```

### 9.2 Step Visualization & Error Propagation

**UI display:**
```
┌─ Step 1: npm install          [✓ done in 2s]
├─ Step 2: npm run build        [⏳ running...]
│   └─ Operator: &&
├─ Step 3: npm run test
│   └─ Operator: &&
└─ Step 4: git push
    └─ Operator: &&
```

**Execution:**
```rust
async fn execute_command_chain(steps: Vec<CommandStep>, shell: &str) -> Result<()> {
  for step in steps {
    let should_run = match step.operator {
      Operator::Seq => true,
      Operator::And => exit_code == 0,
      Operator::Or => exit_code != 0,
    };
    
    if !should_run {
      emit_event(ProcessStepSkipped { step_idx, reason: "operator condition" });
      continue;
    }
    
    emit_event(ProcessStepStarting { step_idx, command: step.command.clone() });
    exit_code = execute_in_shell(shell, &step.command).await?;
    emit_event(ProcessStepCompleted { step_idx, exit_code });
  }
  
  Ok(())
}
```

### 9.3 cd Detection for Single-Shell Mode

**Problem:** `cd` is a shell built-in; spawning subprocesses loses directory changes.

**Solution:** Run entire command chain in single shell session (don't spawn per-step).

**Detection:**
```rust
fn should_use_single_shell(steps: &[CommandStep]) -> bool {
  steps.iter().any(|step| {
    step.command.trim_start().starts_with("cd ")
      || step.command.trim_start().starts_with("pushd ")
  })
}
```

**Implementation:**
- If `cd` detected: spawn shell once, send all commands via stdin
- If no `cd`: can spawn per-step (cleaner isolation)

---

## 10. Ad-Hoc Command System

### 10.1 History Storage & Run Count Tracking

**Table:**
```sql
CREATE TABLE procmgr_adhoc_history (
  id TEXT PRIMARY KEY,
  command TEXT,
  working_dir TEXT,
  executed_at INTEGER,  -- Unix timestamp
  exit_code INTEGER,
  duration_ms INTEGER,
  run_count INTEGER     -- total times this exact command run
);
```

**Flow:**
1. User types command in input bar (not a saved command)
2. Hit Enter → spawn process
3. On exit: INSERT into `procmgr_adhoc_history` with `run_count = 1`
4. User runs same command again → UPDATE `run_count += 1`

**Deduplication:** Group by `(command, working_dir)`; increment `run_count` if exact match found.

### 10.2 Promotion Flow to Saved Commands

**UI gesture:**
1. Right-click on adhoc command in history
2. Select "Save as Command"
3. Dialog opens:
   - Command name (editable, auto-fill with command snippet)
   - Working directory (pre-filled from exec context)
   - Environment variables (empty, user can add)
   - Icon picker
   - Auto-restart toggle
4. Click "Create" → INSERT into `procmgr_commands` table

**Implementation:**
```rust
async fn promote_adhoc_to_saved(
  adhoc_id: &str,
  name: &str,
  icon: Option<&str>,
) -> Result<()> {
  let adhoc = db.query_one::<AdHocRecord>(
    "SELECT * FROM procmgr_adhoc_history WHERE id = ?",
    [adhoc_id],
  )?;
  
  let saved = SavedCommand {
    slug: name.to_lowercase().replace(" ", "_"),
    name: name.to_string(),
    shell_cmd: adhoc.command.clone(),
    working_dir: adhoc.working_dir.clone(),
    icon: icon.unwrap_or("terminal").to_string(),
    ..Default::default()
  };
  
  db.execute(
    "INSERT INTO procmgr_commands (slug, name, shell_cmd, ...) VALUES (?, ?, ?, ...)",
    (&saved.slug, &saved.name, &saved.shell_cmd, ...),
  )?;
  
  Ok(())
}
```

---

## 11. Programmable Terminal API

### 11.1 Trait/Interface for Plugins & AI

**Core interface (Rust trait):**

```rust
#[async_trait]
pub trait TerminalServer: Send + Sync {
  /// Create a new terminal session
  async fn create_session(
    &self,
    name: &str,
    shell: Option<&str>,
    working_dir: Option<&str>,
  ) -> Result<SessionHandle>;
  
  /// Send input to a session (with newline)
  async fn send_input(&self, session_id: &str, input: &str) -> Result<()>;
  
  /// Send raw input (no newline added)
  async fn send_raw_input(&self, session_id: &str, data: &[u8]) -> Result<()>;
  
  /// Read output lines from buffer (with optional start/end range)
  async fn read_output(
    &self,
    session_id: &str,
    start: Option<usize>,
    count: Option<usize>,
  ) -> Result<Vec<OutputLine>>;
  
  /// Search output buffer (text or regex)
  async fn search_output(
    &self,
    session_id: &str,
    query: &str,
    is_regex: bool,
  ) -> Result<Vec<usize>>;
  
  /// Subscribe to events (ProcessStarted, ProcessOutput, ProcessCrashed, etc.)
  async fn subscribe_events(&self, tx: mpsc::UnboundedSender<TerminalEvent>) -> Result<()>;
  
  /// Wait for output matching pattern (with timeout)
  async fn wait_for_pattern(
    &self,
    session_id: &str,
    pattern: &str,
    timeout_ms: u64,
  ) -> Result<()>;
  
  /// Get session metadata
  async fn get_session_info(&self, session_id: &str) -> Result<SessionInfo>;
  
  /// List all sessions
  async fn list_sessions(&self) -> Result<Vec<SessionInfo>>;
}

#[derive(Debug, Clone)]
pub enum TerminalEvent {
  SessionCreated { session_id: String, name: String },
  OutputReceived { session_id: String, line: OutputLine },
  PatternMatched { session_id: String, pattern: String },
  ErrorOccurred { session_id: String, error: String },
}

#[derive(Debug, Clone)]
pub struct OutputLine {
  pub timestamp: u64,
  pub content: String,       // ANSI-stripped text
  pub raw: Vec<u8>,         // Raw with ANSI codes
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
  pub id: String,
  pub name: String,
  pub shell: String,
  pub working_dir: String,
  pub line_count: usize,
  pub created_at: u64,
}
```

### 11.2 Event-Driven Architecture

**Events published to subscribers:**

```rust
pub enum ProcessEvent {
  ProcessStarted { pid: u32, slug: String },
  ProcessStopped { pid: u32, exit_code: i32, duration_ms: u64 },
  ProcessCrashed { pid: u32, reason: String },
  ProcessRestarting { pid: u32, delay_ms: u64 },
  ProcessOutput { pid: u32, line: String },
  ProcessUrlDetected { pid: u32, url: String, position: usize },
  ProcessMemoryUpdate { pid: u32, memory_bytes: u64 },
  ProcessSignalSent { pid: u32, signal: String },
  ProcessPreCommandFailed { pid: u32, step: usize, error: String },
  ProcessStepCompleted { pid: u32, step_idx: usize, exit_code: i32 },
}
```

**Subscription pattern (plugins/AI):**
```rust
let (tx, mut rx) = mpsc::unbounded_channel();
terminal.subscribe_events(tx).await?;

while let Some(event) = rx.recv().await {
  match event {
    ProcessEvent::ProcessOutput { pid, line } => {
      // AI analyzes line, suggests next command
    }
    ProcessEvent::ProcessCrashed { pid, reason } => {
      // AI reads crash log, suggests debugging steps
    }
    _ => {}
  }
}
```

---

## 12. AI Terminal Integration

### 12.1 AI Observing Output & Proposing Commands

**Workflow:**

1. **Process output received** → Event emitted to AI subscriber
2. **AI analyzes output:**
   - Recognizes patterns (test failures, build errors, missing dependencies)
   - Identifies action items (run tests, install package, restart server)
3. **AI proposes commands** via `/suggest-next-command` event or UI hint
4. **User reviews & executes** via one-click button or keyboard shortcut

**Implementation:**

```rust
#[async_trait]
impl AiTerminalAgent for Nexus {
  async fn on_process_output(&self, pid: u32, line: &str) -> Option<Vec<SuggestedCommand>> {
    // Builtin patterns
    if line.contains("error: Failed to compile") {
      return Some(vec![
        SuggestedCommand {
          text: "cargo check --message-format=json",
          reason: "Get detailed error info for debugging",
        }
      ]);
    }
    
    if line.contains("npm ERR! 404") {
      return Some(vec![
        SuggestedCommand {
          text: "npm update",
          reason: "Package may be outdated; update lockfile",
        }
      ]);
    }
    
    // LLM-based analysis (future)
    // send line to Claude API, parse response for suggested next commands
    
    None
  }
}
```

### 12.2 Executing Multi-Step Workflows

**Scenario:** AI detects test failure, reads error message, executes debug workflow.

```
User: "Fix this test"
  ↓
AI reads failure output
  ↓
AI executes:
  1. cargo test --test failing_test -- --nocapture
  2. Wait for test output
  3. Parse output, identify issue (e.g., missing file)
  4. Suggest fix or auto-execute:
     - Create missing file
     - Rerun test
     - Confirm fix
```

**Implementation:**
```rust
async fn fix_test_workflow(&self, test_name: &str, session_id: &str) -> Result<()> {
  // Step 1: Run test with output
  self.terminal.send_input(session_id, 
    &format!("cargo test {} -- --nocapture", test_name)
  ).await?;
  
  // Step 2: Wait for test output
  self.terminal.wait_for_pattern(
    session_id,
    "test result:",
    5000 // 5s timeout
  ).await?;
  
  // Step 3: Read output
  let output = self.terminal.read_output(session_id, None, Some(50)).await?;
  let failure_reason = analyze_test_output(&output);
  
  // Step 4: Execute fix based on reason
  match failure_reason {
    TestFailure::MissingFile(path) => {
      std::fs::create_dir_all(&path)?;
      self.terminal.send_input(session_id, "cargo test").await?;
    }
    TestFailure::CompilationError(msg) => {
      // Suggest fix to user
      emit_event(SuggestFix { message: msg });
    }
    _ => {}
  }
  
  Ok(())
}
```

### 12.3 Context Window Management

**Problem:** Large terminal buffers exceed LLM context limits (100k tokens).

**Solution:**

1. **Summarization:** Compress old output via LLM ("summarize last hour of output in 500 chars")
2. **Windowing:** Send only recent N lines (configurable, e.g., last 100 lines)
3. **Filtering:** Extract relevant lines (errors, warnings, key milestones)

**Implementation:**
```rust
async fn get_context_for_ai(&self, session_id: &str, max_tokens: usize) -> Result<String> {
  let all_output = self.terminal.read_output(session_id, None, None).await?;
  
  // Extract last 100 lines
  let recent = all_output.iter()
    .rev()
    .take(100)
    .rev()
    .collect::<Vec<_>>();
  
  // Filter for errors/warnings
  let filtered = recent.iter()
    .filter(|line| {
      line.contains("error") || line.contains("warning") 
        || line.contains("ERROR") || line.contains("WARN")
    })
    .collect::<Vec<_>>();
  
  // If still too large, summarize
  let context = if filtered.len() > 20 {
    let summary = self.llm_summarize(&filtered).await?;
    format!("Summary:\n{}\n\nRecent output:\n{}", summary, recent.join("\n"))
  } else {
    recent.join("\n")
  };
  
  Ok(context)
}
```

---

## 13. Database Schema (SQLite)

### 13.1 Process Manager Tables

```sql
-- Saved commands
CREATE TABLE procmgr_commands (
  slug TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  shell TEXT NOT NULL,              -- /bin/bash, cmd.exe, etc.
  shell_cmd TEXT NOT NULL,          -- Full command (may include && || ;)
  working_dir TEXT,                 -- cwd for spawned shell
  env_vars TEXT,                    -- JSON: {"KEY": "value"}
  env_file TEXT,                    -- Path to .env file
  icon TEXT,                        -- Icon name: "terminal", "gear", etc.
  auto_restart BOOLEAN DEFAULT 0,   -- Auto-restart on crash?
  auto_restart_delay_ms INTEGER DEFAULT 2000,
  memory_limit_mb INTEGER,          -- Hard limit before kill
  sidebar_order INTEGER,            -- Drag-reorder index
  pre_commands TEXT,                -- JSON: ["cmd1", "cmd2"]
  created_at INTEGER,
  updated_at INTEGER
);

-- Ad-hoc command history
CREATE TABLE procmgr_adhoc_history (
  id TEXT PRIMARY KEY,
  command TEXT NOT NULL,
  working_dir TEXT,
  executed_at INTEGER,
  exit_code INTEGER,
  duration_ms INTEGER,
  run_count INTEGER DEFAULT 1,
  status TEXT                       -- "success", "failure", "timeout"
);

-- Terminal sessions
CREATE TABLE terminal_sessions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT UNIQUE,
  shell TEXT NOT NULL,
  working_dir TEXT,
  created_at INTEGER,
  last_accessed_at INTEGER,
  is_active BOOLEAN DEFAULT 0,
  buffer_size_bytes INTEGER DEFAULT 10485760  -- 10 MB
);

-- Indices for performance
CREATE INDEX idx_procmgr_commands_sidebar_order 
  ON procmgr_commands(sidebar_order);
CREATE INDEX idx_adhoc_history_executed_at 
  ON procmgr_adhoc_history(executed_at DESC);
CREATE INDEX idx_sessions_last_accessed 
  ON terminal_sessions(last_accessed_at DESC);
```

---

## 14. Terminal UI Components

### 14.1 UI Architecture

**Layout:**
```
┌──────────────────────────────────────────────┐
│  Workspace                                   │
├──────────────────────────────────────────────┤
│ ┌─ Process Sidebar ─┬─ Terminal Output ──────┐
│ │ ├─ build [⏳]    │                        │
│ │ ├─ test [✓]     │  $ npm test             │
│ │ ├─ dev [⊗]      │  > Running tests...     │
│ │ │               │  ✓ 234 passed          │
│ │ └─ [+] Add      │  in 2.3s                │
│ │                 │                        │
│ │                 │  [search bar]           │
│ └─────────────────┴────────────────────────┘
└──────────────────────────────────────────────┘
```

**Terminal output pane features:**
- ANSI color rendering (256-color, TrueColor)
- Scrollback with infinite history
- Auto-scroll when new output arrives (unless user scrolls up)
- Text selection and copy/paste
- Right-click context menu (copy, clear, search)

**Process sidebar features:**
- List of saved commands
- Status indicators: ⏳ running, ✓ stopped, ⊗ crashed, 🔄 restarting
- Drag-reorder (reorder via drag, persist in `sidebar_order`)
- Right-click context menu (run, stop, edit, delete, promote)
- Search/filter bar (quick search by command name)
- Visual separators for grouping

### 14.2 Search Overlay

**Trigger:** Ctrl+F in terminal pane

**UI:**
```
Terminal Output:
  ... (output lines)
  [Search: error      ↑↓  Aa  Rx] [✕]
  ... (highlighted matching lines)
```

**Behavior:**
- Real-time search as user types
- Highlight matching lines (light background color)
- Navigate matches with arrow buttons or keyboard (↑/↓)
- Case-insensitive by default; toggle with "Aa" button
- Regex mode toggle with "Rx" button
- Escape or click ✕ to close

### 14.3 ANSI Rendering

**Library:** `crossterm` or custom ANSI parser

**Supported sequences:**
- SGR (Select Graphic Rendition): colors, bold, underline, etc.
  - Standard colors: `\x1b[30m` (black) through `\x1b[37m` (white)
  - 256-color: `\x1b[38;5;123m`
  - TrueColor: `\x1b[38;2;255;0;0m` (red)
  - Attributes: bold (`1`), dim (`2`), italic (`3`), underline (`4`), reverse (`7`)
- Cursor movement: `\x1b[nA`, `\x1b[nB`, `\x1b[nC`, `\x1b[nD`, etc.
- Erase: `\x1b[2J` (clear screen), `\x1b[K` (clear line)

**Rendering strategy:**
- Parse ANSI codes on output capture
- Store (text, color, attributes) tuples per character
- Render to terminal/GUI with native color/style support

---

## 15. Command Editor Dialog

### 15.1 Fields & Validation

**UI form:**
```
┌─ Edit Command ─────────────────┐
│ Name: [build               ]    │
│ Command: [npm run build    ]    │
│ Working Dir: [./            ]   │
│ Shell: [/bin/bash         ▼]    │
│                                 │
│ Pre-Commands:                   │
│  [+ Add]  [1. npm install]      │
│           [✕]                   │
│                                 │
│ Environment Variables:          │
│  [+ Add]  [DEBUG = 1        ]   │
│           [✕]                   │
│                                 │
│ .env File: [.env.local ▼  ]    │
│ Icon: [⚙️ gear           ▼]    │
│ Auto-restart: [☑] Delay: [2s]   │
│ Memory limit: [500 MB         ] │
│                                 │
│                    [Save] [✕]   │
└─────────────────────────────────┘
```

**Validation:**
- Name: required, no leading/trailing spaces
- Command: required, non-empty
- Working dir: validate path exists
- Pre-commands: validate shell syntax (basic)
- Environment variables: parse as KEY=value
- Auto-restart delay: > 0, <= 60s

**Parsing env_vars field:**
```rust
fn parse_env_vars(input: &str) -> Result<HashMap<String, String>> {
  let mut env = HashMap::new();
  for line in input.lines() {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') { continue; }
    if let Some((k, v)) = line.split_once('=') {
      env.insert(k.to_string(), v.to_string());
    } else {
      return Err(format!("Invalid env var: {}", line));
    }
  }
  Ok(env)
}
```

### 15.2 Icon Picker

**Icons (simple emoji or SVG):**
- terminal (default)
- gear (settings)
- package (package manager)
- folder (file-related)
- bug (debugging)
- play (run)
- circle (generic)

**Picker UI:**
```
Icon: [⚙️  ▼]

  [terminal] [⚙️ gear] [📦 package]
  [📁 folder] [🐛 bug] [▶️ play]
  [○ circle] [... more]
```

---

## 16. Keyboard Shortcuts

### 16.1 Terminal Session Shortcuts

| Key | Action | Context |
|-----|--------|---------|
| **Ctrl+T** | Open new terminal session | Global |
| **Ctrl+W** | Close current terminal | Terminal focused |
| **Ctrl+Tab** / **Ctrl+Shift+Tab** | Switch between sessions | Global |
| **Ctrl+Shift+N** | Rename current session | Terminal focused |
| **Ctrl+Shift+C** | Copy selected text | Terminal focused |
| **Ctrl+Shift+V** | Paste text | Terminal focused |
| **Ctrl+F** | Open search overlay | Terminal focused |
| **Escape** | Close search overlay | Search active |
| **Enter** | Send input to terminal | Input field focused |
| **Ctrl+C** | Send SIGINT to process | Terminal running |
| **Ctrl+Z** | Suspend process | Terminal running |

### 16.2 Process Manager Shortcuts

| Key | Action | Context |
|-----|--------|---------|
| **Ctrl+Shift+P** | Open "Run Command" palette | Global |
| **Alt+R** | Run/restart selected command | Sidebar focused |
| **Alt+S** | Stop selected command | Command running |
| **Alt+E** | Edit selected command | Sidebar focused |
| **Alt+K** | Kill selected command (SIGKILL) | Command running |
| **Delete** | Delete saved command | Sidebar focused |
| **Shift+↑** / **Shift+↓** | Reorder command (drag-like) | Sidebar focused |
| **Ctrl+Shift+,** | Open Preferences (terminal) | Global |

### 16.3 Customization

**Storage:**
```sql
CREATE TABLE ui_keybindings (
  action TEXT PRIMARY KEY,
  key_sequence TEXT,
  is_default BOOLEAN
);
```

**UI:** Settings dialog with searchable list of actions, bind via Ctrl+<key>.

---

## 17. Performance Targets

### 17.1 Output Throughput & Render Latency

**Target metrics:**
- **Output throughput:** 10,000 lines/sec sustained (typical dev output much lower)
- **Render latency:** < 100ms between output arrival and UI update
- **Search latency:** < 500ms over 100k-line buffer
- **Memory per session:** < 50 MB per 10,000-line buffer

**Optimization strategies:**
- Batch output writes (buffer 10ms of output before render)
- Async render (don't block input/output threads)
- Lazy ANSI parsing (only parse on display, not on capture)
- Ring buffer recycling (avoid repeated allocations)

### 17.2 Process Startup & Exit Detection

**Target:**
- Process startup detected within 500ms
- Process exit detected within 1s (polling interval)
- Signal delivery confirmed within 100ms

**Implementation:**
- PTY reader spawns dedicated async task (high priority)
- Exit status polling every 100ms
- Signal delivery via direct syscall (not fork/exec overhead)

---

## 18. Testing Strategy

### 18.1 Unit Tests

**Coverage areas:**
- ANSI parser (all SGR codes, edge cases)
- Env var resolution (precedence, interpolation, .env parsing)
- Command chain parser (all operators, cd detection)
- Ring buffer (overflow, deduplication, search)
- Signal escalation (timeout logic, process tree killing)

**Mock components:**
- Fake PTY (in-memory buffer, simulated output)
- Fake process (exit codes, memory tracking)

### 18.2 Integration Tests

**Scenarios:**
1. Create session, run command, capture output, search, close
2. Multi-step command chain (&&, ||, ;) with different exit codes
3. Pre-commands failure → main command doesn't run
4. Process auto-restart after crash
5. Memory limit enforcement (spawn process, monitor, kill at limit)
6. Signal escalation (SIGINT → SIGTERM → SIGKILL)
7. Env var precedence (command override .env override shell)
8. URL detection in output, click handling

### 18.3 Performance Tests

**Benchmarks:**
- Capture 100k lines in buffer; measure memory & search latency
- Render 1000 ANSI-colored lines; measure frame time
- Spawn/kill 10 processes; measure resource cleanup

---

## 19. Future Features (Post-v1.0)

### 19.1 OutputCapture: Writing to Forge Notes

**Design:** Process output (stdout/stderr) automatically saved to forge file.

**Format:**
```markdown
# Build Output — 2026-04-11 14:22

**Process:** npm run build  
**Status:** ✓ Passed  
**Duration:** 2.3s

## Output
\`\`\`ansi
npm notice created a lockfile as package-lock.json
npm WARN deprecated ...
...
\`\`\`

## Metrics
- Build time: 2.3s
- Lines: 145
- Errors: 0
- Warnings: 3
```

**Trigger:** Checkbox in command editor: "Save output to forge"

### 19.2 ProjectCommands: Auto-Populate from Folder

**Design:** Linking saved commands to forge folders; sidebar auto-populates when project opened.

**Example:**
```yaml
# In .nexus/config.yml at project root
commands:
  - name: build
    command: cargo build --release
    icon: package
  - name: test
    command: cargo test
    icon: bug
```

**Behavior:**
1. User opens project folder
2. Nexus reads `.nexus/config.yml`
3. Commands auto-imported into sidebar
4. User can override/customize

### 19.3 OutputIndexer: FTS5 for Terminal Output

**Feature:** Full-text search across all terminal output (current & historical).

**Implementation:**
- On process exit, index buffer to SQLite FTS5 table
- Support complex queries: `AND`, `OR`, `NOT`, phrase search
- UI: Global search bar (Ctrl+Shift+F) searches all sessions

---

## 20. Dependencies & Build

### 20.1 Crates

- **portable-pty:** PTY abstraction (Windows ConPTY, Unix PTY)
- **tokio:** Async runtime (output reader, signal handling, polling)
- **regex:** Pattern matching (command parsing, URL detection, output search)
- **rusqlite:** SQLite interface (process/session persistence)
- **serde/serde_json:** Configuration serialization
- **crossterm:** Terminal manipulation (ANSI rendering reference)
- **nix:** Unix system calls (setsid, signals, process management)
- **winapi:** Windows API (job objects, ConPTY, memory info)
- **anyhow:** Error handling

### 20.2 Compilation Flags

```toml
[profile.release]
opt-level = 3
lto = true
```

---

## 21. Acceptance Criteria

### Implementation Complete When:

1. ✓ PTY lifecycle (create, read output, spawn process, close)
2. ✓ Ring buffer (capture, search, bounds, memory tracking)
3. ✓ Process state machine (all transitions, timeouts, error conditions)
4. ✓ Signal handling (Unix process groups, Windows job objects, escalation)
5. ✓ URL detection (parsing, localhost mapping, UI click)
6. ✓ Memory monitoring (polling, limits, alerting)
7. ✓ Env var resolution (precedence, .env parsing, interpolation, masking)
8. ✓ Command parsing (&&, ||, ;, cd detection, single-shell mode)
9. ✓ Ad-hoc history (storage, dedup, promotion)
10. ✓ Terminal API (trait, events, wait-for-pattern, subscribe)
11. ✓ Saved commands (CRUD, sidebar ordering, pre-commands)
12. ✓ Command editor dialog (fields, validation, env vars, icons)
13. ✓ Terminal UI (panes, ANSI rendering, search overlay, scrollback)
14. ✓ Process sidebar (list, status, drag-reorder, context menu)
15. ✓ Database schema (tables, indices, session persistence)
16. ✓ AI integration (observe output, suggest commands, execute workflows)
17. ✓ Keyboard shortcuts (all terminal/process manager shortcuts, customization)
18. ✓ Unit tests (ANSI parser, env resolution, command parsing, buffer)
19. ✓ Integration tests (full workflows, signal handling, memory limits)
20. ✓ Performance benchmarks (meet throughput/latency targets)

### Success Metrics:

- **Functional:** All features working end-to-end, no unhandled panics
- **Performance:** 10k lines/sec throughput, < 100ms render latency, < 50MB per session
- **Reliability:** Auto-restart reduces manual intervention; no data loss on crash
- **Usability:** Developers prefer terminal for 80% of tasks; AI suggests correct next commands 70%+ of the time

---

## Appendix: SQLite Schema (Complete DDL)

```sql
-- Saved commands
CREATE TABLE procmgr_commands (
  slug TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  shell TEXT NOT NULL,
  shell_cmd TEXT NOT NULL,
  working_dir TEXT,
  env_vars TEXT,                -- JSON
  env_file TEXT,
  icon TEXT DEFAULT 'terminal',
  auto_restart BOOLEAN DEFAULT 0,
  auto_restart_delay_ms INTEGER DEFAULT 2000,
  memory_limit_mb INTEGER,
  sidebar_order INTEGER,
  pre_commands TEXT,            -- JSON array
  created_at INTEGER,
  updated_at INTEGER
);

-- Ad-hoc command history
CREATE TABLE procmgr_adhoc_history (
  id TEXT PRIMARY KEY,
  command TEXT NOT NULL,
  working_dir TEXT,
  executed_at INTEGER,
  exit_code INTEGER,
  duration_ms INTEGER,
  run_count INTEGER DEFAULT 1,
  status TEXT
);

-- Terminal sessions
CREATE TABLE terminal_sessions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT UNIQUE,
  shell TEXT NOT NULL,
  working_dir TEXT,
  created_at INTEGER,
  last_accessed_at INTEGER,
  is_active BOOLEAN DEFAULT 0,
  buffer_size_bytes INTEGER DEFAULT 10485760
);

-- Indices
CREATE INDEX idx_procmgr_commands_sidebar_order ON procmgr_commands(sidebar_order);
CREATE INDEX idx_adhoc_history_executed_at ON procmgr_adhoc_history(executed_at DESC);
CREATE INDEX idx_sessions_last_accessed ON terminal_sessions(last_accessed_at DESC);
```

---

## Document Metadata

- **Version:** 1.0
- **Author:** Nexus Team
- **Date:** April 2026
- **Status:** Implementation-Ready
- **Target Lines:** 550 (content body, excluding metadata/schema)
- **Dependencies:** Parent PRD v0.1, Forge subsystem (notes), AI Integration subsystem
