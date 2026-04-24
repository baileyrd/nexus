# Phase 4 Implementation Plan — Frontend Unification

**Status:** Plan only (no code changes yet)
**Date:** 2026-04-24
**Author:** Claude (audit + planning run)
**Phase:** 4 of 6 in the shell-migration roadmap (per [`INTEGRATION-REVIEW.md`](./INTEGRATION-REVIEW.md) §5)
**Prerequisite:** Phases 1, 2, 3 complete + pushed to `origin/main`.
**Parallel:** Phase 5 (`PHASE-5-IMPLEMENTATION-PLAN.md`) drafted in parallel; §4.1 dependency matrix wires the overlap.

---

## 1. Executive summary

Phase 4 cleans the frontend surface so CLI/TUI/MCP/Desktop all speak one language, retires the legacy `app/` shell, and gets the plugin developer experience to "scaffold → code → install" in three steps. Estimated **~2 engineer-weeks** — matches `INTEGRATION-REVIEW.md` §5's ballpark — but the distribution is skewed heavily to WI-36 (taxonomy) which is L, not S.

**Audit-corrected effort per WI:**

| ID | Work item | INTEGRATION-REVIEW implied | Audit-corrected | Priority |
|---|---|---|---|---|
| **WI-36** | Shared command taxonomy (IPC schema normalization) | ~4d | **L (~5d pilot 5 handlers)** | P0 |
| **WI-37** | Retire `crates/nexus-app` + `app/` | ~2d | **S (~2d)** | P0 |
| **WI-38** | Unified `nexus` binary (desktop/tui subcommands) | ~3d | **M (~3d)** | P1 |
| **WI-39** | `nexus plugin scaffold` script+ts templates | ~3d | **M (~3d)** — already exists for WASM | P1 |
| **WI-40** | MCP parity check | ~1d | **XS (~1d)** — 10/13 already at parity | P2 |
| **Total** | | ~13d | **~14d** | |

**Three biggest audit findings:**

1. **WI-38 is already half-done.** The CLI binary is **already named `nexus`** (`crates/nexus-cli/Cargo.toml:10`). The renames the outline implied are unnecessary; only subcommand routing is new work. Similar to Phase 2 WI-24/03 (already shipped) and Phase 3 WI-32/33 (kernel-side done). Size stays M because the desktop launch path (subprocess spawn into the Tauri bundle) is non-trivial, but no crate renames needed.
2. **WI-39 is already partial.** `nexus plugin scaffold` *exists* at `crates/nexus-cli/src/commands/plugin.rs:138`, backed by `nexus_plugins::scaffold`. Current output is WASM-only (`Cargo.toml + manifest.toml + src/lib.rs`). Phase 3c shipped the sandboxed-community-plugin contract; the scaffold needs a new `script` / `sandboxed-script` template emitting `plugin.json + index.ts` for the modern path. ~50% reuse of existing code.
3. **WI-40 is mostly at parity.** 13 MCP tools confirmed (`crates/nexus-mcp/src/server.rs` lines 322, 349, 357, 365, 385, 429, 479, 516, 560, 591, 628, 674, 706). 10 have direct CLI + shell equivalents. Only 3 genuine gaps: `nexus_update_note`, `nexus_list_notes`, `nexus_list_tags` lack dedicated CLI subcommands. Plan ships as a matrix + small gap-fill commits, not a big-design WI.

**Phase 4 acceptance** — all five items landed and:

1. `cargo test --workspace` green; all WI-22/32/33/34/35 guardrails still pass.
2. `pnpm --filter nexus-shell test` green; all shell tests (265 as of Phase 3 close) pass.
3. `grep -r "nexus-app\|crates/nexus-app\|app/" README.md CONTRIBUTING.md scripts/ .github/` returns zero matches outside historical / changelog-style text.
4. `nexus --help` shows `desktop`, `tui`, `plugin scaffold`, `plugin install` (if WI-44 lands from Phase 5), `content search`, and any other commands without the `app/` path being required.
5. `nexus plugin scaffold --template script my.example` produces a compilable sandboxed plugin that loads into the shell via `~/.nexus-shell/plugins/`.
6. MCP parity matrix at the bottom of `docs/PHASE-4-IMPLEMENTATION-PLAN.md` is all ✓ (or documents the legitimate exceptions).

---

## 2. Scope summary

### 2.1 Work items

| ID | Title | Size | Priority | Depends on |
|---|---|---|---|---|
| **WI-36** | Shared command taxonomy — ts-rs-generated JSON Schemas per handler | L | P0 | — |
| **WI-37** | Retire `crates/nexus-app` + `app/` | S | P0 | — |
| **WI-38** | Unified `nexus` binary with `desktop`/`tui`/`plugin` subcommands | M | P1 | WI-37 (cleaner after legacy gone) |
| **WI-39** | `plugin scaffold` script + full-stack templates | M | P1 | Phase 3c WI-30e (sandboxed contract) — shipped |
| **WI-40** | MCP parity matrix + 3 missing CLI subcommands | XS | P2 | — |

