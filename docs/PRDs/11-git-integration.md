# PRD 13: Git Integration (nexus.git)

**Version:** 1.0  
**Target Release:** April 2026  
**Status:** Implementation-Ready  
**Owner:** Core Platform Team  
**Dependencies:** Kernel Event System (01), Storage Engine (02), Editor Engine (03), Plugin System (05), AI Engine (07)

---

## Executive Summary

The Git Integration subsystem (nexus.git) transforms Nexus from a standalone editor into a Git-aware, version-control-native development environment. It operates across three architectural levels:

- **Level 1 (Passive Awareness):** Non-intrusive status decorations, diff/blame/log views, Git history as AI context
- **Level 2 (Integrated Operations):** Interactive staging, commits, branch operations, merge/rebase workflows
- **Level 3 (Git as Sync Backend):** Auto-commit, collaborative forges, CRDT-over-Git transport

All read operations use the **git2 crate** (libgit2 bindings); no shell invocations. All write operations (commit, push, branch changes) are safe, composable, and emit typed events that integrate with Nexus's event kernel.

---

## 1. Architecture & Core Abstractions

### 1.1 git2 Integration Strategy

**Repository Handle Lifecycle:**
- Repository discovery on workspace load via `.git` directory traversal (respecting `.gitignore` boundaries)
- Single `git2::Repository` handle per repo, cached in `RepositoryPool` (thread-safe with `Arc<Mutex<>>`)
- Explicit `drop()` on workspace close or repo removal
- No persistent repository handles across hot-reload events

**Thread Safety:**
- `git2::Repository` and `git2::Index` are NOT `Send` or `Sync`
- All git operations delegated to dedicated `GitWorkerThread` (unbounded tokio::spawn with message passing)
- Incoming operations queued as `GitCommand` enum; responses returned via oneshot channels
- Status checks, diffs, history queries routed to worker thread even from UI threads

**Memory Management:**
- Diff buffers limited to 10 MB per file; larger files trigger streaming diff or binary marker
- Blame annotations cached per file with timestamp; invalidated on editor changes
- Tree traversals for status use libgit2's internal C buffer pools (no external heap copying)

### 1.2 Core Data Model

```rust
pub struct GitState {
    pub repo_root: PathBuf,
    pub current_branch: String,
    pub head_ref: String, // detached HEAD or branch name
    pub is_dirty: bool,
    pub is_merging: bool,
    pub is_rebasing: bool,
}

pub enum FileStatus {
    Untracked,
    Unmodified,
    Modified,
    Staged,
    Removed,
    Renamed(String), // old name
    Conflicted(ConflictMarkers),
    Typechange(String), // symlink <-> file
}

pub struct HunkDiff {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
    BinaryMarker,
}
```

---

## 2. Level 1: Passive Awareness

### 2.1 File Status Tracking

**Status Computation:**
- On file watcher events (file created/modified/deleted), debounce for 250ms
- Call `repo.statuses(None)` to compute repository-wide status delta
- Cache result with file modification timestamp
- Invalidate on explicit git operations (commit, branch switch, merge)

**Large Repo Optimization (10k+ files):**
- Use `StatusOptions::include_untracked(false)` for performance paths (show only tracked + staged)
- Lazy-load untracked files on explicit "show untracked" UI action
- Monitor file count; if > 50k files, emit performance warning and offer `.gitignore` suggestions

**Incremental Updates:**
- Mark files as "dirty" on watcher event; skip full `statuses()` call
- Batch watcher events into 500ms windows
- On batch, compute delta only for changed paths using `repo.status_file()`

### 2.2 Editor Gutter & File Explorer Decorations

