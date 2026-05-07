# Nexus Feature Backlog

> **Single source of truth for unfinished work.** This file is the index every other planning doc points to.
>
> - **Per-PRD status** lives in [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).
> - **Completed items** are archived verbatim in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).
> - **Full descriptions of OI-\*** items live in [../OPEN-ITEMS.md](../OPEN-ITEMS.md); this file cross-lists by ID.
> - **Formal-release work** (auto-updater, telemetry, marketplace, beta→GA) is deferred to [../REQUIRED-FOR-FORMAL-RELEASE.md](../REQUIRED-FOR-FORMAL-RELEASE.md); the WI-IDs are indexed below for completeness.
> - **Exploratory / unscoped design docs** (AI directions, ambient copilot, memory layer, settings extraction inventory) are linked under "Future directions" — they do not have committed timelines.
>
> Section headings with no listed items are preserved as structural placeholders — consult the archive for what landed under each, and add new follow-ups directly below the heading.

---

## New Features (not addressed in any PRD)

_BL-009 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-108 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-107 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-106 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-105 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-104 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-103 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Five fuzz targets shipped (path validator, event type id, capability parser, manifest parser, wasm instantiation). Stable-Rust smoke runner exercises four of them on every `cargo test -p nexus-fuzz` run; cargo-fuzz / libFuzzer shims under `fuzz_targets/` are operator-side (require nightly). CI 60s-per-target gate deferred to operator wiring._

---

_BL-102 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Verifier scaffolding shipped (rustls custom `ServerCertVerifier`, `KernelConfig::tls_pinning_enabled`, `SecurityError::CertificatePinMismatch`/`NoPinsConfigured`, `NEXUS_TLS_PINNING=1` env opt-in). Default **off** because the shipped `tls_pins::HOST_PINS` table is empty — an operator with network access seeds real fingerprints, then flips the flag. The `nexus ai status` `tls_pinned` field is still TODO._

**Operator action required to actually pin:**
1. Capture leaf SHA-256 for `api.anthropic.com` and `api.openai.com` per the procedure documented in `crates/nexus-security/src/tls_pins.rs`.
2. Populate `HOST_PINS` with at least two values per host (current + expected next leaf) so a routine cert rotation doesn't take the app offline.
3. Set `tls_pinning_enabled = true` in `<forge>/.nexus/config.toml` (or `NEXUS_TLS_PINNING=1` for an ad-hoc test).

---

_BL-101 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). AEAD encryption (ChaCha20-Poly1305) with per-plugin keyring-stored 256-bit master key shipped; PBKDF2 + companion-salt file simplified out as documented in the closure notes (the master key is already uniformly random, so PBKDF2 over a stored salt would not raise the security floor)._

---

_BL-100 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Optional rolling-file JSONL output deferred (the `tracing-appender` daily rotation in `nexus-cli` already covers operational logs; SQLite is the authoritative audit store)._

---

_BL-099 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Verifier infrastructure shipped: ed25519 signature/CRL/keyring + manifest field + loader gate + `nexus plugin verify` CLI. Module landed in `nexus-plugins/src/signing.rs` rather than `nexus-security` to avoid the existing nexus-security → nexus-plugins dep direction. `require_signatures` defaults to `false` per the PRD; flip on once a signed-plugin distribution channel exists._

---

_BL-098 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-097 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Honored ADR 0021 (`<cmd>.v<N>` suffix in the command name, not a request envelope field) and rolled out `with_v1_aliases` to every subsystem. New live-registry deprecation-window guard test scans the actual loader on every CI run. The PRD-spec'd `IpcRequest.schema_version` envelope field was deliberately not adopted — see closure notes for the rationale._

---

_BL-096 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Live runtime revocation, persistence, audit, bus event, and a `nexus plugin revoke` CLI verb shipped. Dedicated kernel-internal IPC handler + shell "Revoke" button deferred — the existing `set_plugin_granted_capabilities` Tauri command already covers the persisted-grant write path; live-mutation through the shell is a follow-up._

---

_BL-095 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Watchdog ships for the `register_core` path with default 30s deadline; “continue with degraded plugin set” + bus event are deferred (bootstrap currently aborts boot with a clear `LifecycleTimeout` error, which is a strict improvement over hanging silently)._

---

_BL-094 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-093 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). In-process counter + histogram registry shipped with IPC, event-bus, capability, and lifecycle recording; `com.nexus.security::metrics_snapshot` IPC handler exposes the JSON snapshot. `event_bus_queue_depth` gauge, Prometheus scrape endpoint, and shell health panel deferred (see closure notes)._

---

_BL-092 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Criterion benches for event bus and IPC dispatch shipped. Measured baselines on this dev box: event publish ~300ns, IPC noop dispatch ~30µs, capability check ~30ns — all comfortably inside the PRD targets. CI integration with regression gates deferred — the harness is available for an operator to wire into CI once a stable bench runner is provisioned._

---

### BL-091: Git-LFS support

**Source**: Git Integration Assessment + user request (2026-05-06)
**Effort**: Medium (1.5–2 weeks)
**Crates**: `nexus-git/src/engine.rs`, `nexus-git/src/core_plugin.rs`, `nexus-storage` (binary file handling)
**Related**: PRD-11 git integration; git2 LFS pointer parsing; `nexus-storage::read_file` (binary path)

Git-LFS (Large File Storage) stores large binary files (images, audio, video, datasets, model weights) as pointer files in the repo, with the actual content stored on a remote LFS server. Without LFS support, Nexus silently serves the raw pointer text to users when they open an LFS-tracked file, which is confusing and wrong.

**What LFS support means in practice:**
- Detect LFS pointer files on read (header `version https://git-lfs.github.com/spec/v1`)
- For binary attachments (images, audio, video) — transparently fetch the actual object from the LFS server and serve bytes to the caller
- For tracked markdown files — warn user that content may be a pointer if LFS server is unavailable
- On write — if a file matches `.gitattributes` LFS filter patterns, write the pointer and stage the object via `git lfs` CLI subprocess (git2 has no native LFS API)

**Implementation approach:** git2 has no native LFS support. The right path is a hybrid:
- Read: detect pointer headers in `nexus-storage::read_file`; if LFS, invoke `git lfs smudge` subprocess to fetch real content
- Write: detect LFS-tracked patterns from `.gitattributes`; if match, invoke `git lfs clean` subprocess before staging
- `nexus-git` exposes `lfs_status()` handler — lists tracked patterns from `.gitattributes` and which tracked files are locally available vs. pointer-only

**Definition of done:**
- `com.nexus.git::lfs_status` (handler id 11) returns `{ tracked_patterns: [], pointer_files: [], available_files: [] }`
- `com.nexus.storage::read_file` detects LFS pointer header, invokes `git lfs smudge <path>` if `git-lfs` binary found, returns real bytes; falls back to pointer text with a warning event if LFS unavailable
- `com.nexus.git::stage_file` detects LFS-tracked path and routes through `git lfs clean` before staging
- `nexus git lfs-status` CLI subcommand lists LFS-tracked files and their local availability
- Graceful degradation: if `git-lfs` binary not found, log warning and operate on pointer text

---

_BL-090 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-089 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

### BL-088: Rebase and cherry-pick

**Source**: Git Integration Assessment (2026-05-06) — deferred feature
**Effort**: Medium (1.5 weeks)
**Crates**: `nexus-git/src/engine.rs`, shell git panel (BL-084)
**Related**: PRD-11 §3.4 (deferred); git2 `repo.rebase*` + `repo.cherrypick` API

git2 exposes `repo.rebase()` for non-interactive rebase and `repo.cherrypick()` for cherry-pick. Interactive rebase (editing the todo list) requires shelling out to `git rebase -i` since git2 doesn't expose it.

**Definition of done (non-interactive rebase):**
- `GitEngine::rebase(onto_branch)` — rebases current branch onto `onto_branch`; returns `RebaseResult { conflicts: [], commits_rebased: u32 }` or conflict list
- `GitEngine::abort_rebase()` — restores pre-rebase state
- `com.nexus.git::rebase` + `abort_rebase` handlers (ids 16–17)
- `nexus git rebase <branch>`, `nexus git rebase --abort` CLI subcommands

**Definition of done (cherry-pick):**
- `GitEngine::cherry_pick(commit_hash)` — applies a single commit to working tree; returns conflict list if any
- `GitEngine::abort_cherry_pick()` — restores pre-cherry-pick state
- `com.nexus.git::cherry_pick` + `abort_cherry_pick` handlers (ids 18–19)
- `nexus git cherry-pick <hash>` CLI subcommand

---

_BL-087 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-086 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-085 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

### BL-084: Shell git panel — commit UI, branch picker, log graph

> **3 of 4 surfaces shipped 2026-05-06** in `shell/src/plugins/nexus/gitPanel/`. Remaining: conflict resolution panel. Note: the original assessment that "all `com.nexus.git` handlers already ship" was incorrect — 6 new IPC handlers (11–16) were added as part of this work.

**Source**: Git Integration Assessment (2026-05-06) — gap #1 (largest UX gap)
**Effort**: Large (3–4 weeks)
**Crates**: `shell/src/plugins/nexus/gitPanel/`, `crates/nexus-git/src/core_plugin.rs`
**Related**: BL-079 (git gutter + diff viewer); BL-085 (hunk-level staging)

**New IPC handlers added (2026-05-06):**
- Handler 11 `file_statuses` — `{}` → `[{path, status}]`
- Handler 12 `diff_staged` — `{}` → `[{path, hunks[]}]`
- Handler 13 `switch_branch` — `{name}` → `GitOk`
- Handler 14 `create_branch` — `{name}` → `GitOk`
- Handler 15 `delete_branch` — `{name}` → `GitOk`
- Handler 16 `push` — `{remote, branch}` → `GitOk`

**Surfaces:**

- **Commit panel** ✅ shipped — staged/unstaged file lists, inline diff preview (`diff_file` / `diff_staged`), stage/unstage individual or all, commit message (⌘Enter), push-after-commit checkbox
- **Branch picker** ✅ shipped — branch list with HEAD + upstream labels, switch, create+switch, delete with confirmation
- **Log** ✅ shipped — 50 most recent commits, hash chip (copy), HEAD badge, author, relative date; click-to-show-diff deferred
- **Conflict resolution panel** ❌ not built — triggered on `repo_state: Merge`, three-way diff, per-hunk accept/reject, abort merge

