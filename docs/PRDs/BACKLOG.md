# Nexus Feature Backlog

> **Single source of truth for unfinished work.** This file is the index every other planning doc points to.
>
> - **Per-PRD status** lives in [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md).
> - **Completed items** are archived verbatim in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).
> - **Full descriptions of OI-\*** items live in [../roadmap/OPEN-ITEMS.md](../roadmap/OPEN-ITEMS.md); this file cross-lists by ID.
> - **Formal-release work** (auto-updater, telemetry, marketplace, beta→GA) is deferred to [../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md](../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md); the WI-IDs are indexed below for completeness.
> - **Doc gaps and product gaps surfaced by the 2026-05-12 traceability audit** live in [../roadmap/DOC-GAPS.md](../roadmap/DOC-GAPS.md) as DG-01..DG-46. Product-gap entries (DG-32..DG-46) are cross-listed in this file under "Doc-audit-surfaced product gaps". Doc-bug entries (DG-01..DG-31) live in DOC-GAPS only since they're documentation edits, not feature work.
> - **Research-surfaced ideas** from external-project assessments under [../research/](../research/) are indexed under "Research-surfaced ideas (2026-05, unscoped)".
> - **Exploratory / unscoped design docs** (AI directions, ambient copilot, memory layer, settings extraction inventory) are linked under "Future directions" — they do not have committed timelines.
>
> Section headings with no listed items are preserved as structural placeholders — consult the archive for what landed under each, and add new follow-ups directly below the heading.

---

## New Features (not addressed in any PRD)

_BL-109 closed 2026-05-13 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `flattenTree.ts` extracts the visible-rows walker as a pure helper (sorting + bundle-dir gate + lazy-children skip); `FilesTree.tsx` rebuilt around `useVirtualizer` over that flat list. Recursive `TreeNode` collapsed into a single `TreeRow` per visible row, drop-hover state per-row, drag/drop / context-menu / keyboard wiring preserved (rows stop event propagation so the container's root-drop / root-context-menu fall through cleanly). Selection scroll-into-view now goes through `virtualizer.scrollToIndex(idx, { align: 'auto' })` so auto-reveal + click both keep working without a per-row ref. 7 new unit tests pin `flattenTree` behaviour (collapsed root, expanded with cached kids, expanded without kids, bundle-dir non-recurse, sort modes per-level, dirs-before-files, isBundleDir matrix). DoD benchmark scenario (10k files + every folder expanded) intentionally not measured against a recorded baseline — BL-112 owns the harness; the structural goal (`O(viewport_height / row_height)` mounted rows regardless of expansion depth) is met by construction since the virtualizer's `getVirtualItems` is bounded by `viewport / estimateSize + 2*overscan`._

_BL-110 closed 2026-05-13 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `shell/src/stores/frameSnapshot.ts` ships a React-free `FrameSnapshot` controller (subscribe N stores → coalesce notifications → flush once per rAF, identity-stable tuple within a frame) plus an injectable `Scheduler` so tests run synchronously without rAF / fake timers. New `shell/src/stores/useFrameSnapshot.ts` is a `useSyncExternalStore` adapter on top, plus a `snap()` helper that builds typed entries without `as const` boilerplate. `shell/src/plugins/nexus/statusBar/FileStats.tsx` migrated as the reference site — was four independent selectors across `useEditorStore` + `useBacklinksStore` (both fed by async kernel events that fire in separate batches), now one rAF-coalesced tuple. 7 new unit tests cover tuple identity stability within a frame, flush invalidation on per-element diff, no-op flush preservation, multi-mutation coalescing, dispose semantics, and start-twice guard. Discovered alongside: the BL-109 / BL-110 src-colocated tests weren't being executed by `pnpm --filter nexus-shell test` (which globs `tests/*.test.ts` only, not `src/**`); both relocated to `shell/tests/{flatten-tree,frame-snapshot}.test.ts`. **Filed separately under "Suspected issues — not fully investigated"**: 107 other src-colocated `*.test.ts` files appear to share the same fate._

_BL-111 closed 2026-05-13 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Audit found vendor-mermaid (2.7 MB) was being eagerly preloaded by every cold start: `index.html` emitted `<link rel="modulepreload" href="/assets/vendor-mermaid-…">` and the entry chunk had a static `import{_ as b}from"./vendor-mermaid-…"`. Root cause: Rollup parked Vite's runtime preload helper (`__vite__preload`) inside whichever `manualChunks` bucket landed first by build order — historically `vendor-mermaid` — and the entry chunk's single static import of that helper symbol silently dragged the whole 2.7 MB host chunk into the eager static-import graph. Fix: a new `vite/preload-helper` rule in `manualChunks` routes the helper into its own 1.13 kB chunk (`vite-preload-helper-…js`) plus `build.modulePreload: { polyfill: false }` to drop the eager polyfill, leaving only the native `<link rel="modulepreload">` path that runs at each dynamic-import call site. Post-fix: entry preloads `vite-preload-helper` (1.13 kB) + `vendor-react` (142 kB); vendor-mermaid only loads when `import('mermaid')` fires from the lazy mermaid plugin (which is itself a default-off catalog entry). jspdf + html-to-image are bundled together into an auto-chunked `exportFormats-…js` (404 kB) loaded only when CanvasView's user-triggered export runs `import('./exportFormats')`. mermaidPromise memoization in `community/mermaid/index.ts` already covers the "subsequent fences reuse the cached module" DoD bullet._

_BL-112 closed 2026-05-13 (first cut) — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `experiments/perf/run.ts` harness ships build-output measurement (eager-vs-lazy chunk inventory, entry static-import set, HTML preload list — exactly the surface that would have caught the BL-111 vendor-mermaid regression at PR review) plus two microbenchmarks against the BL-109/110 hot helpers (`flattenTree` on a synthetic 10k-file forge with every folder expanded, `frameSnapshot` four-store flush). Stable JSON schema (`schemaVersion: 1`) sorted for textual diffability; baseline committed at `experiments/perf/baselines/2026-05-13.json`. Tauri/WDIO runtime scenarios from the original DoD (cold-start trace, 50 MB file open, 10k-row scroll, 500-heading outline, 5-char type latency) are scaffolded for as additional `scenarios` entries in the same JSON shape but **deferred** until a WDIO-Tauri runner exists — the BL itself flagged that piece as already-deferred ("needs a stable runner")._

_BL-122 closed 2026-05-14 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Three new scenario families wired into the BL-112 harness: `editor.apply_transaction.{small,medium,large}` (Rust integration test at `crates/nexus-editor/tests/perf_apply_transaction.rs`, drives `EditorCorePlugin::dispatch(HANDLER_APPLY_TRANSACTION, ...)` against 10 / 100 / 5000-block docs and prints `PERF_RESULT::<json>` lines that `run.ts` parses), `editor.livePreview.decorate.{small,medium,large}` (drives `buildLivePreviewDecorations` against a fully-parsed lezer tree via `ensureSyntaxTree`; happy-dom + CodeMirror reached through a `createRequire` rooted in `shell/package.json`), and `editor.markdownRender.large` (marked + DOMPurify under happy-dom). New `tracing::info_span!("apply_transaction", op_count, bytes_in, bytes_out)` around `handle_apply_transaction` records per-call fields; no subscriber → no-op. Baseline at `experiments/perf/baselines/2026-05-14.json` shows the N-linear shape on `apply_transaction.*` (39 → 330 → 24190 µs p50) — exactly the curve BL-123's slim text-only response will flatten. `live-preview.decorate.*` floors around ~100 µs at 50 / 500 / 1500 lines because per-node walk cost is dominated by fixed overhead (`computeActiveLines`, `Decoration.set` sort) at these sizes; BL-125's viewport-scoping win is still measurable because the production path walks every node on every keystroke + selection change. Runtime (WDIO-Tauri) scenarios from the original DoD remain BL-127's domain — same deferral note as BL-112._

---

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

_BL-102 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Verifier scaffolding shipped (rustls custom `ServerCertVerifier`, `KernelConfig::tls_pinning_enabled`, `SecurityError::CertificatePinMismatch`/`NoPinsConfigured`, `NEXUS_TLS_PINNING=1` env opt-in). Default **off** because the shipped `tls_pins::HOST_PINS` table is empty — an operator with network access seeds real fingerprints, then flips the flag. `nexus ai status` `tls_pinned` field shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New private `tls_pinning_effective` helper in `nexus-ai/src/core_plugin.rs` mirrors the gate in `http_client::build_client` (config flag OR `NEXUS_TLS_PINNING=1`); `handle_status` returns it as a `tls_pinned: bool` JSON field and the CLI prints `TLS Pinned: yes/no`._

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

_BL-096 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Live runtime revocation, persistence, audit, bus event, and a `nexus plugin revoke` CLI verb shipped. Shell-side live-revoke shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `revoke_plugin_capability` Tauri command routes through `SharedPluginLoader::revoke_capability`; new `applyCapabilityChange` helper sequences the existing `set_plugin_granted_capabilities` file-write with one `revoke_plugin_capability` per cap removed by the consent modal, so unchecking a cap in Settings → Plugins now mutates the running plugin's wired context immediately rather than waiting for next boot._

---

_BL-095 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Watchdog ships for the `register_core` path with default 30s deadline. "Continue with degraded plugin set" + bus event shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md): a per-plugin `LifecycleTimeout` is now logged, the plugin skipped, and `com.nexus.kernel.plugin_lifecycle_timeout` published on the bus so the shell can surface a "<plugin> failed to start" notice._

---

_BL-094 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-093 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). In-process counter + histogram registry shipped with IPC, event-bus, capability, and lifecycle recording; `com.nexus.security::metrics_snapshot` IPC handler exposes the JSON snapshot. `event_bus_queue_depth` gauge shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Shell health panel shipped as a follow-up 2026-05-08 — new `nexus.healthPanel` plugin (default-off, sidebar leaf reachable via `nexus.healthPanel.focus`) polls `metrics_snapshot` every 5 s and renders gauges (event-bus queue depth, denial count, dropped-metrics sentinel) plus IPC / capability / event-bus tables sorted to surface the most actionable rows first. Prometheus scrape endpoint still deferred — would need a separate HTTP service; the in-process snapshot covers the immediate triage use case._

---

_BL-092 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Criterion benches for event bus and IPC dispatch shipped. Measured baselines on this dev box: event publish ~300ns, IPC noop dispatch ~30µs, capability check ~30ns — all comfortably inside the PRD targets. CI integration with regression gates deferred — the harness is available for an operator to wire into CI once a stable bench runner is provisioned._

---

_BL-091 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Read path + status surface shipped: pointer detection in `nexus-storage::read_file` with `git lfs smudge` passthrough, `com.nexus.git::lfs_status` IPC handler (id 27), and `nexus git lfs-status` CLI. Write-path routing through `git lfs clean` on `stage_file` shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

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