**Gutter Decorations (Modified/Added/Deleted):**
- Line-level decorations derived from unified diff of `HEAD` vs working tree
- Green for added, blue for modified, red for deleted
- Conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`) render with conflict color + inline accept/reject buttons
- Update on file save via `FileEdited` kernel event

**File Explorer Icons:**
- Modified (M): orange dot
- Staged (S): green checkmark
- Untracked (U): gray dot
- Conflicted (C): red X
- Each file decorated via file metadata in `FileNode` data model

### 2.3 Diff View

**Architecture:**
- Side-by-side and unified modes selected via UI toggle
- Diff computed via `git2::diff::tree_to_index()` (staged) or `git2::diff::index_to_workdir()` (unstaged)
- Syntax highlighting applied line-by-line using Editor Engine's tokenizer
- Large files (> 5 MB): show first 100 hunks; user can load remaining on scroll

**Binary Files:**
- Detect via libgit2's `is_binary()` check
- Render as placeholder: "(Binary file — preview not available)"

### 2.4 Blame View

**Annotation Model:**
```rust
pub struct BlameAnno {
    pub commit_hash: String,
    pub author: String,
    pub date: DateTime,
    pub message: String, // first line of commit message
}
```

- Computed on-demand via `repo.blame_file(path, None)?`
- Cached per file; invalidated on branch switch or commit in that file
- Click annotation → jump to commit in Git Log viewer
- Hover annotation → tooltip with full commit message

### 2.5 Git Log Viewer

**Features:**
- Scrollable commit history with graph visualization (via `gitgraph-rs` or similar)
- Search by commit message (fuzzy), author, date range
- Filter by file path (show history for single file only)
- Each commit row clickable → show full diff, parents, tags
- Blame origin: clicking annotation opens log viewer for that commit

**Performance:**
- Load first 500 commits on open; lazy-load on scroll
- Graph computation deferred to background thread

### 2.6 AI Context Integration

**Git History as LLM Context:**
- On `AiContextRequest` kernel event, include:
  - Last 5 commits to current file (commit message + diff summary)
  - Current branch name and commits ahead/behind default branch
  - Recent merge conflicts (for context on integration issues)
- Max context budget: 8000 tokens
- Preference: recent, high-signal commits (filter noisy auto-commits if auto-commit enabled)

---

## 3. Level 2: Integrated Operations

### 3.1 Staging System

**Stage/Unstage Unit:** file, hunk, or line

**Implementation:**
- File staging: `git2::Index::add_path(path)?` + `index.write()?`
- Hunk staging: apply patch to index via `git apply --cached` (invoke once per hunk)
- Line staging: same, but patch contains only selected lines
- Unstaging: `git2::Index::remove_path(path)?` or apply inverse patch

**Staging UI Data Model:**
```rust
pub struct StagingState {
    pub staged_files: Vec<(PathBuf, FileStatus)>,
    pub unstaged_files: Vec<(PathBuf, FileStatus)>,
    pub unstaged_hunks: Vec<(PathBuf, HunkDiff)>, // for interactive view
    pub untracked_files: Vec<PathBuf>,
    pub selected_items: HashSet<PathBuf>, // UI selection state
}
```

**Hunk/Line Staging UI:**
- Show unstaged hunks as expandable tree under each file
- Checkbox for each hunk; drag-select multiple lines within hunk
- Preview diff as user clicks hunks
- Stage button applies selected hunks to index

### 3.2 Commit Flow

**Conventional Commit Template:**
```
<type>(<scope>): <description>

<body>

<footer>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `chore`, `ci`, `build`

**Commit Composition Panel:**
- Template autocomplete on type/ input (via snippet system)
- Pre-fill scope from current file path (e.g., `editor/diff` for `src/editor/diff.rs`)
- Body: textarea with markdown preview
- Co-author insertion: "Add Co-Author" button → user list search → append to footer

**AI-Generated Messages:**
- On "Suggest" button:
  1. Collect staged diff (context: paths, additions, deletions)
  2. Call AI Engine with prompt: "Generate a conventional commit message for these changes"
  3. Display suggestion in message field; allow user edit before commit

**GPG Signing:**
- If `git.signing.key` configured in `.forge/app.toml`:
  - Call `git2::Repository::sign_buffer(message, gpg_key)?`
  - Append signature to commit metadata
- If signing fails, warn user and proceed unsigned (config error recovery)

**Pre-commit Hooks:**
- Read `.git/hooks/pre-commit` if executable
- Invoke synchronously before commit creation
- If hook fails (exit code != 0), show error and prevent commit
- Output hook stdout/stderr in commit panel error zone

### 3.3 Branch Operations

**List Branches:**
- Local: `repo.branches(Some(git2::BranchType::Local))?`
- Remote: `repo.branches(Some(git2::BranchType::Remote))?`
- Track status: for each local branch, check `repo.graph_ahead_behind(branch, tracking)?`