**Definition of done (remaining):**
- Conflict panel activates automatically when `com.nexus.git.state` publishes `repo_state: Merge`
- Per-hunk accept/reject wired to `conflict_files`, `abort_merge` handlers (not yet exposed via IPC)

---

### BL-083: Forge-to-forge import and migration tool

**Source**: Storage Integration Assessment (2026-05-06) — gap #5
**Effort**: Medium (1 week)
**Crates**: `nexus-storage` (new `import_forge` handler), `nexus-cli` (new `nexus forge import` subcommand)
**Related**: PRD-03 §forge-layout; `com.nexus.storage::rebuild_index` (destination reindex path)

Schema version migrations (SQLite `_schema_version` table) handle version upgrades within a single forge. There is no mechanism to import one forge into another, merge two forges, or produce a portable archive beyond raw file copy. For users who want to consolidate forges, migrate to a new machine without losing index state, or merge a team member's forge, the only path today is copying files manually and rebuilding the index.

**Definition of done:**
- `nexus forge import <source-path> [--into <dest-path>] [--dry-run]` CLI command
- `--dry-run` reports: files to copy, conflicts (same relative path, different content), skips (identical hash)
- Conflict resolution strategies: `--on-conflict skip|overwrite|rename` (default: skip with report)
- After file copy, destination forge runs `rebuild_index` to incorporate imported files
- New `com.nexus.storage::import_forge` IPC handler wraps the same logic for shell UI use
- Progress events published to kernel bus during copy phase

---

### BL-082: Symlink handling in watcher and reconcile

**Source**: Storage Integration Assessment (2026-05-06) — gap #4
**Effort**: Small (0.5–1 day)
**Crates**: `nexus-storage/src/watcher.rs`, `nexus-storage/src/reconcile.rs`
**Related**: PRD-03 §file-watcher; `notify` crate symlink follow behavior

The file watcher and reconcile algorithm treat symlinks as regular files. If a symlink and its target are both within the forge root, reconcile may create two index entries for the same content — one for the symlink path and one for the target path — causing duplicate search results, duplicate backlink entries, and potentially conflicting writes.

**Definition of done:**
- `reconcile.rs` calls `fs::symlink_metadata` + checks `file_type().is_symlink()` before indexing; symlinks are skipped (not followed) during the walk
- `watcher.rs` ignores `EventKind::Modify` for paths that resolve to symlinks pointing outside the forge root; for symlinks within the forge root, the watcher follows to the canonical path and deduplicates events
- Symlink entries in the `files` table are tagged with `file_type = "symlink"` and excluded from FTS indexing, search results, and graph nodes
- `query_files` accepts an optional `include_symlinks: bool` filter (default false)

---

### BL-074: Collaborative editing — CRDT layer

**Source**: Editor Integration Assessment (2026-05-06) — gap #5
**Effort**: Large (4–6 weeks)
**Crates**: new `nexus-crdt`, `nexus-editor` (sync loop), `nexus-storage` (conflict merge)
**Related**: BL-007 (CRDT-over-Git transport); PRD-08 collaborative editing spec; stable block IDs (ADR 0017) were built for this

The block model was designed with collaboration in mind — stable IDs survive upstream edits, annotation ranges adjust on insert/delete, and the transaction system is invertible. The CRDT merge semantics are documented in the spec. What's missing is the live sync loop: a mechanism for two sessions on the same forge to exchange operations and converge.

This is the only editor gap that requires genuinely new infrastructure rather than wiring existing pieces.

**Definition of done:**
- `nexus-crdt` crate implements operation-based CRDT over the `Operation` type from `nexus-editor`
- Two sessions opening the same file exchange operations via the kernel event bus (`com.nexus.editor.ops.<path>`)
- Merge conflicts (concurrent edits to the same block) resolve via CRDT semantics; no user intervention needed for text edits
- Structural conflicts (concurrent delete + edit of same block) surface as a user-resolvable dialog
- BL-007 (CRDT-over-Git) becomes the persistence layer for async collaboration

---

### BL-073: Block auto-stamping on first reference

**Source**: Editor Integration Assessment (2026-05-06) — gap #4
**Effort**: Tiny (0.5 day)
**Crates**: `nexus-editor/src/core_plugin.rs`
**Related**: ADR 0017 (stable block IDs); `stamp_block` handler (id 11) ships today

Stable block IDs (`<!-- ^<uuid> -->` comments) require an explicit `stamp_block` IPC call to assign. Nothing auto-assigns them. A user who creates a wikilink to a specific block (`[[file#^uuid]]`) must first manually stamp the target block — a friction point that breaks the linking flow.

**Definition of done:**
- When `resolve_block_link` is called with a target block that has no stable ID, it auto-stamps the block, writes the comment to the file, and returns the newly assigned ID
- `apply_transaction` auto-stamps any block that gains a `Wikilink` annotation pointing to it (inbound-link trigger)
- Auto-stamped IDs are UUID v4; no user-visible change except the comment appears in the markdown source

---

### BL-072: Undo history persistence across sessions

**Source**: Editor Integration Assessment (2026-05-06) — gap #3
**Effort**: Small–Medium (1 week)
**Crates**: `nexus-editor/src/undo_tree.rs`, `nexus-editor/src/core_plugin.rs`, `nexus-storage`
**Related**: PRD-08 §undo; `UndoTree` struct fully implemented; session-local today

Closing an editor tab destroys the undo tree. On reopen the file loads from disk and history starts fresh. For long documents with complex edit histories this is a real limitation — a crash or accidental close forfeits all undo depth.

**Definition of done:**
- `UndoTree` serializes to JSON (serde derives already present on all operation types)
- On `close(path)`, serialize and write to `.forge/.editor/undo/<path-hash>.json` via `com.nexus.storage`
- On `open(path)`, check for a persisted undo file; if found and the file's current content matches the tree's head state, restore it
- Cap persisted history at 500 operations (ring-buffer eviction); cap file age at 7 days (stale eviction on forge open)
- `close` still clears in-memory session as before; persistence is additive

---

### BL-071: Emacs keybindings mode

**Source**: Editor Integration Assessment (2026-05-06) — gap #2b; PRD-08 §9
**Effort**: Small (2–3 days)
**Crates**: `shell/src/plugins/nexus/editor/cm/` (new `emacsKeymap.ts`)
**Related**: BL-070 (Vim mode — do that first, same infrastructure); CM6 `keymap` extension API

PRD-08 §9 specifies Emacs keybinding support. CM6's keymap extension supports multiple layers; Emacs mode is additive on top of the default bindings. Standard Emacs navigation (C-f/b/n/p movement, C-a/e line, M-f/b word, C-k kill-line, C-y yank, C-space mark, M-w copy, C-w cut) covers the bulk of what Emacs users expect.

**Definition of done:**
- New CM6 extension `emacsKeymap.ts` implements the standard Emacs navigation and editing bindings
- Toggle via Settings → Editor → Keybindings (alongside Default and Vim)
- Mark ring (C-space) stores up to 16 positions; C-y yanks last killed text from kill ring
- Keybinding mode persisted to forge settings; applied on editor mount

---

### BL-070: Vim keybindings mode

**Source**: Editor Integration Assessment (2026-05-06) — gap #2a; PRD-08 §9
**Effort**: Small–Medium (3–5 days)
**Crates**: `shell/src/plugins/nexus/editor/cm/` (new `vimKeymap.ts`)
**Related**: BL-071 (Emacs mode — parallel, same infrastructure); CM6 `@replit/codemirror-vim` or custom

PRD-08 §9 specifies Vim keybinding support. CM6 has `@replit/codemirror-vim` as a well-maintained extension that provides modal editing (Normal/Insert/Visual), motion commands, operators, and ex commands. The integration is a package addition + settings toggle.

The key tension: Nexus slash commands use `/` which conflicts with Vim's search. The extension should intercept `/` in Normal mode for Vim search and pass it through in Insert mode for Nexus slash commands.

**Definition of done:**
- `@replit/codemirror-vim` integrated as a CM6 extension, activated when `editor.keybindings = "vim"` in settings
- Normal/Insert/Visual modes work; mode indicator shown in editor status bar
- `/` in Normal mode → Vim search; `/` in Insert mode → Nexus slash command palette
- `:w` saves the current file via `com.nexus.editor::save`; `:q` closes the tab
- Keybinding mode persisted to forge settings; applied on editor mount
- Settings → Editor → Keybindings toggle (Default / Vim / Emacs)

---

### BL-081: DAP debugger integration

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Large (4–6 weeks)
**Crates**: new `nexus-dap`, new `shell/src/plugins/nexus/debugger/`
**Related**: BL-076 (nexus-lsp — do first); Debug Adapter Protocol (DAP)

DAP is the debugger equivalent of LSP. Requires a `nexus-dap` core plugin (same spawn+bridge architecture as `nexus-lsp`) and a full debug panel shell plugin (Variables, Call Stack, Watch, Breakpoints, toolbar). Breakpoint gutter decorations in CM6. Deferred until the LSP track (BL-075–077) ships — debug without language awareness is painful.

**Definition of done:**
- `nexus-dap` spawns configured DAP adapters (e.g. `codelldb`, `js-debug`) from `.forge/dap.toml`
- Debug panel: Variables, Call Stack, Watch, Breakpoints panels visible in shell
- CM6 breakpoint gutter: click to set/clear, red dot indicator
- Debug toolbar: Continue, Step Over, Step Into, Step Out, Restart, Stop
- `com.nexus.dap` IPC surface mirrors DAP request/response types

---

### BL-080: File tree / project explorer

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Small (3 days)
**Crates**: `shell/src/plugins/nexus/fileTree/` (new shell plugin)
**Related**: `com.nexus.storage` file enumeration (already ships); no new backend work

A project-explorer panel showing the forge as a directory tree with file-type icons, expand/collapse, drag-to-reorder, and a right-click context menu (new file, rename, delete, copy path, reveal in OS). Pure shell work — `com.nexus.storage` already enumerates all forge files and emits file-change events on the kernel bus for live updates.

**Definition of done:**
- `nexus.fileTree` shell plugin registers a sidebar panel
- Tree renders forge root with expand/collapse per directory
- File-type icons (at minimum: `.md`, `.rs`, `.ts`, `.py`, `.toml`, `.json`, generic)
- Right-click context menu: New File, New Folder, Rename, Delete, Copy Path
- Click opens file in editor (document mode for `.md`, code mode for everything else once BL-075 lands)
- File-change bus events keep tree in sync without manual refresh

