# RFC 0001 — Workflow cap delegation (issue #77, P1-08)

- **Status:** Draft
- **Owner:** unassigned
- **Created:** 2026-05-18
- **Tracks:** issue #77, `docs/0.1.2/implementation-plan.md` row **P1-08**
- **Touches:** `crates/nexus-workflow/src/handlers/run.rs`, `crates/nexus-workflow/src/handlers/digest.rs`, `crates/nexus-bootstrap/src/lib.rs` (`workflow_capabilities`), `crates/nexus-bootstrap/cap_matrix.toml`, `crates/nexus-kernel/src/context_impl.rs`

---

## Summary

`com.nexus.workflow::run` and `::run_digest` execute user-authored TOML
files (`<forge>/.workflows/*.workflow.toml`) by walking their `[[steps]]`
list and dispatching each step through `KernelPluginContext::ipc_call`.
The caller-cap check fires against the **workflow plugin's** context
(`IpcCall + AiChat`, `TrustLevel::Core`), not the principal who triggered
the run. Any actor with `fs.write` to the forge root can drop a workflow
file whose steps reach handlers gated only by caps the workflow plugin
already holds — including `internal = true` handlers like
`com.nexus.ai::resolve_credentials` (P1-02), which the Core-trust check
admits.

This RFC enumerates the laundering surface, evaluates five mitigations,
and recommends a layered approach: a **step-target denylist** for
internal-only handlers (small, ships immediately) plus a per-file
**`requires_caps` declaration** that bounds what each workflow can
dispatch (medium, ships once authoring tooling catches up). Carrying a
real principal through `ipc_call` is identified as the long-term answer
but is out of scope for 0.1.x.

## Background

### Current execution path

`run::prepare` ([`crates/nexus-workflow/src/handlers/run.rs:29`](../../../crates/nexus-workflow/src/handlers/run.rs)) loads the workflow from
the in-memory registry (sourced from disk by `core_plugin::reload`),
evaluates the optional `[condition]` gate, then awaits
`run_workflow_with_variables(&workflow, &dispatcher, &variables)`. The
dispatcher is `KernelActionDispatcher { ctx: Arc<KernelPluginContext> }`,
where `ctx` is the **workflow plugin's** context — built once at
bootstrap by `wire_context("com.nexus.workflow", ...)` with caps from
`workflow_capabilities()`.

Every step type (`ipc`/`ipc_call`, `ai_prompt`, `ai_decision`,
`terminal`, `notify`, plus `submit_async_step` for `async = true`)
funnels through `self.ctx.ipc_call(target, command, args, timeout)`.
The kernel's `ipc_call_inner`
([`crates/nexus-kernel/src/context_impl.rs:143`](../../../crates/nexus-kernel/src/context_impl.rs))
performs three checks, all against `self` = the workflow ctx:

1. `caps_contains(Capability::IpcCall)` on the caller (workflow holds it).
2. For each cap returned by
   `dispatcher.required_caller_caps_for_args(target, command, &args)`,
   `caps_contains(required)` on the caller.
3. If the target handler is `internal = true`, the caller's
   `caller_trust_level` must equal `TrustLevel::Core` (workflow is
   wired as `TrustLevel::Core` in `nexus-bootstrap/src/lib.rs:380`).

### Why this is "laundering"

Workflow files are **untrusted input** (any code or user that can write
to `<forge>/.workflows/` can author one) but they are dispatched
through a **trusted intermediary** (workflow plugin, Core trust).
Mitigations layered into the system so far:

- `workflow_capabilities()` is narrow: `IpcCall + AiChat` only
  ([`crates/nexus-bootstrap/src/lib.rs:663-684`](../../../crates/nexus-bootstrap/src/lib.rs)).
  A workflow step cannot reach handlers gated on
  `fs.write` / `process.spawn` / `net.http` / `kv.write` / etc.
  directly, because the cap check at step 2 above runs against the
  workflow ctx and fails.
- Cap-elevation work in Phase 1 (P1-01..P1-07) tightened the per-handler
  cap rows — terminal, git, linkpreview, agent, collab, security, etc.
  are now properly gated.
- The `templates_init` seed path (writes default workflows) is
  read-only from the perspective of `[[steps]]` — templates are still
  user-replaceable but starting templates are vetted in-tree.

What still flows through:

- **Internal-only handlers (`internal = true`).** `com.nexus.ai::resolve_credentials`
  is the load-bearing example: it returns provider API keys. The Core-trust
  check exists precisely to keep community plugins out, but workflow is
  Core-trusted by virtue of being a first-party plugin, so a hostile
  `.workflow.toml` whose first step is
  `type = "ipc"; target = "com.nexus.ai"; command = "resolve_credentials"`
  will succeed.
- **Unrestricted handlers.** Anything marked `unrestricted` in
  `cap_matrix.toml` is reachable. Many of these are read-only (`list`,
  `get`, validators), but some seed writes — e.g.
  `com.nexus.workflow::templates_init` itself, `com.nexus.workflow::set_digest_config`,
  `com.nexus.notifications::send` (which then fans out to Discord /
  Telegram / Desktop without further caps).
- **`AiChat` reach.** The workflow ctx holds `AiChat`, so the
  `ai_prompt` / `ai_decision` step types successfully dispatch
  `com.nexus.ai::ask` / `stream_chat`. A hostile workflow can drive the
  AI plugin without limit (rate limits aside).
- **The async runtime path.** `submit_async_step` packages a target
  plugin + command into `AgentTaskKind::WorkflowAiStep` and submits
  through `com.nexus.ai.runtime::submit`. The runtime later replays
  the call on its own ctx (`ai_runtime_capabilities` = `IpcCall + AiChat
  + EventsPublish`, also Core-trusted), which has the same laundering
  shape and is tracked under BL-134 follow-ups.

### Trigger sources

Workflow runs originate from:

1. **CLI manual run** — `nexus workflow run <name>`. The CLI scope is
   already an IPC bypass (ADR 0031), and the operator typed the name
   explicitly, so this case is least concerning — the principal IS the
   user.
2. **Cron / digest scheduler** — runs at scheduled times against any
   workflow whose `[trigger] type = "cron"`. No human in the loop.
3. **Webhook** — `webhook.rs` listens on a configured port and
   dispatches `com.nexus.workflow::run` on inbound HTTP. The principal
   is an arbitrary network peer.
4. **File-watcher trigger** — `[trigger] type = "fs_changed"` dispatches
   on any matching path write. The principal is whatever process wrote
   the file (which may be any plugin holding `fs.write`).

(2)–(4) are the actual exposure: a hostile actor who can drop a file
into the forge can wait to be triggered automatically, then use the
laundered Core-trust call to read credentials.

## Goals

1. Prevent a `.workflow.toml` from invoking handlers it could not
   invoke if the same call were issued by a `TrustLevel::Community`
   caller with `IpcCall` only.
2. Preserve every legitimate in-tree workflow shape today:
   `ai_prompt`, `ai_decision`, `terminal` start/stop/restart/run_adhoc,
   `notify`, `ipc` to `com.nexus.terminal::list_sessions`, and the
   async-submit path for AI calls.
3. Keep the change reviewable in one Phase-1-sized release — anything
   that requires a principal-passing reshape across the kernel is
   deferred.

## Non-goals

- A general "delegated caller" mechanism for the whole kernel (option E
  below). This is the right long-term answer but a far larger change
  than P1-08 can swallow; tracked separately.
- Hardening the AI runtime's `WorkflowAiStep` replay path; same shape,
  same fix should apply, but the runtime owns its own RFC under BL-134.
- Plumbing per-row signatures into workflow files. Cryptographic
  authorship attestation is a separate question.

## Considered options

### Option A — Status quo + audit logging

Add an `audit::log_workflow_step` call before every dispatched IPC,
recording `(workflow_name, step_index, target, command)`. Keep the
existing trust + cap model. Document the laundering surface.

- **Pros:** zero behaviour change; users keep the full step set; ships
  in a day.
- **Cons:** does not actually close the hole — logs only help after
  the fact. Credential exfiltration is detected, not prevented. Fails
  the goal.
- **Verdict:** rejected as the sole answer, but the audit logging
  itself is a no-regrets addition and should ship alongside whatever
  else lands.

### Option B — Drop workflow trust to Community

Wire `com.nexus.workflow` with `TrustLevel::Community` instead of `Core`.
Internal-only handlers (P1-02 marker) immediately become unreachable
to workflow steps.

- **Pros:** small, declarative, plugs the credential-exfil path
  through `resolve_credentials`. Free.