**Create Branch:**
- Target: current HEAD or user-selected commit
- Call `repo.branch(name, &target_commit, false)?` (false = not force)
- Emit `GitBranchCreated` event

**Switch Branch:**
- If working tree is dirty: show confirmation dialog listing changed files
- Option to stash before switch (creates named stash `auto-switch-{timestamp}`)
- Call `repo.set_head_ref(branch)?` + `repo.checkout_head(opts)?`
- Emit `GitBranchSwitched` event

**Rename Branch:**
- `repo.branch(old_name)?.rename(new_name, false)?`
- Update tracking refs if branch is remote
- Emit `GitBranchRenamed` event

**Delete Branch:**
- Confirm if branch is not fully merged into main/master
- `repo.branch(name)?.delete()?`
- Emit `GitBranchDeleted` event
- Force delete flag available for advanced users

**Branch Comparison:**
- "Commits ahead": `repo.graph_ahead_behind(local_branch, tracking_branch)?.0`
- "Commits behind": `.1`
- Visual indicator in status bar: `main +5 -2` (5 ahead, 2 behind)

### 3.4 Merge & Conflict Resolution

**Merge Initiation:**
- User selects branch to merge from branch picker
- `repo.merge_analysis(&[merge_branch_ref])?` → check for fast-forward, conflicts
- If fast-forward possible, prompt user
- If conflicts: proceed to conflict resolution UI

**Conflict Detection:**
- Libgit2 marks conflicted files after merge attempt
- Iterate index entries; if `stage` == 3 (or multiple stages), file is conflicted
- Extract three versions: ours (stage 1), theirs (stage 3), ancestor (stage 2)

**Three-Way Diff Editor:**
```
┌──────────────────┬──────────────────┬──────────────────┐
│ Ancestor         │ Current (Ours)   │ Incoming (Theirs)│
├──────────────────┼──────────────────┼──────────────────┤
│ common content   │ our changes      │ their changes    │
└──────────────────┴──────────────────┴──────────────────┘

[Accept Current] [Accept Incoming] [Accept Both] [Edit Manually]
```

- Render three-way diff with syntax highlighting
- User clicks "Accept Current" → remove their section, keep ours
- "Accept Both" → concatenate both (careful for semantics)
- "Edit Manually" → open full editor for the file

**Per-Hunk Accept/Reject:**
- Show each conflicted hunk as a collapsible block
- Buttons to resolve that hunk only
- "Resolve All" button to accept strategy for all hunks
- After resolution, stage file and continue

**Conflict Markers in Editor:**
- If user prefers to edit raw conflict markers:
  - Display `<<<<<<<`, `=======`, `>>>>>>>` in editor
  - Gutter decorations highlight conflict regions
  - When user resolves (deletes markers), mark file as resolved
  - Button to "Mark as Resolved" explicitly

**Auto-Merge Non-Conflicting Changes:**
- After user resolves conflicts, run `git merge --continue` to finalize
- If merge succeeds, emit `GitMergeCompleted` event
- Prompt user to create merge commit message

### 3.5 Rebase Support

**Interactive Rebase:**
- Show rebase plan UI: list of commits to be rebased
- User can reorder, squash, reword, drop commits
- Save plan and invoke `git rebase --interactive` via git2 API
- Conflict resolution same as merge (three-way editor)

**Rebase Navigation:**
- Continue: `repo.rebase_continue()?` after conflict resolution
- Abort: `repo.rebase_abort()?` (restore pre-rebase state)
- Skip: `repo.rebase_continue()` with no changes (effectively drops commit)

### 3.6 Push/Pull/Fetch with Progress

**Fetch:**
- `repo.find_remote(name)?` → `remote.fetch(&[refspec], None)?`
- Stream progress via callback: `fetch_options.set_fetch_progress(|_, current, total| { /* emit event */ })`
- Emit `GitFetchCompleted` event

**Pull:**
- Fetch + merge in sequence
- On merge conflict during pull, show same conflict resolution UI
- Emit `GitPullCompleted` on success

