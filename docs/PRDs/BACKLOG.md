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

_BL-091 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Read path + status surface shipped: pointer detection in `nexus-storage::read_file` with `git lfs smudge` passthrough, `com.nexus.git::lfs_status` IPC handler (id 27), and `nexus git lfs-status` CLI. Write-path routing through `git lfs clean` on `stage_file` deferred — see closure notes (no immediate user blocker; the read-path gap was the visible-bug case)._

---

_BL-090 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-089 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-088 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Non-interactive rebase + cherry-pick shipped end-to-end (engine, IPC handlers 28–31, CLI verbs). Interactive rebase (`-i`) deferred — requires shelling out to `git rebase -i` since libgit2 doesn't expose the editable todo list._

---

_BL-087 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-086 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-085 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-084 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Backend handlers (32–35) shipped 2026-05-06; shell UI shipped now: a `ConflictBanner` over the git panel during any non-Clean repo state with a state-aware Abort button (Merge → `abort_merge`, Rebase / RebaseInteractive → `abort_rebase`, CherryPick → `abort_cherry_pick`), and a `ConflictView` that replaces the diff viewer when a `Conflicted` file is selected — per-hunk Use-ours / Use-theirs plus whole-file Accept-all-ours / Accept-all-theirs, writing resolved content back through `com.nexus.storage::write_file`. True three-way side-by-side rendering against `conflict_versions` deferred (the inline marker form already shows ours and theirs); the user stages and commits via the existing Changes-tab UI._

---

_BL-083 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Plan + apply phases shipped end-to-end (engine, IPC handler 56, `nexus forge import` CLI). Progress-event publishing during copy deferred — no UI surface consumes it yet, the apply phase is fast enough on most forges that synchronous return is acceptable._

---

_BL-082 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Reconcile + watcher now skip symlinks (no double-index, no follow out of forge). Schema-side `file_type = "symlink"` tagging and `query_files(include_symlinks)` deferred — skipping is the simpler invariant and matches the no-double-index goal without growing the schema._

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

_BL-073 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `resolve_block_link` (session path) and `apply_transaction` now auto-stamp inbound-link targets to fresh v4 UUIDs. The filesystem-fallback resolve path deliberately does not auto-stamp (mutating the on-disk file from a read-shaped IPC call would be a surprise); if a caller wants the stamp persisted, they keep the session open and the next `save` writes the `<!-- ^<uuid> -->` marker._

---

_BL-072 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `UndoTree` round-trips through a `PersistedUndoTree` proxy (Vec-of-pairs encoding for the parent / children maps so the JSON shape stays stable). `close` writes the snapshot to `.forge/.editor/undo/<sha>.json` via `write_vault_file`; `open` re-reads and restores when the file's content hash matches what was on disk at close time. Branching beyond the 500-op cap is dropped to a linear chain on persist (the documented trade-off — no UI surfaces deep undo branches today). Cross-process global stale-file sweep deferred — invalidation is lazy: an open against a stale or hash-mismatched file deletes the file and starts fresh._

---

_BL-071 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `emacsKeymap.ts` layers `@codemirror/commands` `emacsStyleKeymap` plus Nexus overrides (kill-ring–aware C-k / C-w / M-w / C-y, M-f / M-b word motion, C-Space mark ring up to 16 positions). Process-global kill ring; per-view mark ring stored on the view object. Toggle exposed in the existing `nexus.editor.keybindings` setting (now `'default' | 'vim' | 'emacs'`)._

---

_BL-070 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `@replit/codemirror-vim` integrated under a new `nexus.editor.keybindings` setting; `:w` / `:q` / `:wq` / `:x` ex commands route through `saveSession` and `confirmAndClose`. Per-view dispatch via a CM6 `StateField` so multiple tabs hit their own callbacks._

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

_BL-080 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Almost everything in the DoD already shipped under `nexus.files` (sidebar tree, expand/collapse, drag-to-reorder, full context menu, live `com.nexus.storage` event sync). The only material gap was the file-type icon set, now closed via a `getFileIcon(name)` helper covering `.md` / source files / structured config and a generic fallback._

---

