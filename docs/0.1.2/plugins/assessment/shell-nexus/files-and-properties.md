# Files and Properties

This category covers the file-tree UI and the right-side metadata inspectors that
read frontmatter off the active note. `nexus.files` is the real markdown tree the
user navigates (the shell-core `core.file-explorer` plugin is a thin
command/configuration shim with no live tree). `nexus.fileProperties` and
`nexus.allProperties` are two near-identical readers of `read_frontmatter` IPC,
differing only in chrome. `nexus.bookmarks` is a list of pinned relpaths stored
in shell config.

### nexus.files

- **Path:** `shell/src/plugins/nexus/files/`
- **Surface:** Registers the `file-explorer` view type (sidebar tree), commands
  `nexus.files.focus`, `nexus.files.create.file`, `nexus.files.create.folder`,
  `nexus.files.rename`, `nexus.files.delete`, `nexus.files.reveal`,
  `nexus.files.copyPath`; keybindings Del/F2 gated on
  `nexus.files.focused`; context key `nexus.files.focused`. Subscribes to
  `com.nexus.storage.file_{created,modified,deleted,renamed}` for live
  refresh, invalidates `nexus.status` per-path cache on storage events,
  and emits `files:open` to drive the editor.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.sidebar`;
  backend `com.nexus.storage` (`list_dir`, `create_file`, `create_dir`,
  `rename`, `delete`); host `api.platform.shell.openExternal` for OS reveal.
- **Verdict:** Essential
- **Rationale:** This is the only navigable markdown tree in the shell — the
  shell-core `core.file-explorer` plugin contributes only commands and config
  schema, not a tree view. Removing this leaves the user with no way to
  browse the forge.

### nexus.fileProperties

- **Path:** `shell/src/plugins/nexus/fileProperties/`
- **Surface:** Registers `file-properties` view (right rail), command
  `nexus.fileProperties.focus`. Renders a name/path/type/size/timestamps
  table merged with frontmatter `title`/`tags`/`status` and other fields
  for the active editor tab.
- **Depends on:** `nexus.editor` (`useEditorStore.activeRelpath`), `nexus.files`
  (`getKernel` import); backend `com.nexus.storage::read_frontmatter` and
  `query_files`.
- **Verdict:** Optional
- **Rationale:** Adds a read-only inspector pane; valuable to power users who
  curate metadata but irrelevant to the basic browse/edit workflow.

### nexus.allProperties

- **Path:** `shell/src/plugins/nexus/allProperties/`
- **Surface:** Registers `all-properties` view (right rail), command
  `nexus.allProperties.focus`. Dumps every frontmatter `status` + `fields`
  entry for the active note in a flat table.
- **Depends on:** `nexus.editor` (`useEditorStore.activeRelpath`), `nexus.files`
  (`getKernel`); backend `com.nexus.storage::read_frontmatter`.
- **Verdict:** Optional
- **Rationale:** A simpler, less curated variant of `nexus.fileProperties` —
  same IPC call, same data, different presentation. Two of these in the
  tree is itself a smell; one would suffice. Neither is required for
  basic markdown editing.

### nexus.bookmarks

- **Path:** `shell/src/plugins/nexus/bookmarks/`
- **Surface:** Registers `bookmarks` view (right rail), commands
  `nexus.bookmarks.focus` and `nexus.bookmarks.toggleActive`. Persists
  the bookmark list to `useConfigStore` under
  `nexus.bookmarks.entries`; emits `files:open` when a bookmark row is
  clicked.
- **Depends on:** `nexus.editor` (active relpath), shell `configStore`,
  shell `events` bus. No backend IPC.
- **Verdict:** Optional
- **Rationale:** Convenience feature for jumping to pinned notes; not on the
  basic path. The persistence layer is shell config (not the forge), so a
  user moving forges loses the list silently — a minor wart.

## Category verdict

| Plugin              | Verdict   | Required for basic workflow |
|---------------------|-----------|-----------------------------|
| `nexus.files`         | Essential | Yes — only working file tree |
| `nexus.fileProperties`| Optional  | No                          |
| `nexus.allProperties` | Optional  | No — overlaps with `fileProperties` |
| `nexus.bookmarks`     | Optional  | No                          |