**Push:**
- `repo.find_remote(name)?.push(&[refspec], Some(&mut push_opts))?`
- Retry on network error (3 attempts, exponential backoff)
- If push fails (rejected), offer to pull and retry
- Emit `GitPushCompleted` or `GitPushFailed` event

**Remote Management:**
- Add: `repo.remote(name, url)?`
- Remove: `repo.find_remote(name)?.delete()?`
- Rename: update `.git/config` directly
- List remotes in settings panel

---

## 4. Level 3: Git as Sync Backend

### 4.1 Auto-Commit

**Trigger Options:**
- Time-based: every N minutes (configurable, default 30 min)
- Event-based: on file save (debounced 5 sec window)
- Hybrid: either trigger fires first

**Generated Commit Messages:**
- Format: `auto: {files changed}` (e.g., `auto: editor.rs, diff.rs`)
- Include file count if > 5 files: `auto: 7 files`
- No conventional commit structure (distinguish from user commits)

**Debouncing Multiple Rapid Changes:**
- Batch file saves within 5 sec window
- If user manually commits during window, cancel auto-commit
- Emit `GitAutoCommitScheduled` event (debounced), then `GitAutoCommitCreated` on actual commit

**Failure Handling:**
- If commit fails (dirty tree, merge conflict), skip and retry next cycle
- Log warning but do not interrupt user workflow
- Provide "Resume Auto-Commit" button if stalled

### 4.2 Auto-Push/Pull Cycle

**Auto-Push:**
- After successful auto-commit or user commit, attempt push to tracking branch
- If push fails (remote has new commits):
  - Fetch latest
  - If fast-forward merge possible, merge and retry push
  - If conflict, alert user (emit `GitPushConflict` event) and require manual resolution

**Auto-Pull:**
- Timer: every 5 minutes (configurable)
- Fetch origin/{branch}; if new commits exist, auto-merge
- If conflict, pause auto-pull, alert user, require resolution

**Collaborative Forges Pattern:**
- Pull (with conflict resolution) → Fetch → Push cycle
- Designed for team workflows with frequent small commits
- Conflict resolution UI same as manual merge

### 4.3 Snapshot Sync (Periodic Commits as Named Snapshots)

**Snapshot Commit Format:**
```
snapshot: {timestamp} {label}

Auto-created snapshot commit for backup/recovery.
```

- Triggered every N hours (configurable, default 4 hours)
- Label: user-provided or auto-generated (e.g., `before-large-refactor`)
- Commit all changes (even uncommitted) via force-staging
- Do not push automatically (local backup only)
- Browsable in Git Log as separate commit sequence

**Recovery:**
- User can reset working tree to any snapshot via log viewer
- Confirm "discard all changes since snapshot?"
- Reset to snapshot, preserve uncommitted work if desired

### 4.4 Git-CRDT Transport

**CRDT Commit Format:**
- Nexus CRDT state (rich text buffer) serialized as JSON
- Stored in `.nexus/crdt-state.json` (tracked in git)
- On git push, this file included in commit
- On git pull with merge conflict in `.nexus/crdt-state.json`: apply CRDT merge algorithm

**CRDT Merge Strategy:**
- If conflict on `.nexus/crdt-state.json`:
  - Deserialize ancestor, ours, theirs as CRDT states
  - Apply CRDT merge semantics (operation-based or state-based)
  - Commit merged state to both branches
- Fallback: if CRDT merge fails, treat as content conflict and require manual resolution

**Collaborative Editor Use Case:**
- Multiple users push CRDT state snapshots
- Git merge provides lamport clock ordering
- CRDT merge ensures convergence without manual conflict resolution
- Works across asynchronous pulls/pushes

---

## 5. Event Integration

**Git-Specific Events (emitted by nexus.git):**

```rust
pub enum GitEvent {
    StatusChanged { files: Vec<(PathBuf, FileStatus)> },
    GitCommitCreated { hash: String, message: String, author: String },
    GitBranchSwitched { from: String, to: String },
    GitBranchCreated { name: String },
    GitBranchRenamed { old: String, new: String },
    GitBranchDeleted { name: String },
    GitPullCompleted { commits_merged: u32 },
    GitFetchCompleted { refs_updated: u32 },
    GitPushCompleted { commits_pushed: u32 },
    GitPushFailed { reason: String },
    GitConflictDetected { files: Vec<PathBuf> },
    GitConflictResolved { file: PathBuf },
    GitMergeCompleted { commit_hash: String },
    GitRebaseCompleted { commits_rebased: u32 },
    GitStashApplied { name: String },
    GitAutoCommitScheduled,
    GitAutoCommitCreated { hash: String },
    GitPushConflict { file: PathBuf },
}
```

