# nexus-workflow

> Kind: lib · IPC plugin id: com.nexus.workflow · CorePlugin: yes · Has settings: yes (`[digests]` + `[webhooks]` in `<forge>/.forge/config.toml`; `.workflow.toml` files under `<forge>/.workflows/`) · As of: 2026-05-25

## Overview

`nexus-workflow` is the declarative automation subsystem from PRD-16. A workflow is a `.workflow.toml` file with a **trigger** (what fires it), an optional **condition** (a gate evaluated before any step runs), and an ordered list of **steps** (actions). The crate began life as a pure-logic scaffold — typed model, TOML parser, directory-walk registry, with no kernel dependency — and has since grown a full runtime: a trigger engine, a condition evaluator, a step executor with retries and parallel groups, a cron scheduler, scheduled AI digests, a webhook listener, and a built-in template library. The library half (model, parser, registry, cron, condition, interpolation, executor) stays kernel-free and unit-testable; the kernel-aware glue lives in `core_plugin.rs` and `handlers/`.

The trigger/condition/action model is type-dispatched, not a monolithic enum. `Trigger`, `Condition`, and `Step` each carry a `type` discriminator plus a `#[serde(flatten)] extra: BTreeMap<String, toml::Value>` bag, so a runtime dispatches off the string `type` and each variant pulls its own fields out of `extra`. This keeps the file schema open: community-plugin trigger kinds and unknown step types round-trip through the parser without rejection (unknown step types are executed as informational no-ops). The trade-off, noted in `Cargo.toml`, is that the flattened bag is incompatible with `deny_unknown_fields`, so the schema/TS export feature only covers the IPC *Args* types.

Actions are executed via `ipc_call`, never via direct dependencies. The executor walks `[[steps]]` through an injected `ActionDispatcher`; the production `KernelActionDispatcher` routes known step types (`ipc`/`ipc_call`, `ai_prompt`, `ai_decision`, `terminal`, `notify`, `noop`) through `context.ipc_call(...)` into `com.nexus.storage` / `com.nexus.ai` / `com.nexus.ai.runtime` / `com.nexus.terminal` / `com.nexus.notifications`. The digest pipeline reaches storage + AI the same way. The crate touches no filesystem or AI provider directly except for its own forge-local bookkeeping (the run-history JSON, the digest last-fired watermark, and `templates_init` writes into `.workflows/`).

