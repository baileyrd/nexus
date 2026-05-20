# com.nexus.workflow

- **Path:** `crates/nexus-workflow/`
- **Tier:** Core Rust
- **Bootstrap order:** 13

## Architecture
- Entry point: `crates/nexus-workflow/src/core_plugin.rs` (`WorkflowCorePlugin`). Library modules: `parse`, `registry`, `executor`, `cron`, `webhook`, `digests`, `run_history`, `templates`, `ai_steps`, `condition`, `interpolate`, `trigger_validation`, `handlers/`.
- Owns a `WorkflowRegistry` loaded from `<forge>/.workflows/*.workflow.toml` plus a `RunHistoryStore`, a mutable `DigestConfig`, and a `WebhookConfig`.
- On `wire_context` spawns tokio tasks: cron schedulers, `file_event` subscribers (`com.nexus.storage.file_*`), `git_event` subscribers (`com.nexus.git.*`), `mcp_event` subscribers (`com.nexus.mcp.*`), and a 60 s digest scheduler. Webhook listener is started on demand when `[webhooks].enabled = true` and at least one workflow declares a `webhook` trigger.
- IPC dependencies: `com.nexus.ai`, `com.nexus.ai.runtime`, `com.nexus.notifications`, `com.nexus.storage`, `com.nexus.terminal`, `com.nexus.editor`, `com.nexus.database`, etc. — invoked by step types via the kernel `ipc_call`.

## Persistence
- `<forge>/.workflows/` — `.workflow.toml` files (file-as-truth source).
- `<forge>/.workflows/run_history.json` — ring buffer of ≤ 200 entries (`RUN_HISTORY_CAP`, `src/run_history.rs:30`).
- Built-in templates list at `src/templates.rs`; `templates_init` writes them into `.workflows/`.
- Digest outputs land under the configured `DigestConfig.output_dir` (default `DEFAULT_DIGESTS_DIR`).

## Settings owned
- `[digests]` block in `<forge>/.forge/config.toml` — `DigestConfig` (`crates/nexus-workflow/src/digests.rs`). Loaded by `nexus_bootstrap::load_digest_config` (`crates/nexus-bootstrap/src/lib.rs:488`). Not yet listed in `docs/0.1.2/settings/forge-config.md` for `config.toml`.
- `[webhooks]` block in the same file — `WebhookConfig` (`crates/nexus-workflow/src/webhook.rs`). Loaded by `nexus_bootstrap::load_webhook_config` (`lib.rs:923`). Also not yet in the settings doc.
- Mutated at runtime via `set_digest_config`.

## External dependencies of note
- `tokio`, `chrono`, `regex-lite`, `toml`. Webhook listener binds a TCP port.
- No native libs; no syscalls beyond filesystem + clock.

## Surface
Handlers (`IPC_HANDLERS`, `src/core_plugin.rs:118`):

| Id | Command | Notes |
|---:|---------|-------|
| 1 | `list` | Loaded workflows + metadata |
| 2 | `get` | One workflow by `name` |
| 3 | `reload` | Re-scan `.workflows/` |
| 4 | `validate` | Parse a TOML string |
| 5 | `run` | Execute by name with optional `variables` |
| 6 | `run_digest` | Force a daily/weekly digest run |
| 7 | `set_digest_config` | Live-swap `DigestConfig` |
| 8 | `templates_list` | Built-in workflow templates |
| 9 | `templates_get` | Fetch template body |
| 10 | `templates_init` | Write template into `.workflows/` |
| 11 | `run_history` | Bounded execution log |
| 12 | `next_fire` | Next cron fire times |

Subscribes to: `com.nexus.storage.file_*`, `com.nexus.git.*`, `com.nexus.mcp.*`.

## Necessity
- **Verdict:** Optional
- **Required for basic capabilities?** No — basic markdown browsing/editing/search/git does not need declarative `.workflow.toml` triggers, scheduled digests, or workflow runs.
- **Depended on by:** nothing essential. Shell `workflow` feature plugin and CLI surface workflows; agent/runtime can also be invoked from workflow steps but the reverse is not required.
- **Depends on:** kernel event bus + IPC to `com.nexus.ai`, `com.nexus.ai.runtime`, `com.nexus.notifications`, `com.nexus.storage`, `com.nexus.git`, `com.nexus.mcp.host`, `com.nexus.editor`, `com.nexus.database`, `com.nexus.terminal`. None of these calls fire unless a user-authored workflow runs.
- **What breaks if removed:** declarative pipelines (cron / file-event / git-event / mcp-event / webhook), digest scheduler, workflow templates, and the workflow shell plugin. None of these are part of the basic-capability scope.

## Notes
- The crate is large (16 modules) but the basic-capability path never touches it. Trigger engines spawn background tokio tasks even when no workflows exist — empty registry simply produces no handles.
- `webhook` listener and the `mcp_event` topic surface are new; the latter currently exposes only `host_started`.
- Documented as PRD-16 scaffold; `webhook` was not yet wired when `core_plugin.rs` was last edited but appears to be wired now via `WebhookConfig`.