**Cross-Plugin Reactions:**
- **AI Engine** listens to `GitCommitCreated`, updates semantic index of code changes for context awareness
- **Terminal Manager** listens to `GitBranchSwitched`, updates shell prompt (if shell integration enabled)
- **Storage Engine** listens to `GitPushCompleted`, triggers backup sync to remote storage
- **Agent System** can query git history as part of context gathering (via `GitLogQuery` command)

---

## 6. Configuration (.forge/app.toml)

```toml
[git]
# Operations
auto_commit_interval_mins = 30
auto_push_enabled = true
auto_pull_interval_mins = 5
auto_snapshot_interval_hours = 4

# Commit
default_branch = "main"
conventional_commit_template = true
signing_key = "DEADBEEF" # GPG key ID

# Credentials
credential_helper = "osxkeychain" # or "wincred", "pass", "custom"
ssh_key_path = "~/.ssh/id_ed25519"

# Performance
large_file_threshold_mb = 5
status_cache_ttl_ms = 500
diff_context_lines = 3
```

**Credential Handling:**
- SSH keys: read from `ssh_key_path`; passphrase stored in OS keychain
- HTTPS: use git credential helper (configured per platform)
- Custom: user-provided credential callback via plugin API

---

## 7. Performance & Scalability

**Status Computation Benchmarks:**
- 10k files: < 500 ms (cold), < 100 ms (cached)
- 50k files: < 2 sec (cold), < 300 ms (cached + incremental)
- 100k files: show warning; suggest `.gitignore` expansion

**Diff Generation:**
- Single file < 10k lines: < 50 ms
- Large file (50k lines): < 500 ms; cache result
- Binary files: instant (no diff computation)

**Background Threading:**
- All git operations (except UI reads of cached state) run on `GitWorkerThread`
- UI operations (status button clicks) enqueue and await response via oneshot
- Long operations (push, pull) show progress dialog; user can cancel

**Memory Usage:**
- Per-file blame cache: ~100 bytes per line
- Diff buffer pool: 10 MB max per file
- Status cache: ~50 bytes per file

---

## 8. Error Handling & Recovery

**Network Errors:**
- SSH/HTTPS timeouts: retry 3 times with exponential backoff (1s, 2s, 4s)
- DNS failure: show user suggestion to check connectivity
- Authentication failure: prompt for credentials or offer SSH key setup

**Merge Conflicts:**
- Auto-resolve non-conflicting hunks via libgit2
- Present conflicted files with three-way diff
- If user cancels merge: `git merge --abort` restores pre-merge state

**Dirty Working Tree:**
- Branch switch blocked if uncommitted changes exist
- Offer: stash, commit, or cancel
- Stash naming: `auto-switch-{branch}-{timestamp}`

**Detached HEAD:**
- Show warning in status bar
- Offer "Create branch from detached HEAD" action
- If user commits in detached HEAD, warn about potential loss

**Large Repository Issues:**
- If repo > 1 GB: show warning on open; offer shallow clone option
- If status computation > 3 sec: trigger performance warning; suggest `.gitignore` review

---

## 9. Source Control Panel UI

**Layout:**
```
┌─ Source Control ─────────────────────┐
│ Repository: /path/to/repo            │
│ Branch: main (origin/main +2 -0)     │
│                                      │
│ ▼ Staged Changes                      │
│   ✓ file1.rs                         │
│   ✓ file2.rs                         │
│                                      │
│ ▼ Unstaged Changes                    │
│   M editor.rs                        │
│   M diff.rs                          │
│   ? new_file.rs                      │
│                                      │
│ [Commit Message]                     │
│ ┌──────────────────────────────────┐│
│ │ feat(editor): add inline diffs   ││
│ │                                  ││
│ └──────────────────────────────────┘│
│                                      │
│ [Suggest] [Sign] [Commit] [Cancel]  │
└──────────────────────────────────────┘
```

