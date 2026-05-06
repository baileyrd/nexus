# CommandBook — Application Evaluation
_Evaluated: 2026-05-06 · https://commandbookapp.com_

## What it is

A native macOS (SwiftUI/Swift 6) process manager for long-running terminal commands. Not a terminal
emulator — closer to a persistent supervisor UI with a clean command catalog.

---

## Application model

The core insight is simple and well-executed: **commands are first-class objects**, not transient
terminal sessions. A "command" is a named record with a working directory, environment variables,
pre-commands, icon, and auto-restart policy — stored in a SQLite database shared by both GUI and
CLI. The process runs against it; the record persists regardless of what the process does.

This is meaningfully different from a terminal emulator. When you quit the app, processes get
SIGTERM/SIGKILL and the *records* survive. When you reopen, you see the same sidebar with the same
commands, ready to start. Output *does not* persist across restarts — that's a deliberate tradeoff
(buffer memory vs. disk).

---

## UI structure

**Three-zone layout:**

**Sidebar (left)** — The command list. Each item shows name, status (running/stopped/crashed), hover
buttons for start/stop/restart/dismiss, and memory usage. Drag-to-reorder. Separators act as visual
group headers (named dividers, not true groups). Header shows aggregate running process count + total
memory.

**Output pane (main)** — Per-command stdout/stderr. Full ANSI color rendering (24-bit true color).
URL chips pinned at the top (up to 5 auto-detected links, always visible even after scrolling).
Search bar (⌘F) with regex + case-sensitive support, match count, forward/back navigation.
Right-click context for clear/copy/copy-all. Memory usage shown in the footer.

**Command palette (⌘K)** — The primary interaction surface. Search across saved commands, run
ad-hoc commands, create/edit/duplicate/delete. Keyboard-navigable entirely. Right-click on any
palette item for the full context menu.

**Design principles visible in the UI:**
- Keyboard-first: sidebar has default focus, arrow keys navigate, Space starts/stops, the whole app
  is usable without a mouse.
- No modal dialogs for process actions — everything through the palette or hover buttons.
- Status is ambient: sidebar item color/icon communicates running/stopped/crashed without opening
  anything.
- The URL extraction pin (top of output pane) is the most distinctive UI idea — it solves "I need
  to click that localhost:3000 link but it scrolled past" without any user action.

---

## Feature breakdown

| Feature | Quality | Notes |
|---|---|---|
| Command persistence | Strong | SQLite, CLI+GUI shared, survives restarts |
| Auto-restart | Good | Configurable 1–60s delay, cancel countdown, non-zero-exit only |
| Output buffering | Good | 25K/500K lines, in-memory only (no disk persistence) |
| URL pin extraction | Distinctive | Top-5 URLs per command, always visible, single-click |
| .env file support | Thorough | Import, drag-drop, 4-file cascade (`.env`, `.env.local`, etc.) |
| Pre-commands | Useful | Run before main command each start; non-blocking on failure |
| Icon auto-detection | Nice touch | 60+ tech icons, auto-detect from command text |
| ANSI color | Complete | 8/256/24-bit true color + bold/dim/italic/underline |
| Keyboard shortcuts | Complete | Full set; ⌘. for stop, ⌘R for restart, ⌃C for SIGINT |
| CLI integration | Strong | Shared DB; `run`, `list`, `new`, `edit`, `open` |
| Signal sending | Good | SIGINT, SIGTERM, SIGKILL, SIGHUP, EOF all mapped to shortcuts |
| Cross-command search | Missing | Search is per-command only |
| Output persistence | Missing by design | Clears on restart; mitigated by 500K line buffer |
| Sync across machines | Not yet | Manual export/import only |

---

## Keyboard shortcuts reference

| Shortcut | Action |
|---|---|
| ⌘K | Open command palette |
| ⌘N | Create new command |
| ⌘E | Edit selected command |
| ⌘, | Settings |
| ⌘. | Stop (SIGTERM) |
| ⌘⇧. | Force stop (SIGKILL) |
| ⌘R | Restart |
| ⌃C | SIGINT |
| ⌃D | EOF / SIGHUP |
| ⌘F | Search output |
| ⌘G / ⌘⇧G | Next / previous match |
| ⌘⇧C | Copy all output |
| Space | Start/stop sidebar item |
| ⌘⌫ | Delete command (palette) |

