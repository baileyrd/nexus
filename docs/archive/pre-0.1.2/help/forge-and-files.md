# Forge layout, files, and folders

A forge is a directory of markdown files plus a hidden `.forge/`
subdirectory that stores the index and configuration. Everything you
see in Nexus is computed from those files; if a file isn't on disk, it
doesn't exist in Nexus.

## File-as-truth

The first rule of Nexus is that **markdown files on disk are the source
of truth**. The SQLite index, the Tantivy search index, and the
in-memory graph are derived. You can:

- Edit files in any other editor and Nexus will pick the changes up
  (a filesystem watcher debounces and reindexes within ~300 ms).
- Delete the entire `.forge/` directory and rebuild it with
  `nexus forge reindex`.
- Sync the forge with `git`, `rsync`, Dropbox, Syncthing, etc. — Nexus
  does not lock files.

## Creating notes

In the shell: `+` button in the file tree, drag-and-drop a file in, or
the **Create note** command in the palette.

CLI:

```bash
nexus content create projects/launch.md --content "# Launch plan"
nexus content create daily/2026-05-03.md
```

Notes can live in any folder. Filenames are paths relative to the forge
root.

## Renaming and moving

Rename a file in your file system or in the shell — Nexus updates the
index. Wikilinks are resolved by filename, so renaming `Foo.md` to
`Bar.md` will leave `[[Foo]]` references **unresolved** unless you
update them. (Auto-rename of inbound wikilinks is on the backlog.)

CLI: there is no `content rename` subcommand — `mv` the file at the
filesystem and the watcher reindexes on the next tick. The shell
**File → Rename** menu does the same thing, plus prompts to update
inbound wikilinks.

```bash
mv foo.md bar.md            # watcher picks it up; inbound [[foo]] still breaks
nexus content read bar.md   # confirm the index re-resolved
```

## Deleting

Delete the file. Nexus removes it from the index on the next watcher
tick. CLI: `nexus content delete foo.md`.

## Folders

Folders are just folders. They have no special meaning to Nexus — no
metadata, no settings, no per-folder config. Create them however you
like. The file tree in the shell mirrors the on-disk structure.

You can use folders to scope wikilink resolution: `[[Notes/Foo]]`
resolves to `Notes/Foo.md`.

## Attachments

Any non-markdown file in the forge (images, PDFs, audio, videos) is an
attachment. Embed them with `![[image.png]]` in markdown. Nexus serves
them in the editor preview and tracks them in the index.

## The `.forge/` directory

```
.forge/
├── index.db          SQLite — file tree, blocks, links, tags, properties
├── search/           Tantivy full-text search
├── app.toml          Core settings (editor, panels, search limits)
├── ai.toml           AI provider config
├── mcp.toml          Registered MCP servers
├── workspace.json    UI state (open tabs, layout)
├── kv.sqlite3        Per-plugin key-value storage
├── chat/sessions/    AI chat history (one JSON per session)
├── skills/           Your prompt templates (.skill.md)
├── agents/           Agent run history
├── logs/             ai-activity.jsonl, plugin-events.jsonl, etc.
└── temp/             Atomic-write staging
```

The `.forge/` directory is **not portable across machines** in the
strict sense — paths and timestamps differ — but it's safe to delete and
rebuild at any time. Add `.forge/` to your `.gitignore` if you sync the
forge with git.

## Multiple forges

Forges are independent. Switch with `--forge-path`, `NEXUS_FORGE_PATH`,
or the **File** menu in the shell. Each forge has its own index, AI
config, plugins, and workspace state.
