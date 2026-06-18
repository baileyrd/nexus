# RFC 0002 — Bundled shell (vendor `rush`) for sandboxed terminal sessions

- **Status:** Accepted — Stage 1 landed (`nexus-rush` vendored; bundled shell wired for sandboxed sessions). Stage 2 (hardening) / Stage 3 (AgenticSandbox `/bin/sh`) remain.
- **Owner:** unassigned
- **Created:** 2026-06-17
- **Tracks:** OS-sandbox adoption (Phase 4), AgenticSandbox vision
- **Touches (if accepted):** new workspace crate (`nexus-rush` / vendored `rush`), `crates/nexus-terminal/src/shell.rs` (`ShellSpec`), `crates/nexus-terminal/src/session.rs` (`SessionConfig`, `resolve_spawn_target`), `.forge/sandbox.toml` (`SandboxConfig`)

---

## Summary

[`baileyrd/rush`](https://github.com/baileyrd/rush) is a from-scratch,
bash-compatible **shell** written in Rust. This RFC assesses whether Nexus
should incorporate it and concludes: **yes — staged and opt-in, not as a
system-shell replacement.** It is a strong strategic fit for the OS-sandbox /
AgenticSandbox story and cheap to wire given the spawn plumbing that already
exists in `nexus-terminal`, but it is too young (0.1.0) to be the default shell.

The recommended first step is to vendor `rush` as a workspace **library** crate
and let `nexus-terminal` spawn the bundled shell **only for sandboxed sessions**,
leaving the detected system shell (`$SHELL`) as the default everywhere else.

## Background

### What `rush` is

A standalone POSIX-ish shell — ~2,500 LOC across 10 modules
(lexer → parser → expand → exec → builtins), plus Unix job control. It
implements the *shell language itself*:

- Pipelines and redirection (incl. `2>`, `2>&1`, `&>`, here-docs `<<`/`<<-`/`<<'EOF'`).
- The full expansion set: `$VAR`, `${…}` operator family, `$?`, positional
  params, command substitution `$(…)`, arithmetic `$((…))`, globbing, tilde;
  unquoted results word-split.
- Control flow: `if`/`elif`/`else`, `while`/`until`, `for`, `case`,
  `break`/`continue [n]`.
- Shell functions with recursion, brace groups, subshells `( … )`.
- Builtins: `cd pwd echo export unset test/[ ] true false : break continue
  return exit` (+ `jobs fg bg kill` on Unix).
- Execution modes: interactive REPL (rustyline history), `rush script.sh args`,
  `rush -c "cmds"`.

Pre-release: version 0.1.0, 63 tests, edition 2024, dependencies are just
`rustyline` + `libc` (Unix only). Currently a **binary** crate (`src/main.rs`),
not a library.

### What `nexus-terminal` does today

The crux of the assessment: **`rush` and `nexus-terminal` are complementary,
not competing.**

- **`nexus-terminal`** (~14,800 LOC, `com.nexus.terminal`) is the *terminal
  host / emulator*. It spawns the **user's existing system shell**
  (`$SHELL` → bash/zsh/sh/cmd, see `shell.rs::detect_default_shell`) inside a
  `portable-pty` PTY, then captures output, manages sessions, parses ANSI/URLs,
  offers AI suggestions, and persists scrollback. It does **not** implement a
  shell language.
- **`rush`** is the *shell* — the program that runs **inside** such a PTY.

Incorporating `rush` therefore does not replace `nexus-terminal`; it gives Nexus
a shell **it owns** to run inside it.

## Motivation — the sandbox angle

The prior work shipped the OS sandbox (Landlock filesystem + seccomp network
off-by-default, opt-in per terminal session via `SessionConfig.sandbox`; see
[`docs/0.1.2/os-sandbox.md`](../os-sandbox.md)). A Nexus-owned shell pairs with
it unusually well:

1. **Self-contained sandbox.** The AgenticSandbox vision is a confined
   environment. Today a sandboxed session still spawns *system bash* — a large,
   uncontrolled binary that sources `.bashrc` and may not even exist (Windows).
   Bundling `rush` gives the sandbox its own auditable, minimal, dependency-free
   `/bin/sh`.
2. **Cross-platform consistency.** Agents get POSIX-ish shell semantics on
   Windows without WSL/cmd quirks.
3. **The integration surface already exists.** `SessionConfig` already has
   `shell: Option<ShellSpec>` *and* `sandbox: Option<…>`, and
   `session.rs::resolve_spawn_target()` already wraps the chosen shell with the
   `nexus-sandbox` helper when a policy is set. Wiring a bundled `rush` is
   essentially "point the spawn target at it" — no new IPC handler, no
   architecture change.
4. **Defense in depth.** Because Nexus would own the shell, policy could
   eventually be enforced *inside* it (command resolution, builtin gating) on
   top of the OS-level sandbox.

## The honest case against (why not as a default)

- **Maturity.** 0.1.0, with known gaps that matter for real agent workloads:
  no real `fork`, so compound commands can't appear in pipelines or command
  substitution; `exit` inside a subshell exits the whole shell; a piped-fd dup
  limitation (`cmd 2>&1 | next` leaves stderr on the terminal). It also lacks
  common bash-isms agents rely on (`set -e` / `set -o pipefail`, `[[ … ]]`,
  arrays, process substitution `<()`).
- **Most agent value is in external tools** (git/cargo/npm) — those run
  regardless of host shell; the shell mostly does orchestration
  (pipes/redirects/`&&`), which `rush` *does* cover.
- **Maintenance.** Vendoring another repo to keep green in the workspace. It is
  edition 2024 while the workspace is edition 2021 — not a blocker (cargo allows
  per-crate editions, and the pinned toolchain `1.94.1` supports edition 2024),
  but a wrinkle to note.

## Verdict

**Incorporate it, but staged and opt-in.** Strong fit for the
sandbox / AgenticSandbox story; cheap to wire; too young to be the default.

| Stage | Work | Risk |
|---|---|---|
| **1. Vendor as a library crate behind an opt-in** | Refactor `rush` `main.rs` → `lib.rs` + thin bin; add as a workspace member (`nexus-rush` or similar); let `nexus-terminal` spawn the bundled shell **only for sandboxed sessions**, system shell stays the default | Low — isolated, opt-in, mirrors how the sandbox itself landed |
| **2. Harden against real agent usage** | Close the fork / `pipefail` / `[[ … ]]` gaps as the agent loop surfaces them | Medium |
| **3. Make it the AgenticSandbox `/bin/sh`** | Ship `rush` as the default shell of the self-contained sandbox image | Deferred |

## Recommended first step

Start with **Stage 1**, scoped exactly like the sandbox rollout — a small,
single-purpose change (likely two PRs):

1. **Vendor `rush` as a workspace library crate** — refactor the binary into
   `lib.rs` + a thin `main.rs`, add it as a workspace member.
2. **Wire the bundled shell for sandboxed sessions** — add a `bundled-shell`
   opt-in (a `ShellSpec` resolution and/or a `sandbox.toml` knob) so sandboxed
   terminal sessions run the bundled shell while the detected system shell
   remains the default everywhere else.

## Open questions

- **Vendoring mechanism.** In-tree source copy (simplest; keeps the workspace
  self-building; recommended given `Cargo.lock` is already gitignored and CI is
  the reproducible runner) vs. git submodule (keeps `rush`'s history / upstream
  link but complicates the build). Recommendation: **in-tree copy**.
- **Sequencing vs. the omp agentic loop.** The two are synergistic: the agent
  loop is the *consumer* that spawns sandboxed tool sessions, and a bundled
  shell is what those sessions would run. Decide whether to land Stage 1 first
  (so the agent loop can target it) or build the loop against the system shell
  and adopt the bundled shell later.