This is a clean microkernel fit: `nexus-kernel` does not depend on workflow; the workflow plugin depends on the kernel and reaches every other subsystem only through IPC. A documented consequence (issue #77, the `implied_caps` module) is *capability laundering*: each step is gated by the workflow plugin's own minimal cap set (`IpcCall`, `AiChat`, `AiRuntimeSubmit`), not the caller of `run`, so a caller without `ai.chat` can drive an `ai_prompt` step. Until kernel-level enforcement lands, `run` and `run_digest` emit an `audit = true` warn listing the implied caps.

## Position in the dependency graph

- **Direct nexus-\* deps:** `nexus-kernel` (IPC, events, capability context, `KernelPluginContext`), `nexus-plugins` (`CorePlugin`, `CorePluginFuture`, `PluginError`, `define_dispatch_helpers!`), `nexus-types` (BL-052 activity-timeline emitter types; `constants::IPC_TIMEOUT_LONG`).
- **Notable external deps:** `tokio` (scheduler tasks, async dispatch, webhook TCP listener), `futures` 0.3 (`join_all` for parallel step groups), `regex-lite` (file_event / git_event / condition patterns), `toml` (parse + the `extra` value bag), `chrono` (cron calendar math, digest windows), `serde`/`serde_json` (IPC + persistence), `thiserror`, `tracing`, `fastrand` 2 (retry jitter), `async-trait`. Optional `ts-rs` + `schemars` behind the `ts-export` feature (off by default; Args types only).
- **Crates depending on it:** `nexus-bootstrap` registers `WorkflowCorePlugin` as a core plugin (`build_*_runtime`), supplies the `DigestConfig` / `WebhookConfig` from forge config, and owns the cap policy (`workflow_capabilities`). Frontends (`nexus-cli`, `nexus-tui`, `nexus-mcp`, the shell) reach it only through `ipc_call`.

## Public API surface

Module-by-module (re-exported from `lib.rs`):

- **`lib.rs`** — top-level model types: `Workflow` (root: `workflow`, `inputs`, `trigger`, `condition`, `steps`, `outputs`, `error_handling`, `extra`), `WorkflowMeta`, `Input`, `Trigger`, `Condition`, `Step`, `ErrorHandling`. `Trigger`/`Condition`/`Step` are type-dispatched with a flattened `extra` bag.
- **`parse`** — `parse_workflow_text` / `parse_workflow_file`; `WorkflowParseError` (`Io`, `Toml`, `MissingField`, `InvalidTrigger`). Structural validation (non-empty name / trigger type / step types) plus type-specific trigger validation.
- **`registry`** — `WorkflowRegistry` (in-memory `BTreeMap` keyed by `workflow.name`, recursive `.workflow.toml` directory walk), `WorkflowRegistryError` (`Io`, `PartialParseFailure`). `load` / `empty` / `get` / `iter` / `len` / `source`.
- **`trigger_validation`** — `validate_trigger`: validates `cron` (parse), `webhook` (full spec), `file_event` (regex + event names) at parse time; unknown/`manual` types pass through.
- **`cron`** — `CronSchedule` (5-field `min hour dom month dow`, `*`, lists, ranges, `*/step`; POSIX dom-OR-dow semantics; `0=Sunday`), `next_after`, `next_fire_after`, `CronParseError`. Minute-granularity; no name abbreviations.
- **`condition`** — `evaluate_condition`, `EvaluationContext` (`forge_root`, `variables`), `ConditionError`. Terminal types `always`/`never`/`equals`/`regex_match`/`file_exists`; combinators `and`/`or`/`not`.
- **`interpolate`** — `VariableMap` (`BTreeMap<String, toml::Value>` keyed by dotted paths), `substitute` / `substitute_string` / `interpolate_step`. `${path.to.value}` expansion in string leaves; `$$` escape; unknown vars pass through verbatim.
- **`executor`** — `run_workflow` / `run_workflow_with_variables`, `ActionDispatcher` trait, `WorkflowRun`, `StepOutcome`, `StepOutcomeStatus` (`Ok`/`Failed`/`Skipped`), `WorkflowExecutionError::EmptyPlan`, `condition_skipped_run`. Sequential + maximal-contiguous parallel groups; per-step retry/backoff.
- **`ai_steps`** — `AiPromptArgs`, `AiDecisionArgs`, `build_decision_prompt`, `pick_choice`. Kernel-free parsing/matching for `ai_prompt`/`ai_decision`.
- **`digests`** — `DigestConfig`, `DigestKind` (`Daily`/`Weekly`), `run_digest`, `digest_window`, `output_path`, `build_digest_prompt`, `next_fire`, `LastFired` watermark helpers, default-cron constants.
- **`webhook`** — `WebhookConfig`, `WebhookSpec`, hand-rolled HTTP/1.1 `parse_request` / `route_request` / `build_trigger_vars`, size/timeout constants, `Route`, `RequestError`.
- **`run_history`** — `RunHistoryStore` (JSON ring at `<workflows_dir>/run_history.json`, capped at `RUN_HISTORY_CAP = 200`, newest-first, best-effort), `RunHistoryEntry`, `RUN_HISTORY_CAP`.
- **`templates`** — `Template`, `CATALOG` (5 embedded `include_str!` templates), `find`, `parse`.
- **`implied_caps`** — `compute_implied_caps`, `ImpliedCaps`, `validate_declared_caps` (issue #77 laundering surface).
- **`core_plugin`** — `WorkflowCorePlugin`, `PLUGIN_ID`, `HANDLER_*` ids, `IPC_HANDLERS`, `MANIFEST_DEPS`, and the IPC Args structs.

## IPC handlers

Twelve handlers; ids are append-only (note the ordering: ids 1–5 then 11/6/7/8/9/10/12). `run`, `run_digest`, `set_digest_config`, and `validate` dispatch async; the rest are sync. No handler declares an explicit capability gate (the workflow plugin's own cap set gates the downstream IPC).

| Command | id | Args | Returns | Capability | Description |
|---------|----|------|---------|-----------|-------------|
| `list` | 1 | `{}` | `[Workflow, …]` (JSON array of full parsed workflows) | — | Every loaded workflow, sorted by name. |
| `get` | 2 | `{ name }` (`GetWorkflowArgs`) | `Workflow` JSON | — | One workflow by exact name; errors `no workflow named '<name>'` if absent. |
| `reload` | 3 | `{}` | `{ loaded: <count> }` | — | Re-scan `<forge>/.workflows/` and replace the registry. |
| `validate` | 4 | `{ text }` (`ValidateWorkflowArgs`) | validated `Workflow` JSON | — | Parse a `.workflow.toml` string; async path also cross-checks `terminal`-step slugs against `com.nexus.terminal::saved_list` when a context is wired. |
| `run` | 5 | `{ name, variables? }` (`RunWorkflowArgs`) | `WorkflowRun` JSON | — | Async. Evaluate gate condition, run steps via `KernelActionDispatcher`, persist a run-history row, emit activity start/end. `variables` is a nested object flattened to dotted keys. **AUDIT** (issue #77). |
| `run_digest` | 6 | `{ kind: "daily"\|"weekly" }` | `DigestRunReport` JSON | — | Async. Run a digest end-to-end via storage + AI IPC. **AUDIT** (implied `ai.chat`). |
| `set_digest_config` | 7 | `DigestConfig` JSON | `{ applied: true }` | — | Async. Replace the live `DigestConfig` under the shared lock; scheduler picks it up within ~60 s. |
| `templates_list` | 8 | `{}` | `[{ slug, description, tags, filename }, …]` | — | Enumerate the 5 built-in templates. |
| `templates_get` | 9 | `{ slug }` (`GetTemplateArgs`) | `{ slug, description, tags, filename, body }` | — | Fetch one template's TOML body; errors on unknown slug. |
| `templates_init` | 10 | `{ slug, filename?, overwrite? }` (`InitTemplateArgs`) | `{ written: true, path, slug }` | — (downstream `fs.write`) | Write a template into `.workflows/`. Refuses to clobber unless `overwrite=true`; sanitizes filename (bare basename only, rejects path separators / `..`). |
| `run_history` | 11 | `{ name?, limit? }` (`RunHistoryArgs`) | `[RunHistoryEntry, …]` | — | List persisted run-history rows (newest-first), optionally filtered by name and capped. |
| `next_fire` | 12 | `{ name? }` (`NextFireArgs`) | `[{ name, expression, next_fire_at: RFC3339\|null }, …]` | — | Next scheduled fire for cron workflows; unparseable schedules yield `next_fire_at: null`. Non-cron triggers are skipped. |

`IPC_HANDLERS` (in `core_plugin.rs`) is the single source of truth for `(name, id)` pairs consumed by `nexus_bootstrap::plugins::workflow::register`. Cross-check with `docs/0.1.2/ipc-handlers.md` (which lists 12 and matches).

## Capabilities

No handler in this crate calls `require_capability` itself — gating happens downstream. The workflow plugin's `KernelPluginContext` is constructed by bootstrap (`workflow_capabilities`) with a minimal set: `IpcCall`, `AiChat`, `AiRuntimeSubmit`. Each step's IPC dispatch is checked against *those* caps. `MANIFEST_DEPS` is `["com.nexus.storage", "com.nexus.ai", "com.nexus.ai.runtime"]` — only plugins that load before workflow; `com.nexus.terminal` and `com.nexus.notifications` are deliberately omitted (they load after workflow but are only reached at step-execution time, long after the loader's `check_dependencies` ran at boot).

Implication of actions calling other plugins: capability laundering (issue #77). The `implied_caps` module computes the cap set the caller of `run` *would* need under caller-cap parity — `ai_prompt`/`ai_decision` ⇒ `ai.chat`, `terminal` ⇒ `process.spawn`, `ipc`/`ipc_call` ⇒ free-form target (surfaced but not statically resolvable), `notify`/`noop`/unknown ⇒ none. `run` and `run_digest` log this set as an `audit = true` warn at run entry. `validate_declared_caps` is shipped as a building block for the eventual `[workflow].required_caps` schema addition; kernel-side enforcement (forge-root-aware cap policy) is the residual gap.

## Settings / Config

Two forge-config blocks plus the `.workflow.toml` files themselves.

**`[digests]` in `<forge>/.forge/config.toml`** → `DigestConfig` (bootstrap loads it; defaults are "off but pre-configured"):
- `enabled` (default `false`) — master switch for the cron loop; manual `run_digest` always works.
- `daily_cron` (default `"0 9 * * *"`, `None` disables) — daily-digest cron.
- `weekly_cron` (default `"0 9 * * 1"`, `None` disables) — weekly-digest cron.
- `scope_path` (default `None` = whole forge) — forge-relative subtree to scan.
- `digests_dir` (default `"Digests"`) — output directory; daily → `Daily-YYYY-MM-DD.md`, weekly → `Weekly-YYYY-Www.md` (ISO week).

**`[webhooks]` in `<forge>/.forge/config.toml`** → `WebhookConfig`:
- `enabled` (default `false`) — explicit opt-in.
- `bind` (default `"127.0.0.1:18080"`) — bind address; non-loopback binds emit an `audit = true` warn at arm time.

**`.workflow.toml` schema** (under `<forge>/.workflows/`, recursed; extension filter strictly `.workflow.toml`):
- `[workflow]` — `name` (required, non-empty), `description`, `version`, `author`, `tags`.
- `[inputs]` — per-input `{ type (default "string"), default, description, … }`.
- `[trigger]` — `type` (required) selects the kind; remaining keys in `extra`:
  - `cron` — `schedule` (5-field), `timezone` (parked, unused). Validated at parse time.
  - `file_event` — `watch_dir` (prefix), `pattern` (regex), `events` (subset of `created`/`modified`/`deleted`; omit = all). Fires with `trigger.{path,event_type,content_hash}`.
  - `git_event` — `events` (`state`/`commit`/`branch_changed`/`dirty_changed`; default omits `state`), `branch`, `branch_pattern`. Fires with `trigger.{event_type,branch,head,…}`.
  - `mcp_event` — `events` (`host_started`; excluded by default). Fires with `trigger.{event_type,…payload}`.
  - `webhook` — `path` (must start with `/`, no `?`/`#`), `method` (POST only in v1), `secret` (optional `X-Webhook-Secret` header). Fires with `trigger.{path,method,body,remote_addr}`.
  - `manual` — no engine; driven by `run`. Unknown types accepted (community plugins).
- `[condition]` (optional) — `type` ∈ `always`/`never`/`equals`/`regex_match`/`file_exists`/`and`/`or`/`not`; combinators take `conditions = [...]` (and/or) or `condition = {...}` (not). All string fields interpolate.
- `[[steps]]` — `name?`, `type` (required), `parallel` (default false), `async` (default false; `ai_prompt`/`ai_decision`/`notify` only), `on_error` (`stop` default / `continue` / `log_warn` / `branch_to_recovery`), per-step retry knobs (`max_retries` 0, `retry_backoff` "exponential", `retry_initial_delay_ms` 100, `retry_max_delay_ms` 30000, `retry_jitter` true), plus action-specific fields in `extra`. Action types: `ipc`/`ipc_call`, `noop`, `ai_prompt`, `ai_decision`, `terminal`, `notify` (others no-op).
- `[outputs]` — opaque map (parked).
- `[error_handling]` — `max_retries`, `retry_backoff`, `on_step_failure`, `recovery_step`. Shadowed by per-step settings.

## Events

- **Published:** BL-052 activity-timeline entries via `ctx.publish(ACTIVITY_APPENDED_TOPIC, …)` (`nexus_types::activity`) — a `started <name>` (Ok) entry before steps dispatch and a `completed <name>` (Ok) / `failed <name>` (Error) entry after, with `ActivitySurface::Workflow` / `ActivityOrigin::Workflow`.
- **Subscribed (trigger engines, spawned in `wire_context`):**
  - `EventFilter::CustomPrefix("com.nexus.storage.file_")` — file_event triggers (`file_created`/`modified`/`deleted`).
  - `EventFilter::CustomPrefix("com.nexus.git.")` — git_event triggers (`state`/`commit`/`branch_changed`/`dirty_changed`).
  - `EventFilter::CustomPrefix("com.nexus.mcp.")` — mcp_event triggers (`com.nexus.mcp.host.started`).
- Cron, digest, and webhook triggers do not subscribe to the bus; they run timer loops / a TCP accept loop. When an event/trigger matches, the engine dispatches `com.nexus.workflow::run` (self-IPC) so all run paths share history persistence + activity emission.

## Internals & notable implementation details

- **Parsing** (`parse.rs`): `toml::from_str` → structural `validate` (non-empty name / trigger type / each step type) → `trigger_validation::validate_trigger` (cron parse, webhook spec, file_event regex/events). The flattened `extra` bags mean unknown fields and unknown trigger/step types survive a round trip.
- **Registry** (`registry.rs`): recursive directory walk; missing root = empty; `PartialParseFailure` carries good entries inserted before the error returns. `open_full` re-loads on partial failure to recover the known-good subset, else starts empty.
- **Trigger registration** (`core_plugin.rs` `wire_context`): spawns one tokio task per cron / file_event / git_event / mcp_event workflow, plus a webhook accept loop (when enabled and at least one webhook workflow exists) and one digest scheduler. All `JoinHandle`s are held in `scheduler_handles` and aborted on `Drop`. Spec parse failures log-and-skip per workflow. The cron `scheduler_loop` sleeps to `next_after`, dispatches `run`, loops; an impossible schedule parks for a year. The webhook loop scopes per-connection handlers in a `JoinSet` so plugin drop aborts in-flight requests; bind defaults to loopback with a non-loopback warn.
- **Condition evaluation** (`condition.rs`): recursive over `and`/`or`/`not`; short-circuits; empty `and`=true, empty `or`=false. Operands (`left`/`right`/`source`/`pattern`/`path`) interpolate against the same `VariableMap` as steps. `file_exists` resolves relative paths against `forge_root`. A `ConditionError` is a run-level failure (gate stays closed). The `run` handler persists condition-skipped runs to history too (success=true, condition_skipped=true).
- **Action execution** (`handlers/run.rs`): `KernelActionDispatcher::run` matches `step.step_type` and routes through `ipc_call` (60 s `DEFAULT_STEP_TIMEOUT`). `terminal` maps `start`/`stop`/`restart`/`run_adhoc` onto `com.nexus.terminal::run_saved` / `list_sessions` / `close_session` (stop closes sessions named `saved:<slug>`). `ai_decision` post-processes the answer with `pick_choice` (exact-then-longest-substring) and errors on no match. `notify` routes to `com.nexus.notifications::send` with source-tag default `"workflow"`. `async = true` steps instead submit a `workflow_ai_step` envelope to `com.nexus.ai.runtime::submit` (background priority) and record the returned `task_id` as the step output. Unknown step types log a warn and return `{ unsupported: true, … }`.
- **Run lifecycle** (`executor.rs`): walks steps; a maximal contiguous run of `parallel = true` steps becomes a group run via `futures::join_all` (branches retry independently; outcomes recorded in source order; in-flight siblings are not cancelled on a failure, but the post-group sequential step is skipped). Per-step retry resolves step → `[error_handling]` → defaults, with constant/linear/exponential backoff, cap, and optional full jitter (`fastrand`). `WorkflowRun.success` tracks correctness (any `Failed` ⇒ false); empty plan ⇒ `EmptyPlan` error.
- **Cron scheduler** (`cron.rs`): pure `BTreeSet<u32>` per-field matcher; chrono for calendar math; minute granularity; POSIX dom-OR-dow when both restricted; returns `None` after scanning ~366 days for intrinsically impossible expressions.
- **Run history** (`run_history.rs`): JSON ring `<workflows_dir>/run_history.json`, capped at 200 newest-first, best-effort (corrupt/missing file → empty, write errors logged not propagated, never clobbers a corrupt file until first append).
- **Digests** (`digests.rs`): scheduler wakes ≤60 s, computes `next_fire` across daily/weekly, fires `run_digest`. FU-6 last-fired watermark (`<forge>/.forge/digests/last_fired.json`, 30 s suppression window) defends against backwards clock jumps. The digest pipeline walks markdown via `storage::list_dir`/`read_file`, filters by mtime window, asks `ai::ask`, and writes via `storage::write_file` with YAML frontmatter; short-circuits (no AI call) when no sources fall in the window.

## Tests

No `tests/` integration directory; coverage is in-module `#[cfg(test)]` across the crate:

- `parse.rs` — minimal/cron/inputs/condition/retry/error_handling round trips; empty-name and missing-trigger-type rejections; `extra` capture.
- `cron.rs` — daily/step/range/comma/weekday parsing, POSIX dom-OR-dow, impossible-expression `None`, field-count and range errors.
- `condition.rs` — always/never, unknown type, equals/regex/file_exists with interpolation, and/or short-circuit + identities, not.
- `interpolate.rs` — single/multiple vars, unknown passthrough, `$$` escape, numeric/bool stringify, unterminated/invalid names, recursion into arrays/tables, `interpolate_step`.
- `executor.rs` — sequential success/failure/skip, `on_error = continue`, empty plan, variable interpolation, retry success/exhaustion/zero, constant + exponential-cap backoff (paused-clock), workflow-level error_handling fallback, parallel concurrency/source-order/independent-retry/abort/mixed-group walking.
- `core_plugin.rs` — `list`/`get`/`reload`/`validate`/`next_fire`/`templates_*` through `dispatch`; FileEventSpec / GitEventSpec / McpEventSpec parsing + defaults + rejections; event-type id mappings.
- `handlers/run.rs` — terminal-step + notify-step parser tables, async-submit envelope, variable flattening.
- `handlers/validate.rs` — async validate fallback when ctx absent / skips IPC without terminal steps / propagates parse errors.
- `ai_steps.rs` — prompt/decision arg parsing + rejections, decision-prompt build, `pick_choice` strategies.
- `digests.rs` — suppression window (forward/backward/other-kind), watermark round trip, window/output-path/ISO-week, prompt build, `next_fire` earliest-pick, and two `run_digest` integration tests against a stub `IpcDispatcher`.
- `implied_caps.rs` — per-step-type cap mapping, dedupe/sort, ipc-target recording, declared-cap validation.
- `run_history.rs` — open-missing, append/readback, name filter, limit, cap drop-oldest, corrupt-file-no-clobber.
- `templates.rs` — catalog size, kebab-case unique slugs, embedded-body parse/validate (not all shown).
- `webhook.rs` — HTTP parsing/routing tests (in-module; not all shown).
