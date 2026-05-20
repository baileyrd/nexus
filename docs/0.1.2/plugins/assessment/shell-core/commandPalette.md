# commandPalette (core)

- **Path:** `shell/src/plugins/core/commandPalette/`
- **Tier:** Shell Core
- **Plugin id:** `core.command-palette`

## Architecture
- Entry point: `shell/src/plugins/core/commandPalette/index.ts:10`
- Activation: `onStartup`
- Modules:
  - `index.ts` — manifest + activate hook
  - `CommandPaletteView.tsx` — overlay UI (fuzzy filter list)
- Persistence: none; visibility tracked through the plugin context key `commandPaletteVisible`
- Settings owned: `commandPalette.maxResultsLimit` (default imported from `../../nexus/commandPalette/match`)
- External deps: imports `DEFAULT_MAX_PALETTE_RESULTS` from the sibling `nexus.commandPalette` plugin

## Surface
- **Commands:** `workbench.action.showCommandPalette` (category `View`)
- **Keybindings:** `ctrl+shift+p` / `cmd+shift+p`
- **Views:** `commandPalette` registered into the `overlay` slot at priority 100
- **Settings schema:** one entry — `commandPalette.maxResultsLimit` (number)
- **Context keys set:** `commandPaletteVisible`
- **Consumes from `@nexus/extension-api`:** `Plugin`, `PluginAPI` types only

## Necessity
- **Verdict:** Useful (the role); this implementation is superseded
- **Required for basic capabilities?** No — files can be opened from the file tree and editing works without it; many users rely on it but core editing is fine without.
- **Depended on by (this exact plugin):** nothing; `catalog.ts` does not import it
- **Depends on:** `nexus.commandPalette` (imports `DEFAULT_MAX_PALETTE_RESULTS`); the command registry + plugin context store provided by the extension host
- **What breaks if removed:** Nothing in the live shell — the loaded palette is `nexus.commandPalette` (`shell/src/plugins/catalog.ts:273`). Removing this directory only removes a dormant duplicate.

## Notes
- **Dead code in 0.1.2.** The catalog wires up `nexus.commandPalette`, not `core.command-palette`. The two manifests register overlapping commands/keybindings; only the active one runs.
- Awkward upward dep: `core/commandPalette` imports a constant from `nexus/commandPalette` — keeping it makes the layering inverted (core depending on nexus).
- Cleanup candidate: delete after confirming no on-disk `plugins.enabled` value references `core.command-palette`.