---

### BL-079: Git gutter + diff viewer

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Medium (1.5 weeks)
**Crates**: `shell/src/plugins/nexus/editor/cm/` (new `gitGutter.ts`), `shell/src/plugins/nexus/diffView/` (new panel); `nexus-git` backend already ships
**Related**: PRD-11 (git integration); `nexus-git` `git diff` already operational

`nexus-git` provides `git diff` output today. What's missing is surface: line-level change indicators in the editor margin and a side-by-side diff panel.

**Definition of done:**
- `gitGutter.ts` CM6 extension: green bar (added), yellow bar (modified), red triangle (deleted) in the left margin; hover shows the removed lines
- Clicking a gutter mark opens an inline diff hunk with Stage/Revert actions dispatched to `com.nexus.git`
- Diff view panel: side-by-side or unified mode via CM6 `MergeView`; opens from palette or file-tree context menu
- Inline blame: `com.nexus.git::blame` result shown as muted end-of-line annotation on hover (togglable)
- No new IPC handlers needed — `com.nexus.git` already exposes diff, blame, and stage operations

---

### BL-078: Multi-file search and replace

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Medium (1 week)
**Crates**: `shell/src/plugins/nexus/searchPanel/` (new shell plugin); `com.nexus.storage` Tantivy FTS already ships
**Related**: `com.nexus.storage::search` (existing FTS); `com.nexus.storage::write_file` (replace path)

`com.nexus.storage` has Tantivy FTS and file enumeration. A search panel that queries across all forge files and supports in-place replacement is pure shell work with no new backend infrastructure.

**Definition of done:**
- `nexus.searchPanel` shell plugin opens via ⌘⇧F; supports plain text, regex, case-sensitive, whole-word
- Results grouped by file with surrounding context lines; click jumps to match
- Replace field: preview all replacements before applying; apply-all or per-file confirmation
- CM6 decorations highlight all matches in open tabs when search is active
- Progress indicator for large forges; results stream incrementally via existing FTS pagination

---

### BL-077: CM6 LSP client extension

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Medium (1 week)
**Crates**: `shell/src/plugins/nexus/editor/cm/` (new `lspClient.ts`)
**Related**: BL-076 (nexus-lsp — required first); BL-075 (dual-mode routing — required first); `codemirror-languageserver` (MIT)

`codemirror-languageserver` provides a CM6 extension that handles completion widget, diagnostic squiggles, hover tooltip, and go-to-definition for any LSP server. The adapter proxies CM6 LSP requests through `com.nexus.lsp` IPC instead of a WebSocket.

**Blocked by:** BL-075 (dual-mode routing must exist so LSP client only activates in code mode) and BL-076 (`nexus-lsp` plugin must be running).

**Definition of done:**
- `lspClient.ts` CM6 extension activates only in code mode (file-type gated)
- Completion widget shows LSP suggestions with kind icons (method, variable, keyword, etc.) and documentation popover
- Diagnostic squiggles: red (error), yellow (warning), blue (info) underlines; hover shows message
- Hover tooltip shows type signature + documentation for symbol under cursor
- Cmd+Click (go-to-definition) opens the target file at the correct line
- Format on save: calls `com.nexus.lsp::format` on `⌘S`
- Change notifications fire on every CM6 transaction (no 800ms debounce in code mode)

---

### BL-076: `nexus-lsp` — Language Server Protocol core plugin

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Large (2–3 weeks)
**Crates**: new `nexus-lsp`
**Related**: BL-075 (dual-mode routing — do first); BL-077 (CM6 client — do after); mirrors `nexus-mcp` architecture

The load-bearing piece. Without LSP, code editing in Nexus is a syntax-highlighted textarea. With it, completions, diagnostics, hover, go-to-definition, rename, and format-on-save all become available for any language with a language server.

Architecture mirrors `nexus-mcp`: reads `.forge/lsp.toml` for server configs, spawns each server as a child process over stdin/stdout JSON-RPC, bridges requests/responses to the kernel IPC surface, manages server lifecycle (restart on crash, shut down on forge close). The `ConnectionPool` + reconnect pattern from `nexus-mcp` is the template.

**IPC surface (`com.nexus.lsp`, ~12 handlers):**
```
open_file(path, content, language_id)
close_file(path)
change_file(path, content, version)
completions(path, line, col)           → CompletionList
hover(path, line, col)                 → Hover
definition(path, line, col)            → Location[]
references(path, line, col)            → Location[]
rename(path, line, col, new_name)      → WorkspaceEdit
code_actions(path, range)             → CodeAction[]
format(path)                          → TextEdit[]
list_servers()                        → Vec<LspServerInfo>
```

**Push events:** `com.nexus.lsp.diagnostics.<path>` published on every server push.

**Definition of done:**
- `nexus-lsp` crate registered by `nexus-bootstrap`
- `rust-analyzer` confirmed working end-to-end (completions, diagnostics, go-to-def)
- `typescript-language-server` confirmed working
- Server crash triggers automatic restart with exponential backoff
- `scripts/check_ipc_drift.sh` passes (new IPC types exported via ts-rs)

---

### BL-075: Dual-mode editor — code files vs. document files

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Small (3 days)
**Crates**: `shell/src/plugins/nexus/editor/EditorView.tsx`, `shell/src/plugins/nexus/editor/sessionManager.ts`
**Related**: BL-076 (nexus-lsp — this is its prerequisite); PRD-08 editor spec

The current editor routes all files through the `nexus-editor` block tree. Code files (`.rs`, `.ts`, `.py`, etc.) should bypass the block tree entirely and open in a raw CM6 mode backed by `nexus-lsp` instead of `nexus-editor`.

**Two modes:**
- **Document mode** (existing): `.md` + forge notes → block tree + `com.nexus.editor` IPC. Unchanged.
- **Code mode** (new): all other text files → raw CM6 on file content, `com.nexus.lsp` IPC, no block tree.

The routing decision is made at file open by checking the file extension against a configurable code-file list. The CM6 instance in code mode uses standard language extensions (via `@codemirror/lang-*` packages) instead of the custom block handle/slash command extensions.

**Definition of done:**
- `EditorView.tsx` accepts a `mode: "document" | "code"` prop; code mode renders bare CM6 without block-tree extensions
- `sessionManager.ts` routes opens by file type; `.md` → document mode, everything else → code mode
- Code mode reads file content via `com.nexus.storage::read_file` directly (no `com.nexus.editor::open`)
- Code mode saves via `com.nexus.storage::write_file` on ⌘S
- Code mode applies correct CM6 language extension for common types (Rust, TypeScript, Python, Go, TOML, JSON, YAML, Markdown-in-code)
- File-type list configurable in Settings → Editor

---

### BL-069: Database query executor for `[[{db:query}]]` blocks

**Source**: Editor Integration Assessment (2026-05-06) — gap #1 (largest functional gap)
**Effort**: Medium–Large (2–3 weeks)
**Crates**: `nexus-editor/src/core_plugin.rs` (`execute_database_view` handler), `nexus-database`, `nexus-storage`
**Related**: PRD-08 database view spec; PRD-10 (database engine); `execute_database_view` handler (id 12) registered but query executor missing; BL-012 (DB query blocks)

The database view renderers are complete — Table, Kanban, Calendar, and Gallery layouts all render, the filter engine has 14 operators, and the sort engine supports multi-level null-last sorting. The gap is the query executor: `[[{db:query}]]` inline blocks parse and display a widget, but the widget doesn't fetch or display data.

`execute_database_view` (handler 12) is registered and callable. It needs to be wired to `com.nexus.database` to run the query, apply filters and sorts, and return `{ records, groups }` to the view renderer.

**Definition of done:**
- `execute_database_view` handler calls `com.nexus.database::query` with the view's filter + sort spec, returns typed records
- Table view renders live rows with column types respected (text, number, date, select, checkbox)
- Kanban view groups records by the specified column, drag-to-reorder updates the record's group field
- Calendar view places records on month grid by ISO date field; undated bucket shown below
- Gallery view renders records as cards with configurable cover field
- Filter + sort round-trip: changes in the view UI write back to the block's frontmatter via `apply_transaction`
- `com.nexus.editor::execute_database_view` timeout: 30s (large datasets)

---

### BL-061: Terminal memory backpressure — enforce kill policy

**Source**: Terminal Integration Assessment (2026-05-06) — gap #5
**Effort**: Small (0.5 days)
**Crates**: `nexus-terminal` (`memory.rs`, `manager.rs`, `core_plugin.rs`)
**Related**: PRD-09 §7 (memory monitoring); `MemoryMonitor` shipped Phase R

`MemoryMonitor` tracks RSS per session and exposes `SoftExceeded`/`HardExceeded` thresholds. Nothing reads those thresholds and acts on them. A long-running process that leaks memory accumulates indefinitely.

**Definition of done:**
- `SessionManager` or the drainer thread polls `MemoryMonitor` results and calls `manager.kill(id)` when a session crosses `hard_mb`
- `com.nexus.terminal.events.<id>` publishes a `MemoryLimitExceeded { rss_mb, limit_mb }` lifecycle event before kill
- `get_session_info` response includes current RSS so the shell UI can display it

---

### BL-060: Ad-hoc command history — IPC exposure and shell UI

**Source**: Terminal Integration Assessment (2026-05-06) — gap #3
**Effort**: Small–Medium (1 day)
**Crates**: `nexus-terminal` (`core_plugin.rs`, `adhoc.rs`), `shell/src/plugins/nexus/terminal/`
**Related**: PRD-09 §10; `SqliteAdHocStore` shipped Phase M; IPC exposure deferred

`SqliteAdHocStore` exists with deduplication (same command + cwd increments `run_count`), status tracking, and promotion to saved commands. No IPC handlers expose it. Users can't browse, re-run, or promote their command history from the shell or CLI.

**Definition of done:**
- New handlers on `com.nexus.terminal`: `adhoc_list` (id 18), `adhoc_get` (id 19), `adhoc_delete` (id 20), `adhoc_promote` (id 21 — wraps existing `promote_adhoc_to_saved`)
- Shell UI gains a "History" tab or panel in the terminal plugin listing ad-hoc entries with run count, last-run time, status chip, re-run button, and promote-to-saved button
- `nexus proc history` CLI subcommand wraps `adhoc_list`
- `scripts/check_ipc_drift.sh` passes (new IPC types exported)

