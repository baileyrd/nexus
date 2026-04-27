# What Nexus Could Borrow From Tolaria

Tolaria and Nexus solve overlapping problems with very different philosophies, but Tolaria has invested heavily in the "shipping product" axis where Nexus is still light. The list below ranks possible borrowings by signal-to-noise.

---

## High-value borrowings

### 1. Git-driven incremental cache invalidation (Tolaria ADR 0014)

Tolaria's single cleverest piece of engineering. On startup it uses `git diff old..new --name-only` between the cached HEAD and the current HEAD to decide *which files* need re-parsing; falls back to `git status --porcelain` for uncommitted changes; only does a full walk on cold start or version bump.

**Why Nexus should care:** Nexus uses `notify` for live updates (great), but cold-start sync against SQLite + Tantivy + petgraph for a large forge presumably re-walks everything. Adopting git-aware cold-start invalidation would dramatically speed up opening big forges, and Nexus already has the `nexus-git` crate to plug it into.

### 2. Crash-safe rename transactions (Tolaria ADR 0075)

Tolaria stages renames in a hidden `.tolaria-rename-txn/` folder with manifest files. On the next vault scan, unfinished transactions are recovered before entries are listed, so users never see a missing note or a duplicate after a crash.

**Why Nexus should care:** The blast radius of a rename is *bigger* in Nexus than in Tolaria — renaming a markdown file invalidates wikilinks, backlinks, embeddings tied to block IDs, the petgraph node, and possibly Bases-record references. A transactional rename pattern is more important here, not less.

### 3. Concurrent-safe cache replacement (Tolaria ADR 0077)

Two Tolaria windows can refresh the same cache simultaneously without corruption: temp file + fsync + writer lock + on-disk fingerprint check before rename.

**Why Nexus should care:** The plugin-first shell already supports multiple windows, and core plugins all touch SQLite. The pattern translates directly to "two plugins reindex at the same time."

### 4. `ui_*` MCP tools that drive the host UI

Tolaria's MCP server exposes `open_note` and `highlight_editor` so a remote AI client (Claude Code, Cursor) can *steer the running Tolaria window*, not just query the vault.

**Why Nexus should care:** Nexus's MCP server is purely data-access right now (`nexus_read_note`, `nexus_search`, etc.). Adding `nexus_focus_panel`, `nexus_open_note_in_panel`, `nexus_highlight_block`, `nexus_set_filter` would let Claude Code "show me what you found" instead of just returning text. Slots into the plugin contribution registry naturally.

### 5. Explicit external-tool MCP setup flow (Tolaria ADR 0074)

Tolaria writes its MCP entry into `~/.claude.json`, `~/.claude/mcp.json`, `~/.cursor/mcp.json`, and a generic `~/.config/mcp/mcp.json` — but only when the user explicitly clicks "set up." Non-destructive, additive, and reversible.

**Why Nexus should care:** The current Nexus README asks users to hand-edit Claude Desktop config. A `nexus mcp install --client claude|cursor|all` command (plus a desktop UI affordance) would meaningfully lower the bar to first AI integration.

---

## Medium-value borrowings

### 6. Auto-update via the Tauri updater plugin

Nexus ships unsigned binaries you build yourself. Tolaria publishes signed `.app.tar.gz`, AppImage, and deb/rpm with auto-update channels (alpha/stable/canary, ADRs 0057 and 0066). If Nexus wants real users (vs developers running `cargo build`), this is table stakes.

### 7. Sentry + structured release telemetry (Tolaria ADR 0016)

Nexus has `tracing` (great for local debugging) but no way to learn what's actually crashing for users. Sentry catches Rust panics and JS errors; PostHog with feature flags (ADR 0042) lets you canary-test plugin-loading changes without breaking everyone. The capability-gated IPC layer in Nexus would actually make telemetry *easier* — you can record which capabilities deny most often.

### 8. Localization runtime (Tolaria ADR 0087)

Tolaria has app-owned i18n with JSON catalogs and Lara CLI sync. If Nexus wants a global community plugin ecosystem, the shell's UI strings need to be translatable. The `nexus-extension-api` could expose a `t()` function so plugins inherit the same machinery.

### 9. Linux window-chrome handling

Tolaria does custom React-rendered titlebar plus an AppImage WebKit env override (`WEBKIT_DISABLE_DMABUF_RENDERER=1`) for Fedora/Wayland DMA-BUF crashes. Nexus's shell is also Tauri 2 on Linux with the same exposure. Copy-paste fix.

### 10. Window-state persistence with monitor-reattach handling

Tolaria persists window position and size in logical points, migrates older physical-pixel state on read (Retina vs non-Retina), and clamps to currently-available monitor work areas on restore. Standard polish, but easy to miss until users complain.

### 11. Co-located component tests + Playwright smoke suite

Tolaria tests every `.tsx` with a sibling `.test.tsx` and runs Playwright smoke on critical flows. Nexus's TUI/CLI test discipline is good but the desktop shell tests are lighter; this pattern would lock in plugin-host stability.

### 12. CodeScene-managed code-health gates (Tolaria ADR 0064)

Ratcheted thresholds catch hot-spot regressions automatically. Nexus's microkernel is exactly the kind of code where you want to *prevent* people from dumping logic into `nexus-kernel`.

---

## Borrow philosophically, not literally

### 13. "Convention over configuration" for frontmatter

Tolaria's standard field names (`type:`, `status:`, `belongs_to:`, `related_to:`, `_field` for system properties) trigger UI behavior automatically with zero setup. Nexus has a `properties` table but no shared semantic vocabulary — every plugin invents its own. A documented "Nexus frontmatter conventions" spec would make plugins interoperate. Docs/PRD effort, not code.

### 14. "AI without storing API keys" as one supported mode

Tolaria's ADR 0028 ("CLI agent only") is a strong stance: spawn Claude Code or Codex as a subprocess and let them auth themselves. Nexus shouldn't *replace* its in-process multi-provider trait with this — RAG and embeddings need real provider access — but adding a CLI-agent adapter alongside the existing providers would let users who already pay for Claude Max use Nexus without entering an API key.

### 15. The Pulse view (commit feed UI)

Tolaria's `git log --name-status` plus grouping by day is a delightful surface that turns "what changed in my notes lately?" into a one-click answer. Nexus has `nexus-git` and an event bus — a Pulse panel as a core plugin contribution would be natural.

---

## Things Nexus should *not* adopt

A few Tolaria choices are right for Tolaria and wrong for Nexus.

- **No plugin system / "edit the React app to extend it."** Nexus's whole identity is the plugin-first model. Don't backslide.
- **`walkdir` keyword search instead of an index.** Fine for 10k notes; bad for 100k notes plus RAG queries. Keep Tantivy.
- **System `git` CLI instead of `libgit2`.** Tolaria's ADR 0056 makes sense given its "no provider OAuth" stance, but Nexus's libgit2 path gives finer-grained control (worker thread async, fewer subprocess hops) and is a better fit for a runtime.
- **Single-binary Rust backend.** The 20-crate workspace is a feature, not a bug.
- **No CLI/TUI surface.** Tolaria is a desktop app and doesn't need them. Nexus's headless surfaces are a major differentiator.

---

## If I had to pick three

If I were shipping the next Nexus release and could only pull three things from Tolaria:

1. **Crash-safe rename transactions** — biggest correctness win for the largest blast radius.
2. **`ui_*` MCP tools** — biggest UX win for the AI-integration story Nexus is building toward.
3. **Auto-update + signed releases** — biggest gap between "developer toy" and "thing real users run."

The git-driven incremental cache is a close fourth, especially if Nexus has users with vaults over 10k notes.
