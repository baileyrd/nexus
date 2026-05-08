# Terminal and process manager

The shell ships with a real PTY-backed **Terminal** panel — not a
fake REPL. It runs your default shell (`$SHELL`, or PowerShell on
Windows), sources your shell profile, supports ANSI color and full
keyboard control.

## Open the panel

Activity bar → **Terminal**, or palette → "Terminal: Open". You can
have multiple sessions; the session manager is in the panel header.

Keybindings inside the terminal:

| Key | Effect |
|---|---|
| `Ctrl+C` | Send SIGINT |
| `Ctrl+D` | EOF / close session if shell exits |
| `Ctrl+L` | Clear screen |
| `Ctrl+Shift+C` | Copy selection |
| `Ctrl+Shift+V` | Paste |
| `Ctrl+Shift+T` | New session |
| `Ctrl+Shift+W` | Close session |

## Saved commands

The sidebar shows **Saved Commands** — snippets you run often. Click
to insert; Shift-click to insert and execute. To save the current
input, click the **Save** button or use `Ctrl+Shift+S` in the
terminal.

CLI:

```bash
nexus term saved                          # list saved commands
nexus term run "deploy"                   # run a saved command by name
```

## Process manager

The process panel lists running child processes started by Nexus
(terminal sessions, plugin processes, agents). Each row shows PID,
command, CPU, memory, and uptime.

```bash
nexus proc list
nexus proc kill <pid>
```

`nexus proc kill` shells out to `kill` (POSIX) or `taskkill` (Windows).

## Ad-hoc command history

Each session keeps a deduplicated history with run counts. The history
picker (`Ctrl+R`) is fuzzy-searchable across all sessions.

## Pre-command runner

Some plugins inject a small **pre-command** before what you type runs
(e.g. to set environment, load nvm). Currently supported for `bash`,
`sh`, `zsh`, and `fish`. Windows `cmd` and `pwsh` support is on the
backlog.

## Shell profile

The terminal sources your normal shell profile (`~/.bashrc`,
`~/.zshrc`, etc.) by default. Disable in `.forge/app.toml`:

```toml
[terminal]
source_profile = false
```

## Memory and signals

The PTY supervisor monitors session memory; you'll get a notification
if a session goes runaway, with a one-click **Kill** action. SIGINT
and SIGQUIT propagate cleanly.

## Standalone use

The terminal is also available as the `nexus-terminal` crate if you
want to embed PTY support in a different host. See the crate README.