---

### BL-059: "Open in external terminal" escape hatch

**Source**: Terminal Integration Assessment (2026-05-06) — gap #6; CommandBook evaluation (2026-05-06)
**Effort**: Small (0.5–1 day)
**Crates**: `nexus-terminal` (new handler), `shell/src/plugins/nexus/terminal/SavedCommandsView.tsx`
**Related**: `docs/research/commandbook-evaluation.md` — "Run in Terminal" pattern

Nexus terminal doesn't support PTY-dependent programs (`vim`, `htop`, `less`, interactive REPLs). There's no escape hatch to hand a saved command's working directory and environment to an external emulator. Users who need interactivity have no path back to the forge's process context.

**Definition of done:**
- New IPC handler `com.nexus.terminal::open_in_terminal` (id 22): takes a `SavedCommand` slug, detects available terminal emulators in priority order (iTerm2, Warp, Ghostty, Kitty, Alacritty, Terminal.app, system default), opens a new window at the command's `working_dir` with env vars pre-loaded
- Context menu on `SavedCommandsView` sidebar items gains "Open in Terminal" entry
- Detection order configurable in Settings → Terminal

---

### BL-058: Terminal URL chip extraction — shell UI surface

**Source**: Terminal Integration Assessment (2026-05-06) — gap #2; CommandBook evaluation (2026-05-06)
**Effort**: Small (0.5 day)
**Crates**: `shell/src/plugins/nexus/terminal/TerminalView.tsx`
**Related**: `nexus-terminal/src/urls.rs` (410 lines, fully implemented, not wired to UI)

`urls.rs` detects HTTP(S), FTP, SSH, and `file://` URLs from output lines and classifies them by kind. Nothing surfaces this in the shell. The CommandBook URL-pin pattern (top-5 detected links pinned above the output pane, always visible regardless of scroll position, single-click to open) is the highest-value terminal UI pattern identified in the CommandBook evaluation.

**Definition of done:**
- `TerminalView.tsx` gains a `useUrlExtraction` hook that subscribes to the output stream, runs URL detection on new lines, and maintains a deduped top-5 list
- URLs render as pill chips above the output pane; click opens in default browser / file manager / SSH client per `UrlKind`
- Chips clear when the session is reset or explicitly dismissed
- Zero new backend work — all detection happens via the existing library exposed through `read_output`

---

### BL-057: Terminal activity timeline integration

**Source**: Terminal Integration Assessment (2026-05-06) — gap #4
**Effort**: Small (0.5 day)
**Crates**: `nexus-terminal/src/core_plugin.rs`
**Related**: BL-052 (universal activity timeline — defines the schema and topic convention); PRD-09 lifecycle events

The lifecycle forwarder thread already publishes `com.nexus.terminal.events.<id>` for `SessionCreated`, `ProcessCrashed`, and `SessionClosed`. It does not publish to the universal `com.nexus.activity.appended` topic. The activity timeline is therefore blind to all terminal events — a user can't audit what processes started, crashed, or exited alongside their AI tool calls.

**Blocked by:** BL-052 (generalized `ActivityEntry` schema must land first so the emitter format is stable).

**Definition of done:**
- On `SessionCreated`, `ProcessCrashed`, and `SessionClosed` events, the core plugin publishes a parallel `com.nexus.activity.appended` event with `origin: "terminal:<session_id>"`, `surface: "process"`, and relevant metadata (slug, exit_code, crash reason)
- Activity timeline panel renders terminal events with a terminal icon and appropriate filter chip
- No schema changes to `nexus-terminal` types — payload is assembled in `core_plugin.rs` from existing `SessionInfo`

---

### BL-056: Terminal workflow step type

**Source**: Terminal Integration Assessment (2026-05-06) — gap #1 (part 2)
**Effort**: Small (1 day)
**Crates**: `nexus-workflow/src/executor.rs`, `nexus-workflow/src/ai_steps.rs`
**Related**: BL-055 (agent tool registry — do that first); PRD-16 §step-types

`com.nexus.workflow` can dispatch `ipc` step types, but there's no `terminal` step type. Foundation-class workflows (always-on dev services started at forge open) and capability-class workflows (build triggers, test runners, linters on file save) all need to start/stop named saved commands.

**Definition of done:**
- New step type `type = "terminal"` in `.workflow.toml` with fields: `slug` (required, matches a `SavedCommand`), `action` (start | stop | restart | run_adhoc), `command` (for `run_adhoc` only), `working_dir` (override)
- `executor.rs` dispatches terminal steps through `com.nexus.terminal::run_saved` (BL-055) via `PluginContext::ipc_call`
- Workflow validate handler rejects `terminal` steps where `slug` doesn't match any saved command at validation time
- `nexus workflow run` respects terminal steps in CLI context

---

### BL-066: Terminal sidebar hover buttons

**Source**: Terminal Integration Assessment (2026-05-06); CommandBook evaluation (2026-05-06)
**Effort**: Tiny (0.5 day)
**Crates**: `shell/src/plugins/nexus/terminal/SavedCommandsView.tsx`
**Related**: BL-055 (run_saved handler must exist for start button to have a target)

Every polished process manager surfaces start/stop/restart actions inline on each sidebar row without requiring a right-click. `SavedCommandsView` has run/copy buttons but not contextual hover buttons. Users must open a context menu for the most common actions.

**Definition of done:**
- Each sidebar row gains hover-revealed icon buttons: Start (if stopped), Stop (if running), Restart, Dismiss (remove from active sessions without deleting the saved command)
- Buttons dispatch to `com.nexus.terminal` IPC handlers directly — no modal, no confirmation for Start/Stop/Restart
- Visual treatment matches existing shell hover patterns (opacity fade-in, same icon size)

---

### BL-065: Windows pre-command support (cmd.exe / PowerShell)

**Source**: Terminal Integration Assessment (2026-05-06) — Phase Q follow-up
**Effort**: Medium (2 days)
**Crates**: `nexus-terminal/src/precmd.rs`
**Related**: PRD-09 §4.4; Phase Q (POSIX-only sentinel approach shipped)

`run_pre_commands` uses a POSIX sentinel (`; printf '<SENTINEL> %d\n' $?`) to capture exit codes while preserving shell state across steps. This doesn't work on cmd.exe or PowerShell, where the sentinel syntax is different and state inheritance across commands works differently.

**Definition of done:**
- `precmd.rs` detects shell family (bash/zsh/fish vs. cmd.exe vs. pwsh) and uses the appropriate sentinel:
  - cmd.exe: `& echo <SENTINEL> %ERRORLEVEL%`
  - PowerShell: `; Write-Host "<SENTINEL> $LASTEXITCODE"`
- Pre-command state inheritance tested on Windows (PATH changes, env exports, directory changes carry forward)
- Existing POSIX tests continue to pass; Windows tests added (can be skipped on non-Windows CI)

---

### BL-064: Terminal AI suggestion LLM bridge

**Source**: Terminal Integration Assessment (2026-05-06) — gap in `ai.rs`
**Effort**: Small–Medium (1 day)
**Crates**: `nexus-terminal/src/ai.rs`, `nexus-terminal/src/core_plugin.rs`
**Related**: PRD-09 §12; `AiSuggestionEngine` shipped Phase S; `com.nexus.ai::stream_ask`

`AiSuggestionEngine` has 5 built-in pattern-match rules that return static suggestion strings. When a rule matches, the explanation is a hardcoded string rather than an LLM-generated response. The `SuggestionRule` trait already supports extension, and the IPC path to `com.nexus.ai` exists.

**Definition of done:**
- New `com.nexus.terminal::suggest` handler (id 23): takes `{ session_id, line_count }`, runs `AiSuggestionEngine` over recent output, and if a rule matches routes the matched context + rule explanation through `com.nexus.ai::stream_ask` for an enriched response
- Falls back to the static rule explanation if `com.nexus.ai` is unavailable or times out (10s)
- Shell terminal panel surfaces the suggestion as a dismissible chip below the output pane with a "Run suggested command" action
- Requires `ai.chat` capability; no additional capability needed (read-only terminal access)

---

### BL-063: Terminal FTS5 scrollback index

**Source**: Terminal Integration Assessment (2026-05-06); PRD-09 §19.3
**Effort**: Medium (2–3 days)
**Crates**: `nexus-terminal/src/persist.rs`, `nexus-terminal/src/core_plugin.rs`, `shell/src/plugins/nexus/terminal/`
**Related**: PRD-09 §19.3; `SqliteSessionStore` shipped Phase M

Current output search (`search_output`, handler 7) is per-session substring/regex over in-memory `LineBuffer`. There's no way to search across sessions or query scrollback that has been evicted from the in-memory buffer. FTS5 over the persisted scrollback blobs enables cross-session search and historical grep.

**Definition of done:**
- `SqliteSessionStore` gains an FTS5 virtual table (`scrollback_fts`) over `session_id` + `line_text` + `timestamp`
- Scrollback lines are indexed on write (`save_scrollback` path) with ANSI codes stripped before ingest
- New handler `cross_session_search` (id 24): `{ query, is_regex, session_ids?, since_ts?, limit? }` → `Vec<{ session_id, line_index, text, timestamp }>` — searches FTS5 index, optionally constrained to specific sessions or time range
- Shell terminal plugin gains a "Search all sessions" mode (⌘⇧F) that calls `cross_session_search` and renders results grouped by session with jump-to links
- FTS5 index excluded from SQLite backup exports (rebuildable; reduces export size)

---

### BL-062: Terminal session LRU eviction policy

**Source**: Terminal Integration Assessment (2026-05-06) — gap in `manager.rs`
**Effort**: Small (0.5 day)
**Crates**: `nexus-terminal/src/manager.rs`
**Related**: PRD-09 §2.3; `last_accessed` timestamp tracked but policy not implemented

`SessionManager` enforces the 50-session cap by hard-rejecting new spawns when at capacity. The `last_accessed` timestamp is tracked per session but nothing reads it. A forge with many long-lived sessions can exhaust the cap and block new spawns indefinitely.

