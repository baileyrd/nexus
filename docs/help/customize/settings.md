# Settings

Nexus settings live in three places: per-forge config files in
`.forge/`, per-plugin settings stored alongside each plugin, and a
small set of process-level environment variables.

## Settings UI

In the shell, **Settings** opens a panel with sections for every
contributing plugin. Each section is auto-generated from the plugin's
declared JSON schema, so:

- New plugins automatically get a settings UI.
- Changing a value writes back to the right config file.
- Validation is live (typing an invalid value shows a hint).

## Per-forge config files

| File | Owner | What's in it |
|---|---|---|
| `.forge/app.toml` | Core | Editor, panels, search, graph, terminal defaults |
| `.forge/ai.toml` | `com.nexus.ai` | Provider keys, models, embeddings |
| `.forge/mcp.toml` | `com.nexus.mcp` | Registered MCP servers |
| `.forge/workspace.json` | Shell | Open tabs, layout, view state |
| `.forge/plugins/<id>/settings.json` | per-plugin | Plugin-specific config |

All TOML/JSON files do `${ENV_VAR}` substitution before parsing, so
you can keep secrets out of the file:

```toml
api_key = "${ANTHROPIC_API_KEY}"
```

## CLI

```bash
nexus config list                          # all keys
nexus config get editor.fontSize
nexus config set editor.fontSize 14
nexus config set editor.fontSize 14 --plugin com.nexus.editor
```

Same effect as editing the file by hand, but with validation.

## Environment variables

| Variable | Purpose | Default |
|---|---|---|
| `NEXUS_FORGE_PATH` | Forge root directory | `~/.nexus/default` |
| `NEXUS_CONFIG` | Override config file path | unset |
| `NEXUS_SHELL_BIN` | Path to `nexus-shell` binary (for `nexus desktop`) | unset |
| `NEXUS_SAFE_MODE` | Skip community plugins (0 or 1) | 0 |
| `NEXUS_NO_KEYRING` | Use plaintext API keys instead of OS keyring | unset |
| `RUST_LOG` | Tracing filter (`warn`, `debug`, `trace`) | `warn` |

## Reset

To wipe a single plugin's settings:

```bash
nexus plugin reset com.example.foo
```

To reset the entire shell layout:

```bash
rm <forge>/.forge/workspace.json
```

To start fresh while keeping notes — delete `.forge/` and run
`nexus forge reindex`.

## Per-machine vs. per-forge

Settings are **per-forge** by design — different forges (work,
personal, project) have different needs. There is no global config
beyond environment variables. If you want to share settings across
forges, symlink `.forge/app.toml` to a common file.