**Interactions:**
- Click file → show inline diff preview
- Right-click file → "Stage", "Unstage", "Discard"
- Drag file → reorder in staging area
- "Staged Changes" section expandable with checkbox to unstage all

---

## 10. Branch Picker

**Status Bar Display:**
- Current branch name (clickable)
- Ahead/behind counts if tracking remote

**Popup on Click:**
```
┌─ Branches ──────────────────────────┐
│ [Search branches...]                 │
│                                      │
│ main (current)  [ahead +2]           │
│ feature/diffs                        │
│ feature/ai-commit                    │
│ hotfix/typo                          │
│                                      │
│ [+ Create New Branch]                │
│ [... Show Remote Branches]           │
└──────────────────────────────────────┘
```

**Create Branch Inline:**
- Click "+ Create" → text input
- Enter branch name → creates from current HEAD
- Auto-switches to new branch

---

## 11. Conflict Resolution UI

**Three-Way Diff View (as described in section 3.4):**
- Side-by-side with syntax highlighting
- Per-hunk accept/reject buttons
- Manual edit mode for complex merges
- "Mark Resolved" button per file

---

## 12. Git Log Viewer

**Features:**
- Commit graph with branch/tag indicators
- Search by message (fuzzy), author name, commit hash (prefix)
- Date range filter (last week, month, year, custom)
- File history: show commits affecting specific file
- Click commit → full diff + commit metadata

**Context Menu:**
- Copy commit hash
- Revert commit (new commit undoing changes)
- Cherry-pick commit (apply to current branch)
- Blame file at this commit (show file content + blame for this revision)

---

## 13. Acceptance Criteria

- [ ] All three levels (Passive, Integrated, Sync Backend) functional
- [ ] Status computation < 500 ms on 10k-file repos
- [ ] Diff generation < 50 ms for typical files
- [ ] Merge conflict resolution tested with real multi-file conflicts
- [ ] Auto-commit + auto-push cycle verified (no race conditions)
- [ ] AI context integration tested (commit history included in LLM queries)
- [ ] All events emitted correctly (verified with event listener tests)
- [ ] GPG signing and pre-commit hooks functional
- [ ] CRDT merge strategy tested with simultaneous pushes
- [ ] All error paths (network, merge conflict, auth) have recovery flows

---

## 14. Dependencies & Integration Points

- **Kernel Event System (01):** Event emission, listener registration
- **Editor Engine (03):** Gutter decorations, inline diff preview, syntax highlighting for diffs
- **AI Engine (07):** Commit message suggestion, context augmentation from history
- **Storage Engine (02):** Snapshot sync integration (backup to remote storage)
- **Terminal Manager (06):** Shell prompt integration for branch name
- **Plugin System (05):** nexus.git registered as core plugin with read/write capabilities

---

## 15. Success Metrics

- **Adoption:** > 80% of users enable at least Level 1 (passive awareness)
- **Staging Efficiency:** Interactive staging reduces commit prep time by 50% vs manual
- **Conflict Resolution:** 90% of merge conflicts resolved in UI without manual editing
- **Auto-Commit Reliability:** Zero data loss in auto-commit cycles; < 1% retry rate
- **Performance:** P99 status computation < 1 sec on 100k-file repos
- **User Satisfaction:** 4.5+/5 rating for "git integration quality" in feedback surveys

---

## 16. Timeline & Milestones

| Milestone | Target | Deliverables |
|-----------|--------|--------------|
| M1: Core Git Ops | Week 1-2 | git2 integration, file status, basic diff |
| M2: Level 2 Staging | Week 2-3 | Interactive staging, commit flow, branch ops |
| M3: Merge/Rebase | Week 3-4 | Conflict resolution, rebase support |
| M4: Level 3 & Polish | Week 4-5 | Auto-commit, CRDT transport, error handling |
| M5: Testing & Docs | Week 5-6 | Integration tests, user docs, performance tuning |
| **1.0 Release** | **April 2026** | Feature-complete, performance targets met |

---

## Revision History

| Version | Date | Changes |
|---------|------|---------|
| 1.0 | April 2026 | Initial implementation-ready PRD |