**Definition of done:**
- `SessionManager::spawn` checks cap; if at limit, finds the oldest stopped session by `last_accessed` and evicts it (persisting scrollback first via `SqliteSessionStore`)
- If all sessions are running, returns a `SessionLimitExceeded` error (current behavior) rather than killing a live process
- `last_accessed` updated on every `drain`, `read_output`, `send_input` call
- Eviction emits a `SessionEvicted { id, reason: "lru" }` lifecycle event on the kernel bus

---

### BL-055: Terminal commands in agent tool registry

**Source**: Terminal Integration Assessment (2026-05-06) — gap #1 (highest leverage)
**Effort**: Small (0.5–1 day)
**Crates**: `nexus-ai/src/tools/registry.rs`, `nexus-terminal/src/core_plugin.rs`
**Related**: PRD-15 (agent system); PRD-12 §tool-calling; `com.nexus.terminal` IPC surface

The agent tool registry (`com.nexus.ai`) has no terminal tools. An agent that needs to start a dev server, run a build, check process status, or send a signal has no IPC path to do it. The terminal is the most common execution surface for developer workflows and it's entirely absent from agent plans.

Three tools are sufficient to unlock the core use cases:

| Tool name | IPC target | Purpose |
|---|---|---|
| `terminal_run_saved` | `com.nexus.terminal::run_saved` (new, wraps handler 1) | Start a saved command by slug |
| `terminal_get_status` | `com.nexus.terminal::get_session_info` (handler 9) | Check if a process is running, get exit code |
| `terminal_send_signal` | `com.nexus.terminal::send_raw_input` (handler 4) | Send SIGINT, SIGTERM, etc. |

**Definition of done:**
- New `run_saved` handler (id 18 on `com.nexus.terminal`) starts the named `SavedCommand`, returns the new session id — reuses `SessionManager::spawn` + `SavedCommand` lookup
- `ToolRegistry` gains three built-in terminal tools with JSON Schema definitions
- Tool advertisement policy `auto` includes terminal tools; `auto_readonly` excludes them (writes to processes are write-class)
- `ai.tools.write` capability required for `terminal_run_saved` and `terminal_send_signal`; `ai.chat` sufficient for `terminal_get_status`
- Agent planner system prompt updated to describe available terminal tools
- `scripts/check_ipc_drift.sh` passes

---

### BL-068: Theme Builder — visual token editor with live preview

> **Fully shipped 2026-05-06.** BL-105 (contrast checker) and BL-106 (light/dark dual mode + hue-lock) both closed. Original spec: [BL-067-068-builders.md](BL-067-068-builders.md).

**Source**: Idea capture (2026-05-06) — full doc in [BL-067-068-builders.md](BL-067-068-builders.md)
**Effort**: ~1 week (0.5d `preview_override` handler + 4d UI + 0.5d export)
**Crates**: `nexus-theme` (new `preview_override` IPC handler), new `shell/src/plugins/nexus/themeBuilder/`
**Related**: PRD-07 (theming system), BL-053 (forge visual target), bundled ember themes

Nexus themes are TOML files that override 400+ CSS variables (`--nx-{category}-{property}-{variant}`). Today, authoring one means editing the file externally and waiting for the file-watcher to hot-reload. The Theme Builder closes that loop inside the shell: a visual token editor with live preview, WCAG contrast checking, and one-click export to `.forge/themes/<name>/`.

The theme system already has live reload; the only new backend work is a `preview_override` handler that applies an in-memory token overlay without touching any files — cleared on cancel, persisted on save.

**Key surfaces:**
- Token palette grouped by category (Surface, Text, Accent, Border, Editor/Syntax) with color pickers and sliders ✅ shipped
- Base theme selector — start from any installed theme, write only the delta ✅ shipped
- Export writes `.theme.toml` to `.forge/themes/` and activates via hot-reload ✅ shipped (save-to-disk + `reload` handler call)
- Live split-view preview against a representative forge document ⬜ not built (uses live shell as preview instead)
- Per-token WCAG AA/AAA contrast pass/fail ✅ BL-105 closed 2026-05-06
- Light/dark side-by-side when theme supports both modes ✅ BL-106 closed 2026-05-06

**Definition of done:** ✅ All items shipped.

---

### BL-067: Shell View Builder — visual layout composer for plugin panels

**Source**: Idea capture (2026-05-06) — full doc in [BL-067-068-builders.md](BL-067-068-builders.md)
**Effort**: ~1.5–2 weeks (1d introspection API + 5–7d drag-drop UI + 1d export-as-plugin template)
**Crates**: `ExtensionHost` (JS introspection API), new `shell/src/plugins/nexus/viewBuilder/`
**Related**: ADR 0011 (plugin-first shell), BL-053 (forge visual target), BL-054 (Nexus OS Mode)

Every panel, sidebar, and pane in the Nexus shell is a registered plugin contribution loaded by `ExtensionHost`. Arranging them today requires editing TypeScript. The View Builder exposes that composition layer as a visual drag-and-drop tool — move panels, resize splits, configure options — and saves the result as a layout definition file (`.forge/layouts/<name>.layout.toml`) the shell already knows how to read.

The output is immediately usable and optionally exportable as a redistributable shell plugin.

**Key surfaces:**
- Live canvas showing current layout alongside the actual shell — drag to reorder, drag dividers to resize
- Plugin contribution palette — searchable list of all registered `contributes.views` entries
- Per-panel configuration (default size, dock side, float vs. docked)
- Named layouts saved to forge and switchable from the command palette
- "Export as plugin" — generates a valid `manifest.toml` + contribution block

**One prerequisite:** `ExtensionHost` needs a read-only JS introspection API exposing the current contribution layout as a structured snapshot. Currently implicit; must be explicit before the UI can read it.

**Definition of done:**
- `ExtensionHost.getLayoutSnapshot()` returns current panel arrangement as a typed structure
- Builder plugin opens from command palette, renders editable layout canvas
- Drag-and-drop repositioning updates the live shell in real time
- Named layouts persist to `.forge/layouts/` and restore on forge open
- "Export as plugin" generates a runnable plugin directory

---

### BL-054: Nexus OS Mode — Agentic OS methodology layer

**Source**: AI Integration Assessment + Chase AI "Agentic OS" framework analysis (2026-05-06) — full plan in [BL-054-agentic-os-mode.md](BL-054-agentic-os-mode.md)
**Effort**: ~1 week total across 5 independent phases (0.5 + 1.5 + 1 + 2 + 0.5 days)
**Crates**: `nexus-skills` (new `invoke` handler), `shell/src/plugins/nexus/skills/`, new `shell/src/plugins/nexus/osArchitecture/`, new `shell/src/plugins/nexus/observability/`
**Related**: BL-037 (activity timeline), BL-052 (universal activity timeline), PRD-13 (skills), PRD-15 (agent), PRD-16 (workflow)

Nexus is already 85% of the substrate for the "Agentic OS" pattern (Domains → Tasks → Skills → Automations with a markdown memory layer and observability dashboard). The forge is the vault. `com.nexus.skills` is the skill system. `com.nexus.agent` is the sub-agent layer. `com.nexus.workflow` is the automation engine. The shell plugin system is the dashboard. What's missing is the *methodological layer* — conventions, scaffolding, and UI affordances that surface these capabilities as a coherent operating system.

Five independent phases, each shippable standalone:

- **Phase 1 — Forge OS template** (0.5d): `nexus forge init --template os` scaffolds `raw/wiki/output/projects/ops/` layout with a template `CLAUDE.md` memory map.
- **Phase 2 — Architecture panel** (1.5d): New `nexus.osArchitecture` shell plugin renders `architecture.md` (Domains → Tasks hierarchy with four-attribute tags) and cross-references it against actual `.skill.md` and `.workflow.toml` files to surface drift.
- **Phase 3 — Skills invocation** (1d): New `com.nexus.skills::invoke` IPC handler + "Run" button in `SkillsPanel`. Dispatches through `com.nexus.agent::run` with the skill body as system prompt. Foundation-class skills get a "Schedule" button that pre-fills a `.workflow.toml`.
- **Phase 4 — Observability panels** (2d): Three shell plugins — usage panel (token/cost from activity log), automation status panel (foundation workflow last-run/next-run), vault feed panel (file-change events on `raw/wiki/output/`).
- **Phase 5 — OS Setup skill** (0.5d): Built-in skill seeded into OS-template forges that runs the architecture elicitation interview and produces `architecture.md`.

No new backend services. Every phase is UI additions or thin IPC handlers over fully-operational existing infrastructure.

### BL-053: Forge visual target — close the gap to the design mockup

**Source**: Forge Color System mockup + ember-on-slate exploration (2026-05-06) — full plan in [BL-053-forge-visual-target.md](BL-053-forge-visual-target.md)
**Effort**: Phase 1 ~1 day · Phase 2 ~2 days · Phase 3 ~3–5 days · Phase 4 ~3–5 days (~3–4 weeks for the lot)
**Crates**: `shell/src/shell/`, `shell/src/plugins/nexus/editor/`, `shell/src/plugins/nexus/outline/`, `shell/src/plugins/core/editorArea/`, possibly a new markdown-extension surface in `nexus-editor`
**Related**: bundled themes `nexus-ember-dark` / `nexus-ember-light` (delivered 2026-05-06) supply the tokens; this BL styles against them

The bundled ember themes ship the right token values, but the shell renders a much plainer surface than the Forge mockup — mostly because rich rendering (callouts, status pills, frontmatter metadata bars, path-style inline code, ember wikilinks) is renderer/plugin work, not theme work. The companion plan splits the gap into four phases ordered by ROI, identifies what's reachable through theme+CSS alone vs. what needs renderer extensions, and lists the four product decisions that gate code (callout syntax, status data source, font bundling, scope commitment).

**Phase 1 alone delivers ~70% of the visible win.** Subsequent phases are independent and can be greenlit individually.

**Definition of done (per phase):** acceptance criteria filled in when a phase is scoped in — see §6 of the companion doc. The plan itself does not commit to any phase.

### BL-052: Universal activity timeline (beyond AI)