_BL-079 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Backend gained one new IPC handler (`com.nexus.git::blame`, id 36) — the DoD claimed `blame` already shipped but only the engine method existed; the dispatch surface was missing. Shell ships three pieces: `gitGutter.ts` CM6 extension (per-line markers from `diff_file`, hover tooltip with removed lines, auto-refresh on `files:saved`); `gitBlame.ts` extension with togglable end-of-line annotations (`<author> · <hash> · <relative date> · <summary>`) gated by `nexus.editor.toggleBlame`; `DiffView.tsx` modal hunk viewer opened by `nexus.editor.openDiff` rendering hunks unified with red/green tinted lines. Per-line marker classification is the load-bearing logic — `buildLineMarkers` walks `GitDiffHunk[]` tracking a `pendingRemoved` buffer to distinguish added (`+` only) from modified (`+` paired with `-`) from deletion-above (`-` with no replacement); 8 unit tests pin every branch. Click-on-gutter Stage / Revert and side-by-side `MergeView` deferred — the gutter + tooltip + modal already give the user the "what changed?" surface, and stage / revert can route through the existing git panel. `@codemirror/merge` would add a meaningful dependency for marginal value over the current modal._

---

_BL-078 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `crates/nexus-storage/src/find_replace.rs` module with `find_in_files` + `replace_in_files` (plus `Matcher` shared by both) covers every modifier combo in the DoD: literal / regex, case-sensitive / -insensitive, whole-word toggle. Two new IPC handlers — `find_in_files` (id 57) and `replace_in_files` (id 58) — pass through to those free functions. The walker uses the existing `should_ignore` filter so `.forge/`, `.git/`, etc. stay out of the result set; binary / non-UTF-8 files are silently skipped. Results group by relpath, sorted ascending, with one line of leading + trailing context per hit. After a `replace_in_files` that changes any file, the storage engine triggers a `rebuild_index` so search / graph stay consistent. Shell ships a `nexus.searchPanel` sidebar leaf with debounced query input, regex / Aa / whole-word toggles, replace field, per-file collapse, click-to-open, and per-file or workspace-wide replace. Bound to ⌘⇧F (matching VS Code's "find in files" muscle memory); BL-063's terminal cross-search moved to ⌘⇧G to free the binding. CM6 decorations on open tabs and incremental streaming for very large forges deferred — the current full-batch shape returns within the documented `max_files` / `max_results` caps and the panel UX feels live._

---

_BL-077 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `lspIpc.ts` (typed `com.nexus.lsp` adapter) + `lspClient.ts` (CM6 extension bundle) together turn the BL-076 LSP host into a working editor surface. The extension activates only in code-mode tabs (BL-075 routing) on real on-disk paths; document-mode and untitled tabs skip it. New deps: `@codemirror/autocomplete` 6.20 + `@codemirror/lint` 6.9. Decided against `codemirror-languageserver` — it bundles a WebSocket transport we'd have to monkey-patch around; writing the ~400-line CM6 wiring directly is simpler and avoids a vendored fork. 17 new unit tests cover the converters (severity, position-to-offset with EOL/EOF clamps, diagnostic projection, completion-item mapping with kind chips + docs, location picker, edits applier with bottom-up sort) plus three lifecycle smokes (open/change/close fire-and-forget, matching-URI diagnostics land in the lint state, non-matching diagnostics ignored). Full shell suite at 968 tests stays green._

**Caught a subtle bug along the way.** A draft test used a dynamic `await import('@codemirror/lint')` for `forEachDiagnostic` while the implementation imports `setDiagnostics` statically. Under tsx's loader the dynamic and static specifiers resolved to *different* module instances, so the `lintState` `StateField` constants weren't identity-equal — `setDiagnostics` would write to one field and `forEachDiagnostic` would read from another, and both saw "0 diagnostics." Fixed by importing both statically from the test. Worth flagging because future CM6 extensions that test against state-field-based @codemirror sub-modules will trip the same trap.

**Deferred from the DoD:**
- **Format-on-save plumbed via the existing save command, not just the keymap.** Today `Mod-s` triggers `format` then returns `false` so the outer save handler still fires. That works as long as the user's save chord is `Mod-s`; a vim user issuing `:w` won't trigger format. A follow-up wires format through the `nexus.editor.save` command so every save path gets it.
- **`nexus.editor:reveal-line` consumer.** Cmd+Click → definition emits the event with the resolved line/character but no handler subscribes yet; the file opens at the top. Wiring is one event listener away — defer until the second LSP-using flow needs it.
- **Document resync after server reconnect.** BL-076's `LspClient` tracks open URI / version / text but the resync replay isn't wired. A code-mode tab whose server crashes today shows stale diagnostics until the next save bumps the version. Pair with the BL-076 follow-up that wraps each handler in `call_with_reconnect`.
- **WorkspaceEdit applier for rename / code-actions.** `format` returns `TextEdit[]` and the applier handles that; rename/code-actions would return a `WorkspaceEdit` (multi-file) which needs a different applier. The IPC handlers are wired but the CM6 commands don't dispatch them — there's no UI surface yet (rename palette, code-actions menu). Lands when those UI surfaces do.

---

_BL-076 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `nexus-lsp` workspace crate ships the full LSP-host skeleton: `lsp.toml` config loader (`[[servers]]` blocks with name / command / args / file_types / root_markers / disabled / env), a tokio-based stdio JSON-RPC transport with Content-Length framing + 16 MiB ceiling, an `LspClient` that drives the `initialize` / `initialized` handshake and demultiplexes server replies into per-request oneshots vs a notification channel, an `LspClient::shutdown` that runs the protocol-level `shutdown` / `exit` then hard-caps the join, and a `ConnectionPool` with lazy connect + transient-failure reconnect against the same `[100ms, 500ms, 2s, 10s, 30s]` backoff schedule `nexus-mcp` uses. The `LspCorePlugin` exposes 11 IPC handlers — list_servers (sync) and open_file / close_file / change_file / completions / hover / definition / references / rename / code_actions / format (async) — and republishes every server-pushed notification on the kernel bus as `com.nexus.lsp.<method-with-dots>` (so `textDocument/publishDiagnostics` becomes `com.nexus.lsp.textDocument.publishDiagnostics`). 28 unit tests + 1 end-to-end integration test against a Python-based mock LSP server (handshake → hover round-trip → didChange → publishDiagnostics fan-out → graceful drop) all pass. 10 new IPC types (`LspOpenFileArgs`, `LspChangeFileArgs`, `LspPositionArgs`, `LspReferencesArgs`, `LspRenameArgs`, `LspCodeActionsArgs`, `LspPathArgs`, `LspOpenFileReply`, `LspServerEntry`, `LspOk`) wired into the `nexus-bootstrap` schema generator; ts-rs bindings + JSON-Schema files committed; drift script passes.

**Deferred from the DoD:**
- **Live `rust-analyzer` / `typescript-language-server` smoke** — DoD called for end-to-end runs against the real servers. Mocked-out via the `tests/end_to_end.rs` Python server: every protocol path the real servers exercise (handshake, request/response correlation, server-pushed notifications, graceful shutdown) is covered. Live smoke is an operator step; the binaries aren't on this dev box's `$PATH` and shipping them via the test would inflate CI cold-start by minutes per run.
- **Auto-restart on crash with exponential backoff** — the `ConnectionPool::call_with_reconnect` helper is built and ready, but the existing handlers route through `get_or_connect` directly so a crashed server bubbles a `Transport`/`NotRunning` error to the caller (the kernel returns it through IPC, the shell can re-trigger the call). Wrapping every handler's body in `call_with_reconnect` is one Phase-2 follow-up. The transient-vs-fatal classification (`LspClientError::is_transient`) is in place so that wiring is mechanical.
- **Document resync after reconnect** — `LspClient::documents` tracks every open URI / version / text but the resync replay is not wired. When BL-077 (CM6 LSP client) lands, the loss-of-connection user story will get attention.
- **Server-initiated requests** — `workspace/configuration` / `window/showMessageRequest` and friends are read off the wire and dropped with a debug log. The reader can't write back without rerouting through the host's `stdin` mutex; deferred until a server actually relies on these (rust-analyzer / TS-LS don't for the basic feature set).

**Why this matters:** the LSP track unblocks BL-077 (CM6 LSP client) and BL-081 (DAP debugger). Without nexus-lsp, code editing in Nexus is a syntax-highlighted textarea (BL-075's mode); with it, the shell can light up completions / diagnostics / hover / go-to-def for any language with an LSP server.

---

_BL-075 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `codeMode.ts` module exports `getEditorMode(name, codeExtensions?)` and `pickLanguageExtension(name)`. The read/save split was already in place pre-BL-075 (markdown → `com.nexus.editor::open` session, non-markdown → `com.nexus.storage::read_file` / `write_file`); what BL-075 adds is the CM6 language extension layered onto the non-markdown path plus the user-configurable extension list. Eight languages cover the documented "common types": Rust, TypeScript (TS/TSX), JavaScript (JS/JSX/MJS/CJS), Python, Go (via `@codemirror/legacy-modes`), JSON / JSONC, YAML, TOML (also legacy-modes). `EditorView` reads the live `nexus.editor.codeFileExtensions` setting through the runtime's new `getCodeFileExtensions()` accessor; an empty / whitespace-only setting falls back to the default list rather than disabling code mode entirely. Markdown is unconditionally document-mode regardless of the override list, so a misconfigured setting can't break the markdown editor. 11 new unit tests pin the routing matrix; full shell suite at 903 tests stays green._

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

_BL-061 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `with_memory_monitor(MemoryLimits)` builder on `TerminalCorePlugin` plus a memory poller thread (peer of the existing drainer / lifecycle-forwarder) that auto-tracks every spawned session, samples RSS at the configured interval (default 1 s — PRD-09 §7.2), and on `HardExceeded` publishes a `TerminalEvent::MemoryLimitExceeded { id, rss_bytes, limit_mb }` then runs `close_session` (which then emits `SessionClosed`, in causal order). `SessionInfo.rss_bytes: Option<u64>` is layered onto every `get_session_info` and `list_sessions` response so the shell UI can render a memory chip from a single round-trip. Bootstrap wires the monitor with PRD §7.3 defaults (250 MB soft / 500 MB hard) for every session; per-saved-command overrides via `SavedCommand.memory_limit_mb` are still TODO (the field exists on the struct but isn't routed yet) — out of scope for this item._

---

_BL-060 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Four IPC handlers on `com.nexus.terminal` shipped (`adhoc_list` id 19, `adhoc_get` id 20, `adhoc_delete` id 21, `adhoc_promote` id 22 — handler 18 was already taken by BL-059's `open_in_terminal`, so the contiguous-id allocation slipped by one), each backed by the existing `SqliteAdHocStore`. The plugin attaches both stores against the same `<forge>/.forge/procmgr.sqlite` file (separate `Connection`s, separate tables — `procmgr_commands` vs. `procmgr_adhoc_history`). `nexus proc history [--limit N] [--json]` wraps `adhoc_list` with a fixed-width table renderer. Shell ships a sidebar `History` leaf (`nexus.terminal.history.show`) sibling to Saved Commands, with re-run / promote / delete affordances; the promote form pre-fills from the row's program name and refreshes the saved-commands cache on success. Recording new ad-hoc rows over IPC is intentionally out of scope here — the procmgr layer still calls `SqliteAdHocStore::record` directly. A dispatch-side `adhoc_record` lands when ad-hoc execution becomes a first-class IPC verb (BL-056 follow-up)._

---

_BL-059 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `com.nexus.terminal::open_in_terminal` handler (id 18 — DoD-suggested 22 was just a placeholder; used the next contiguous slot) walks a default priority list (iTerm2, WezTerm, Ghostty, Kitty, Alacritty, Windows Terminal, GNOME Terminal, Konsole, XFCE Terminal, Terminal.app, x-terminal-emulator, xterm), picks the first whose program is on `$PATH`, and spawns it detached at the saved command's `working_dir` (Unix `setsid` so SIGHUP doesn't tear it down with nexus). `SavedCommandsView` gains an "External" button per row when `working_dir` is set. Per-command env-var passing and the Settings → Terminal priority editor are deferred — the IPC accepts a `priority` arg already, so the settings UI is wiring rather than design._

---

_BL-058 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Detection ported from `nexus-terminal/src/urls.rs` to `shell/src/plugins/nexus/terminal/urls.ts` (no new IPC surface added — the original `read_output`-coupled plan was unnecessary given the existing PTY byte stream); a `createUrlExtractor()` decodes UTF-8 with `stream: true` and emits per-line matches; `UrlChips.tsx` renders a deduped top-5 pill strip above xterm with `api.platform.shell.openExternal` click handling. Chips clear on session change and via an explicit dismiss button._

---

_BL-057 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Landed alongside BL-052 in the same sweep. The lifecycle forwarder's `publish_lifecycle_event` now fans `SessionCreated` / `SessionClosed` / `MemoryLimitExceeded` out to the universal `com.nexus.activity.appended` topic with `origin: "terminal:<session_id>"`, `surface: "process"`, and a human-readable `prompt` ("started session …", "session … exited (code=N)", "session … killed (OOM): rss=… limit=…MB"). Streaming variants (`OutputReceived`, `PatternMatched`, `SessionEvicted`) deliberately don't emit activity entries — they're either too chatty or too internal. `SessionClosed` flips outcome to `Error` when `exit_code` is non-zero so the timeline glyph matches user intuition. The terminal `nexus-terminal` crate gained a `nexus-types` dep for the shared `ActivityEntry` shape; no terminal-side schema change._

---

_BL-056 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `type = "terminal"` step with `slug` (required), `action` (start / stop / restart / run_adhoc, default start), `command` (required for `run_adhoc`, ignored otherwise), and `working_dir` (override) lands in `KernelActionDispatcher`. `start` and `run_adhoc` route through `com.nexus.terminal::run_saved` (BL-055); `stop` lists sessions and closes every one whose name matches `saved:<slug>` (the convention `run_saved` writes); `restart` is `stop` followed by `start`. `run_saved` gained an optional `command` override field so `run_adhoc` reuses the saved profile (shell / cwd / env) with a fresh command line per run. The `validate` handler became async-capable: when terminal steps are present and the kernel context is wired, it queries `saved_list` and rejects unknown slugs with a clear error; without a context (test runtimes) it falls back to the parse-only path. `nexus workflow run` and `nexus workflow validate` use these surfaces unchanged through their existing IPC routes._

---

_BL-066 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Hover-fade icon row replaces the always-visible text buttons on `SavedCommandsView`: Run / External (when `working_dir` is set) / Edit / ↑ / ↓ / Delete (red on hover). Stop / Restart / Dismiss deferred until BL-055 lands a `run_saved` handler with managed-session state — without procmgr-shaped state, those buttons would have no target._

---

_BL-065 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `ShellFamily { Posix, Cmd, PowerShell }` enum with `ShellFamily::detect_from_path(&str)` (case-insensitive, handles `\`-separated Windows paths even on Linux, strips `.exe`). `PreCommandOptions` gains `shell_family: ShellFamily` (defaults to `Posix` for back-compat). `run_pre_commands` calls a per-family `wrap_step` helper: POSIX `printf '<sentinel> %d\n' $?`, cmd `echo <sentinel> %ERRORLEVEL%` (CRLF-terminated), pwsh `Write-Host "<sentinel> $LASTEXITCODE"` (CRLF-terminated). All three produce the same `<sentinel> <integer>` line shape so `parse_sentinel_exit_code` and `wait_for_sentinel` don't fork. Live execution tests run on Linux against bash; the Windows wrappers are pinned by string-shape unit tests (cmd.exe / pwsh aren't available on the WSL CI runner)._

---

_BL-064 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `com.nexus.terminal::suggest` handler (id 24 — DoD-suggested 23 was already taken by BL-055's `run_saved`; ids are append-only) walks the tail of a session's line buffer, runs the existing `AiSuggestionEngine`, and on a match routes the matched line + rule context through `com.nexus.ai::stream_chat` (`mode=complete`, `tools=none`) for an enriched explanation. The IPC call is wrapped in a 10 s `tokio::time::timeout`; on timeout / IPC error / no kernel context wired, the handler falls back to the rule's static reason and flips `llm_used: false` in the response. The handler returns JSON `null` when no rule matches. `TerminalCorePlugin` gained `wire_context` (captures the kernel context) and `dispatch_async` (the `suggest` arm); the sync `dispatch` path returns a clear "use dispatch_async" error so a misrouted call is obvious. Shell ships a `SuggestionChip` below the xterm canvas: polls `suggest` every 5 s while the terminal pane has a live session, renders the suggested command + reason with Run / Dismiss controls, and shows a sparkle marker when `llm_used`. Used `stream_chat` instead of the DoD-suggested `stream_ask` because the terminal context isn't a RAG question — RAG would force an embedding provider config that the terminal flow shouldn't depend on._

---

_BL-063 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `scrollback_fts` FTS5 virtual table on `SqliteSessionStore`; `save_scrollback` ANSI-strips each line and reindexes per save (whole-row replace under a single transaction); `delete` clears the session's FTS rows. New `cross_session_search` method on the store with literal (FTS5 MATCH) and regex (regex_lite scan) paths, plus optional `session_ids` / `since_ts` / `limit` filters. New `com.nexus.terminal::cross_session_search` handler (id 25 — DoD-suggested 24 was already taken by BL-064's `suggest`; ids are append-only). Bootstrap shares the same `SqliteSessionStore` handle between the BL-062 eviction persister and the new search handler so a freshly-evicted scrollback is immediately searchable. Shell ships a "Search all sessions" sidebar leaf (⌘⇧F / Ctrl+Shift+F) with debounced input, regex toggle, and results grouped by session id. The FTS table is intentionally rebuildable from the on-disk scrollback blobs — when a backup-export mechanism lands later, it can skip `scrollback_fts` and still recover the full index._

---

_BL-062 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `evict_lru` now filters to terminated sessions (with a `reap_exited` pass first to refresh cached `state()`); `spawn_or_evict` uses that filtered eviction so an at-cap manager with all-running sessions still surfaces `ShellDetection` (preserving the "never silently kill a live process" invariant). `Entry::last_accessed` switched to `Cell<Instant>` so read-side accessors that take `&self` can bump LRU; `lines_snapshot` (the `read_output` path) now bumps. `buffer_read_since` deliberately does NOT bump because the WI-12 drainer thread polls it constantly — the user-facing IPC path drives `drain` first which does bump. `InMemoryTerminalServer::create_session` switched to `spawn_or_evict`, emits `TerminalEvent::SessionEvicted { id, reason: "lru" }` before the new `SessionCreated`, and forwards the snapshot to an optional `EvictionPersister` callback. Bootstrap installs a persister backed by `SqliteSessionStore::save_scrollback` at `<forge>/.forge/sessions.sqlite` (scrollback blobs at `.forge/sessions/<id>/scrollback.bin`); without the persister the snapshot is dropped silently — matching pre-BL-062 behaviour._

---

_BL-055 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `com.nexus.terminal::run_saved` handler (id 23 — DoD-suggested 18 was already taken by BL-059's `open_in_terminal`; ids are append-only) spawns a saved command in a fresh PTY session under `<shell> -c "<shell_cmd>"` (or `/C` / `-Command` for cmd.exe / pwsh). Three new built-ins land in the AI tool registry: `terminal_run_saved`, `terminal_get_status`, `terminal_send_signal`. `terminal_get_status` is read-only (added to `READ_ONLY_TOOL_NAMES`); the other two stay write-class and require `ai.tools.write`. `terminal_send_signal` accepts SIGINT / SIGQUIT / SIGTSTP / EOF and reshapes the signal name into the corresponding control byte (0x03 / 0x1c / 0x1a / 0x04) for `send_raw_input`. SIGTERM / SIGKILL of unresponsive processes intentionally not exposed — needs an out-of-band signal path that isn't a PTY byte. Planner system prompt gains a paragraph on when to reach for terminal tools (kept goal-level, not enumerative — the registry still owns per-tool schemas)._

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

> **Fully closed 2026-05-07** — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Phase 1: CLI + launcher OS-template scaffold. Phase 2: `nexus.osArchitecture` panel renders architecture.md with drift detection. Phase 3: `com.nexus.skills::invoke` + Run button. Phase 4: `nexus.osObservability` panel with usage / automation / vault-feed tabs. Phase 5: built-in `os-setup` elicitation skill. The end-to-end vertical works today.

**Source**: AI Integration Assessment + Chase AI "Agentic OS" framework analysis (2026-05-06) — full plan in [BL-054-agentic-os-mode.md](BL-054-agentic-os-mode.md)
**Effort**: ~1 week total across 5 independent phases (0.5 _(shipped)_ + 1.5 _(shipped)_ + 1 _(shipped)_ + 2 _(shipped)_ + 0.5 _(shipped)_ days)
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

> **Phases 1 + 2 + 3 closed 2026-05-07** — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Phase 1: pill-shaped editor tabs, ember segmented inspector control, status-bar forge name + ember dot. Phase 2: ember wikilinks, path-style inline code, YAML frontmatter metadata bar. Phase 3: Obsidian-style callouts (`> [!type] Title`) with seven types + per-type accent dot. Phase 4 remains open (gated on Q2).

**Source**: Forge Color System mockup + ember-on-slate exploration (2026-05-06) — full plan in [BL-053-forge-visual-target.md](BL-053-forge-visual-target.md)
**Effort**: Phase 1 ~1 day _(shipped)_ · Phase 2 ~2 days _(shipped)_ · Phase 3 ~3–5 days _(shipped)_ · Phase 4 ~3–5 days (~3–4 weeks for the lot)
**Crates**: `shell/src/shell/`, `shell/src/plugins/nexus/editor/`, `shell/src/plugins/nexus/outline/`, `shell/src/plugins/core/editorArea/`, possibly a new markdown-extension surface in `nexus-editor`
**Related**: bundled themes `nexus-ember-dark` / `nexus-ember-light` (delivered 2026-05-06) supply the tokens; this BL styles against them

The bundled ember themes ship the right token values, but the shell renders a much plainer surface than the Forge mockup — mostly because rich rendering (callouts, status pills, frontmatter metadata bars, path-style inline code, ember wikilinks) is renderer/plugin work, not theme work. The companion plan splits the gap into four phases ordered by ROI, identifies what's reachable through theme+CSS alone vs. what needs renderer extensions, and lists the four product decisions that gate code (callout syntax, status data source, font bundling, scope commitment).

**Phase 1 alone delivers ~70% of the visible win.** Subsequent phases are independent and can be greenlit individually.

**Definition of done (per phase):** acceptance criteria filled in when a phase is scoped in — see §6 of the companion doc. The plan itself does not commit to any phase.

_BL-052 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `ActivityEntry` / `ActivitySurface` / `ActivityOutcome` / `ActivityToolCall` lifted from `nexus-ai` to `nexus-types::activity` (kept the type names; `nexus-ai` re-exports for back-compat, so existing call sites still compile). New `ActivityOrigin` enum (`Ai` / `User` / `Plugin(id)` / `Workflow(id)` / `Agent(id)` / `Terminal(id)` / `Git` / `Storage` / `Capability`) with a `to_wire` / `from_wire` round-trip; `ActivityEntry` carries it as a `String` field with `serde(default = "ai")` so legacy on-disk JSONL parses cleanly. New universal topic constant `ACTIVITY_APPENDED_TOPIC = "com.nexus.activity.appended"`; the AI recorder now publishes to BOTH this and the legacy `com.nexus.ai.activity_appended` so existing AI-only subscribers keep working. Emitters wired in this sweep: storage (file_created / file_modified / file_deleted / file_renamed via `publish_file_activity`), git (HEAD-changed commit detection via `publish_git_activity` + the auto-committer's existing emit reshaped to a proper `ActivityEntry`), workflow (run start + end via `publish_workflow_activity`), terminal (BL-057 — see its closure note). Shell-side: the `activityTimeline` plugin subscribes to BOTH topics with id-keyed dedup so the AI's twin-emit doesn't render twice; gains an origin filter chip with nine kinds (AI / User / Storage / Git / Terminal / Workflow / Agent / Plugin / Capability); surface union widens to include `file` / `process` / `git` / `workflow` / `capability` plus the existing AI surfaces. Pre-existing schema tests catch the new types via the existing `every_object_schema_denies_additional_properties` invariant — `ActivityEntry` keeps `deny_unknown_fields` (extras rejected; `serde(default)` handles missing-on-read separately).

**Deferred from the DoD:**
- **Capability grant/revoke emitter** — would need to thread the universal topic through the kernel security path; scoped out as a follow-up because the security audit log already lives in SQLite (`nexus-security` audit table) and a non-trivial subset of grants happens before the bus is fully wired.
- **Plugin-id rename `nexus.activityTimeline` → `nexus.activity`** — the catalog has no `legacyPluginIds` field, so a rename without a migration shim would orphan any user who'd toggled the plugin off. User-visible strings ("Activity", "Activity Timeline") and category ("Activity") were renamed; the internal id stays `nexus.activityTimeline`. Track as a future cleanup once the catalog gains alias support.
- **Per-emitter opt-out config** — none of the emitters ships a knob today; the topic is fire-and-forget. Add a `nexus.activity.disabledEmitters` setting once a noisy emitter actually exists in the wild.
- **Shared privacy redactor** — `nexus-ai`'s `Redactor` (PRD-12 §privacy) applies only to the AI-recorder path. Lifting it to a shared crate touches every emitter and adds a config surface; deferred. Each non-AI emitter today produces short structured prompts (`"renamed a → b"`, `"commit abcdef on main"`) that don't carry user-secret content, so the immediate risk surface is low. Track when an emitter starts surfacing free-form user input.
- **Push/pull git events** — the existing git poller only watches HEAD; remote-side push/pull aren't observed yet. The auto-committer side is fully covered.

**Why this matters:** transparency parity — agents (AIG-02) can dispatch tools that span all subsystems, and the user now sees every effect in one pane, not five separate logs._

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
