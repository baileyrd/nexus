# Attribution — `nexus-rush`

This crate is an **in-tree vendoring** of [`baileyrd/rush`](https://github.com/baileyrd/rush),
a from-scratch, bash-compatible shell written in Rust, adopted per
[RFC 0002 — Bundled shell (`rush`)](../../docs/0.1.2/rfcs/0002-bundled-shell-rush.md).

## What was vendored

The shell modules are copied verbatim: `arith`, `expand`, `glob`, `lexer`,
`parser` (plus the unchanged bulk of `builtins`, `exec`, `func`, `job`, `vars`).
The upstream is a single binary crate (`src/main.rs`); here it is refactored into
a **library + thin binary**.

## Nexus-specific changes

- **Binary → library + thin bin.** `src/lib.rs` is the embeddable, testable shell
  core; `src/main.rs` is a thin wrapper that owns the single `process::exit` and
  the argv dispatch. The interactive REPL lives in `lib::run_repl` (it needs the
  private `parser`/`exec`/`job` internals) and returns an exit code rather than
  diverging.
- **No `process::exit` in library code.** The `exit` builtin (`builtins.rs`) sets
  a thread-local latch (`vars::EXIT_REQUESTED`) that the executor early-outs on
  and `lib::eval` resolves into a returned status. The three `process::exit`
  sites in the original `main.rs` are replaced by the binary's single exit.
- **Embeddable state reset.** `vars::reset_state()` clears all thread-local shell
  state (variables, `$?`, loop/return control, positional params, function
  definitions, the exit latch) so one-shot `eval_fresh` calls don't leak state.
- **Job control no-op when embedded.** `vars::set_embedded(true)` (driven by the
  `NEXUS_EMBEDDED_SHELL=1` env var that `nexus-terminal` sets) makes `job::init`
  skip claiming the controlling terminal (`tcsetpgrp`/`setpgid`/signal-ignore),
  and routes foreground execution through the plain spawn-and-wait path. The PTY's
  session leader — not rush's process group — owns the terminal in that case.

## Not adopted

No GUI and no `l13` side channel exist in `rush`; nothing of that kind is here.
This crate is shell-language-only. Hardening of known `rush` gaps (`fork` in
pipelines/command-substitution, `set -o pipefail`, `[[ … ]]`, arrays) is RFC 0002
Stage 2 and tracked as a follow-up.

**Teaching bundled rush to emit OSC 133** command/exit-code marks (so terminal
introspection — RFC 0003 — works for rush sessions without the printf-sentinel
fallback) is a deliberate **follow-up**: rush has no precmd/preexec hook today.