**Source**: AIG-04 follow-up (2026-05-05) — see [../AI-GAPS.md](../AI-GAPS.md#aig-04--activity-audit-panel)
**Effort**: Medium (1 week)
**Crates**: `nexus-kernel` (event bus convention), `nexus-storage`/`nexus-git`/`nexus-terminal`/`nexus-workflow` (emitters), `shell/src/plugins/nexus/activityTimeline/` (consumer)
**Related**: AIG-02 agent approval log shares this schema

Today the `nexus.activityTimeline` pane (BL-037) is AI-only — it hydrates from `com.nexus.ai::activity_list` and subscribes to one bus topic. The same surface is the natural home for **every** observable side effect a user-or-agent triggers: file writes, git commits/pushes, terminal commands, workflow runs, plugin enables/disables, capability grants. Without it, the audit story is partial — the model writing a file is logged, but the user (or another plugin) writing the same file isn't.

**Definition of done:**
- Generalised `ActivityEntry` schema lifted out of `nexus-ai` into `nexus-types` (or a shared `nexus-activity` crate) with an `origin` discriminator: `ai` / `user` / `plugin:<id>` / `workflow:<id>` / `agent:<session>`.
- Bus topic convention: `com.nexus.activity.appended` (kernel-owned), with each emitter publishing a typed payload. Existing `com.nexus.ai.activity_appended` becomes one source.
- Storage emits on file write/delete/rename; git on commit/push/pull; terminal on command-exit; workflow on run-start/end; capability system on grant/revoke.
- Timeline pane gains an `origin` filter chip alongside the existing surface filter; rename plugin id from `nexus.activityTimeline` to `nexus.activity` (with a settings-key migration shim).
- Per-emitter opt-out via plugin config so noisy emitters don't drown the pane.
- Privacy: redactor pass shared with PRD-12 §privacy applies to all emitters, not just AI.

**Why this matters:** transparency parity — once agents (AIG-02) can dispatch tools that span all subsystems, the user needs one place to see every effect, not five separate logs.

## Partially New Features (concept exists in PRDs but design is unspecified)

### BL-007: CRDT-over-Git Transport

**Source**: PRD 11, Section 4.4 (Level 3)
**Effort**: Large (2–3 weeks)
**Crate**: `nexus-git`, new `nexus-crdt`
**Related PRD**: PRD 11 (specified but deferred — requires collaborative editing layer)

Serialize Nexus CRDT state (rich text buffer) as JSON in `.nexus/crdt-state.json`, tracked in git. On push, CRDT state is included in commits. On pull with merge conflict in the CRDT file, apply CRDT merge semantics (operation-based or state-based) for automatic convergence. Fallback to content conflict if CRDT merge fails. Enables multi-user async collaboration via git push/pull without manual conflict resolution. Prerequisite: a CRDT-based editor engine (PRD 08) or collaborative editing layer.

---

## Post-migration carryover gaps (2026-04-24)

Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement. Full descriptions and acceptance criteria in [../OPEN-ITEMS.md](../OPEN-ITEMS.md). Resolved entries are archived in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).

### Open

