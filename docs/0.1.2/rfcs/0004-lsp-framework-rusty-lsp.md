# RFC 0004 — `rusty_lsp`: don't incorporate (Nexus hosts LSP, doesn't build it)

- **Status:** Draft (assessment / recommendation)
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** protocol-crate hygiene (`nexus-{lsp,acp,dap,mcp,remote}`), AgenticSandbox vision
- **Touches (if ever accepted):** would be a new leaf `nexus-jsonrpc` crate + refactor of the five protocol crates' `transport.rs` — **not recommended now**
- **Related:** [RFC 0003 — `rusty_term`](0003-terminal-emulator-rusty-term.md) (whose `l13` side channel depends on `rusty_lsp`)

---

## Summary

[`baileyrd/rusty_lsp`](https://github.com/baileyrd/rusty_lsp) is a small,
deps-light (tokio + serde only) **framework for building Language Server
Protocol servers** — it owns JSON-RPC framing, dispatch, the
`initialize`/`shutdown` lifecycle, and request cancellation, and hands you one
`LanguageServer` trait to implement.

This RFC concludes: **do not incorporate it.** It is the weakest fit of the
repos assessed so far, for a structural reason — **Nexus *hosts* language servers,
it does not *build* them.** The one piece that could in principle be reused (the
JSON-RPC + Content-Length framing core) is already implemented five times over in
Nexus, and that duplication is a **deliberate, documented design choice**, not an
accident to be cleaned up with a vendored crate.

Keep `rusty_lsp` on the shelf as a reference for exactly one speculative future:
if Nexus ever exposes the forge / knowledge graph **as** a language server for
external editors, it is the natural foundation.

## Background

### What `rusty_lsp` is

A reusable async LSP **server** engine (edition 2024, 0.1.0, single initial
commit, 28 tests, dual MIT/Apache). Deps are just `tokio`, `serde`,
`serde_json`. Modules:

| Module | Responsibility |
|---|---|
| `transport` | `Content-Length` framing over any async byte stream |
| `jsonrpc` | JSON-RPC 2.0 request/response/notification model |
| `lsp` | Typed LSP protocol data structures |
| `text` | UTF-16 ↔ byte position conversions for buffer indexing |
| `service` | The `LanguageServer` trait you implement |
| `client` | `Client` handle for server → client messages |
| `server` | Runtime: dispatch, lifecycle, exactly-once cancellation |

Notable correctness discipline: notifications run in receipt order while
requests are spawned; `$/cancelRequest` aborts in-flight handlers and guarantees
each request is answered **exactly once**; `initialize` is enforced first. It is
explicitly *"not a server for any particular language; it is the reusable engine
you build one on top of."*

### What Nexus has today

- **`nexus-lsp` is an LSP *host / client*, not a server.** Its own crate docs:
  *"Spawns Language Server Protocol servers from `<forge>/.forge/lsp.toml`,
  bridges their JSON-RPC stdio streams to the kernel IPC surface
  (`com.nexus.lsp`) … The host is a transparent proxy."* It consumes
  rust-analyzer / tsserver / etc.; it does not implement language servers.
- **`nexus-dap`** is likewise a host for Debug Adapter Protocol adapters.
- **`nexus-acp`**, **`nexus-mcp`**, **`nexus-remote`** each have *both* client
  and server sides — Nexus reaches agents through its own MCP/ACP servers and
  kernel IPC, not through an LSP server.
- **JSON-RPC + framing is reimplemented per protocol, on purpose.** Each of
  `nexus-{lsp,acp,dap,mcp,remote}` carries its own `transport.rs`. Two framing
  styles are in use — Content-Length-prefixed (LSP, DAP) and line-delimited
  (ACP, remote) — and the code documents the duplication as **intentional**:

  > `nexus-remote/transport.rs`: *"Same shape as `nexus_acp::transport` —
  > duplicated rather than shared so the two crates can evolve independently."*

## Fit analysis

| Possible use | Fit | Why |
|---|---|---|
| Adopt `rusty_lsp` as an **LSP-server framework** | ✗ Wrong role | Nexus is an LSP *consumer/host*. There is no language server in the workspace for the framework to power. |
| Extract its **`jsonrpc` + `transport`** as a shared framing core for the five protocol crates | ✗ Not now | The duplication is a **deliberate, documented** decision (crates evolve independently); two distinct framing styles are in play; this would be a churny refactor against working, shipped code for internal tidiness, not user value. |
| Reuse **`text.rs`** (UTF-16 ↔ byte math) and the **cancellation/lifecycle** discipline | ◻ Reference only | Correctness-sensitive and well-tested. Worth consulting *if* Nexus ever builds a JSON-RPC server surface; nothing to adopt today. |
| Foundation for a future **"forge-as-LSP-server"** (expose Nexus code-intelligence to external editors over LSP) | ◻ Speculative | The one scenario where `rusty_lsp` would be the right tool — but it is not on the roadmap (agent loop / sandbox / packaging are). |

## The honest case for a future second look

There is a real, coherent product idea hiding here: today Nexus's code
intelligence (the forge, the knowledge graph, GitNexus-style symbol data) is
reachable only *inside* Nexus. Exposing it **as a language server** would let any
LSP-speaking editor (VS Code, Neovim, Helix) get Nexus hover/definition/
diagnostics/completion against a forge — turning Nexus into a backend other
tools consume. If that idea is ever prioritized, `rusty_lsp` is precisely the
foundation to build it on (and it already ships a runnable `text_server`
example showing the shape). That is the trigger to revisit this RFC.

## Verdict

**Do not incorporate `rusty_lsp`.** It solves a problem Nexus does not have
(authoring language servers) and its reusable plumbing overlaps code Nexus
deliberately keeps duplicated. No track, no first step — this is a documented
"no," with a clear future trigger (a forge-as-LSP-server surface) that would
reopen it.

## Open questions

- **Is "forge-as-LSP-server" ever in scope?** If product direction moves toward
  Nexus-as-a-backend-for-other-editors, reopen this RFC and prototype on
  `rusty_lsp`. Until then, no action.
- **Is the per-protocol framing duplication ever worth consolidating?** If the
  five `transport.rs` copies become a maintenance burden, a shared leaf
  `nexus-jsonrpc` crate is the move — but that is an independent internal
  refactor to evaluate on its own merits, with `rusty_lsp`'s `jsonrpc`/
  `transport`/`text` modules as one reference implementation among several
  (the existing Nexus copies are equally valid starting points).
