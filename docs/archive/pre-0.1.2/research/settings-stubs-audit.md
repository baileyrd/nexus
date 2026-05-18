# Settings stubs audit — what overlaps with real Nexus plugins

Snapshot date: 2026-05-04. Locates every "Coming soon" stub in the
settings panel and maps it to a real Nexus plugin where one exists.
Goal: identify which stubs are duplicates we can delete, which need
real config schemas added, and which are genuine placeholders.

Source: `shell/src/plugins/core/settings/SettingsPanelView.tsx`.

## 1. Stubs in the "Core plugins" rail group

These are the ten Obsidian-parity stub entries (`cp-stub:*` tab ids).

| Stub rail entry | Real Nexus plugin | Has config schema today? | Verdict |
|---|---|---|---|
| `cp-stub:backlinks` | `nexus.backlinks` | No | Overlap — give the real plugin a minimal schema and delete the stub, OR delete the stub and accept no settings page for backlinks |
| `cp-stub:canvas` | `nexus.canvas` | **Yes** (already in rail) | **Duplicate** — delete the stub |
| `cp-stub:command-palette` | `nexus.commandPalette` | **Yes** (already in rail) | **Duplicate** — delete the stub |
| `cp-stub:daily-notes` | none | n/a | Genuine stub — leave |
| `cp-stub:file-recovery` | none | n/a | Genuine stub — leave |
| `cp-stub:note-composer` | none | n/a | Genuine stub — leave |
| `cp-stub:page-preview` | `nexus.linkpreview` only loosely (URL OG-fetch, not internal link hover) | No | Genuine stub — leave |
| `cp-stub:quick-switcher` | none (functionally part of command palette + files) | n/a | Genuine stub — leave |
| `cp-stub:sync` | none | n/a | Genuine stub — leave |
| `cp-stub:templates` | `nexus.templates` | No | Overlap — same call as backlinks |

## 2. Real plugins that *should* surface in the rail but don't

These plugins exist but ship without a `configuration:` block so the
auto-populated rail skips them. Adding even an empty schema would
make them rail-visible under "Core plugins".

```
nexus.bases             nexus.bookmarks         nexus.outline
nexus.outgoingLinks     nexus.tags              nexus.fileProperties
nexus.allProperties     nexus.activityTimeline  nexus.workspace
nexus.git               nexus.workflow          nexus.mcp
nexus.skills            nexus.notion            nexus.semanticSearch
nexus.processes         nexus.comments          nexus.graph
nexus.formats           nexus.statusBar
```

Plugins that already have schemas and *are* in the rail:

```
nexus.ai            nexus.canvas        nexus.commandPalette
nexus.editor        nexus.enrich        nexus.linkSuggest
nexus.memory        nexus.recall        nexus.search
nexus.terminal
```

## 3. Stubs on the General page

| Stub | Real Nexus mapping | Verdict |
|---|---|---|
| Automatic updates | none | Leave |
| Language | none (no i18n yet) | Leave |
| **Help → Open button** | **`workbench.action.openHelp`** already exists and opens the GitHub repo | **Wire it up** — one-line fix replacing the toast with `api.commands.execute('workbench.action.openHelp')` |
| Notify if startup takes longer than expected | none | Leave |
| Command line interface | `nexus` CLI exists but a desktop on/off toggle doesn't apply | Leave |

## 4. Editor / Files-and-links / Appearance pages

These pages render hardcoded Obsidian-shaped rows (~22 in Editor,
~14 in Files and links, plus the Interface/Font/Advanced sections in
Appearance). The corresponding real plugins (`nexus.editor`,
`nexus.files`, the theme + snippets stack) each have their own
configuration schemas and rail entries elsewhere — so the stubs
duplicate-conceptually but do not share state.

Three options for reconciling:

1. **Delete the stubs**; rely on the existing per-plugin rail entries
   under Core plugins. Loses Obsidian shape, gains accuracy.
2. **Replace stubs with composed real schemas** — the Editor page would
   pull from `nexus.editor` (plus any `editor.*` keys from related
   plugins) and render them in Obsidian's row order. Highest fidelity,
   most code.
3. **Leave the stubs**, accept the duplication. Lowest effort.

No clear right answer yet; this should probably be its own design
pass when there's appetite for it.

## 5. Recommended action order

1. **Wire General → Help button** to `workbench.action.openHelp`
   (one-line edit; trivially wins).
2. **Delete `cp-stub:canvas` and `cp-stub:command-palette`** — they
   duplicate real rail entries.
3. **Decide** on backlinks / templates: either give the real plugins
   minimal config schemas so they land in the rail, or delete the stubs
   and accept no settings page for those features yet.
4. **Defer** the Editor / Files-and-links / Appearance reconciliation
   — bigger design question, treat as a follow-up.

## 6. References

- Stub source: `shell/src/plugins/core/settings/SettingsPanelView.tsx`
  (`STUB_CORE_PLUGINS`, `GeneralTab`, `EditorOptionsTab`,
  `FilesLinksTab`, `KeychainTab`)
- Real configuration schemas: each plugin's `index.ts` `manifest.contributes.configuration`
- Help command wiring: `shell/src/plugins/core/settings/index.ts`
  (`workbench.action.openHelp`)
