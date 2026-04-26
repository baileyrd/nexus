# `nexus` — Unified CLI

One binary, multiple faces. `nexus` is the single entry point for every
non-GUI interaction with Nexus: headless CLI operations, the terminal UI,
and launching the desktop shell. This page covers the three subcommands
added by Phase 4 WI-38: `nexus tui`, `nexus desktop`, and the `nexus plugin
install|list|remove` additions. For the full command reference see
`docs/PRDs/05-cli.md`.

## `nexus tui`

Launch the ratatui-based terminal UI in the current terminal.

```
nexus tui
```

Internally this calls `nexus_tui::run_tui()` as a library function — the
`nexus-tui` crate exposes a `pub fn run_tui() -> Result<()>` entry point
that owns terminal setup/teardown via an RAII guard. No subprocess is
spawned, so Ctrl+C behaves as expected and future shared-kernel-state
optimizations become possible.

The standalone `nexus-tui` binary still works (thin wrapper around the
same library entry) for users and scripts that rely on it.

## `nexus desktop`

Launch the desktop shell (`nexus-shell`, a Tauri app). Any arguments after
`desktop` are forwarded to the shell binary unchanged, and the shell's
exit code is propagated.

```
nexus desktop                 # launch the shell
nexus desktop --my-flag foo   # passthrough args
```

**Shell-binary resolution order.** The CLI looks for `nexus-shell` in
this order — the first match wins:

1. `$NEXUS_SHELL_BIN` env var (if set and non-empty).
2. Sibling of the current executable — e.g. if `nexus` runs from
   `<prefix>/bin/nexus`, looks at `<prefix>/bin/nexus-shell`
   (`<prefix>\bin\nexus-shell.exe` on Windows).
3. `PATH` lookup.

If none match, the CLI exits with:

```
Error: Could not find `nexus-shell` binary. Set NEXUS_SHELL_BIN env var
or install the shell bundle.
```

Release packages ship both binaries side-by-side so (2) covers the
common install path without extra configuration.

Source: `crates/nexus-cli/src/commands/desktop.rs`.

## `nexus plugin` (Phase 4 additions)

Phase 4 WI-39 updates `plugin scaffold` to emit sandboxed JS/TS projects
by default (see [writing-your-first-plugin.md](./writing-your-first-plugin.md)
for the full tutorial). `plugin call|enable|disable|reset|settings|uninstall`
are unchanged. Phase 4 also adds three new capabilities:

### `nexus plugin install <plugin>`

Dispatches on the argument:

- **If `<plugin>` is an existing local directory**, the kernel loads it
  from disk (legacy behavior — unchanged from the README examples).
- **Otherwise** the argument is treated as a marketplace plugin id and
  the CLI prints the Phase 5 stub message and exits 2:

  ```
  Plugin install requires the marketplace (Phase 5 WI-44).
  See docs/archive/planning/PHASE-5-IMPLEMENTATION-PLAN.md.
  ```

  Marketplace fetch-and-unpack lands in Phase 5 WI-44.

### `nexus plugin list [--shell]`

Without `--shell`: lists kernel plugins from the forge (existing behavior).

With `--shell`: enumerates directories under `~/.nexus-shell/plugins/`
and reads each `plugin.json` manifest to surface id, name, version:

```
ID                           Name                             Version
------------------------------------------------------------------------------
community.hello-world        Hello World                      0.1.0
```

### `nexus plugin remove <id> [-y]`

Deletes the shell-plugin directory at `~/.nexus-shell/plugins/<id>/`.
Prompts for confirmation unless `-y` / `--yes` is passed. Returns an
error if no such plugin is installed.

This is distinct from `nexus plugin uninstall` (kernel plugins) — the two
live in separate worlds with different storage locations.

### `nexus plugin scaffold --template <script|core|community>` (WI-39)

Generates a new plugin project from a built-in template. The default —
`script` — emits a sandboxed JS/TS project consuming
`@nexus/extension-api`; it's the recommended path for community plugins
after Phase 3c WI-30e.

```
nexus plugin scaffold --template script --id com.example.hello --name "Hello"
```

Output layout (script):

```
com.example.hello/
├── plugin.json    # sandboxed manifest (apiVersion 1)
├── index.ts       # SandboxedPlugin source (one command + one panel)
├── package.json   # pnpm scripts + pinned @nexus/extension-api
├── tsconfig.json
└── README.md
```

`cd` into the directory, run `pnpm install && pnpm build` to produce
`index.js`, then drop `index.js` + `plugin.json` into
`~/.nexus-shell/plugins/<id>/`. See
[writing-your-first-plugin.md](./writing-your-first-plugin.md) for the
end-to-end tutorial.

The `--type` long-form is still accepted as an alias (preserves
pre-WI-39 invocations). `core` and `community` continue to emit the
legacy WASM project shape (`Cargo.toml` + `manifest.toml` +
`src/lib.rs`).

## See also

- `crates/nexus-cli/src/main.rs` — clap subcommand definitions.
- `crates/nexus-cli/src/commands/tui.rs` — TUI dispatcher.
- `crates/nexus-cli/src/commands/desktop.rs` — shell-binary resolution.
- `crates/nexus-cli/src/commands/plugin.rs` — plugin handlers.
- `docs/archive/planning/PHASE-4-IMPLEMENTATION-PLAN.md` §4.1 — the WI-38 spec.
- `docs/archive/planning/PHASE-4-IMPLEMENTATION-PLAN.md` §4.2 — the WI-39 scaffold spec.
- `docs/writing-your-first-plugin.md` — plugin-authoring tutorial.
- `docs/archive/planning/PHASE-5-IMPLEMENTATION-PLAN.md` — marketplace (WI-44) plans.