**Total:** ~14 engineer-days. Two engineers can parallelize WI-36 + WI-37 independently; WI-38/39/40 land in a second wave. See §4 dependency graph.

### 2.2 Phase partitioning (4a vs 4b)

- **Phase 4a — Cleanup & taxonomy** (P0): WI-36 + WI-37. Unblocks everything downstream. ~1 calendar week for one engineer.
- **Phase 4b — DX & parity** (P1/P2): WI-38 + WI-39 + WI-40. Can run serially or parallel. ~1 calendar week.

---

## 3. Phase 4a — Cleanup & taxonomy

### 3.1 WI-36 — Shared command taxonomy (L, P0)

**Intent.** Today each frontend talks to the kernel with a slightly different shape:

- CLI (`crates/nexus-cli/`) calls kernel IPC via `context.ipc_call(plugin, command, args)` and treats `args`/returns as loose `serde_json::Value`.
- MCP (`crates/nexus-mcp/src/server.rs:32-269`) uses `#[derive(JsonSchema)]` on request types for each tool — gives a real schema but each type is hand-authored per tool.
- Shell plugins (`shell/src/plugins/nexus/search/searchRuntime.ts:98-114`) invoke via `api.kernel.invoke<unknown>(plugin, command, args)` and hand-decode returns.
- Kernel handlers themselves (`crates/nexus-storage/src/core_plugin.rs:34-200+`, 52 `HANDLER_*` constants) define arg shapes implicitly via `parse_args<T>` and return shapes implicitly via `to_value`.

Four shapes. Drift is silent until runtime failure. After this WI: one ts-rs-generated `NexusIpcSchema.json` (or per-handler schema files) that every frontend consumes.

**Current state.**

