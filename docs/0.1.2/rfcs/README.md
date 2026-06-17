# Nexus RFCs

Design proposals and assessments for Nexus. An RFC captures a decision worth
recording — a non-trivial design change, or an evaluation of whether to take
something on — with enough context that a reader can follow the reasoning
without re-deriving it. When the code and an RFC disagree, the code wins; an RFC
is a snapshot of intent at its `Created` date, not a maintained spec.

## Index

| # | Title | Status | One-line |
|---|-------|--------|----------|
| [0001](0001-workflow-cap-delegation.md) | Workflow cap delegation | Draft | Close the capability-laundering surface where workflow steps dispatch through the workflow plugin's own caps rather than the triggering principal's. |
| [0002](0002-bundled-shell-rush.md) | Bundled shell (`rush`) | Draft — assessment | **Incorporate, staged & opt-in.** Vendor `baileyrd/rush` as a workspace library crate and run it as the bundled shell for *sandboxed* terminal sessions; system shell stays the default. |
| [0003](0003-terminal-emulator-rusty-term.md) | `rusty_term` terminal emulator | Draft — assessment | **Selectively adopt.** Not the GUI (conflicts with ADR 0011); do adopt the headless VT grid core + OSC 133 / terminal-as-MCP-resource agent introspection. |
| [0004](0004-lsp-framework-rusty-lsp.md) | `rusty_lsp` LSP framework | Draft — assessment | **Don't incorporate.** Nexus *hosts* language servers, it doesn't *build* them; its JSON-RPC core is already (deliberately) duplicated per protocol. Revisit only for a future forge-as-LSP-server. |

## Assessment series (0002–0004)

RFCs 0002–0004 are a single sweep: evaluating the sibling `baileyrd/*` Rust
repos for whether Nexus should incorporate them, given the OS-sandbox /
AgenticSandbox direction. They share a method — locate the equivalent subsystem
already in Nexus, decide what (if anything) is genuinely additive, and recommend
the smallest opt-in first step rather than a wholesale merge. Their combined
through-line: a bundled shell (0002) plus OSC 133 command/exit-code capture
(0003 Track A) would give the sandbox a fully Nexus-owned, agent-observable
shell + terminal stack.

Repos still unassessed at time of writing: `rusty_omp`, `remind_me`
(the latter's engine already landed as `com.nexus.memory` — see
[`../memory.md`](../memory.md)).

## Conventions

- **Filename:** `NNNN-short-slug.md`, zero-padded, allocated in order.
- **Front matter:** `Status`, `Owner`, `Created`, `Tracks`, `Touches`, and
  `Related` where useful. Assessment RFCs use `Status: Draft — assessment` and
  open with a one-paragraph **Summary** that states the verdict up front.
- **Status values:** `Draft` → `Accepted` / `Rejected` → `Superseded`. Update
  the row above when a status changes.
