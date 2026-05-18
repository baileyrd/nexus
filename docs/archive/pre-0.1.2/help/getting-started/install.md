# Install Nexus

Nexus ships as a Rust workspace plus a Tauri desktop shell. There is no
prebuilt installer yet — you build from source.

## Prerequisites

- **Rust** — stable toolchain via [rustup](https://rustup.rs)
- **Node.js ≥ 18** and **pnpm ≥ 10** (only if you want the desktop shell)
- **Linux** desktop shell only: `webkit2gtk-4.1`, `libsoup-3.0`,
  `libayatana-appindicator3-dev` (Tauri 2 dependencies)

## Build

```bash
git clone https://github.com/<your-fork>/nexus.git
cd nexus

# Build everything Rust (CLI, TUI, MCP server, all service plugins)
cargo build --workspace --release

# Optional: build the desktop shell
pnpm install
pnpm --filter nexus-shell tauri:build       # production bundle
pnpm --filter nexus-shell tauri:dev         # iterate during development
```

The release binaries land in `target/release/`:

| Binary | What it is |
|---|---|
| `nexus` | The CLI — does everything from the terminal |
| `nexus-tui` | A keyboard-driven terminal UI |
| `nexus-mcp` | An MCP server (stdio) for Claude Code / Cursor |
| `nexus-shell` (in `shell/src-tauri/target/release/`) | Tauri desktop GUI |

Put `target/release/` on your `PATH`, or symlink the binaries you use.

## Verify

```bash
nexus --version
nexus forge init ~/notes
nexus content list --forge-path ~/notes
```

If the last command prints an empty list (or your existing markdown
files), you're ready. Continue to [Create your first forge](first-forge.md).

## Troubleshooting

- **`webkit2gtk` missing on Linux** — install your distro's `webkit2gtk-4.1`
  development package. Tauri 2 specifically requires the **4.1** ABI, not
  the older 4.0.
- **`pnpm: command not found`** — `corepack enable && corepack prepare pnpm@latest --activate`.
- **Keyring errors at startup** — set `NEXUS_NO_KEYRING=1` to fall back to
  plaintext config storage. See
  [ADR 0009](../../adr/0009-keyring-hard-fail-policy.md) for the trade-off.