- [ ] **OI-05: Rust dep duplication** — Blocked on upstream. 34 crates with duplicated versions all trace through `wasmtime 42` (toml/sha2/digest/rand_core/reqwest/rustix/nix/hashbrown) or `portable-pty → filedescriptor` (`thiserror 1`). Revisit after the next wasmtime major release.
- [ ] **OI-15: Manifest signature / provenance** — Optional `manifest.toml.sig` Ed25519 over manifest bytes, verified against trusted-publisher keyring. Marketplace prerequisite (paired with WI-44).
- [ ] **OI-18: Snippet trigger collision detection** — Same hazard as OI-10 but for snippets; emit `plugins:snippet-conflict` and surface a "which plugin wins" control. **Blocked: no snippet registry exists yet.** `Snippet` type + `editor.registerSnippet` are declared in [`@nexus/extension-api`](../../packages/nexus-extension-api/src/index.ts#L101) but never implemented in the shell — every existing "snippet" reference is the unrelated CSS theme-snippet system. Doing this properly means building the script-plugin code-snippet registry first; closer to OI-15 than OI-10 in scope. Reopen when `registerSnippet` lands.

### Resolved (preserved here for cross-reference; full notes in [../OPEN-ITEMS.md](../OPEN-ITEMS.md))

- [x] OI-01 — Settings modal + `registerSettingsTab` API _(2026-04-24)_
- [x] OI-02 — Split-size persistence (editor splits gained drag handles + `setSplitSizes` mutator) _(2026-04-24)_
- [x] OI-03 — Workspace-wide clippy `-D warnings` sweep _(2026-04-24)_
- [x] OI-04 — Kernel-contract promotion TODOs (`SlotId` and `list_archetypes` IPC) _(2026-04-24)_
- [x] OI-06 — ESLint 8 → 9 + typescript-eslint 7 → 8 + xterm → `@xterm/*` scoped _(2026-04-24)_
- [x] OI-07 — Capability grants/denials/path-traversal routed through `audit::*` _(2026-04-24)_
- [x] OI-08 — "Running Extensions" Settings tab (live plugin state + errors + Disable) _(2026-04-26)_
- [x] OI-09 — `pluginsStatusStore` aggregates plugin lifecycle events into a per-plugin `{ state, lastError }` map _(2026-04-26)_
- [x] OI-10 — `KeybindingRegistry.getConflicts()` + `plugins:keybindings-conflict` event with signature-dedup; per-row `!` badge + summary banner in Settings → Keybindings _(2026-04-27)_
- [x] OI-11 — `CommandRegistry.execute` races handlers against a configurable cancel deadline (`shell.command.timeoutCancelMs`, default 5s) with a soft warn at `shell.command.timeoutWarnMs` (default 250ms); emits `command:cancelled` and throws `CommandCancelledError` so the palette can dismiss in-flight state _(2026-04-27)_
- [x] OI-12 — Auto-promotion was already gone on the kernel side; this pass tightened the `confine_path` / `read_file` doc comments to spell out the contract, documented the script-plugin `PlatformFsAPI` path-semantics in `@nexus/extension-api`, and added two kernel tests that pin the loud `PermissionDenied` + traversal-message AC for absolute reads / writes _(2026-04-27)_
- [x] OI-13 — Deleted dead `nexus_kernel::PluginRegistry` + `Kernel::plugins()` (zero callers; `PluginLoader::loaded` is authoritative) _(2026-04-26)_
- [x] OI-16 — `ExtensionHost.deactivateAllForShutdown(perPluginCapMs)` runs every active plugin's `deactivate()` in parallel with a per-plugin soft cap; wired from a `beforeunload` listener in `main.tsx` so flush-on-stop hooks get one last shot before the WebView tears down _(2026-04-27)_
- [x] OI-17 — Deprecation policy lands as a three-way handshake — `@deprecated` JSDoc on the symbol + an entry in `packages/nexus-extension-api/DEPRECATED.md` + an `importNames` row in `shell/eslint.config.js`'s `no-restricted-imports` block. CI gate works without enabling type-aware lint (kept defer-decision intact); empty list today, table headers + protocol ready for the first deprecation _(2026-04-27)_
- [x] OI-20 — Terminal copy/paste — `attachCustomKeyEventHandler` claims `Ctrl+Shift+C/V` (Linux/Windows) and `Cmd+C/V` (macOS) without disturbing plain `Ctrl+C` SIGINT, right-click pastes from clipboard, paste honours bracketed-paste mode (`\e[200~ … \e[201~`) when xterm signals it. Uses `navigator.clipboard.{read,write}Text` from user-gesture handlers; denial logs a follow-up note pointing at `@tauri-apps/plugin-clipboard-manager` _(2026-04-27)_
- [x] OI-14 — `api.workspace.forgeRoot()` + `api.editor.active()/onChange()` exposed via `@nexus/extension-api` _(2026-04-26)_
- [x] OI-19 — Deferred createRoot/unmount in `TerminalPaneView` + `EmptyView`; React 18 commit-phase warnings on drawer collapse + StrictMode double-mount cleared _(2026-04-27)_
- [x] OI-22 — `com.nexus.git` passive-mode crash: `HANDLER_STATUS` now returns JSON null instead of `Err(ExecutionFailed)` so the IPC layer no longer wraps it as `PluginCrashedDuringCall`; shell handles null silently _(2026-05-01)_

---

## Formal release scope (deferred)

Tracked in full in [../REQUIRED-FOR-FORMAL-RELEASE.md](../REQUIRED-FOR-FORMAL-RELEASE.md). Out of scope for personal-tool use; surface here so the IDs are findable.

- [ ] **WI-41: Tauri auto-updater + code-signing + release channel.** ~5–7 eng-days plus 1–3 weeks calendar for signing-cert procurement.
- [ ] **WI-42: Crash reporting & telemetry.** ~5 eng-days, opt-in via Settings.
- [ ] **WI-44: Minimal marketplace.** ~5 eng-days; index schema + shell UI + CLI install + tarball publishing. Paired with **OI-15** (manifest signing) and **F-8.1.1 / F-8.1.2** (iframe sandbox + boundary-bound `pluginId`) before opening to untrusted plugins.
- [ ] **WI-46: Beta → GA logistics.** Triage rubric, test-group recruitment, ship criteria. ~3 eng-days plus 2-week calendar.

---

## Future directions (scoped 2026-04-28)

Previously: design-only docs without committed timelines. **Scoped into the implementation plan on 2026-04-28** — each FD piece now has a BL-* ID (see "Future-direction items minted into the backlog" above) and the docs themselves remain authoritative for design rationale.

- **AI integration directions** — see [../AI-INTEGRATION-DIRECTIONS.md](../AI-INTEGRATION-DIRECTIONS.md). Mapping: "inline rewrite/summarize" → BL-034 (engine) + BL-035 (action surface); "auto-link suggestions" → BL-039; "semantic search" → BL-040; "per-surface chat" → merged into BL-010 (reshape note); "skills as prompts" → composed via BL-021 / BL-022; "agent loops" → merged into BL-027 (same surface); "MCP exposure" (Nexus-as-server) → BL-042; "background indexing" → BL-041. Direction "tool-calling" was already BL-016.
- **Ambient copilot UX patterns** — see [../AI-AMBIENT-COPILOT-PLAN.md](../AI-AMBIENT-COPILOT-PLAN.md). Mapping: Cmd+I overlay → BL-032; context chips + model switcher → BL-033; ghost suggestions → BL-034; right-click AI actions → BL-035 (shared with NB block AI actions); margin suggestions + inline correction → BL-036; activity timeline → BL-037; citations → BL-038; capture → AI → folded into BL-043 (memory quick-capture).
- **AI memory layer** — see [../AI-MEMORY-LAYER-PLAN.md](../AI-MEMORY-LAYER-PLAN.md). Mapping: quick-capture → BL-043; auto-enrichment on save → BL-045; recall hotkey → BL-044; implicit chat context → merged into BL-010 (reshape note); code-aware capture → BL-046; scheduled digests → BL-047.
- **Notion-style block UX out-of-scope follow-ups** — see [../notion-block-ux-plan.md](../notion-block-ux-plan.md). Mapping: drag-to-embed into canvas → BL-048; block-links navigator → BL-049 (gated on block-id stability ADR); side-margin comments → BL-050; block AI actions → merged into BL-035; multi-cursor from multi-block → BL-051.

---

## Settings extraction queue

Inventory of named-constant / hardcoded settings candidates lives in [../../shell/HARDCODED_SETTINGS_AUDIT.md](../../shell/HARDCODED_SETTINGS_AUDIT.md). Pickable in any order; each is a 1–2 hour change.

- [x] **Zoom settings schema** _(shipped)_ — `ui.zoomStep` / `ui.zoomMin` / `ui.zoomMax` / `ui.zoomDefault` registered in `shell/src/plugins/core/zoom/index.ts` with bounds, step, and reset target read through `api.configuration.getValue` + `onChange`.
- [x] **Notification durations schema** _(shipped)_ — `ui.notificationDurationMs` (notificationService), `ui.fileCreationNotificationMs` (fileExplorer), `ui.commandSaveNotificationMs` + `ui.commandCopiedNotificationMs` (terminal `index.ts` schema; SavedCommandsView reads via `useConfigValue`), `ui.copiedNotificationMs` (`nexus.ai`'s `index.ts`; ChatView reads via `useConfigValue`).
- [x] **Search / palette result limits** _(shipped)_ — `search.maxResultsLimit` (schema in `shell/src/plugins/nexus/search/index.ts`, read in `searchRuntime.ts`); `commandPalette.maxResultsLimit` (schema in `shell/src/plugins/core/commandPalette/index.ts`, read by `match.ts`).
- [x] **Long-running operation timeout consolidation** _(shipped)_ — `LONG_RUNNING_OP_TIMEOUT_MS` defined once in `shell/src/plugins/nexus/constants.ts` and consumed by `nexus/agent/index.ts` (`RUN_TIMEOUT_MS`) and `nexus/workflow/index.ts` (`RUN_TIMEOUT_MS`); `SERVICE_CONNECT_TIMEOUT_MS` similarly consumed by `nexus/mcp/index.ts`.
- [x] **Buffer / event caps** _(shipped)_ — `PROCESS_EVENTS_CAP` named in `processesStore.ts`; `UNDO_HISTORY_CAP` lives in `shell/src/plugins/nexus/constants.ts` and is shared by `bases/basesStore.ts` + `canvas/canvasStore.ts` so the user-perceptible undo depth is consistent across surfaces.

---

## Architecture review (2026-04-16) — microkernel adherence

## UI architecture review (2026-04-16) — editor-shell pattern

### Code gaps

### PRD gap — no owner for plugin-contributed tab surfaces

## Editor-shell capability gaps (2026-04-16) — vs VS Code / Obsidian / IntelliJ

### Spec'd in a PRD, not yet implemented

### Half-specced: manifest keys exist, but no UI/wiring spec in PRD-07

### Not in any PRD — new spec work needed

## Architecture audit (2026-04-16) — follow-ups

Findings surfaced by the microkernel + editor-shell audit that weren't already tracked above.

## Microkernel hardening — 2026-04-16 audit findings

Findings from `docs/archive/planning/MICROKERNEL-AUDIT.md` not yet tracked. Ordered by audit priority. The three 🔴 items and F-9.2.1 are blockers before any public plugin marketplace.

### 🔴 Red — blockers for untrusted plugin distribution

_None outstanding._ F-2.1.1 closed 2026-04-22 — see archive.

### 🟠 Orange — address before marketplace or next minor release

### 🟡 Yellow — quality / correctness improvements

## Suspected issues — not fully investigated

Threads from `docs/archive/planning/MICROKERNEL-AUDIT.md §Suspected Issues` that warrant a targeted code walk.

- [ ] **Hot-reload timing on macOS and Windows.** `notify-debouncer-mini` behaviour differs across platforms; F-4.3.1 covers one class of issue. A targeted cross-platform reliability pass on the hot-reload path would be worthwhile before shipping community plugin hot-reload as a feature. **Deferred** — requires running the shell on macOS and Windows hardware to reproduce and measure; this repo's test host is Linux/WSL only. Track for a dedicated cross-platform QA pass once a macOS/Windows CI runner or test machine is available.

## UI audit (2026-04-16) — follow-ups

Findings from `docs/archive/planning/UI-AUDIT.md` not yet tracked above. IDs reference the audit. The 🔴 items plus F-9.1.1 are blockers before any untrusted-plugin distribution.

### 🔴 Red — cannot ship to untrusted users without these

_F-8.1.1 (sub-tasks 1–5: iframe scaffold + sandbox flags, postMessage protocol, `NexusPluginContext` proxy, per-plugin manifest `sandboxed` flag, CSP + tests), **F-8.1.1-fo1** (precompiled `bootstrapSandboxedPlugin` runtime bundle + hello-world migration), and **F-8.1.2** (boundary-bound `pluginId` — orchestrator builds a per-plugin `PluginAPI` from the handshake-set id; `assertValidPluginId` rejects empty / colon-bearing ids) shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). All red-tier UI items now closed; remaining gating for community marketplace launch is **WI-44** (marketplace UI / index / signing) and **OI-15** (manifest signing) at the orange tier._

> F-9.1.1 (validate `api_version` at load time) is the UI twin of the microkernel 🟠 item of the same ID already tracked above — no duplicate entry.

### 🟠 Orange — substantive design gaps, schedule before next external release

- [ ] **Memory budget / accounting for script plugins (UI F-8.3.1).** WASM plugins have `memory_mb = 8` in their manifest; script plugins have no equivalent and allocate against the WebView heap directly. A plugin that accumulates a 500 MB structure OOMs the whole shell. **Now unblocked** — F-8.1.1 shipped 2026-04-28 (per-plugin iframe boundary in `shell/src/host/sandbox/SandboxOrchestrator.ts`). `performance.measureUserAgentSpecificMemory()` is per-frame, so the orchestrator can poll each iframe and attribute usage by `data-sandbox-plugin`. Today still unimplemented; a misbehaving script plugin's RSS is indistinguishable from the shell's. Track as a sandboxed-plugin watchdog enhancement.

### 🟡 Yellow — rough edges to fix opportunistically

### Suspected issues — UI audit §6 spike candidates

Threads from `docs/archive/planning/UI-AUDIT.md §6` not yet confirmed. Each is a 1–2 day targeted code walk or runtime probe.

- [x] **SI-1 — Blob-URL same-origin inheritance.** **Closed 2026-04-28** as a duplicate of F-8.1.1. The blob-URL same-origin inheritance behaviour is confirmed (MDN spec — a `blob:` URL inherits the origin of its creator), but it no longer matters for sandboxed plugins: `manifest.sandboxed === true` routes the plugin through `SandboxOrchestrator`, which mounts a null-origin iframe (`sandbox="allow-scripts"`, no `allow-same-origin`). Inside that iframe the host's blob URL is reachable for the bundle import but the iframe runs at `event.origin === "null"` so it can't read `window.parent.document` / `document.cookie` / Tauri's IPC bridge. Legacy non-sandboxed plugins still inherit the shell's origin — that's the "first-party only" trust posture documented in `DEPRECATED.md`.
- [ ] **SI-6 — `PluginManager` Mutex contention.** **Deferred — requires a dedicated load-test harness that doesn't exist yet.** Measuring requires 20+ chatty plugins and wall-clock profiling while a human drives the UI, which this environment cannot replicate. Hypothesis: per-plugin dispatch already uses `try_lock` + reentrancy guard + per-plugin backend mutex, so the `PluginManager` top-level mutex is only held during scan/load/unload/reload — not during steady-state dispatch. If the hypothesis holds this is cosmetic; if not, the fix is likely `RwLock<HashMap<id, …>>` inside the loader with per-plugin reader locks. Track as an explicit Phase-3 stability task once the load-test tooling exists.

## Audit findings (2026-04-28)

> Cross-PRD docs audit ([DOCS_AUDIT_2026-04-28.md](DOCS_AUDIT_2026-04-28.md)) — items spec'd in a PRD that are not yet built and were not previously assigned a backlog ID. Each cites the PRD section, target crate, and estimated effort. Effort scale: small ≈ ½–2 days, medium ≈ 3–10 days, large ≈ 2+ weeks.

_BL-010 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-011 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-012 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-012 close-out shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-013 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-015 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-016 shipped 2026-04-28 across three commits — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-019 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-021 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-022 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-023 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-025 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-027 shipped 2026-04-29 — see BACKLOG_COMPLETED.md._

_BL-028 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-029 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-030 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-031 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

## Future-direction items minted into the backlog (2026-04-28)

> The four future-direction tracks were brought into the implementation plan on 2026-04-28. The IDs below carry their FD doc as design rationale; the original entries in the "Future directions" section now point here. Effort scale: S ≈ ½–2 days, M ≈ 3–10 days, L ≈ 2+ weeks.

_BL-032 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-033 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-034 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-035: Right-click AI actions + block AI actions (shared registry)

_BL-035 shipped 2026-04-29 — see BACKLOG_COMPLETED.md._

_BL-036 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-037 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-038 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-039 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-040 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-041 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-042 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-043 shipped 2026-04-28 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-044 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-045: MEM auto-enrichment on save

_BL-045 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-046 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-046 phase 3 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### BL-047: MEM scheduled digests

_BL-047 shipped 2026-04-29 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-048 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-048 phase 3 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-049 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-050 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-051 shipped 2026-04-30 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

### Verification notes (no BL ID — informational)

- **ADR-0009 keyring hard-fail enforcement** — Verified 2026-04-30 and resolved as **OI-21** the same day: `SecurityCorePlugin::on_init` now runs an injected `KeyringProbe` (default `CredentialVault::new().available()`) and returns `PluginError::LifecycleError` with the platform hint when the OS keyring is unavailable. Bootstrap propagates the lifecycle error so frontends exit non-zero. See [../OPEN-ITEMS.md](../OPEN-ITEMS.md) §OI-21.
- **PRD-04a MockPluginContext / MockEventBus** — referenced in template tests as TODO but not yet exposed from `nexus-plugin-api`. Low priority; community plugin authors are not yet writing many tests, and the issue surfaces only when someone tries.

## Decisions — PRD-04 audit (2026-04-17)

## Design notes — 2026-04-28

- **Global cross-surface undo is a non-goal.** Considered alongside BL-030. Per-surface undo is the idiom in VS Code / Obsidian / IntelliJ; a unified Cmd+Z spanning editor + canvas + bases + file ops creates ambiguous "what does this undo right now" behaviour and would require every mutating IPC handler to register an inverse op against the file-as-truth + IPC-only invariants. The right primitive for cross-surface time-travel in this architecture is git-based history (point-in-time restore via the existing commit graph) rather than a unified action stack. New BL items for undo should be scoped to a single surface.

### Phase-0 ADRs (gating the implementation plan)

Two design decisions sat on the critical path of the multi-phase rollout. Both Phase-0 ADRs were drafted, reviewed, and accepted on 2026-04-28; the rest of the plan now executes against their answers.

- **[ADR-0017: Block-ID stability via lazy inline stamping](../adr/0017-block-id-stability.md)** _(Accepted 2026-04-28)_ — chooses HTML-comment stamping inside markdown, materialised on-demand the first time a block is referenced cross-session. Unblocks BL-048 (drag-to-embed), BL-049 (block-links navigator), BL-050 (side-margin comments).

- **[ADR-0018: Local embedding backend — fastembed-rs](../adr/0018-embedding-backend.md)** _(Accepted 2026-04-28)_ — chooses fastembed-rs over candle and sqlite-vec's bundled gguf path on the 5-axis comparison (model quality, RAM, cold-start, cross-platform binary cost, license). Unblocks BL-019 plus the nine downstream consumers (BL-038 / BL-039 / BL-040 / BL-041 / BL-044 / BL-045 / BL-047 and the BL-010 / BL-011 / BL-034 retrieval variants).

---

## Implementation plan (2026-04-28)

> Phased rollout for every non-deferred BL item including the future-direction items minted as BL-032..BL-051 above. Cross-references all live in those entries; this section is the schedule.

### Agent-load assumptions

- **One agent ≈ 1–3 days of focused work, single tractable PR.** Items rated >medium must split into multiple agent-sized chunks (splits are listed per-item below).
- **2 concurrent foreground agents + 1 background long-runner.** The fg slots are sized so the human review queue stays drainable; the bg slot is reserved for multi-week work (F-8.1.1 in particular).
- **Agents that overlap files waste work in merges**, so file-conflict groups must serialize within their group.
- Retune assumptions: 1 fg + 0 bg roughly doubles the timeline; 3 fg + 1 bg lets BL-022 / BL-029 / BL-037 land earlier and compresses Phases 3–6 by ~3 weeks.

### File-conflict groups (serialize within group)

| Group | Items |
|---|---|
| Bases plugin | BL-015 → BL-030 → BL-031 |
| nexus-cli AI subcommands | BL-010 → BL-011 |
| nexus-mcp client | BL-023 → BL-025 |
| nexus-mcp server | BL-042 (distinct from client group above) |
| Skills | BL-021 → BL-022 |
| nexus-ai (Cargo + provider mods) | BL-016, BL-019 — keep one full PR apart |
| Shell host / sandbox | F-8.1.1 → F-8.1.2 |
| AI overlay surface | BL-032 → BL-033 → BL-034 |
| Memory inbox surface | BL-043 → BL-046 |

### Hard dependency chain

| Prereq | Unblocks |
|---|---|
| BL-016 tool-calling | BL-010, BL-011, BL-027, BL-035, BL-036, BL-044 |
| BL-019 embeddings | BL-038, BL-039, BL-040, BL-041, BL-044, BL-045, BL-047, plus BL-010/11/34 retrieval variants |
| BL-013 stream convention | future plugin streaming work |
| BL-015 trash view | BL-030 (reuses row-restore code path) |
| BL-030 undo stack | BL-031 (paste = one undo step) |
| BL-032 Cmd+I overlay | BL-010 / BL-011 / BL-033 / BL-044 (shared UX) |
| BL-041 indexing daemon | BL-045 (auto-enrichment reads the index) |
| F-8.1.1 iframe sandbox | F-8.1.2, marketplace |
| Block-id stability ADR | BL-048, BL-049, BL-050 |

### Phased rollout

| Phase | Wks | Agent A (fg) | Agent B (fg) | Agent C (bg) | Phase exit criteria |
|---|---|---|---|---|---|
| **0 — Quick wins + ADRs** | 1.5 | settings ×5 + BL-009 + BL-015 | (idle / pulls Phase-1 prep) | block-id ADR + embedding-backend ADR | both ADRs signed and recorded under "Decisions"; trash view live in bases; foundations clear for Phase 1 |
| **1 — Foundations** | 6 | **BL-016** (split ×3) | **BL-013** stream convention + **BL-032** Cmd+I overlay | **F-8.1.1** kickoff (split ×5; per-plugin migration posture — see below) | BL-016 merged → unblocks AI surfaces; BL-032 lands → unblocks BL-010/11; F-8.1.1 sandbox scaffold reachable |
| **2 — Bases + AI CLI/UI** | 4 | BL-030 → BL-031 → **BL-043** quick-capture hotkey | BL-010 + BL-034 ghost suggestions (paired engine) → BL-011 | F-8.1.1 cont. | bases polish complete; shared chat + completion engine live in CLI and editor; global capture hotkey live |
| **3 — Skills + MCP client + small AMB** | 5 | BL-021 (split ×4) → BL-022 | BL-023 → BL-025; BL-033 chips/switcher slots in | F-8.1.1 wraps; **F-8.1.2** | skills composition lands; MCP client gains WS/SSE + auth |
| **4 — Heavy AI core** | 8 | **BL-019** (split ×4) | **BL-027** agent loops (split ×5) | BL-035 right-click + block-AI actions | BL-019 unblocks all retrieval consumers; BL-027 unlocks orchestrated agents |
| **5 — Retrieval consumers** | 5 | BL-040 semantic search → BL-039 auto-links → BL-038 citations | BL-041 indexing daemon → BL-045 auto-enrichment → BL-044 recall | BL-047 scheduled digests | the BL-019 dependency tail drains |
| **6 — Heavyweights + multi-window** | 8 | BL-028 workflow umbrella (split ≥6) | BL-029 multi-window → BL-037 timeline → BL-050 side-margin comments | BL-042 Nexus-as-MCP-server | multi-window opens, panes follow; workflow gains every spec'd trigger |
| **7 — Editor + Notion polish** | 6 | BL-012 DB query blocks (split ×5) | BL-049 block-links → BL-051 multi-cursor → BL-048 drag-to-embed | BL-046 code-aware capture; BL-036 margin / inline correction | tail polish; backlog drained to deferred-only items |

Cumulative: ~44 weeks raw, ~50–55 with PR-review buffer at the assumed 2 fg + 1 bg slot budget.

### Sub-task splits (items >medium)

| BL | Split |
|---|---|
| BL-016 | (1) `ToolRegistry` + `ToolExecutor` core, (2) Anthropic + OpenAI tool-call wire format, (3) Ollama tool-call format + dispatch loop |
| BL-019 | (1) backend impl (per ADR), (2) `EmbeddingModel` trait + cache, (3) RAG wire-up, (4) batch indexer hook for BL-041 |
| BL-021 | (1) parse `depends_on`, (2) topo + cycle detection, (3) prompt-fragment merge order, (4) conflict-warning UX |
| BL-027 | (1) `AgentOrchestrator` skeleton, (2) `delegate`, (3) `parallel`, (4) `pipeline`, (5) shared scratch state + replay hooks |
| BL-028 | one agent per primitive: webhook trigger → git_event → mcp_event → parallel scheduler → retry/backoff → AI step types → templates |
| BL-012 | (1) executor over `apply_view`, (2) CM6 widget, (3) decoration plumbing, (4) undo integration, (5) filter/sort UX |
| F-8.1.1 | (1) iframe scaffold + sandbox flags, (2) postMessage protocol, (3) `NexusPluginContext` proxy, (4) per-plugin migration via `manifest.toml` `sandbox: "iframe" \| "legacy"` flag, (5) CSP + tests. Per-plugin migration posture (decided 2026-04-28) — community plugins keep working during the multi-week build window; cost is +1–2 wks vs hard cutover. |

### Risks tracked

1. **Phase-2 lock-in.** BL-010 / BL-011 / BL-034 share an engine. If BL-032 (Cmd+I) shifts after Phase-1, three tracks rework.
2. **BL-019 is the single biggest schedule bet.** Nine tracks queue behind it; a backend mistake costs weeks. The Phase-0 ADR is non-negotiable.
3. **BL-029 promotion** means earlier multi-window, which means earlier per-window plumbing problems for plugin lifecycle. Worth a lightweight design pass before Phase-6 begins.
4. **F-8.1.1** runs 1–2 eng-months in the background. If it slips into Phase-4, BL-035 (right-click in iframe-sandboxed plugins) gets harder to test.
5. **BL-022 absorbs MEM "code-aware capture" UI patterns** in Phase 3 — make sure the skill-editor surface is pluggable enough to host them rather than blocking on a separate capture UI.

### Phase-0 entry / exit checklist

- [x] Block-id stability ADR drafted, reviewed, recorded under "Decisions".
- [x] Embedding-backend ADR drafted with the 5-axis comparison (quality / RAM / cold-start / binary cost / license), recorded under "Decisions".
- [x] BL-009 mermaid whole-file viewer merged.
- [x] BL-015 bases trash view merged.
- [x] Settings extraction queue (5 items) — all shipped; see "Settings extraction queue" section above for per-item file references.
- [x] No outstanding regressions in `cargo test --workspace` / `pnpm --filter nexus-shell test` / `scripts/check_ipc_drift.sh` _(verified 2026-04-30 on `claude/review-backlog-AOGDH`: 75 result blocks all `0 failed`; 681/681 shell tests; drift `OK — generated trees match HEAD`)_.

(BL-043 quick-capture hotkey moved to Phase 2 — Tauri global-hotkey plumbing is a 1–2 day task disguised as "small" and would eat into ADR review.)