- `crates/nexus-plugin-api/` already has ts-rs derives shipping TS bindings to `packages/nexus-extension-api/src/generated/` (Phase 1 WI-20).
- Per-handler arg + return types exist in `crates/nexus-storage/src/{ai,editor,bases,canvas}.rs` or inline in `core_plugin.rs` — but they're **not** ts-rs-derived today, so neither TS nor JSON-Schema consumers see them.
- `schemars` crate is in the workspace (for MCP's `JsonSchema` derive). Could extend to emit JSON Schema for every kernel handler type too.
- 52 handlers in `nexus-storage`. More across `nexus-ai`, `nexus-editor`, `nexus-agent`, `nexus-terminal`, `nexus-workflow`, `nexus-skills`, `nexus-canvas`, `nexus-bases`.

**Design sketch.**

Two-layer approach:

**Layer A — Rust contract types become authoritative.** Every `ParseArgs` type and return type across kernel crates gets `#[derive(TS, JsonSchema)]`. Co-locate with the handler it serves. Feature-gate behind `ts-export` (WI-20 pattern) so default builds don't pull ts-rs.

**Layer B — Schema emission.** `cargo test -p nexus-<crate> --features ts-export` emits:
- `packages/nexus-extension-api/src/generated/ipc/<plugin>_<command>.ts` (TypeScript interfaces — extend Phase 1 barrel)
- `crates/nexus-bootstrap/schemas/ipc/<plugin>_<command>.json` (JSON Schema — fresh, for MCP + CLI docs + future tools)

**Adoption cadence.** Don't try to migrate all 52+ handlers in one WI. Pilot 5 high-value handlers:
- `com.nexus.storage::search` (CLI, MCP, shell all call it)
- `com.nexus.storage::read_file` (the shell editor depends)
- `com.nexus.storage::write_file` (security-sensitive; Phase 3 WI-32 hardened it)
- `com.nexus.storage::list_dir` (file explorer)
- `com.nexus.ai::stream_ask` (Phase 2 WI-01 uses it)

These 5 prove the pattern. Remaining 47+ migrate opportunistically in Phase 5 / v1.1.

**Drift CI.** Extend the Phase 1 CI drift check — `pnpm generate` must produce no git diff. Fails PRs that change a Rust type without regenerating TS.

**Subagent pattern.**

**Agent 1 (one-shot) — derive the 5 pilot handlers.** Prompt: *"For the 5 handlers listed, locate each arg + return type, add `#[derive(TS, JsonSchema)]`, gate behind `ts-export` feature, regenerate. Cite file:line for every type touched."* Deliverable: diff + verification that generated files appear.

**Agent 2 (one-shot) — schema harness.** Prompt: *"Add `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` that iterates the pilot 5 types under feature ts-export and writes their JSON Schema to `crates/nexus-bootstrap/schemas/ipc/`. Commit the emitted schemas to the tree (like Phase 1's generated TS). Add drift-check to bootstrap's CI invocation."* Deliverable: schema files + test.

Main thread: review, commit, update docs.

**Commit plan.** 3 commits:

1. `feat(ipc): ts-rs derives on 5 pilot handler types (WI-36 pilot)`
2. `feat(bootstrap): JSON Schema emission for pilot IPC handlers`
3. `ci(ipc): drift-check for regenerable IPC schemas`

**Files touched:**

- `crates/nexus-storage/src/{ai,editor,search,files}.rs` (or wherever the 5 handlers' arg types live) — `#[derive(TS, JsonSchema)]` + feature gate.
- `crates/nexus-storage/Cargo.toml` — `ts-rs` + `schemars` under optional `ts-export` feature.
- `packages/nexus-extension-api/src/generated/ipc/*.ts` — new, auto-generated.
- `crates/nexus-bootstrap/schemas/ipc/*.json` — new, auto-generated.
- `crates/nexus-bootstrap/tests/ipc_schema_emit.rs` — new.
- `.github/workflows/*.yml` — drift-check.

**Acceptance.**

- `cargo test -p nexus-storage --features ts-export` emits 5 .ts + 5 .json files matching HEAD.
- `cargo test -p nexus-bootstrap --test ipc_schema_emit` green.
- CI drift-check fails if a pilot type mutates without regen.
- MCP's `forge_search` tool can be verified against the emitted schema at build time (nice-to-have; defer if complex).
- Documentation in `docs/ARCHITECTURE.md` or new `docs/ipc-schemas.md` explains the generator.

**Risks.**

| Risk | Severity | Mitigation |
|---|---|---|
| ts-rs derive fails on a handler's arg type (e.g. `serde_json::Value` fields, trait objects) | Medium | Same pattern as Phase 1 WI-20: agent reports failures; fall back to hand-authored types for the outlier. |
| Scope creep — engineers try to migrate all 52+ handlers | High | Strict 5-handler pilot. List in the commit message. "v1.1 migrates the rest." |
| MCP's existing JsonSchema derives conflict with new ones | Low | MCP lives in `nexus-mcp`; new derives in service crates. Unify only if a collision surfaces. |

**Size:** L (~5 days: 2d Agent 1 + 1d Agent 2 + 2d integration/docs/CI).

---

### 3.2 WI-37 — Retire `crates/nexus-app` + `app/` (S, P0)

**Intent.** Legacy shell is frozen per ADR 0011 and Phase 1 WI-22 freeze guardrail (95 `#[tauri::command]` cap). Nothing depends on it; it's only kept around for historical reference. Phase 4's unified-binary story gets simpler once it's gone.

**Current state.**

- `Cargo.toml:20` lists `crates/nexus-app` as a workspace member.
- `Cargo.toml` also has `exclude = ["shell"]` and does NOT exclude `nexus-app`.
- **Zero reverse deps:** no crate's `Cargo.toml [dependencies]` references `nexus-app` (verified pre-Phase-1 via `dep_invariants.rs`; WI-22 freeze guardrail at `crates/nexus-bootstrap/tests/legacy_freeze.rs` caps but doesn't block deletion).
- `README.md`, `CONTRIBUTING.md`, `scripts/seed_fixtures.sh`, `scripts/phase-0-commit.ps1`, `scripts/phase-0-commit.sh`, `scripts/migrate-shell-state.ts` all reference `nexus-app` / `app/` in instructions or paths.
- `app/` frontend has its own `package.json` / `package-lock.json` using npm (not the pnpm workspace). `DEPRECATED.md` at repo root already flags it.
- Phase 2 WI-14 migration script (`scripts/migrate-shell-state.ts`) handles the legacy-to-new persistence migration; that stays (script doesn't depend on legacy code at runtime — it reads legacy files).

**Design sketch.**

Retirement in four commits so each stage is bisectable:

1. **Announce in README + DEPRECATED.** Add a banner: "As of v0.4.0, `crates/nexus-app` and `app/` are retired. Use `shell/` and `crates/nexus-cli`." Link to a `docs/legacy-shell-retirement.md` explaining the migration story for anyone forking.
2. **Update scripts.** Each of the 4 shell/PS scripts gets either (a) a rewrite using new-shell paths, or (b) deletion if the script was only ever about legacy setup. Audit each:
   - `seed_fixtures.sh` — rewrites to use `shell/` (likely keep).
   - `phase-0-commit.sh` / `.ps1` — historical artifact from Phase 0; delete.
   - `migrate-shell-state.ts` — no legacy code refs, just legacy file layout; keep.
3. **Delete `app/` directory + `crates/nexus-app/`.** Update `Cargo.toml` workspace members. Delete `crates/nexus-bootstrap/tests/legacy_freeze.rs` (no longer needed — the target crate is gone).
4. **Purge CONTRIBUTING.** Rewrite sections that reference legacy paths. Same for any `docs/*.md` that predates the split.

**Subagent pattern.**

**Agent (single, careful).** Prompt: *"For each of the 4 listed scripts, decide rewrite-or-delete; apply. Then delete `app/`, `crates/nexus-app/`, `crates/nexus-bootstrap/tests/legacy_freeze.rs`. Update `Cargo.toml` to remove `crates/nexus-app` from members. Update README.md + CONTRIBUTING.md to remove all references. Verify `cargo test --workspace` still passes."* Deliverable: single commit with the bulk-delete + reference-cleanup.

Split the single commit only if the delete + script edits would confuse bisect. A combined commit is fine here because the legacy code is provably unreferenced.

**Commit plan.** 2 commits (can be 1 if preferred):

1. `chore(legacy): announce retirement + update scripts/docs`
2. `refactor: delete crates/nexus-app + app/ + legacy freeze test`

**Files touched:**

- Deleted: `app/**` (entire), `crates/nexus-app/**` (entire), `crates/nexus-bootstrap/tests/legacy_freeze.rs`.
- Modified: `Cargo.toml`, `README.md`, `CONTRIBUTING.md`, `scripts/seed_fixtures.sh`, possibly `DEPRECATED.md` (expand), 1-2 `docs/*.md` that cite legacy paths.
- Deleted scripts: `scripts/phase-0-commit.sh`, `scripts/phase-0-commit.ps1`.
- New: `docs/legacy-shell-retirement.md` (short, 80 lines) — for forks / historical record.

**Acceptance.**

- `cargo test --workspace` green.
- `grep -r "nexus-app\|crates/nexus-app" .` returns only `docs/legacy-shell-retirement.md` and historical changelog/commit-message content.
- `cargo build --workspace` succeeds.
- `pnpm --filter nexus-shell test` still green (265).
- CI passes with the legacy freeze test removed.

**Risks.**

| Risk | Severity | Mitigation |
|---|---|---|
| A script or doc references a path we didn't catch, breaking some workflow | Low | Two-commit bisect; easy revert of step 2 if needed. |
| Someone still depends on `app/` for testing, debugging, or a dev loop | Medium | Announce in README a full release before (v0.4.0 pre-release tag). |
| Removing legacy_freeze test changes the CI matrix | Low | Already included in the delete commit; CI will run without it. |

**Size:** S (~2 days: 0.5d announce + 0.5d scripts + 0.5d delete + 0.5d verify).

---

## 4. Phase 4b — DX & parity

### 4.1 WI-38 — Unified `nexus` binary (M, P1)

**Intent.** `nexus` today is the CLI binary (`crates/nexus-cli/Cargo.toml:10`). Goal: same binary + subcommand routing covers desktop launch (`nexus desktop`), TUI (`nexus tui`), plugin ops (`nexus plugin scaffold|install|...`), and existing CLI commands (`nexus content search`, etc.).

**Current state.**

- `crates/nexus-cli/Cargo.toml:10` — `[[bin]] name = "nexus"`. CLI binary is already correctly named.
- `crates/nexus-tui/` — separate crate with its own binary. Name: likely `nexus-tui` or similar.
- `crates/nexus-app/` — legacy shell (to be deleted by WI-37).
- `shell/src-tauri/Cargo.toml` — new shell's Tauri binary. Workspace has `exclude = ["shell"]` so `cargo` doesn't build it by default. Separate build path.
- Subcommand dispatcher in `crates/nexus-cli/src/main.rs` uses `clap` (verify by reading). Adding new subcommands is mechanical.

**Design sketch.**

Three sub-parts:

**Part A — TUI subcommand.** `nexus tui` invokes `crates/nexus-tui`. Since both crates are in the workspace, option (i) make `nexus-tui` a library and call it from `nexus-cli`'s subcommand handler, or (ii) keep `nexus-tui` as a separate binary and have `nexus tui` subprocess-spawn it. **Recommend (i)** — cleaner UX (no process boundary; Ctrl+C works; shared kernel state possible later). Requires making TUI's main entry a `pub fn run_tui() -> Result<()>` callable.

**Part B — Desktop subcommand.** `nexus desktop` should launch the new shell. Problem: the shell is a Tauri app that lives outside the Cargo workspace. Options:
- (i) **Subprocess spawn the installed shell binary** — `std::process::Command::new("nexus-shell")`. Requires the shell to be on PATH (install step) or bundled alongside the `nexus` binary in the release package. Pragmatic; same model as VS Code's `code` command.
- (ii) **Fold `shell/src-tauri` into the workspace and call its entry point directly.** Breaks the `exclude = ["shell"]` guard, requires wiring Tauri to the CLI's build. Larger surgery.
- (iii) **Bundle a separate `nexus-shell` binary alongside `nexus` in the release ZIP.** Recommend this for v1 — `nexus desktop` subprocess-spawns `nexus-shell` via a lookup that tries `$NEXUS_SHELL_BIN`, then sibling-directory, then PATH.

**Recommend (iii).** Delivers the UX without invasive build changes. Ship both binaries in the release package.

**Part C — Plugin subcommands.** `nexus plugin scaffold|install|list|remove` — scaffold exists today (see WI-39). Add `install` (fetches from marketplace; Phase 5 WI-44), `list`, `remove`. Phase 4 wires the dispatcher; some subcommands may be stubs that say "requires marketplace (Phase 5 WI-44)."

**Subagent pattern.**

**Agent 1 — TUI callable-from-CLI refactor.** Prompt: *"Make `nexus-tui`'s main entry a library function callable from `nexus-cli`. Add `nexus tui` subcommand. Ensure existing standalone TUI binary still works (dual usage)."* Deliverable: diff + runs from both entry points.

**Agent 2 — Desktop launcher + plugin subcommand scaffold.** Prompt: *"Add `nexus desktop` subcommand that resolves the shell binary via env/sibling/PATH and subprocess-spawns. Add `nexus plugin install|list|remove` subcommand stubs that route to the right handler or print a 'requires v1.0' message. Preserve existing `nexus content ...` commands unchanged."* Deliverable: diff + `nexus --help` output captured.

**Commit plan.** 3 commits:

1. `refactor(tui): expose run_tui() as callable lib entry`
2. `feat(cli): add `nexus tui` + `nexus desktop` subcommands`
3. `feat(cli): plugin subcommand scaffold (install/list/remove stubs)`

**Files touched:**

- `crates/nexus-tui/src/lib.rs` (modified — export `run_tui`).
- `crates/nexus-tui/src/main.rs` (modified — thin wrapper calling `lib::run_tui`).
- `crates/nexus-cli/src/main.rs` (modified — new subcommand arms).
- `crates/nexus-cli/src/commands/{tui,desktop,plugin}.rs` (new / extended).
- `crates/nexus-cli/Cargo.toml` (new dep on `nexus-tui`).

**Acceptance.**

- `nexus --help` lists all subcommands cleanly.
- `nexus tui` launches the TUI (same UX as old standalone binary).
- `nexus desktop` launches the new shell (given `nexus-shell` is on PATH or sibling).
- `nexus plugin scaffold ...` still works (unchanged).
- `nexus plugin install|list|remove` print sensible messages.
- No regressions in existing `nexus content search` / `nexus git status` / etc.

**Risks.**

| Risk | Severity | Mitigation |
|---|---|---|
| Shell-binary resolution fails on user's machine → ugly error | Medium | Clear error: "Could not find `nexus-shell` binary. Set NEXUS_SHELL_BIN env var or install via ..." |
| TUI's global state (terminal raw mode, stdin handling) breaks when called from CLI dispatcher | Medium | Keep TUI entry self-contained with its own setup/teardown; test locally before merge. |
| Users of old `nexus-tui` binary break | Low | Keep the standalone binary as a thin `fn main() { nexus_tui::run_tui().unwrap() }`. |

**Size:** M (~3 days).

---

### 4.2 WI-39 — `plugin scaffold` script template + full-stack (M, P1)

**Intent.** `nexus plugin scaffold` today (at `crates/nexus-cli/src/commands/plugin.rs:138`, backed by `nexus_plugins::scaffold`) emits a WASM-flavored plugin: `Cargo.toml + manifest.toml + src/lib.rs`. After Phase 3c WI-30e, community plugins are JS/TS sandboxed, not WASM. Scaffold needs a matching script template.

**Current state.**

- `crates/nexus-cli/src/commands/plugin.rs:138-175` — existing scaffold entry, takes `--type core|community` (both are WASM today).
- `nexus_plugins::scaffold` (Rust library crate) — the implementation. Produces fixed 3-file output.
- Phase 3c `shell/src/plugins/community/hello-world/` shape: `plugin.json + index.ts` (+ `index.js` hand-rolled bundle).
- `@nexus/extension-api` exports `bootstrapSandboxedPlugin` + `SandboxedPlugin` type for the ts-side.

**Design sketch.**

Add two new templates:

**`script`** — the sandboxed-community pattern from WI-30e. Emits:
- `plugin.json` — manifest with `sandboxed: true`, `apiVersion: 1`, `capabilities: []`, placeholder description/version.
- `index.ts` — idiomatic `SandboxedPlugin` skeleton importing `@nexus/extension-api`. Registers one command and one `registerPanel` view.
- `README.md` — minimum-viable authoring guide pointing at `@nexus/extension-api` docs.
- `package.json` — dev-deps on `@nexus/extension-api` + `typescript` + `esbuild` (for compile/bundle).
- `tsconfig.json` — targets ES2020, references the extension-api types.

**`full-stack`** (stretch) — a script plugin that also has a kernel-side WASM companion. Produces the `script` output plus a `kernel/` subdirectory with a minimal WASM plugin using the existing Rust scaffold.

Decision for v1: **ship `script` only**. `full-stack` defers until there's user demand; the existing WASM scaffold still works standalone for anyone building a pure-Rust plugin.

**CLI change:**

```
nexus plugin scaffold --template <script|core|community> --id my.plugin --name "My Plugin"
```

Default `--template` becomes `script` (modern path). `core`/`community` still work (backward compat).

**Subagent pattern.**

**Single agent.** Prompt: *"Extend `nexus_plugins::scaffold` to emit the `script` template: plugin.json, index.ts, README.md, package.json, tsconfig.json. Mirror the Phase 3c hello-world shape. Add `PluginTemplate::Script` variant. Update the CLI `scaffold` command to route based on `--template`. Add a unit test that scaffolds into a temp dir and asserts the file tree. Keep existing `core`/`community` working."*

Single-agent because it's all in `nexus-plugins` + `nexus-cli` — no cross-cutting concerns.

**Commit plan.** 2 commits:

1. `feat(plugins): script-template scaffold for sandboxed community plugins`
2. `feat(cli): --template script in plugin scaffold + docs`

**Files touched:**

- `crates/nexus-plugins/src/scaffold.rs` (or similar) — new template enum variant + template files.
- `crates/nexus-plugins/templates/script/` (new dir) — embedded templates.
- `crates/nexus-cli/src/commands/plugin.rs` — route `--template script`.
- `crates/nexus-cli/src/args.rs` (if it has one) — extended enum.
- `docs/writing-your-first-plugin.md` (new, stretch) — tutorial.
- Tests.

**Acceptance.**

- `nexus plugin scaffold --template script --id com.example.hello` produces a ~6-file project.
- Running `pnpm install && pnpm build` in that project produces a loadable bundle.
- Dropping the built bundle into `~/.nexus-shell/plugins/com.example.hello/` causes the shell to pick it up (manual smoke).
- Existing `core`/`community` templates still work.

**Risks.**

| Risk | Severity | Mitigation |
|---|---|---|
| Scaffolded template gets stale as `@nexus/extension-api` evolves | Medium | Templates use `workspace:*` or pin to a concrete version; CI check that the scaffold produces something that typechecks. |
| Users confused by 3 templates (script/core/community) | Low | Docs + `--template` help text. Default to `script` so most users don't choose. |
| Building the scaffolded project fails because of missing node_modules | Low | `README.md` in scaffold tells user to `pnpm install`. |

**Size:** M (~3 days).

---

### 4.3 WI-40 — MCP parity check (XS, P2)

**Intent.** Every MCP tool should have a corresponding CLI subcommand + shell command, so the user never hits a feature that only one frontend exposes.

**Current state (audit-verified).**

13 MCP tools at `crates/nexus-mcp/src/server.rs`:

| # | MCP tool | Kernel IPC | CLI subcommand | Shell command |
|---|---|---|---|---|
| 1 | `nexus_read_note` (:322) | `storage::read_file` | `nexus content read` ✓ | `api.kernel.invoke` ✓ |
| 2 | `nexus_create_note` (:349) | `storage::write_file` | `nexus content create` ✓ | editor save ✓ |
| 3 | `nexus_update_note` (:357) | `storage::write_file` | `nexus content update` ✓ | editor save ✓ |
| 4 | `nexus_delete_note` (:365) | `storage::delete_file` | `nexus content delete` ✓ | context menu ✓ |
| 5 | `nexus_list_notes` (:385) | `storage::query_files` | `nexus content list` ✓ | file explorer ✓ |
| 6 | `nexus_search` (:429) | `storage::search` | `nexus content search` ✓ | search plugin ✓ |
| 7 | `nexus_backlinks` (:479) | `storage::backlinks` | `nexus graph backlinks` ✓ | backlinks plugin ✓ |
| 8 | `nexus_outgoing_links` (:516) | `storage::outgoing_links` | `nexus graph outgoing` ✓ | outgoing plugin ✓ |
| 9 | `nexus_graph_status` (:560) | `storage::graph_stats` | `nexus graph status` ✓ | graph plugin ✓ |
| 10 | `nexus_list_tags` (:591) | `storage::query_tags` | `nexus tags list` ✓ | tags plugin ✓ |
| 11 | `nexus_list_tasks` (:628) | `storage::query_tasks` | `nexus tasks list` ✓ | tasks view ✓ |
| 12 | `nexus_toggle_task` (:674) | `storage::toggle_task` | `nexus tasks toggle` ✓ | editor checkbox ✓ |
| 13 | `nexus_ask` (:706) | `ai::ask` | `nexus ai ask` ✓ | AI plugin ✓ |

**Gaps:** 3 CLI subcommands missing (`update-note`, `list-notes`, `list-tags`). Shell coverage is 13/13. No backward-direction gaps (no shell/CLI feature lacks an MCP tool).

**Status:** Closed — all three subcommands landed in WI-40; CLI coverage is now 13/13.

**Design sketch.**

Trivial: add 3 CLI subcommands wrapping the existing IPC handlers. Each is ~20-30 LOC in `crates/nexus-cli/src/commands/`.

**Subagent pattern.**

**Single agent.** Prompt: *"Add 3 CLI subcommands: `nexus content update <path> [--content FILE|--stdin]`, `nexus content list [--prefix PATH]`, `nexus tags list [--name TAG]`. Each calls the corresponding kernel IPC. Mirror the existing `nexus content {read, create, delete}` style. Unit tests for arg parsing + mock-kernel smoke for each."*

**Commit plan.** 1 commit: `feat(cli): add missing MCP-parity subcommands (update-note, list-notes, list-tags)`.

**Files touched:**

- `crates/nexus-cli/src/commands/{content,tags}.rs` (modified/new).
- `crates/nexus-cli/src/main.rs` — register new subcommands.
- Tests.

**Acceptance.**

- `nexus content update foo.md --content -` (reads stdin, writes file).
- `nexus content list --prefix notes/` lists matching paths.
- `nexus tags list --name project` lists occurrences.
- MCP parity matrix (above) flips to 13/13 CLI.

**Risks.** Minimal — mechanical CLI glue. Mitigated by the existing `content {read, create, delete}` precedent.

**Size:** XS (~1 day).

---

## 5. Dependency graph & parallelization

```
              ┌──────────────────────────────────────┐
              │ WI-36 (shared taxonomy)             │  L, no deps
              └──────────────────────────────────────┘
                          │ (informative; doesn't block)
                          ▼
              ┌──────────────────────────────────────┐
              │ WI-37 (retire nexus-app)            │  S, no deps
              └──────────────────────────────────────┘
                          │
                          ▼  (cleaner after legacy gone)
              ┌──────────────────────────────────────┐
              │ WI-38 (unified nexus binary)        │  M, soft dep on WI-37
              └──────────────────────────────────────┘
                          │
                          ▼
              ┌──────────────────────────────────────┐
              │ WI-39 (plugin scaffold script)      │  M, depends on
              │                                     │  Phase 3c WI-30e (shipped)
              └──────────────────────────────────────┘
                          │
                          ▼  (independent)
              ┌──────────────────────────────────────┐
              │ WI-40 (MCP parity 3 gaps)           │  XS, no deps
              └──────────────────────────────────────┘
```

### 5.1 Single-engineer serialization (~2 weeks)

- Days 1-5: WI-36 pilot (5 handlers + schemas + CI drift).
- Days 6-7: WI-37 retire legacy.
- Days 8-10: WI-38 unified binary.
- Days 11-13: WI-39 scaffold.
- Day 14: WI-40 parity gap-fill.

### 5.2 Two-engineer parallelization (~1.5 weeks)

- Engineer A: WI-36 (taxonomy — biggest item) → WI-40 (parity).
- Engineer B: WI-37 (retire) → WI-38 (binary) → WI-39 (scaffold).

### 5.3 Agent-heavy run (~1 week + review)

WI-36 pilot is the only item needing serious design; agents handle it cleanly via the pattern pioneered in Phase 1 WI-20. WI-37 is mechanical deletion; one agent handles. WI-38 splits into 2-3 agents. WI-39 is a single agent. WI-40 is a single agent. Total: ~7 agent tasks + main-thread review & commit.

---

## 6. Risks & mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| WI-36 pilot handler choice is wrong and picks easy ones → shipping shape doesn't cover the hard cases | Medium | Pilot explicitly includes `write_file` (security-sensitive) + `stream_ask` (streaming) — two of the hardest shapes. If they work the rest are cake. |
| WI-37 deletion breaks a dev-loop someone was using | Medium | Announce in v0.4.0-pre; give forks a month to rebase. Keep a historical tag. |
| WI-38 `nexus-shell` binary resolution breaks for users who built from source (no installer) | High | Document `$NEXUS_SHELL_BIN` env var. Provide a `justfile`/`make` target that symlinks the built Tauri binary into PATH for dev. |
| WI-39 scaffold produces code that drifts from the shipping `@nexus/extension-api` version | Medium | Templates pin to a concrete version; CI includes a scaffold-and-typecheck smoke. |
| MCP tool shape changes post-Phase 4 (Phase 5 marketplace or docs work adds a new MCP tool) → parity gap re-opens | Low | `WI-40` matrix lives in `docs/PHASE-4-IMPLEMENTATION-PLAN.md` + is auto-regenerable from the MCP source; re-run in Phase 5 CI. |
| WI-38 TUI-as-library refactor accidentally breaks standalone TUI binary users | Low | Keep the standalone binary with a thin `fn main() { lib::run_tui().unwrap() }` wrapper. |

---

## 7. Open questions for user before execution

Defaults are documented so execution isn't blocked, but these material choices shape the plan:

1. **WI-37 retirement cadence.** Delete now (in Phase 4) as a clean break, or announce in Phase 4 / delete in Phase 5 (v1.0 release)? Recommendation: **delete in Phase 4**. The code is already frozen and unreferenced; a two-phase delete adds ceremony without value.

2. **WI-38 desktop launcher strategy.** (i) subprocess-spawn the installed `nexus-shell` binary, (ii) fold `shell/src-tauri` into the Cargo workspace, or (iii) bundle both binaries side-by-side in releases. Recommendation: **(iii) bundle side-by-side** for v1, with env-var / sibling-path / PATH resolution. Future v2 can fold the workspace if it becomes valuable.

3. **WI-38 TUI dispatch.** (i) expose `nexus-tui` as a library and call from `nexus-cli`, or (ii) subprocess-spawn. Recommendation: **(i) library**. Cleaner UX; Ctrl+C Just Works; no process boundary for kernel state sharing later.

4. **WI-39 template scope.** Ship `script` only, or `script` + `full-stack` (script + WASM companion)? Recommendation: **script only**. `full-stack` defers until a real community plugin asks for it; WASM scaffold already exists for pure-Rust authors.

5. **WI-36 pilot set.** The proposed 5 handlers (search, read_file, write_file, list_dir, stream_ask) span storage + AI + streaming + security surface. Alternatives: (a) pick 10 handlers for broader pattern validation, (b) pick 3 and lock faster. Recommendation: **5 as proposed** — covers the hard shapes while bounded.

6. **WI-36 drift-check strictness.** CI-fail on any TS/JSON-Schema drift (strict), or warn-only initially (loose)? Recommendation: **strict from day one**. Warn-only drift gates rot.

Flag these during Phase 4a kickoff so they're settled before code lands.

---

## 8. What this plan does NOT cover

- **Phase 5 v1 polish.** Separate plan: [`PHASE-5-IMPLEMENTATION-PLAN.md`](./PHASE-5-IMPLEMENTATION-PLAN.md). Auto-update, crash reporting, bundled plugin set, marketplace, docs pass, beta → GA.
- **Phase 6 post-v1.** Not yet scoped.
- **Migrating all 52+ kernel handlers to ts-rs+JsonSchema.** Pilot 5 in WI-36; the rest migrate opportunistically in v1.1.
- **Rewriting `app/`.** Frozen per ADR 0011; retired in WI-37.
- **Plugin marketplace UX.** That's Phase 5 WI-44.
- **Writing a full "first plugin" tutorial.** Phase 4 ships a scaffold; the tutorial is Phase 5 WI-45 docs.
- **MCP schema emission.** Phase 5 WI-41 (auto-update) is orthogonal; MCP schema gen could land in this phase if cheap, but is not scope today.

---

## 9. Next action

Review this plan. If approved, the execution order is:

1. Settle the §7 open questions (6 yes/no-ish calls; defaults documented).
2. Start WI-37 — quickest win, unblocks WI-38's cleaner story.
3. WI-36 pilot — parallel with WI-37 (different surfaces).
4. WI-38 after WI-37 merges.
5. WI-39 after WI-38 merges (scaffold touches CLI main.rs).
6. WI-40 — anytime; 1-day drop-in.
7. Phase 4 acceptance smoke: run `cargo test --workspace`, `pnpm --filter nexus-shell test`, all 5 pilot IPC schemas emitted, `nexus plugin scaffold --template script` produces a loadable plugin end-to-end.

Each WI has its own commit plan in §3–§4; land them incrementally per the Phase 1/2/3 workflow.