- **Cons:** today the cron-driven digest pipeline runs on the same
  workflow ctx, and uses `internal = true` handlers? — need to audit.
  Spot-check: digest steps look like ordinary IPC calls, no internal
  hits.  But the `ai_runtime` peer plugin **is** Core-trusted on the
  assumption that the workflow it serves shares that trust, so we'd
  need to confirm Core-only paths aren't reachable through
  `submit_async_step` either. Risk of breaking a non-obvious in-tree
  call.
- **Verdict:** likely safe in isolation, ship it. But it does **not**
  close the `unrestricted`-handler surface (notifications fan-out,
  `set_digest_config`, etc.). Option B alone is insufficient.

### Option C — Per-workflow `requires_caps` declaration

Workflow TOML grows a top-level array:

```toml
[workflow]
name = "Nightly digest"
requires_caps = ["ai.chat", "notifications.send"]
```

The dispatcher refuses to issue a step whose target handler's
`required_caller_caps_for_args` is not a subset of `requires_caps`.
Workflow's wired cap set continues to act as the hard ceiling — `requires_caps`
can only be *less* than what the workflow ctx holds.

- **Pros:** explicit, declarative, reviewable in code review or
  diff (workflow files live in the forge, so changes are visible).
  Lines up with the existing "manifest-declared caps" pattern for
  plugins.
- **Cons:** UX cost — workflow authors must enumerate caps;
  ai_prompt/ai_decision authors will universally write `["ai.chat"]`.
  Migration churn — every in-tree template gains a `requires_caps`
  row. Doesn't help against `unrestricted` targets, since they
  require nothing — the laundering of `set_digest_config`,
  `notifications::send`, etc. is unaffected.
- **Verdict:** good fit for **gated** handlers (`ai.chat`, future
  `net.http` if exposed). Doesn't help for `unrestricted`. Pair with
  another option.

### Option D — Step-target denylist + step-type allowlist

Two static lists in `nexus-workflow`:

1. **Step-type allowlist** — only `noop`, `ipc`, `ipc_call`, `ai_prompt`,
   `ai_decision`, `terminal`, `notify` are accepted. Already enforced
   informally; the executor today returns `unsupported: true` for
   unknown types. Tighten to a hard error.
2. **Per-step-type target allowlist** — `ipc` / `ipc_call` may only
   target a closed list of `(plugin_id, command)` pairs vetted as
   "safe to expose to workflow authors":

   ```rust
   const WORKFLOW_IPC_ALLOWLIST: &[(&str, &str)] = &[
       ("com.nexus.terminal", "list_sessions"),
       ("com.nexus.notifications", "send"),
       ("com.nexus.workflow", "run_history"),
       // …explicit additions as use cases land
   ];
   ```

   `ai_prompt` is hard-coded to `com.nexus.ai::ask`; `terminal` to
   `com.nexus.terminal::run_saved`/`list_sessions`/`close_session`;
   `notify` to `com.nexus.notifications::send`. Those step types
   *already* hard-code their targets, so the only attack surface is the
   open-ended `ipc` step.

- **Pros:** kills the `resolve_credentials` path immediately (it's not
  on the list, and `internal = true` would reject anyway under Option B
  — defence in depth). Kills the `set_digest_config` /
  `templates_init` self-laundering. Survives without touching the
  kernel.
- **Cons:** allowlist maintenance — every new legitimate `ipc` step
  needs a code change in `nexus-workflow`, not just a workflow-file
  change. Acceptable tradeoff given the security stakes; future
  authoring tooling could publish a curated catalog.
- **Verdict:** **recommended core mechanism**. Bounds the surface to
  the actual menu of things a workflow author should be doing.

### Option E — Delegated caller / principal-passing

Thread a `Principal` (caller identity + cap snapshot) through
`KernelPluginContext::ipc_call`. The workflow dispatcher captures
the principal from `run`'s caller and passes it down; the target
handler's cap check runs against the *principal*, not the
intermediary. Mirrors the OAuth `act_as` pattern.

- **Pros:** correct, general, fixes the laundering shape everywhere
  it appears (AI runtime, agent delegate, future amplifier plugins).
- **Cons:** large reshape — every kernel surface and every plugin
  context needs to thread the principal. Forward-compatible with
  WASM plugins (Phase 4 of the WASM runtime), but ADR 0030 deferred
  WASM. The async-runtime case is also non-trivial: `submit_async_step`
  serialises the call and replays later, so the principal must be
  serialisable and re-verified at replay time.
- **Verdict:** **deferred**. Right answer, wrong release. Track under
  a new ADR ("Principal-passing IPC") for 0.2.x once the WASM runtime
  lands and forces the question anyway.