---

## What it deliberately is not

The constraints are as revealing as the features:

**No TTY/PTY.** `vim`, `htop`, `less`, interactive REPLs — none work. The "Run in Terminal" escape
hatch (opens the command in iTerm2/Warp/Ghostty/Kitty/Alacritty) is the designed answer. This is an
honest tradeoff: supporting PTY would make the app significantly more complex and most long-running
background services don't need it.

**No subscription, no account.** $14.99 one-time. The free tier is genuinely functional (5 saved
commands, 10 ad-hoc, 25K output lines — not crippled). The pitch is "less than 15 minutes of
developer time."

**Not sandboxed.** Explicitly disabled so it can spawn arbitrary processes. Notarized/signed but
not sandboxed — macOS requires the entitlement to spawn processes outside the app's container.

---

## Pricing

| Tier | Price | Limits |
|---|---|---|
| Free | $0 forever | 5 saved, 10 ad-hoc, 25K output lines |
| Full | $14.99 one-time | Unlimited saved + ad-hoc, 500K output lines, separators |

No account required. License is email-tied at purchase. Updates included within major version.

---

## Technical profile

- **Language:** Swift 6 + SwiftUI — no Electron, no web technologies
- **Binary size:** 21 MB
- **Runtime memory:** ~73 MB base; 100–300 MB total with active buffers
- **Storage:** SQLite at `~/Library/Application Support/CommandBook/commandbook.sqlite`
- **Platform:** macOS 15+ only; universal binary (Apple Silicon + Intel); Windows planned
- **Distribution:** Direct download + notarized; not on the Mac App Store

---

## Relevance to Nexus

The `nexus-terminal` service (PRD-09) handles process management for the Nexus shell. CommandBook
is essentially a polished standalone version of what that service does. Several patterns are worth
carrying forward.

### What CommandBook gets right that Nexus should adopt

**URL pin extraction.** Pinning the top 5 auto-detected URLs above the output pane is genuinely
useful and costs almost nothing to implement. Solves "I need to click that localhost:3000 link but
it scrolled past" without any user action. High value, low complexity.

**Hover buttons on sidebar items.** Start/stop/restart without opening a context menu. Reduces
friction for the most common actions on the most-used element of the UI. The Nexus shell's sidebar
items should have the same pattern for terminal processes.

**Memory + process count header.** Aggregate "X proc, Y GB" in the sidebar header is low-cost,
high-signal ambient status. A similar footer or header in the terminal panel would be worthwhile.

**Command palette as the primary process interaction surface.** CommandBook shows how far a
palette-first UX can go — create, edit, duplicate, delete, run ad-hoc, search, all in one place.
The Nexus shell has palette infrastructure; this depth of process management belongs there.

**The record/process distinction.** The most important conceptual contribution: *the command record
is permanent; the process running against it is ephemeral.* This mental model should be explicit
in Nexus's terminal abstraction (`nexus-terminal`). If a command config and a running process are
conflated in the data model, the UX degrades into what terminal emulators already do.

### Where Nexus is already ahead or will be

**Cross-platform.** CommandBook is macOS-only (Windows planned, Linux not). Nexus targets all
platforms by design.

**Composable via IPC.** The terminal service routes through `com.nexus.terminal` IPC — available to
CLI, TUI, MCP, shell, and agents uniformly. CommandBook's CLI is a thin wrapper around a single
app's database; it doesn't compose with other subsystems.

**Workflow automation.** `com.nexus.workflow` can trigger process starts via cron or file events
without any additional app. CommandBook has no automation layer.

**Forge as config store.** Nexus's markdown-as-truth model means process configs could live as
versioned files in the forge, diff-friendly and review-friendly. CommandBook's SQLite database is
opaque to version control.

**AI integration.** CommandBook has none. Nexus's agent system can plan and execute multi-step
workflows that include terminal commands as IPC steps.

### One gap worth addressing (BL candidate)

CommandBook's "Run in Terminal" escape hatch — routing a command to an external terminal emulator
when TTY/interactivity is needed — is a clean pattern that Nexus should explicitly support.
`nexus-terminal` likely needs a corresponding "open in terminal" action that passes the command's
working directory and environment to an external emulator, maintaining the forge's record of the
command while delegating execution when PTY is required.
