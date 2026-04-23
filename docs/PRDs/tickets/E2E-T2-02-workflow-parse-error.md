# E2E-T2-02 — Populate `WorkflowEntry.parseError` + seed bad `.workflow.toml`

**Status**: open
**Opened**: 2026-04-23
**Context**: follow-up from Tier-2 testability pass (shell/ commit `8a4ce44`, PR #1).
**Unblocks**: 1 `it.skip` in [shell/e2e/specs/tier2/workflow.spec.ts](../../../shell/e2e/specs/tier2/workflow.spec.ts) — "invalid workflow surfaces a per-row parse error".

## Scope

### Kernel (`crates/nexus-workflow`)

- In `com.nexus.workflow::list`, catch per-file TOML decode errors instead of failing the whole list or folding them into the global `loadError`.
- Add a `parse_error: Option<String>` field to the `Workflow` list projection struct.
- When decode fails: still emit a list entry with `name` derived from the filename, `parse_error` set to the decode message, and other fields zero/empty.

### Shell

- Map the kernel's `parse_error` → `WorkflowEntry.parseError` in the workflow IPC decoder. The TS field already exists on [workflowStore.ts](../../../shell/src/plugins/nexus/workflow/workflowStore.ts).
- No `WorkflowView.tsx` changes needed — the invalid-row indicator already renders when `parseError != null`.

### Fixture

- Add `shell/e2e/fixtures/vault/.workflows/broken.workflow.toml` containing malformed TOML (e.g. unterminated string, bad trigger type).

### Spec

- Un-skip "invalid workflow surfaces a per-row parse error" in `workflow.spec.ts`. Assert:
  - `[aria-label="Workflow broken invalid"]` exists.
  - `[data-invalid="true"]` exists on the same row.
  - Valid workflows (if any siblings) do NOT carry either attribute.

## Selectors (already landed)

| Element | Selector |
| --- | --- |
| Invalid row | `[data-invalid="true"]`, `[aria-label="Workflow {name} invalid"]` |