### Option F — Workflow file trust tagging

Distinguish "blessed" workflows (in-tree templates, user-signed) from
"ambient" workflows (anything else in `<forge>/.workflows/`). Only
blessed workflows get the current cap set; ambient workflows get a
reduced set.

- **Pros:** preserves the existing functionality for blessed flows;
  cleanly handles the "hostile fs.write dropped a file" case.
- **Cons:** introduces a user-visible trust system with no current
  UX. Users would have to learn what blessing means; CLI/TUI/shell
  would need a UI to bless/unbless. Out of scope for a P1 item.
- **Verdict:** rejected for 0.1.x. Reconsider if option D's
  allowlist becomes painful to maintain.

## Recommendation

Ship layered:

1. **Immediately (P1-08 implementation, S effort):**
   - **Option D step-type allowlist + ipc-target allowlist.** Hard
     error on unknown `step_type`; hard error on `ipc`/`ipc_call`
     steps whose `target::command` isn't in the curated list.
   - **Option B trust drop.** Wire `com.nexus.workflow` (and
     `com.nexus.ai.runtime`'s WorkflowAiStep replay) as
     `TrustLevel::Community`. Re-audit `internal = true` rows; if any
     are reachable only from workflow → file new RFC.
   - **Option A audit logging.** Emit an `audit::workflow_dispatch`
     event before every step's IPC fires, with workflow name +
     step index + target + command. Cheap insurance.
2. **Follow-up (separate PR, M effort):**
   - **Option C `requires_caps` declaration.** Add the top-level
     field, plumb it through `parse_workflow_text`, enforce against
     `required_caller_caps_for_args`. Authoring tooling and template
     migration in the same patch.
3. **Long-term (separate ADR, post-0.1.x):**
   - **Option E principal-passing.** Tracked under "Principal-passing
     IPC" ADR. Forced by the WASM runtime work anyway (ADR 0030
     follow-up).

## Acceptance

This RFC is "accepted" when:

- The recommendation section is signed off by the workflow,
  security, and AI-runtime owners (today: one person, but the next
  reviewer should still tick the boxes).
- A `docs/0.1.2/architecture/` note records the new step-target
  allowlist as the authoritative menu.
- `cap_matrix.toml`'s `# AUDIT` comments on `com.nexus.workflow::run`
  and `::run_digest` are rewritten to point at this RFC, and the
  `unrestricted = ...` strings updated to reflect the new constraints.

## Open questions

1. **Is `com.nexus.ai.runtime` reachable through a workflow step?**
   `submit_async_step` only fires when `step.async_submit` is true,
   and it targets `com.nexus.ai.runtime::submit` directly. If the
   runtime's submit handler is gated on a cap workflow doesn't hold
   under the proposed Community-trust change, the async path breaks.
   Likely we need to add the runtime to the allowlist explicitly or
   keep its caller as Core (the runtime itself, not workflow).
2. **Cron-only workflows in `.forge/`.** The digest scheduler reads
   from a fixed registry path. Are there in-tree digest definitions
   that rely on `internal = true` targets? Suspected no, but a one-time
   sweep of `nexus-workflow/src/digests.rs` is part of the
   implementation pass.
3. **Where does the allowlist live?** `nexus-workflow` is the obvious
   home, but it could also be expressed as a column in `cap_matrix.toml`
   (`workflow_callable = true`) so the menu is co-located with cap
   declarations. Slight preference for the latter — single source of
   truth — but it means every cap_matrix reviewer needs to think about
   workflow callability.
4. **Webhook source.** A webhook-triggered workflow's "principal" is
   *whoever's on the other end of the socket*. Even with full
   principal-passing, the principal there is anonymous. Option D's
   allowlist remains the only defence; the question is whether to
   restrict webhook-triggered workflows to a tighter allowlist than
   cron-triggered ones.

## References

- Issue #77 — workflow laundering tracking issue
- ADR 0030 — WASM community runtime deferral (archive)
- ADR 0031 — CLI-scope IPC exceptions (archive)
- `docs/0.1.2/implementation-plan.md` — P1-02 (in-tree marker), P1-08
- `crates/nexus-bootstrap/cap_matrix.toml` — workflow handler rows
- `crates/nexus-kernel/src/context_impl.rs` — `ipc_call_inner` cap-check path
- `crates/nexus-workflow/src/handlers/run.rs` — `KernelActionDispatcher`