_BL-074 closed 2026-05-14 (umbrella close — Phases 1–4 + editor wiring + BL-007 transport loop + compaction + conflict modal all shipped 2026-05-08 through 2026-05-09; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

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

> **Parked on branch `bl-081-dap-debugger` (2026-05-13)** pending [BL-113](#bl-113-protocol-host-contribution-model-for-lsp--dap--mcp). The first-cut implementation (nexus-dap host crate, 19-handler IPC surface, shell debugger panel, Python mock e2e test, 46 Rust + 17 shell tests, green workspace) builds out the existing flat-`dap.toml` pattern that BL-113 is proposing to replace. Rather than land that pattern on main and then immediately rework it, the work is held on the branch and lands once BL-113's contribution model is decided. Full spec in [BL-081-dap-debugger.md](BL-081-dap-debugger.md) lives on the branch.

**Source**: Code editor capability analysis (2026-05-06) — full plan in [BL-075-081-code-editor.md](BL-075-081-code-editor.md)
**Effort**: Large (4–6 weeks). First cut implemented; depends on BL-113 for the adapter-config shape before merging.
**Crates**: new `nexus-dap`, new `shell/src/plugins/nexus/debugger/`
**Related**: BL-076 (nexus-lsp — shipped); BL-113 (protocol-host contribution model — gating); Debug Adapter Protocol (DAP)

**Definition of done:**
- `nexus-dap` spawns configured DAP adapters (e.g. `codelldb`, `js-debug`)
- Debug panel: Variables, Call Stack, Watch, Breakpoints panels visible in shell
- CM6 breakpoint gutter: click to set/clear, red dot indicator
- Debug toolbar: Continue, Step Over, Step Into, Step Out, Restart, Stop
- `com.nexus.dap` IPC surface mirrors DAP request/response types
- Adapter configuration shape settled per BL-113

---

### BL-113: Protocol-host contribution model for LSP / DAP / MCP / ACP

> **Phase 0a shipped 2026-05-14.** ADR 0027 accepted (open questions pinned to decisions). New `[[registrations.protocol_hosts.{lsp,dap,mcp,acp}]]` manifest section: public types (`ProtocolHostsContribution`, `LspProtocolHostReg`, `DapProtocolHostReg`, `McpProtocolHostReg`, `AcpProtocolHostReg`) live in `nexus-plugins::manifest`; new `nexus-plugins::contributions` module exposes `ContributedAdapter<T>` + `collect_contributions(&[&PluginManifest])`. 10 unit tests across parsing + aggregation.
>
> **Phases 2a + 3a shipped 2026-05-14.** Host-side merge primitives on `main` ready for activation. `LspHostConfig::merge_contributed(Vec<(LspServerSpec, plugin_id)>)` and `McpHostConfig::merge_contributed(Vec<(name, McpServerSpec, plugin_id)>)` accept contributed adapter specs and merge with **TOML-wins** precedence. Both return a typed skip report (`LspMergeSkip` / `McpMergeSkip`) carrying `(name, plugin_id, reason)` so the bootstrap-side caller can log conflicts. MCP factors per-spec validation out of `validate` into a shared `validate_spec` rule so contributions go through the same transport-aware checks as TOML entries. New `nexus-bootstrap::protocol_host_specs` is the only place in tree that maps `ContributedAdapter<*ProtocolHostReg>` → host spec; preserves contribution order; MCP transport string (`http` / `ws|websocket` / unknown → `stdio`) parsed defensively. 8 + 4 unit tests across the merge + converter layers.
>
> **Phases 1, 2b, 3b, 4 still open.** Phase 1 (DAP on the new shape; lands on the parked `bl-081-dap-debugger` branch). Phase 2b / 3b (bootstrap-side activation: register_adapter / unregister_adapter IPC pair + post-plugin-scan merge call) waits for Phase 1's lifecycle-callback design. Phase 4 (greenfield ACP host when Hermes Feature 7 is picked up).

**Source**: BL-081 review (2026-05-13) — full design in [ADR 0027](../adr/0027-protocol-host-contribution-model.md).
**Effort**: Large. Phase 0 (ADR + spike) ~1–2 days; Phase 1 (DAP on the new shape) ~1 wk; Phases 2–3 (LSP, MCP) ~1 wk each; Phase 4 (ACP) lands greenfield when the Hermes Feature-7 / ACP integration BL is picked up.
**Crates**: `nexus-lsp`, `nexus-dap` (on the parked branch), `nexus-mcp`, future `nexus-acp`, `nexus-plugins` (contribution loader), shell-side plugin manifest schema.
**Related**: ADR 0011 (plugin-first shell), [BL-081](#bl-081-dap-debugger-integration) (parked pending this), [BL-076](BACKLOG_COMPLETED.md) (nexus-lsp host — shipped under the legacy flat-TOML pattern this would replace), [Hermes Agent port plan](../research/hermes-agent-implementation-plan.md) Feature 7 (ACP adapter — not yet scoped as a BL; when it lands it inherits this model).

Today `nexus-lsp`, `nexus-mcp`, and (parked) `nexus-dap` each ship a flat TOML config (`lsp.toml` / `mcp.toml` / `dap.toml`) listing their external adapters. ACP (Agent Communication Protocol — Hermes Feature 7, agent-to-agent stdio JSON-RPC) is the same shape and queued as a future `nexus-acp` crate. The pattern is consistent with the microkernel invariant but blocks per-adapter UX (launch-config schemas, variable renderers, hover providers, agent capability descriptors), has no marketplace/install path, and duplicates ~80%-identical config shapes across the host crates.

**Proposed**: lift adapter declarations from TOML to a shared plugin contribution point (`contributes.protocolHosts.{lsp,dap,mcp,acp}`). The host crates stay core and protocol-only; community/first-party plugins contribute adapter definitions plus optional shell-side exports (launch-config form schema, value renderers, hover provider, agent capability metadata). Phased rollout: ADR + spike on DAP first (since BL-081 is already on a branch), then LSP, then MCP; ACP lands greenfield when its host crate is built. Flat TOML stays as legacy fallback for the existing three during the transition; ACP never gets one.

**Definition of done:**
- ADR 0027 accepted with open-question section resolved
- New `contributes.protocolHosts.{lsp,dap,mcp,acp}` contribution path lands in plugin-manifest schema + loader
- `nexus-dap` reads contributions alongside `dap.toml`; one example first-party plugin (e.g. `first-party.dap.rust`) demonstrates the new shape end-to-end including a typed launch-config form on the shell side
- BL-081 rebases onto the new shape and lands on main
- `nexus-lsp` migrates next; `lsp.toml` keeps working
- `nexus-mcp.host` migrates last; `mcp.toml` keeps working
- Future `nexus-acp` crate (Hermes Feature 7) consumes the contribution loader as its only adapter source — no `acp.toml` to migrate from
- Capability surface for `register_adapter` is decided (whether it's a new capability or rides on the existing contribution path)

---

_BL-114 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `2ce5e645`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-115 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `11c2836f`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-116 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `9c1b7fb9`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

---

_BL-117 closed 2026-05-14 (umbrella close — shipped 2026-05-13 across commits `4d39016d` + `5c81d173`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-118 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `3bedd31f`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-119 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `d1ad15e4`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-120 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `a3adfd08`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-121 closed 2026-05-14 (umbrella close — shipped 2026-05-13 in commit `d620b2b0`; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

---

_BL-122 closed 2026-05-14 — see the closure note at the top of this file and [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

_BL-123 closed 2026-05-14 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `ApplyTransactionResponse` tagged union (`{ kind: 'slim', revision }` vs `{ kind: 'full', ...EditorSnapshot }`); `handle_apply_transaction` returns Slim for any all-`insert_text`/`delete_text` transaction and Full for anything structural (including `update_annotations` — the bridge's optimistic mirror doesn't track annotations, so the snapshot is the only authoritative source for the post-apply annotation list). Bridge updated to skip `setSnapshot?.(...)` on slim, strip the discriminator on full; downstream consumers (drag-bridge, comments) keep correct block IDs + structure across text-only ops since neither changes under InsertText/DeleteText. BL-122's `editor.apply_transaction.*` baseline collapses from 39 / 330 / 24190 µs p50 (small/medium/large) to **4 / 4.45 / 4 µs** — flat curve, ≈ 6000x speedup on the 5000-block case, well under the DoD's < 1 ms target. New Rust response-shape test (`apply_transaction_response_shape_per_op_kind`) exercises every op kind plus the wire-shape JSON. New shell coherency tests cover the slim path (setSnapshot NOT called, revision propagates) and the full path (setSnapshot called with the post-apply tree, discriminator stripped). `scripts/check_ipc_drift.sh` clean — `EditorSnapshot` isn't a ts-rs-generated binding (defined manually in `shell/src/plugins/nexus/editor/types.ts`), so the new discriminator type ships as a hand-written `ApplyTransactionResponse` in `kernelClient.ts`._

_BL-124 closed 2026-05-14 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `EditorView` switches from `useEditorStore((s) => s.tabs)` (subscribes to the entire array; every keystroke in any leaf re-renders every other leaf) to a per-relpath `useFrameSnapshot` over `[active-tab-object, tabs.length]` selectors. `useFrameSnapshot` gains an optional `rebuildKey` parameter so the controller re-binds if a leaf is ever re-targeted to a different file without unmounting; default behaviour (no key, captures entries on first render) is preserved for the BL-110 reference site. `activeRelpath` and `isDirty` are intentionally excluded from the snapshot — EditorView never renders them, so including them would force a re-render on every session-revision bump for no visible change; the tab-strip dirty dot still flows through `WorkspaceRenderer.useLeafDirty` per-leaf subscription, unchanged. 4 new tests pin the contract: 10 keystrokes against tab A flush tab A's controller exactly 10 times and tab B's controller zero times; `setContent` / `setMode` on tab A preserve tab B's object identity; the rebuildKey-driven re-bind reads the new tab's slice. `pnpm --filter nexus-shell test` 1386/1387 (one pre-existing skip). The render-count DoD bullet is verified at the FrameSnapshot layer (not the React tree) because mounting `EditorView` in happy-dom needs CM6, the runtime, the storage IPC stub, etc. — too heavy for a unit test; the per-relpath narrowing is the actual mechanism, and the test pins the mechanism._

_BL-125 closed 2026-05-14 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Live-preview decoration source split into two: a `StateField` calling new `buildLivePreviewBlockDecorations(state)` (full-tree walk emitting only HR / Table / FencedCode block widgets — CM6 requires block widgets to be StateField-sourced) and a `ViewPlugin` calling new `buildLivePreviewInlineDecorations(state, view.visibleRanges)` (viewport-scoped walk emitting marks / line decorations / non-block replaces). The walker uses `tree.iterate({ from, to })` to bound iteration by the supplied ranges; everything else is unchanged. Both sources contribute to `EditorView.atomicRanges` so cursor motion still respects hidden marks across the split. Legacy `buildLivePreviewDecorations(state)` is preserved as a thin combined entry-point so the pre-existing test suite + the BL-122 perf harness backward-compat scenario continue to work. New `visitBlock` / `visitInline` split the dispatch table; `handleFencedCode` factors into `handleFencedCodeBlockOnly` + `handleFencedCodeInlineOnly` so each visitor emits only its branch. 6 new tests cover the BL-125 split: block source emits widgets and no inline marks, inline source emits inline marks within the requested range only, empty-range inline source emits nothing, heading line decoration appears inside its range, cursor-on-heading reveal works across the viewport split, and `block ∪ inline (full doc)` reproduces the full-walk reference set. BL-122 perf scenarios bumped — `large` now exercises a 5000-line doc against a synthetic 50-line viewport (the natural CM6 frame shape). Baseline: small=85 µs p50, medium=49 µs p50, **large=12.66 µs p50 / 51 µs p95** — `large` p95 ≤ `small` p95 (the DoD target), and large is now ≈ 7x faster than the pre-BL-125 full-walk equivalent on the same 5000-line doc. `pnpm --filter nexus-shell test` 1392/1393 (one pre-existing skip)._

_BL-126 closed 2026-05-14 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). The `serde_json::to_vec(&tx_value).len()` payload-size pre-check in `handle_apply_transaction` (a throwaway full-tx serialize purely to count bytes) is replaced by a new `transaction_payload_size(tx)` helper that sums the user-controlled string fields op-by-op (`text` / `deleted_text` / block content / content-update old/new strings) plus a 64-byte fixed cost per annotation. The same 16 MiB safety ceiling is enforced; the deserialize-then-check ordering is preserved (typed `Transaction` → structural sum → cap). Tracing span field `bytes_in` renamed to `payload_bytes` to reflect the new semantics. New `apply_transaction_rejects_payload_above_structural_cap` test pins three properties: a small payload reports the expected byte count, the apply path goes through for small txs, and a 17 MiB `InsertText` is rejected before any mutation with a message that names both the actual payload size and the cap. The BL DoD's second bullet (concurrency overlap test that exercises two relpaths in parallel) is intentionally deferred — the `Mutex<HashMap<String, Session>>` map-level lock guarantees serial mutation of different relpaths today, and breaking that out into per-session `Arc<Mutex<Session>>` would touch ~22 handler signatures, larger than the BL's "Small. Mostly cleanup" tag warrants. Snapshot-clone-outside-the-lock is also out of scope for the same reason; the snapshot SERIALIZE was already moved outside the lock by BL-123. `cargo test -p nexus-editor` 233/233; `cargo clippy -p nexus-editor --all-targets` clean for new code; perf baseline unchanged in shape (typing path 7–11 µs p50 across small/medium/large — flat curve preserved)._

---

_BL-124 closed 2026-05-14 — see the closure note at the top of this file and [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-125 closed 2026-05-14 — see the closure note at the top of this file and [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-126 closed 2026-05-14 — see the closure note at the top of this file and [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

### Follow-up: per-session locks for concurrent multi-relpath mutation

**Source**: BL-126 deferral. Filed 2026-05-14.
**Effort**: Medium. ~22 handler signature changes.
**Crates**: `nexus-editor`.

BL-126's DoD asked for a "multi-relpath concurrency test [that] exercises two sessions being mutated simultaneously" — currently the `Mutex<HashMap<String, Session>>` map-level lock serialises mutations even when they target different files. To make concurrent edits actually overlap, switch the map to `Mutex<HashMap<String, Arc<Mutex<Session>>>>` so handlers acquire the outer lock briefly to clone the per-session Arc, drop the outer lock, then acquire the inner session lock. Snapshot-clone-outside-the-lock becomes feasible after that change (the inner lock guards a single session, so the tree clone can move outside the outer lock's scope, with the session-Arc clone replacing it). Tracing spans should then show two `apply_transaction` enters overlapping when the two sessions are independent.

---

### BL-127: Runtime typing-latency perf scenarios (Phase 5 of TYPING-LATENCY-PLAN)

**Source**: Typing-latency analysis — see [../roadmap/TYPING-LATENCY-PLAN.md](../roadmap/TYPING-LATENCY-PLAN.md). Filed 2026-05-14.
**Effort**: Medium. Gated on the WDIO-Tauri runner that BL-112 also defers.
**Crates**: `experiments/perf` + a small instrumentation hook in shell `nexus.editor`.

The microbenchmarks from BL-122 gate the kernel and decoration paths but miss IPC, Tauri serialization, webview paint, and React re-render. This BL fills in the runtime side from BL-112's deferred slot — `typing.small/medium/large` scenarios that boot the shell against a synthetic forge, open a markdown tab, programmatically dispatch keydown events, and measure keystroke → next-frame-paint via `performance.mark`/`performance.measure`. Wires into CI as non-blocking first, then promotes to a regression gate once variance settles.

**Definition of done:**
- WDIO-Tauri runner stood up (track separately; reference here)
- `typing.{small,medium,large}` scenarios produce stable numbers (< 10% variance across 3 runs)
- Targets: p95 keystroke → paint < 16 ms (small/medium), < 33 ms (large)
- Instrumentation hook in `EditorView.tsx` gated by `NEXUS_PERF_TYPING=1` so production paths aren't paying for it
- Regression in any of BL-123/BL-124/BL-125/BL-126 surfaces as a CI delta

---

### BL-128: Personal entity knowledge graph (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Large. Independent of other Thoth ports.
**Crates**: `nexus-storage` (indexing + graph extension), new shell plugin `nexus.entityGraph`.

Nexus's petgraph today is a document-link graph: nodes are files, edges are wikilinks. Thoth proves there is significant value in a second, orthogonal layer — a **personal entity graph** where nodes are typed entities (people, places, events, projects, concepts, etc.) and edges are typed directional relationships with confidence scores. The two graphs are complementary: the document graph answers "what files reference what?"; the entity graph answers "what do I know about this person / event / concept?"

**Design constraint (file-as-truth):** Entities must live as Markdown files, not as opaque DB rows. Each entity is a `.md` file in a user-designated folder (e.g. `<forge>/entities/`) with YAML frontmatter carrying `entity_type`, `aliases`, `tags`, and `relations` (a list of `{target, type, confidence}` objects). `nexus-storage` indexes these files like any other, extending the existing petgraph with typed-edge support and a FAISS-backed semantic recall index over entity descriptions.

**Entity types (11 canonical):** `person`, `preference`, `fact`, `event`, `place`, `project`, `organisation`, `concept`, `skill`, `media`, `self_knowledge`.

**Relation types (40+ controlled vocabulary):** family/social (`knows`, `friend_of`, `married_to`), location (`lives_in`, `works_at`, `born_in`), work (`works_on`, `manages`, `employed_by`), knowledge (`proficient_in`, `certified_in`, `studies`), media (`reading`, `watching`, `authored`), plus temporal, ownership, membership, and causality variants. A `normalize_relation_type` lookup maps LLM-generated variants to canonical forms.

**IPC handlers** (new, in `nexus-storage`): `entity_search(query, [type])` — semantic recall via FAISS; `entity_get(id)` — fetch by ID or subject; `entity_relations(id, [direction])` — graph traversal; `entity_upsert(frontmatter)` — write-through to disk; `entity_find_duplicates(threshold)` — similarity scan for Dream Cycle (BL-129).

**Definition of done:**
- `nexus-storage` parses `entity_type` + `relations` frontmatter; extends petgraph edges with `type: String` and `confidence: f32`
- FAISS embedding index (reuse `nexus-ai`'s embedding provider) built over entity `description` fields; incremental upsert on file-watch events
- `normalize_relation_type` table covering at least the 40 relation types from the Thoth vocabulary
- Five new `com.nexus.storage` IPC handlers: `entity_search`, `entity_get`, `entity_relations`, `entity_upsert`, `entity_find_duplicates`
- `nexus.entityGraph` shell plugin: visual node/edge explorer (reuses existing graph canvas), sidebar entity detail panel, entity-creation palette command
- Agent integration: `nexus-agent` system-prompt assembly queries `entity_search` for context-relevant entities and prepends them, mirroring the existing RAG retrieval path
- CLI: `nexus graph entity list|show|search|related`

---

### BL-129: Dream Cycle — knowledge refinement engine (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Medium. Depends on BL-128 (entity graph).
**Crates**: `nexus-workflow` (new built-in workflow type), `nexus-storage` (dedup API), `nexus-ai` (enrichment handler).

A scheduled background pass that keeps the entity knowledge graph (BL-128) clean and enriched without manual curation. Modelled on Thoth's "Dream Cycle" daemon. Four phases run in sequence during a user-configured maintenance window (default: daily at 02:00 local, or on-demand via `nexus graph dream-cycle run`):

1. **Duplicate detection** — calls `entity_find_duplicates(threshold: 0.92)` to surface semantically near-identical entity pairs; auto-merges pairs above a higher threshold (0.97), queues the rest for user review via a shell notification.
2. **Description enrichment** — for entities whose `description` field is short (< 80 chars) or was not updated in the last 30 days, asks the AI to expand it using existing relations and linked document context.
3. **Confidence decay** — reduces `confidence` on relations not corroborated by any document link within the last 90 days (multiplicative decay factor 0.95 per cycle, floor 0.1).
4. **Relationship inference** — presents entity clusters to the AI and asks it to propose new relations; each proposal written as a draft relation with `confidence: 0.5` pending user confirmation.

**Definition of done:**
- Built-in Dream Cycle workflow registered by `nexus-bootstrap` alongside user-defined workflows; uses the existing cron trigger engine
- `com.nexus.storage::entity_find_duplicates` returns ranked pairs with similarity scores
- `com.nexus.ai::enrich_entity(entity_id)` handler: RAG-retrieves linked document context and calls the LLM to expand the description
- Decay logic: SQL update over the entity SQLite table; skips entities with `confidence` already at floor
- Inference proposals written as draft `.md` relation frontmatter; shell notification surfaces "N new relation proposals from Dream Cycle" with an approve/skip action
- Configurable in `<forge>/.forge/config.toml`: `[dream_cycle] enabled = true`, `schedule = "0 2 * * *"`, `merge_threshold = 0.97`, `review_threshold = 0.92`, `decay_factor = 0.95`
- CLI: `nexus graph dream-cycle run [--phase <dedup|enrich|decay|infer>]`

---

### BL-130: Prompt injection detection (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Small. Self-contained; adds one pre-prompt sanitisation pass.
**Crates**: `nexus-ai` (new `sanitize` module).

Nexus's `PrivacyPolicy` redactor (BL-017, shipped) handles outbound PII. The missing piece is inbound content sanitisation: RAG chunks, tool call results, channel-sourced messages, and MCP tool outputs can all carry adversarially crafted content targeting the agent's LLM context. Thoth's `_check_prompt_injection()` proves a lightweight pattern scan catches the most common attack vectors before the prompt is assembled.

**Scanning targets** (applied to every string that flows into prompt assembly — RAG chunks, tool results, MCP outputs, entity descriptions prepended by BL-128):

- **Role-override patterns**: case-insensitive match for `"ignore previous instructions"`, `"you are now"`, `"disregard your"`, `"act as"` combined with `"without restrictions"`, `"your new instructions"`, and similar.
- **Invisible Unicode**: scan for U+200B (zero-width space), U+200C/D (joiners), U+FEFF (BOM), U+2060 (word joiner), and other zero-width / directional override code points.
- **Hidden HTML directives**: detect `<!--`, `<script`, `<style` as markers of embedded HTML injection attempts.
- **Data exfiltration patterns**: `base64` + `curl`/`wget` in close proximity; unusually long URL query strings (> 500 chars with high entropy).

**Response options per detection:**
- `Warn` (default): log to audit store, prepend `[INJECTION RISK]` tag to the flagged chunk, continue.
- `Redact` (opt-in): replace the flagged chunk with a placeholder.
- `Reject`: surface an error to the caller, do not invoke the LLM.

Mode is configurable per source type in `<forge>/.forge/config.toml::[ai.injection_policy]`.

**Definition of done:**
- `nexus-ai/src/sanitize.rs` exports `fn scan(text: &str, policy: InjectionPolicy) -> ScanResult`
- Called by `stream_ask` (RAG chunks), `stream_chat` (tool results), and `nexus-agent` context assembly (entity prepend, MCP tool outputs)
- Audit log entry for every flagged chunk (source, detection type, policy applied)
- Test matrix: each pattern class fires on a crafted fixture; legitimate content passes clean
- Config: `[ai.injection_policy] rag_chunks = "warn"`, `tool_results = "warn"`, `mcp_outputs = "redact"`

---

### BL-131: Pre-invocation message sanitisation in the agent loop (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Small. Complements BL-120's summarisation approach; independent.
**Crates**: `nexus-agent` (new `context_sanitize` module).

BL-120 handles context budget exhaustion by summarising oldest turns. This item is a complementary, cheaper pass that runs **before every LLM invocation** to eliminate waste that inflates context without adding information:

1. **Deduplication**: if two consecutive tool call results are byte-for-byte identical, keep only the first and annotate it `(result repeated N more times)`.
2. **Base64 URI stripping**: scan message content for inline `data:image/...;base64,...` URIs; replace with `[image data stripped — N bytes]`. These typically come from browser snapshot or vision tool results that were already consumed.
3. **Stale snapshot compression**: browser/DOM snapshots older than the last 2 turns are compressed to a one-line stub `[browser snapshot from <timestamp>, <N> nodes — compressed]`.
4. **Hard trim**: if total estimated token count (using the provider's tiktoken equivalent) still exceeds 85% of the configured `max_context_tokens` after the above passes, trim from the oldest non-system messages until under budget, logging what was dropped.

This is distinct from BL-120's LLM-based turn summarisation: it targets mechanical waste (duplicates, inert binary data) rather than semantic redundancy, and is O(n) without an LLM call.

**Definition of done:**
- `nexus-agent/src/context_sanitize.rs`: four passes implemented and unit-tested individually
- Integrated into the agent plan executor just before each `com.nexus.ai::stream_chat` invocation
- Token estimation: reuse the `token_budget` counter already present in `nexus-ai` (BL-018 path)
- Metrics: count of deduplicated results, stripped bytes, and trimmed turns emitted on the kernel bus as `com.nexus.agent.context_sanitize` event so the shell health panel (BL-093 follow-up) can surface them
- Test: synthetic 30-turn session with embedded base64 URIs and repeated tool results is visibly smaller after the pass

---

### BL-132: Runtime approval gates in the agent loop (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Medium. Touches kernel event system, agent executor, and all three frontends.
**Crates**: `nexus-kernel` (new event type), `nexus-agent` (executor gate), shell plugin `nexus.agentApproval`, TUI modal, CLI `--interactive` flag.

Nexus's capability system enforces access at IPC dispatch time — a plugin either holds `exec.spawn` or it doesn't. There is no runtime checkpoint: when an agent step is about to execute a destructive action (shell command, file deletion, git force-push), it proceeds without surfacing the pending action to the user. Thoth's `LangGraph interrupt()` pattern shows that pausing the agent mid-plan and routing an approval request to the active frontend is both implementable and significantly improves trust in agentic workflows.

**Mechanism:**

1. A new `approval_required: bool` flag on IPC handlers (set in the handler registration, not in capabilities). Handlers that mutate the filesystem destructively, execute arbitrary shell commands, or perform git operations with `--force` semantics set this flag.
2. Before dispatching a flagged handler, the agent plan executor emits `com.nexus.agent.approval_required { plan_id, step_idx, handler_id, args_summary }` on the kernel bus, then `await`s a `com.nexus.agent.approval_response { plan_id, step_idx, approved: bool }` event with a configurable timeout (default 120 s; configurable in `[agent] approval_timeout_s`).
3. Frontends subscribe to `approval_required` and surface an approve/cancel UI:
   - **Shell**: inline card in the active chat panel with action summary and ✓ / ✗ buttons
   - **TUI**: modal overlay over the current pane
   - **CLI**: `--interactive` flag; non-interactive runs auto-reject destructive steps with a clear error
4. If the timeout elapses without a response, the step is cancelled and the plan marked `AwaitingApproval` (resumable).

**Definition of done:**
- `approval_required` flag on at minimum: `nexus-terminal::execute_command`, `nexus-storage::delete_file`, `nexus-git::push` (force flag path), `nexus-storage::replace_in_files` (> N files)
- Kernel event pair (`approval_required` / `approval_response`) with typed payload
- Agent executor awaits the response event with timeout; plan state tracks `AwaitingApproval`
- Shell plugin: card-style approval UI, dismissable, auto-expires at timeout
- TUI: modal overlay (reuse existing dialog pattern)
- CLI: `--interactive` flag honours approvals; without it, destructive steps log a clear skip message
- Test: mock executor with injected `approval_response` events; timeout path cancels correctly

---

### BL-133: Multi-channel notification output (Thoth port)

**Source**: Thoth capability assessment — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Filed 2026-05-14.
**Effort**: Medium. New crate; does not touch existing service plugins.
**Crates**: new `nexus-notifications` core plugin.

Nexus agent and workflow outputs are surfaced only in the active frontend session. A background workflow that completes at 02:00 (e.g., the Dream Cycle, a scheduled agent run, a file-event workflow) has no delivery channel if the Tauri shell is closed. Thoth ships five messaging backends; Nexus needs at minimum desktop OS notifications and one async channel.

**v1 scope:**

| Channel | Transport | Trigger |
|---|---|---|
| Desktop OS notification | Tauri `notification` plugin | All workflow / agent completions |
| Telegram bot | HTTP polling via `reqwest` | Opt-in; configured in `[notifications.telegram]` |
| Discord webhook | HTTP POST to webhook URL | Opt-in; configured in `[notifications.discord]` |
| Email (SMTP) | `lettre` crate | Opt-in; TLS, credential in `nexus-security` keyring |

**Integration points:**

- Workflows: the `.workflow.toml` `[[steps]]` spec gains an optional `notify: [telegram, discord]` key; if set, the workflow executor publishes a `com.nexus.notifications::send` IPC call on step or run completion.
- Agents: `com.nexus.agent.run_done` kernel-bus subscribers in `nexus-notifications` send a summary automatically if a notification channel is configured and the run took > 30 s (configurable threshold).
- Any plugin can call `com.nexus.notifications::send(channel, message)` directly.

**Definition of done:**
- `nexus-notifications` registered by `nexus-bootstrap` after `nexus-workflow`
- `com.nexus.notifications::send(channel, message, [title])` IPC handler
- Desktop notification: Tauri `notification` plugin call, falls back gracefully on Linux without `libnotify`
- Telegram: bot token + authorised chat ID stored in `nexus-security` keyring; `notifications.telegram.enabled = true` in config; message split at 4096-char limit
- Discord: webhook URL in keyring; `notifications.discord.enabled = true`
- SMTP: `[notifications.email]` block in config; credential in keyring
- Workflow TOML: `notify` key on `[[steps]]` and at `[metadata]` level (run-level)
- Agent auto-notify on run completion > threshold
- Shell settings panel for configuring channels and testing delivery
- CLI: `nexus notify send --channel telegram "message"` for ad-hoc delivery

---

_BL-080 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Almost everything in the DoD already shipped under `nexus.files` (sidebar tree, expand/collapse, drag-to-reorder, full context menu, live `com.nexus.storage` event sync). The only material gap was the file-type icon set, now closed via a `getFileIcon(name)` helper covering `.md` / source files / structured config and a generic fallback._

---

_BL-079 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Backend gained one new IPC handler (`com.nexus.git::blame`, id 36) — the DoD claimed `blame` already shipped but only the engine method existed; the dispatch surface was missing. Shell ships three pieces: `gitGutter.ts` CM6 extension (per-line markers from `diff_file`, hover tooltip with removed lines, auto-refresh on `files:saved`); `gitBlame.ts` extension with togglable end-of-line annotations (`<author> · <hash> · <relative date> · <summary>`) gated by `nexus.editor.toggleBlame`; `DiffView.tsx` modal hunk viewer opened by `nexus.editor.openDiff` rendering hunks unified with red/green tinted lines. Per-line marker classification is the load-bearing logic — `buildLineMarkers` walks `GitDiffHunk[]` tracking a `pendingRemoved` buffer to distinguish added (`+` only) from modified (`+` paired with `-`) from deletion-above (`-` with no replacement); 8 unit tests pin every branch. Click-on-gutter Stage shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Click-on-gutter Revert shipped as a follow-up 2026-05-08 — new `com.nexus.git::discard_hunks` IPC verb (id 37) reverse-applies the selected hunks of `diff_file` to the working tree (`ApplyLocation::WorkDir`); Alt+click on a gutter marker triggers it, plain click still stages. Side-by-side `MergeView` (would add `@codemirror/merge` for marginal value) still deferred._

---

_BL-078 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `crates/nexus-storage/src/find_replace.rs` module with `find_in_files` + `replace_in_files` (plus `Matcher` shared by both) covers every modifier combo in the DoD: literal / regex, case-sensitive / -insensitive, whole-word toggle. Two new IPC handlers — `find_in_files` (id 57) and `replace_in_files` (id 58) — pass through to those free functions. The walker uses the existing `should_ignore` filter so `.forge/`, `.git/`, etc. stay out of the result set; binary / non-UTF-8 files are silently skipped. Results group by relpath, sorted ascending, with one line of leading + trailing context per hit. After a `replace_in_files` that changes any file, the storage engine triggers a `rebuild_index` so search / graph stay consistent. Shell ships a `nexus.searchPanel` sidebar leaf with debounced query input, regex / Aa / whole-word toggles, replace field, per-file collapse, click-to-open, and per-file or workspace-wide replace. Bound to ⌘⇧F (matching VS Code's "find in files" muscle memory); BL-063's terminal cross-search moved to ⌘⇧G to free the binding. CM6 decorations on open tabs and incremental streaming for very large forges deferred — the current full-batch shape returns within the documented `max_files` / `max_results` caps and the panel UX feels live._

---

_BL-077 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `lspIpc.ts` (typed `com.nexus.lsp` adapter) + `lspClient.ts` (CM6 extension bundle) together turn the BL-076 LSP host into a working editor surface. The extension activates only in code-mode tabs (BL-075 routing) on real on-disk paths; document-mode and untitled tabs skip it. New deps: `@codemirror/autocomplete` 6.20 + `@codemirror/lint` 6.9. Decided against `codemirror-languageserver` — it bundles a WebSocket transport we'd have to monkey-patch around; writing the ~400-line CM6 wiring directly is simpler and avoids a vendored fork. 17 new unit tests cover the converters (severity, position-to-offset with EOL/EOF clamps, diagnostic projection, completion-item mapping with kind chips + docs, location picker, edits applier with bottom-up sort) plus three lifecycle smokes (open/change/close fire-and-forget, matching-URI diagnostics land in the lint state, non-matching diagnostics ignored). Full shell suite at 968 tests stays green._

**Caught a subtle bug along the way.** A draft test used a dynamic `await import('@codemirror/lint')` for `forEachDiagnostic` while the implementation imports `setDiagnostics` statically. Under tsx's loader the dynamic and static specifiers resolved to *different* module instances, so the `lintState` `StateField` constants weren't identity-equal — `setDiagnostics` would write to one field and `forEachDiagnostic` would read from another, and both saw "0 diagnostics." Fixed by importing both statically from the test. Worth flagging because future CM6 extensions that test against state-field-based @codemirror sub-modules will trip the same trap.

**Deferred from the DoD:**
- ✅ **Format-on-save plumbed via the existing save command, not just the keymap.** Shipped in [`0d2cace3`](https://github.com/baileyrd/nexus/commit/0d2cace3). New `cm/saveFormatHooks.ts` registry: every code-mode tab with a live LSP extension registers a per-relpath format hook; `nexus.editor.save` awaits the hook for the active tab before writing. So vim's `:w`, custom save chords, and the `Mod-s` keymap all hit the same format-on-save path. Hook errors surface as a warning toast rather than blocking save.
- ✅ **`nexus.editor:reveal-line` consumer.** Shipped in [`0d2cace3`](https://github.com/baileyrd/nexus/commit/0d2cace3). New `cm/revealLine.ts` helper (`lspPositionToCmOffset` + `revealLineInView`) plus an `api.events.on('nexus.editor:reveal-line', …)` handler in the editor plugin that mirrors the existing reveal-block staging — fire when the destination tab's CM view is mounted, queue otherwise. So Cmd+Click → definition now scrolls + cursors the destination line/character instead of opening at the top.
- ✅ **Document resync after server reconnect.** Shipped under BL-076 (above) — `ConnectionPool` snapshots open documents and replays each `did_open` against the freshly-spawned successor before the original op retries.
- ✅ **WorkspaceEdit applier for rename / code-actions.** Shipped 2026-05-09. New `cm/workspaceEdit.ts` module ships `applyWorkspaceEdit(edit, opts)` plus the supporting helpers (`uriToRelpath`, `groupEditsByRelpath`, `applyTextEditsToString`). The applier walks the LSP `WorkspaceEdit.changes` map, maps each URI to a forge-relative path (skips outside-forge URIs with a debug log), and routes per-file slices through either the live CM6 view (active tab — preserves cursor + undo) or `com.nexus.storage::write_file` (every other URI). Bottom-up edit application matches the format-on-save path so earlier edits don't invalidate later positions. New `nexus.editor.lsp.rename` command (bound to F2) prompts for the new name (pre-filled with the word at cursor), dispatches `com.nexus.lsp::rename`, applies the WorkspaceEdit, and surfaces a "Renamed in N files" toast. New `nexus.editor.lsp.codeActions` command (bound to Mod-.) dispatches `com.nexus.lsp::code_actions` for the cursor range and surfaces the result through the new `api.input.pick` list-picker primitive (also new — see below); chosen action's `WorkspaceEdit` applies through the same `applyWorkspaceEdit` path. Command-only actions (no `edit`) are listed in the picker with a "requires workspace command" disabled note since the host doesn't expose `workspace/executeCommand` yet. 24 unit tests cover URI normalisation, the applier matrix, and the picker store.

**New primitive shipped alongside:** `api.input.pick(items, options)` (see `shell/src/plugins/nexus/pick/`) — list-picker overlay modal with arrow-key nav + substring filter + Esc-to-cancel, mirroring the `nexus.confirm` queue-and-current pattern so concurrent calls serialise behind one modal. Backs LSP code-actions and any future quick-pick surface (rename palette filter, plugin command picker, etc.).

---

_BL-076 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `nexus-lsp` workspace crate ships the full LSP-host skeleton: `lsp.toml` config loader (`[[servers]]` blocks with name / command / args / file_types / root_markers / disabled / env), a tokio-based stdio JSON-RPC transport with Content-Length framing + 16 MiB ceiling, an `LspClient` that drives the `initialize` / `initialized` handshake and demultiplexes server replies into per-request oneshots vs a notification channel, an `LspClient::shutdown` that runs the protocol-level `shutdown` / `exit` then hard-caps the join, and a `ConnectionPool` with lazy connect + transient-failure reconnect against the same `[100ms, 500ms, 2s, 10s, 30s]` backoff schedule `nexus-mcp` uses. The `LspCorePlugin` exposes 11 IPC handlers — list_servers (sync) and open_file / close_file / change_file / completions / hover / definition / references / rename / code_actions / format (async) — and republishes every server-pushed notification on the kernel bus as `com.nexus.lsp.<method-with-dots>` (so `textDocument/publishDiagnostics` becomes `com.nexus.lsp.textDocument.publishDiagnostics`). 28 unit tests + 1 end-to-end integration test against a Python-based mock LSP server (handshake → hover round-trip → didChange → publishDiagnostics fan-out → graceful drop) all pass. 10 new IPC types (`LspOpenFileArgs`, `LspChangeFileArgs`, `LspPositionArgs`, `LspReferencesArgs`, `LspRenameArgs`, `LspCodeActionsArgs`, `LspPathArgs`, `LspOpenFileReply`, `LspServerEntry`, `LspOk`) wired into the `nexus-bootstrap` schema generator; ts-rs bindings + JSON-Schema files committed; drift script passes.

**Deferred from the DoD:**
- **Live `rust-analyzer` / `typescript-language-server` smoke** — DoD called for end-to-end runs against the real servers. Mocked-out via the `tests/end_to_end.rs` Python server: every protocol path the real servers exercise (handshake, request/response correlation, server-pushed notifications, graceful shutdown) is covered. Live smoke is an operator step; the binaries aren't on this dev box's `$PATH` and shipping them via the test would inflate CI cold-start by minutes per run.
- ✅ **Auto-restart on crash with exponential backoff** — shipped in [`746dc5cb`](https://github.com/baileyrd/nexus/commit/746dc5cb): every async LSP IPC handler now routes through `pool.call_with_reconnect` so transient transport / request-timeout / `NotRunning` failures trigger a reconnect-and-retry against the configured backoff. A shared `proxy_request` helper backs every position-style request; the lifecycle handlers (open / close / change) inline the closure since their arguments don't share a common shape.
- ✅ **Document resync after reconnect** — shipped alongside the reconnect wrapping. Between attempts the pool snapshots the broken client's documents (new public `OpenDocument` type + `documents_snapshot` accessor) and replays each `did_open` against the freshly-spawned successor before the original op retries.
- ✅ **Server-initiated requests** — shipped 2026-05-09. The reader task now writes spec-compliant replies back through the shared `Arc<Mutex<ChildStdin>>` (lifted out of `LspClient::connect`'s scope so the reader can capture an `Arc` clone before being spawned). New private `build_server_request_reply(method, params, id)` helper covers every method the major servers (rust-analyzer, typescript-language-server, gopls) issue at boot: `workspace/configuration` returns `[null, …]` matching the requested item count; `workspace/workspaceFolders` / `window/showMessageRequest` / `window/workDoneProgress/create` / `client/(un)registerCapability` / `workspace/{codeLens,diagnostic,inlayHint,semanticTokens,foldingRange}/refresh` return `null`; `window/showDocument` and `workspace/applyEdit` return `{ "success": false }` / `{ "applied": false }` so servers fall back gracefully; unknown methods get the `-32601 method not found` error rather than a hung request. `minimal_client_capabilities()` now advertises `workspace.configuration: true`, `didChangeConfiguration.dynamicRegistration: true`, `window.showMessage`, and `window.workDoneProgress: true` so servers know to ask. 13 unit tests pin the dispatch table; new end-to-end test (mock Python server fires `workspace/configuration` after `initialized`, captures the host's reply via `publishDiagnostics`, asserts it's `[null, null]` for a 2-item request).

**Why this matters:** the LSP track unblocks BL-077 (CM6 LSP client) and BL-081 (DAP debugger). Without nexus-lsp, code editing in Nexus is a syntax-highlighted textarea (BL-075's mode); with it, the shell can light up completions / diagnostics / hover / go-to-def for any language with an LSP server.

---

_BL-075 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `codeMode.ts` module exports `getEditorMode(name, codeExtensions?)` and `pickLanguageExtension(name)`. The read/save split was already in place pre-BL-075 (markdown → `com.nexus.editor::open` session, non-markdown → `com.nexus.storage::read_file` / `write_file`); what BL-075 adds is the CM6 language extension layered onto the non-markdown path plus the user-configurable extension list. Eight languages cover the documented "common types": Rust, TypeScript (TS/TSX), JavaScript (JS/JSX/MJS/CJS), Python, Go (via `@codemirror/legacy-modes`), JSON / JSONC, YAML, TOML (also legacy-modes). `EditorView` reads the live `nexus.editor.codeFileExtensions` setting through the runtime's new `getCodeFileExtensions()` accessor; an empty / whitespace-only setting falls back to the default list rather than disabling code mode entirely. Markdown is unconditionally document-mode regardless of the override list, so a misconfigured setting can't break the markdown editor. 11 new unit tests pin the routing matrix; full shell suite at 903 tests stays green._

---

_BL-069 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). The original backlog entry described "the widget doesn't fetch or display data" — that was outdated by the time work started. The Rust handler (`HANDLER_EXECUTE_DATABASE_VIEW = 12`) was already wired through `com.nexus.storage::base_load` → `crate::database_view::config_to_view` → `com.nexus.database::apply_view`, and the shell widget already fetched + cached + rendered (BL-012 splits 1–4). The actual gap was that the widget rendered every view type as either a flat table or grouped table-sections — no real Kanban / Calendar / Gallery layouts and no type-aware cell formatting. This pass closes that gap. New `databaseViewFormat.ts` ships a `formatCell(value, fieldDef?)` that maps `nexus_types::bases::FieldType` to renderable strings (text/long-text/url/email/uuid pass through; number/currency/percent use `Intl.NumberFormat` with locale grouping; date/datetime/time render in ISO form; checkbox renders ✓; select / multi-select / relation pull `.label` / `.name` / `.id`). `databaseViewWidget.ts`'s `renderApplied` now switches on `applied.view_type` first (kanban / calendar / gallery / table-list-timeline-fallback) before falling back on `layout.kind`, and the original `viewConfig` is threaded through so the layout renderers can read `column_by` (kanban), `date_field` (calendar), and `title_field` (gallery). Three new layouts: `renderKanban` (horizontal flex of column sections, each header carries the group value + count, records render as compact cards via the shared `buildCard` helper), `renderCalendar` (7×6 month grid anchored on the median group-key date, weekday headers, pill per record per day, plus an "Undated" bucket for the `(none)` sentinel groups from `MISSING_GROUP_KEY`), and `renderGallery` (grid of cards with title from `title_field` falling back through the first text field, body capped at 5 labeled rows). `EditorKernelClient.executeDatabaseView` now passes an explicit `30_000` ms timeout per the BL-069 DoD. CSS styles for every new class committed alongside in `livePreview.css`. 16 new shell tests (12 in `databaseViewFormat.test.ts` covering every FieldType + lookupFieldDef defensive paths; 4 in `databaseViewWidget.test.ts` for kanban columns / gallery cards / calendar grid + undated bucket / type-aware cells). Full shell suite at 984 tests stays green; typecheck + lint clean. The pre-existing kanban "grouped" test was rewritten to assert against the new `.cm-md-dbview-kanban-column` DOM (the legacy `.cm-md-dbview-group` shape only fires for `view_type=table` with a `grouped` layout and `view_type=list/timeline` — neither user-driven in this codebase yet)._

**Deferred from the DoD:**
- ✅ **Kanban drag-to-reorder with write-back** — shipped 2026-05-08; closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).
- ✅ **Inline cell editing** — shipped 2026-05-08; closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Click-to-edit on typed cells across table / kanban-card / gallery-card layouts; Enter commits, Escape restores, blur commits, checkbox toggles directly.
- ✅ **Calendar navigation** — shipped 2026-05-08; closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `‹` / Today / `›` controls layered over the median-of-data initial anchor.
- **Gallery cover field.** Cards show a title + body fields but no image cover. Adding cover support requires a `cover_field` on the gallery view-type variant (which the parser doesn't accept yet) plus an `<img>` slot on the card; deferred until either the schema gains an image FieldType or a user asks for it.
- **Multi-select / relation inline editing.** Currently read-only — `isEditableType` excludes them. A picker UI for these is its own scope (tag chips with autocomplete for multi-select; search-with-create-new for relations).
- **Filter + sort round-trip.** The renderHeader chips already wire to `onUpdateConfig` which the decoration extension translates into a markdown rewrite (BL-012 split 5 — landed previously). DoD bullet was already satisfied before this pass; mentioning here for completeness.

---

_BL-061 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `with_memory_monitor(MemoryLimits)` builder on `TerminalCorePlugin` plus a memory poller thread (peer of the existing drainer / lifecycle-forwarder) that auto-tracks every spawned session, samples RSS at the configured interval (default 1 s — PRD-09 §7.2), and on `HardExceeded` publishes a `TerminalEvent::MemoryLimitExceeded { id, rss_bytes, limit_mb }` then runs `close_session` (which then emits `SessionClosed`, in causal order). `SessionInfo.rss_bytes: Option<u64>` is layered onto every `get_session_info` and `list_sessions` response so the shell UI can render a memory chip from a single round-trip. Bootstrap wires the monitor with PRD §7.3 defaults (250 MB soft / 500 MB hard) for every session. Per-saved-command overrides via `SavedCommand.memory_limit_mb` shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md)._

---

_BL-060 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Four IPC handlers on `com.nexus.terminal` shipped (`adhoc_list` id 19, `adhoc_get` id 20, `adhoc_delete` id 21, `adhoc_promote` id 22 — handler 18 was already taken by BL-059's `open_in_terminal`, so the contiguous-id allocation slipped by one), each backed by the existing `SqliteAdHocStore`. The plugin attaches both stores against the same `<forge>/.forge/procmgr.sqlite` file (separate `Connection`s, separate tables — `procmgr_commands` vs. `procmgr_adhoc_history`). `nexus proc history [--limit N] [--json]` wraps `adhoc_list` with a fixed-width table renderer. Shell ships a sidebar `History` leaf (`nexus.terminal.history.show`) sibling to Saved Commands, with re-run / promote / delete affordances; the promote form pre-fills from the row's program name and refreshes the saved-commands cache on success. Recording new ad-hoc rows over IPC is intentionally out of scope here — the procmgr layer still calls `SqliteAdHocStore::record` directly. A dispatch-side `adhoc_record` lands when ad-hoc execution becomes a first-class IPC verb (BL-056 follow-up)._

---

_BL-059 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `com.nexus.terminal::open_in_terminal` handler (id 18 — DoD-suggested 22 was just a placeholder; used the next contiguous slot) walks a default priority list (iTerm2, WezTerm, Ghostty, Kitty, Alacritty, Windows Terminal, GNOME Terminal, Konsole, XFCE Terminal, Terminal.app, x-terminal-emulator, xterm), picks the first whose program is on `$PATH`, and spawns it detached at the saved command's `working_dir` (Unix `setsid` so SIGHUP doesn't tear it down with nexus). `SavedCommandsView` gains an "External" button per row when `working_dir` is set. Settings → Terminal priority editor shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `terminal.externalPriority` setting (comma- or whitespace-separated list of emulator tags); `parseExternalPriority` canonicalises kebab → snake and drops unrecognised tokens against an allowlist mirroring the kernel's `parse_kind`; `openInExternalTerminal` threads the parsed list into the `priority` IPC arg, blank string falling through to the kernel default. Per-command env-var passing shipped as a follow-up 2026-05-08 — `SavedCommandDraft` now includes `env_vars`, the form gained a `KEY=VALUE` textarea (one pair per line, comments + blanks tolerated), and `parseEnvVars` / `envVarsToText` round-trip without quote-stripping (Bash treats anything after the first `=` as the literal value, and we match)._

---

_BL-058 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Detection ported from `nexus-terminal/src/urls.rs` to `shell/src/plugins/nexus/terminal/urls.ts` (no new IPC surface added — the original `read_output`-coupled plan was unnecessary given the existing PTY byte stream); a `createUrlExtractor()` decodes UTF-8 with `stream: true` and emits per-line matches; `UrlChips.tsx` renders a deduped top-5 pill strip above xterm with `api.platform.shell.openExternal` click handling. Chips clear on session change and via an explicit dismiss button._

---

_BL-057 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Landed alongside BL-052 in the same sweep. The lifecycle forwarder's `publish_lifecycle_event` now fans `SessionCreated` / `SessionClosed` / `MemoryLimitExceeded` out to the universal `com.nexus.activity.appended` topic with `origin: "terminal:<session_id>"`, `surface: "process"`, and a human-readable `prompt` ("started session …", "session … exited (code=N)", "session … killed (OOM): rss=… limit=…MB"). Streaming variants (`OutputReceived`, `PatternMatched`, `SessionEvicted`) deliberately don't emit activity entries — they're either too chatty or too internal. `SessionClosed` flips outcome to `Error` when `exit_code` is non-zero so the timeline glyph matches user intuition. The terminal `nexus-terminal` crate gained a `nexus-types` dep for the shared `ActivityEntry` shape; no terminal-side schema change._

---

_BL-056 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). New `type = "terminal"` step with `slug` (required), `action` (start / stop / restart / run_adhoc, default start), `command` (required for `run_adhoc`, ignored otherwise), and `working_dir` (override) lands in `KernelActionDispatcher`. `start` and `run_adhoc` route through `com.nexus.terminal::run_saved` (BL-055); `stop` lists sessions and closes every one whose name matches `saved:<slug>` (the convention `run_saved` writes); `restart` is `stop` followed by `start`. `run_saved` gained an optional `command` override field so `run_adhoc` reuses the saved profile (shell / cwd / env) with a fresh command line per run. The `validate` handler became async-capable: when terminal steps are present and the kernel context is wired, it queries `saved_list` and rejects unknown slugs with a clear error; without a context (test runtimes) it falls back to the parse-only path. `nexus workflow run` and `nexus workflow validate` use these surfaces unchanged through their existing IPC routes._

---

_BL-066 closed 2026-05-06 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Hover-fade icon row replaces the always-visible text buttons on `SavedCommandsView`: Run / External (when `working_dir` is set) / Edit / ↑ / ↓ / Delete (red on hover). Spawn / Stop / Restart icons shipped as a follow-up 2026-05-08 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md): the row now polls `com.nexus.terminal::list_sessions` every 2s, attributes any `saved:<slug>`-named session back to its row (BL-055 / BL-056 convention), surfaces a green dot + Stop / Restart icons when one or more matching sessions are live, and adds an always-visible Spawn (⚡) icon that calls `run_saved` to start a fresh managed session. Dismiss not implemented — the running indicator naturally clears when sessions exit and there's no separate "managed but ignorable" state in the model._

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

_BL-068 closed 2026-05-14 (umbrella close — every phase shipped 2026-05-06 through 2026-05-09; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

_BL-067 closed 2026-05-14 (umbrella close — Phase 1 / 2a / 2b / 2c / 2d / Phase 0 introspection / finish polish all shipped 2026-05-07 through 2026-05-14; see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))._

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

> **All four phases closed 2026-05-07** — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Phase 1: pill-shaped editor tabs, ember segmented inspector control, status-bar forge name + ember dot. Phase 2: ember wikilinks, path-style inline code, YAML frontmatter metadata bar. Phase 3: Obsidian-style callouts (`> [!type] Title`) with seven types + per-type accent dot. Phase 4: status pills (`info`/`warn`/`risk`/`ok`) in the frontmatter metadata bar + status dots in the file-tree row, sourced from `status:` frontmatter via a new `com.nexus.storage::read_frontmatter` IPC. Q2 was decided in favor of frontmatter as the canonical source.
>
> **Tail items shipped 2026-05-14** in commit `165e0b1f`. Three remaining mockup elements that the prior PRD listed as deferred follow-ups: table chrome (border-separate + dashed row separators + alternating tints + uppercased header band + hover lift), outline numbered prefix (`.nx-outline__prefix` chip at depth 0 with `01 / 02` tabular numerics on `--interactive-accent-soft`), and word-count badge (new `countWordsIn` + per-section accumulation in both `parseHeadings` and `treeToHeadings`; `OutlineHeading.wordCount` rendered through `compactCount` for sub-4-char chip widths; punctuation-only tokens skipped). Font bundling stays out of scope (separate offline-first workstream).

**Source**: Forge Color System mockup + ember-on-slate exploration (2026-05-06) — full plan in [BL-053-forge-visual-target.md](BL-053-forge-visual-target.md)
**Effort**: Phase 1 ~1 day _(shipped)_ · Phase 2 ~2 days _(shipped)_ · Phase 3 ~3–5 days _(shipped)_ · Phase 4 ~3–5 days _(shipped)_
**Crates**: `shell/src/shell/`, `shell/src/plugins/nexus/editor/`, `shell/src/plugins/nexus/outline/`, `shell/src/plugins/core/editorArea/`, possibly a new markdown-extension surface in `nexus-editor`
**Related**: bundled themes `nexus-ember-dark` / `nexus-ember-light` (delivered 2026-05-06) supply the tokens; this BL styles against them

The bundled ember themes ship the right token values, but the shell renders a much plainer surface than the Forge mockup — mostly because rich rendering (callouts, status pills, frontmatter metadata bars, path-style inline code, ember wikilinks) is renderer/plugin work, not theme work. The companion plan splits the gap into four phases ordered by ROI, identifies what's reachable through theme+CSS alone vs. what needs renderer extensions, and lists the four product decisions that gate code (callout syntax, status data source, font bundling, scope commitment).

**Phase 1 alone delivers ~70% of the visible win.** Subsequent phases are independent and can be greenlit individually.

**Definition of done (per phase):** acceptance criteria filled in when a phase is scoped in — see §6 of the companion doc. The plan itself does not commit to any phase.

_BL-052 closed 2026-05-07 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `ActivityEntry` / `ActivitySurface` / `ActivityOutcome` / `ActivityToolCall` lifted from `nexus-ai` to `nexus-types::activity` (kept the type names; `nexus-ai` re-exports for back-compat, so existing call sites still compile). New `ActivityOrigin` enum (`Ai` / `User` / `Plugin(id)` / `Workflow(id)` / `Agent(id)` / `Terminal(id)` / `Git` / `Storage` / `Capability`) with a `to_wire` / `from_wire` round-trip; `ActivityEntry` carries it as a `String` field with `serde(default = "ai")` so legacy on-disk JSONL parses cleanly. New universal topic constant `ACTIVITY_APPENDED_TOPIC = "com.nexus.activity.appended"`; the AI recorder now publishes to BOTH this and the legacy `com.nexus.ai.activity_appended` so existing AI-only subscribers keep working. Emitters wired in this sweep: storage (file_created / file_modified / file_deleted / file_renamed via `publish_file_activity`), git (HEAD-changed commit detection via `publish_git_activity` + the auto-committer's existing emit reshaped to a proper `ActivityEntry`), workflow (run start + end via `publish_workflow_activity`), terminal (BL-057 — see its closure note). Shell-side: the `activityTimeline` plugin subscribes to BOTH topics with id-keyed dedup so the AI's twin-emit doesn't render twice; gains an origin filter chip with nine kinds (AI / User / Storage / Git / Terminal / Workflow / Agent / Plugin / Capability); surface union widens to include `file` / `process` / `git` / `workflow` / `capability` plus the existing AI surfaces. Pre-existing schema tests catch the new types via the existing `every_object_schema_denies_additional_properties` invariant — `ActivityEntry` keeps `deny_unknown_fields` (extras rejected; `serde(default)` handles missing-on-read separately).

**Deferred from the DoD:**
- ✅ **Capability grant/revoke emitter (runtime path)** — shipped 2026-05-08; closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). `PluginLoader::grant_capability` + `revoke_capability` now publish `com.nexus.kernel.capability_granted` / `_revoked` plus a universal-activity entry (origin=`Capability`, surface=`Capability`). Bootstrap-time grants that happen before the bus is wired still skip the emit path — those are captured in the SQLite audit table only, same as before. A future pass could thread the bus into the install-time consent flow if a noisy gap surfaces.
- ✅ **Plugin-id rename `nexus.activityTimeline` → `nexus.activity`** — shipped 2026-05-08 alongside a new `legacyPluginIds: readonly string[]` field on the catalog `PluginEntry` type and a `buildLegacyIdAliases` helper that runs the persisted `plugins.enabled` list through the per-entry alias map at boot. Internal command / view / activity-bar ids deliberately keep the `nexus.activityTimeline.*` prefix (saved layouts + user keybindings would break otherwise). Closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).
- **Per-emitter opt-out config** — none of the emitters ships a knob today; the topic is fire-and-forget. Add a `nexus.activity.disabledEmitters` setting once a noisy emitter actually exists in the wild.
- **Shared privacy redactor** — `nexus-ai`'s `Redactor` (PRD-12 §privacy) applies only to the AI-recorder path. Lifting it to a shared crate touches every emitter and adds a config surface; deferred. Each non-AI emitter today produces short structured prompts (`"renamed a → b"`, `"commit abcdef on main"`) that don't carry user-secret content, so the immediate risk surface is low. Track when an emitter starts surfacing free-form user input.
- ✅ **Push/pull git events** — shipped 2026-05-08 via tracking-branch SHA observation in the existing git poller; closure note in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).

**Why this matters:** transparency parity — agents (AIG-02) can dispatch tools that span all subsystems, and the user now sees every effect in one pane, not five separate logs._

## Partially New Features (concept exists in PRDs but design is unspecified)

_BL-007 closed 2026-05-09 — fully subsumed by [BL-074](#bl-074-collaborative-editing--crdt-layer). Every DoD bullet shipped under that umbrella: `nexus-crdt::PersistedCrdt` lives at `<forge>/.forge/.editor/crdt/<sha>.json` (rebuildable per file-as-truth, but the CRDT state file specifically rides through git per the gitignore policy that shipped 2026-05-09); the `nexus crdt merge-driver` shim runs `OpLog::merge` on pull conflicts; `CrdtPublisher::reload_after_external_change` + `start_pull_landing_subscriber` close the transport loop end-to-end against `com.nexus.git.commit`; structural conflicts that can't merge silently surface on `com.nexus.editor.crdt.conflict.<relpath>` and through the `nexus.crdtConflict` shell-side toast. Multi-user async collaboration via git push/pull without manual conflict resolution works today. The "Partially New Features" framing pre-dated the BL-074 plan and is no longer accurate._

---

## Doc-audit-surfaced product gaps (2026-05-12)

Features spec'd in a PRD or ADR but missing from code, found by the 2026-05-12 doc-traceability audit. Full descriptions in [../roadmap/DOC-GAPS.md](../roadmap/DOC-GAPS.md) under DG-32 through DG-46. Doc-only bugs (DG-01..DG-31) live in DOC-GAPS but are not cross-listed here since they're not BL-shaped work.

- [x] **DG-32: PRD-15 §4 ToolRegistry** — closed 2026-05-12. Typed `AgentToolRegistry` + `Capability` enum + `AgentToolSpec` shipped in `crates/nexus-agent/src/tool_registry.rs`; seeded with the eight in-tree tools at bootstrap. New `com.nexus.agent::list_tools` (handler id 18) + `nexus tool list [--capability ID]…` CLI close the discoverability DoD. See [../roadmap/DOC-GAPS.md#dg-32](../roadmap/DOC-GAPS.md).
- [x] **DG-33: PRD-15 §5 Memory** — closed 2026-05-12. Filesystem-backed agent memory shipped at `crates/nexus-agent/src/memory.rs` with the eight `MemoryEntry` variants from the PRD, append-only `.forge/agents/<id>/history.jsonl`, decision-preserving prune, and four IPC handlers (`memory_record`, `memory_query`, `memory_prune`, `memory_export`). **Auto-recording follow-up shipped 2026-05-14**: `events_from_session(&session, now_ms)` + `serialize_entries_jsonl` pure helpers + `record_session_memory(ctx, &session)` fire from `handle_session_run` so every completed session appends its tool calls / errors / compactions to the agent's `history.jsonl`. **Prompt-time recall follow-up shipped 2026-05-14**: `format_memory_preamble(entries, decision_cap, recent_cap)` pure renderer + async `compose_memory_preamble(ctx, agent_id)` + `system_prompt_with_skills(ctx, goal, agent_id)` splice the most recall-worthy entries into the planner's system prompt at session start. Decisions are pinned (PRD-15 §5); recent non-decisions are capped independently so the budget stays bounded. Dated snapshots remain a deferred follow-up. See [../roadmap/DOC-GAPS.md#dg-33](../roadmap/DOC-GAPS.md).
- [x] **DG-34: PRD-15 §7 interactive approval round-trip** — closed 2026-05-12. Risk classification (`round_requires_approval`) layered on top of the existing ADR 0024 Phase 2b bus-bridge: `BusBridgePolicy` now auto-approves rounds where every proposed tool is registered with `requires_approval=false`, prompts otherwise. `round_proposed` payload carries `requires_approval` + `registered` per tool call so the shell can render per-call risk badges. New `strict_approval` arg restores the prompt-every-round behaviour. See [../roadmap/DOC-GAPS.md#dg-34](../roadmap/DOC-GAPS.md).
- [x] **DG-35: PRD-15 §8 six built-in agent classes** — closed 2026-05-12. `auditor` (read-heavy reviewer), `librarian` (knowledge organisation), and `coach` (guidance-over-execution) archetypes added in `crates/nexus-agent/src/archetypes.rs` with stable ids `com.nexus.agent.{auditor,librarian,coach}`. `list_archetypes` IPC now returns all six. See [../roadmap/DOC-GAPS.md#dg-35](../roadmap/DOC-GAPS.md).
- [x] **DG-36: PRD-15 §9 `.agent.toml`** — closed 2026-05-12. Parser + loader + scanner shipped in `crates/nexus-agent/src/custom_agent.rs`; new `com.nexus.agent::list_custom` IPC handler (id 19) + `nexus agent list-custom` CLI surface the manifests. **Routing follow-up shipped 2026-05-14**: `nexus agent plan/run --archetype <slug>` now accepts custom-manifest slugs — new `resolve_archetype_for_run(ctx, name)` reads `<forge>/.forge/agents/<slug>/agent.toml` and layers `[system_prompt]` over the manifest's `[agent].archetype` baseline; custom agents run with a `com.nexus.agent.custom.<slug>` id. **Tool allow/deny enforcement shipped 2026-05-14**: new `ManifestToolPolicy` + `ManifestPolicyGate<P>` decorator over `SessionPolicy` — manifest denials surface via `RoundDecision::Partial` so the session loop's existing failure path (denied call → `is_error: true` tool-result turn → model recovers) handles them. DG-36 is fully closed. See [../roadmap/DOC-GAPS.md#dg-36](../roadmap/DOC-GAPS.md).
- [x] **DG-37: PRD-15 §10 agent-to-agent comms** — closed 2026-05-12. New `com.nexus.agent::delegate` (handler id 24) runs a sub-session via the existing session machinery; new `delegate_to_agent` registered in the agent tool registry so a planner LLM sees A2A as a first-class tool call. Parallel / pipeline remain caller composition patterns (no resurrected orchestrator). See [../roadmap/DOC-GAPS.md#dg-37](../roadmap/DOC-GAPS.md).
- [x] **DG-38: PRD-17 cross-platform reframe-or-build** — closed 2026-05-12 with **Option A** (reframe as desktop-only). PRD-17 retitled "Desktop Strategy"; per-section "Deferred (DG-38)" callouts at §3/§5/§6/§15/§16 and a partial-deferral note at §4; `00-index.md` + `IMPLEMENTATION_STATUS.md` reflect the new scope. Web/mobile content preserved as design rationale; future multi-platform work would split into per-platform BL entries. See [../roadmap/DOC-GAPS.md#dg-38](../roadmap/DOC-GAPS.md).
- [x] **DG-39: PRD-14 §10 dynamic MCP tool registration** — closed 2026-05-12. New `crates/nexus-mcp/src/dynamic_tools.rs` registry plus `register_tool` / `unregister_tool` / `list_dynamic_tools` IPC handlers on `com.nexus.mcp.host`; `NexusMcpServer::list_tools` + `call_tool` merge static + dynamic surfaces. `notifications/tools/list_changed` broadcast + dedicated capability deferred as documented follow-ups. See [../roadmap/DOC-GAPS.md#dg-39](../roadmap/DOC-GAPS.md).
- [x] **DG-40: PRD-14 §12.2 MCP audit logging** — closed 2026-05-12 via `nexus_kernel::audit::log_mcp_tool_call` + `log_mcp_resource_read` wired through `crates/nexus-mcp/src/server.rs`. See [../roadmap/DOC-GAPS.md#dg-40](../roadmap/DOC-GAPS.md).
- [x] **DG-41: PRD-10 §7 relations + rollup** — closed 2026-05-12. New `crates/nexus-database/src/relations.rs` ships `resolve_relation` + `compute_rollup` with the existing `RollupAggregation` enum (Count / CountUnique / Sum / Average / Min / Max / Percent*). Two new IPC handlers (ids 5 + 6) on `com.nexus.database`. See [../roadmap/DOC-GAPS.md#dg-41](../roadmap/DOC-GAPS.md).
- [x] **DG-42: PRD-10 §8 SQL compilation** — closed 2026-05-12. The SQL compiler already shipped in `crates/nexus-storage/src/bases/query.rs` (16 FilterOps, sorts, pagination, parameter binding) and is exposed via `com.nexus.storage::base_query` (handler id 26). The 2026-05-12 audit conflated it with `apply_view`'s in-memory pipeline — both ship and target different use cases. See [../roadmap/DOC-GAPS.md#dg-42](../roadmap/DOC-GAPS.md).
- [x] **DG-43: PRD-06 §9 frontmatter versioning + migration** — closed 2026-05-12. `Frontmatter.version` typed field + new `nexus-formats::migration` module (FormatVersion parser, MigrationRegistry, scan_versions walker) + `nexus migrate scan|registered` CLI. Registry is empty in this build — no breaking change has shipped yet — but the infrastructure is in place. See [../roadmap/DOC-GAPS.md#dg-43](../roadmap/DOC-GAPS.md).
- [x] **DG-44: PRD-04 §10 dynamic .so/.dll loading** — rejected 2026-05-12; PRD-04 §10 gained a "Superseded by ADR 0011 + ADR 0016" callout.
- [x] **DG-45: ADR 0013 macOS menu-bar plugin** — re-phased 2026-05-12 to formal-release WI-45 alongside Mac packaging/notarization (WI-41). ADR 0013 addendum documents the slip; decision stands. See [../roadmap/DOC-GAPS.md#dg-45](../roadmap/DOC-GAPS.md).
- [ ] **DG-46: ADR 0006 first in-tree consumer** — convention-only ADR; no in-tree user yet because community WASM plugins haven't shipped.

---

## Post-migration carryover gaps (2026-04-24)

Capabilities described in legacy `app/` documentation that were not carried over to `shell/` during the Phase 4 WI-37 retirement. The single still-open OI lives in [../roadmap/OPEN-ITEMS.md](../roadmap/OPEN-ITEMS.md); the 21 resolved-OI audit trail was archived 2026-05-12 to [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md). BL-shaped follow-ups land in [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md).

### Open

- [ ] **OI-05: Rust dep duplication** — Blocked on upstream. 34 crates with duplicated versions all trace through `wasmtime 42` (toml/sha2/digest/rand_core/reqwest/rustix/nix/hashbrown) or `portable-pty → filedescriptor` (`thiserror 1`). Revisit after the next wasmtime major release.

### Resolved (preserved here for cross-reference; full notes in [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md))

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
- [x] OI-15 — Manifest signature / provenance — `ed25519-dalek` verification of `plugin.json.sig` against a trusted public-key list; `VerificationStatus` enum + `verify_plugin_signature` in `shell/src-tauri/src/lib.rs`; untrusted/invalid plugins filtered at scan time; "verified" / "unsigned" pill in Settings → Plugins. `TRUSTED_PUBLIC_KEYS` is empty pre-marketplace; populate when the marketplace CA exists _(2026-05-01)_
- [x] OI-18 — Snippet trigger collision detection — `SnippetRegistry` with `getConflicts()`, `plugins:snippets-conflict` event, Settings → Snippets tab with conflict banner + per-row badge; `editor.registerSnippet` API + `contributes.snippets` manifest path landed alongside _(2026-05-01)_

---

## Formal release scope (deferred)

Tracked in full in [../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md](../roadmap/REQUIRED-FOR-FORMAL-RELEASE.md). Out of scope for personal-tool use; surface here so the IDs are findable.

- [ ] **WI-41: Tauri auto-updater + code-signing + release channel.** ~5–7 eng-days plus 1–3 weeks calendar for signing-cert procurement.
- [ ] **WI-42: Crash reporting & telemetry.** ~5 eng-days, opt-in via Settings.
- [ ] **WI-44: Minimal marketplace.** ~5 eng-days; index schema + shell UI + CLI install + tarball publishing. Paired with **OI-15** (manifest signing) and **F-8.1.1 / F-8.1.2** (iframe sandbox + boundary-bound `pluginId`) before opening to untrusted plugins.
- [ ] **WI-46: Beta → GA logistics.** Triage rubric, test-group recruitment, ship criteria. ~3 eng-days plus 2-week calendar.

---

## Future directions (scoped 2026-04-28)

Previously: design-only docs without committed timelines. **Scoped into the implementation plan on 2026-04-28** — each FD piece now has a BL-* ID (see "Future-direction items minted into the backlog" above) and the docs themselves remain authoritative for design rationale.

- **AI integration directions** — see [../roadmap/AI-INTEGRATION-DIRECTIONS.md](../roadmap/AI-INTEGRATION-DIRECTIONS.md). Mapping: "inline rewrite/summarize" → BL-034 (engine) + BL-035 (action surface); "auto-link suggestions" → BL-039; "semantic search" → BL-040; "per-surface chat" → merged into BL-010 (reshape note); "skills as prompts" → composed via BL-021 / BL-022; "agent loops" → merged into BL-027 (same surface); "MCP exposure" (Nexus-as-server) → BL-042; "background indexing" → BL-041. Direction "tool-calling" was already BL-016.
- **Ambient copilot UX patterns** — see [../roadmap/AI-AMBIENT-COPILOT-PLAN.md](../roadmap/AI-AMBIENT-COPILOT-PLAN.md). Mapping: Cmd+I overlay → BL-032; context chips + model switcher → BL-033; ghost suggestions → BL-034; right-click AI actions → BL-035 (shared with NB block AI actions); margin suggestions + inline correction → BL-036; activity timeline → BL-037; citations → BL-038; capture → AI → folded into BL-043 (memory quick-capture).
- **AI memory layer** — see [../roadmap/AI-MEMORY-LAYER-PLAN.md](../roadmap/AI-MEMORY-LAYER-PLAN.md). Mapping: quick-capture → BL-043; auto-enrichment on save → BL-045; recall hotkey → BL-044; implicit chat context → merged into BL-010 (reshape note); code-aware capture → BL-046; scheduled digests → BL-047.
- **Notion-style block UX out-of-scope follow-ups** — see [../archive/notion-block-ux-plan.md](../archive/notion-block-ux-plan.md) (archived 2026-05-12 — all 6 phases shipped). Mapping: drag-to-embed into canvas → BL-048; block-links navigator → BL-049 (gated on block-id stability ADR); side-margin comments → BL-050; block AI actions → merged into BL-035; multi-cursor from multi-block → BL-051.

---

## Research-surfaced ideas (2026-05)

Four external-project assessments under [`../research/`](../research/) each carry an explicit Adopt / Adapt / Skip column. Items that have been promoted into the backlog carry their BL-NN cross-ref; items left here are held discoverable until they're picked up. Skip items stay in the research doc only.

- **GitNexus capability porting** — see [../research/gitnexus-capability-assessment.md](../research/gitnexus-capability-assessment.md). Seven scoped ports recommended; three promoted as a coordinated track (2026-05-13):
    - Cross-repo code intel index → **[BL-114](#bl-114-code-symbol-index-foundation-gitnexus-port)**
    - Three new MCP tools (`nexus_context` / `nexus_impact` / `nexus_detect_changes`) → **[BL-115](#bl-115-mcp-tools-for-code-intel-gitnexus-port)**
    - `ai.generate_docs(symbol_id)` doc generator → **[BL-116](#bl-116-aigenerate_docs-symbol-aware-doc-generator-gitnexus-port)**
  Remaining four (diff→symbol detection, `codegraph.impact(depth)` handler, BM25 over code symbols, optional clustering pass) are downstream of BL-114 and stay held here until BL-114 lands.
- **AFFiNE portability** — see [../research/affine-portability-assessment.md](../research/affine-portability-assessment.md). 9 Adopt items (snapshot API, command pattern, linked-docs UX, database views, mind-map output, multimodal embed pipeline, custom block schemas pattern, inline-embed extensions pattern, widget extensions — last two already adopted in spirit) plus 8 Adapt items (canvas, PDF embed, slide gen, image gen, custom blocks, AI panel UX, inline primitive, doc-on-canvas). 11 items judged already-adopted-in-spirit. **Held as reference material — not promoted to BL-NN entries.** The 2026-05-13 review judged the Adopt list as diffuse pattern-borrowing without clean PR shape, and most universally-compelling Adapt items (canvas, etc.) overlap shipped work (BL-049 / BL-051 / BL-067 / BL-068). Mine specific items when a feature need surfaces. See §6 of the assessment.
- **Anything-LLM portability** — see [../research/anything-llm-assessment.md](../research/anything-llm-assessment.md). Two Adapt items promoted (2026-05-13):
    - `nexus-audio` crate with Whisper STT + TTS provider-trait backends → **[BL-117](#bl-117-nexus-audio-stt--tts-crate-anything-llm-port)**
    - Web Speech API native STT/TTS in the Tauri webview → **[BL-118](#bl-118-web-speech-api-shell-integration-anything-llm-port)**
  Per-user scoped API token issuance held here — conflicts with the personal-tool / single-user stance. Skip items (multi-user, PostHog telemetry, cloud deploys, scheduled-jobs engine, "desktop app") are settled as out-of-scope per existing PRDs/ADRs.
- **Hermes Agent native-Rust port** — see [../research/hermes-agent-implementation-plan.md](../research/hermes-agent-implementation-plan.md). Seven features with merge order; three promoted (2026-05-13), three already shipped, one covered by another BL:
    - Feature 1 — `SessionConfig` + iteration budget (S, merge-first) → **[BL-119](#bl-119-sessionconfig--iteration-budget-hermes-feature-1)**
    - Feature 2 — memory persistence on `com.nexus.agent` → **already shipped as DG-33** (see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md))
    - Feature 3 — skills system + RAG retrieval at session start → **already shipped per PRD-13 + DG-32**
    - Feature 4 — context compression in the session loop (L) → **[BL-120](#bl-120-context-compression-in-the-agent-session-loop-hermes-feature-4)**
    - Feature 5 — session-transcript FTS5 search (M) → **[BL-121](#bl-121-session-transcript-fts5-search-hermes-feature-5)**
    - Feature 6 — multi-agent delegation → **already shipped as DG-37**
    - Feature 7 — ACP protocol adapter crate → **covered by [BL-113](#bl-113-protocol-host-contribution-model-for-lsp--dap--mcp--acp) Phase 4**
- **Thoth capability assessment** — see [../research/thoth-capability-assessment.md](../research/thoth-capability-assessment.md). Thoth is a local-first Python AI assistant (NiceGUI + LangGraph + Ollama + FAISS + NetworkX) with broad tool coverage and a personal entity knowledge graph. Six high-priority ports promoted (2026-05-14); eight medium-priority items held here. Voice (STT/TTS) already covered by BL-117 / BL-118 from the Anything-LLM assessment; context compression overlap noted with BL-120 but BL-125 targets the cheaper pre-invocation sanitisation pass rather than LLM-based summarisation.
    - Typed personal entity graph (people/places/events/concepts, 40+ relation types, FAISS recall) → **[BL-128](#bl-128-personal-entity-knowledge-graph-thoth-port)**
    - Dream Cycle refinement engine (nightly dedup, decay, enrich, infer) → **[BL-129](#bl-129-dream-cycle--knowledge-refinement-engine-thoth-port)**
    - Prompt injection detection (role-override patterns, invisible Unicode, HTML directives) → **[BL-130](#bl-130-prompt-injection-detection-thoth-port)**
    - Pre-invocation message sanitisation (dedup tool results, strip base64 URIs, 85% trim) → **[BL-131](#bl-131-pre-invocation-message-sanitisation-in-the-agent-loop-thoth-port)**
    - Runtime approval gates in the agent loop (pause-and-ask before destructive steps) → **[BL-132](#bl-132-runtime-approval-gates-in-the-agent-loop-thoth-port)**
    - Multi-channel notification output (OS, Telegram, Discord, SMTP) → **[BL-133](#bl-133-multi-channel-notification-output-thoth-port)**
  Medium-priority items held here until capacity opens: vision (`nexus-vision` plugin — screen/file image analysis via local Ollama vision model); built-in web search (Tavily + DuckDuckGo handlers in `nexus-ai`); browser automation (Playwright via `nexus-browser` plugin); chart generation (10 Plotly-equivalent chart types in `nexus-formats` or `nexus-ai`); health/habit tracker (`nexus-tracker` plugin or Bases extension); Docker sandbox for terminal sessions (shadow workspace + patch-apply in `nexus-terminal`); custom tool builder UX (wizard shell plugin wrapping `nexus plugin scaffold`). Skip items (Gmail, Calendar, image/video gen, X/Twitter, Arxiv/Wolfram/Weather — all reachable via MCP): these are community plugin territory.

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
_Three 2026-05-13 spikes (catalog-plugin Enable / src-colocated test gap / srcdoc sandbox boot errors) closed 2026-05-13 — see [BACKLOG_COMPLETED.md](BACKLOG_COMPLETED.md). Suite went from 1260 → 1331 (+71 previously-uncounted tests + 1 documented skip on a stale workspace-Leaf spec)._

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

> Cross-PRD docs audit ([DOCS_AUDIT_2026-04-28.md](../audits/DOCS_AUDIT_2026-04-28.md)) — items spec'd in a PRD that are not yet built and were not previously assigned a backlog ID. Each cites the PRD section, target crate, and estimated effort. Effort scale: small ≈ ½–2 days, medium ≈ 3–10 days, large ≈ 2+ weeks.

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

- **ADR-0009 keyring hard-fail enforcement** — Verified 2026-04-30 and resolved as **OI-21** the same day: `SecurityCorePlugin::on_init` now runs an injected `KeyringProbe` (default `CredentialVault::new().available()`) and returns `PluginError::LifecycleError` with the platform hint when the OS keyring is unavailable. Bootstrap propagates the lifecycle error so frontends exit non-zero. See [../archive/OPEN-ITEMS-resolved-2026-04-26.md](../archive/OPEN-ITEMS-resolved-2026-04-26.md) §OI-21.
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
