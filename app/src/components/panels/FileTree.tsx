import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type MouseEvent as ReactMouseEvent,
} from "react";
import { ChevronDown, ChevronRight, File, Folder } from "lucide-react";
import { ContextMenu, type ContextMenuItem } from "../ContextMenu";
import { contributions } from "../../contributions";
import {
  createForgeDir,
  createForgeFile,
  deleteForgeEntry,
  listForgeDir,
  renameForgeEntry,
  type ForgeDirEntry,
} from "../../ipc/forge";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import { useOpenFilesStore } from "../../stores/openFiles";
import { useLayoutStore } from "../../stores/layout";
import { usePaletteStore } from "../../stores/palette";

/**
 * File-tree renderer for panels with `contentType = "files"`. Lists the
 * active forge root, lazily loading subdirectories when expanded.
 * Right-click rows or the empty area for create / rename / delete.
 */
export function FileTree() {
  const forge = useForgeStore((s) => s.info);

  if (!forge) {
    return (
      <div className="file-tree-empty" role="status">
        No forge open.
      </div>
    );
  }

  return <FileTreeForForge key={forge.root} />;
}

/**
 * Filter pill for the Forge file panel. The palette is Nexus's real
 * file-finder (fuzzy, cross-forge, command-aware), so this button
 * opens the palette rather than maintaining a separate local filter —
 * keeps the UI aligned with the `⌘P` hint in the design.
 */
function FileTreeFilter() {
  const openPalette = usePaletteStore((s) => s.openPalette);
  return (
    <button
      type="button"
      className="file-tree-filter"
      onClick={() => openPalette()}
      aria-label="Filter files (opens command palette)"
    >
      <span className="file-tree-filter-label">Filter files…</span>
      <span className="file-tree-filter-kbd" aria-hidden="true">⌘P</span>
    </button>
  );
}

type RequestMenuFn = (
  target: ForgeDirEntry | null,
  x: number,
  y: number,
) => void;

const MenuContext = createContext<RequestMenuFn | null>(null);

function FileTreeForForge() {
  const [rootEntries, setRootEntries] = useState<ForgeDirEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [menu, setMenu] = useState<{
    x: number;
    y: number;
    target: ForgeDirEntry | null;
  } | null>(null);
  const fsVersion = useForgeStore((s) => s.fsVersion);
  const openAction = useCallback(async (relpath: string) => {
    // Name = last path segment; used as the tab label.
    const name = relpath.slice(relpath.lastIndexOf("/") + 1) || relpath;
    await useOpenFilesStore.getState().open(relpath);
    useLayoutStore.getState().openTabForFile(relpath, name);
  }, []);
  const closeFile = useOpenFileStore((s) => s.close);

  useEffect(() => {
    let cancelled = false;
    listForgeDir("")
      .then((entries) => {
        if (!cancelled) setRootEntries(entries);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [fsVersion]);

  const requestMenu = useCallback<RequestMenuFn>((target, x, y) => {
    setMenu({ target, x, y });
  }, []);

  const onRootContextMenu = useCallback(
    (e: ReactMouseEvent<HTMLDivElement>) => {
      // Only trigger when the click landed on the container itself, not
      // a row that already handles its own context menu.
      if (e.target === e.currentTarget) {
        e.preventDefault();
        requestMenu(null, e.clientX, e.clientY);
      }
    },
    [requestMenu],
  );

  const items = menu ? buildMenuItems(menu.target, openAction, closeFile) : [];

  return (
    <MenuContext.Provider value={requestMenu}>
      <div className="file-tree-root" onContextMenu={onRootContextMenu}>
        <FileTreeFilter />
        {error && (
          <div className="file-tree-error">Failed to list forge: {error}</div>
        )}
        {!error && !rootEntries && (
          <div className="file-tree-loading">loading…</div>
        )}
        {rootEntries?.length === 0 && (
          <div className="file-tree-empty">Forge is empty.</div>
        )}
        {rootEntries && rootEntries.length > 0 && (
          <ul className="file-tree" role="tree">
            {rootEntries.map((entry) => (
              <TreeNode key={entry.relpath} entry={entry} depth={0} />
            ))}
          </ul>
        )}
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={items}
          onClose={() => setMenu(null)}
        />
      )}
    </MenuContext.Provider>
  );
}

interface TreeNodeProps {
  entry: ForgeDirEntry;
  depth: number;
}

function TreeNode({ entry, depth }: TreeNodeProps) {
  const [children, setChildren] = useState<ForgeDirEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const openFile = useCallback(async (relpath: string) => {
    const name = relpath.slice(relpath.lastIndexOf("/") + 1) || relpath;
    await useOpenFilesStore.getState().open(relpath);
    useLayoutStore.getState().openTabForFile(relpath, name);
  }, []);
  const openRelpath = useOpenFileStore((s) => s.file?.relpath);
  const fsVersion = useForgeStore((s) => s.fsVersion);
  const expanded = useForgeStore((s) => s.expandedPaths.has(entry.relpath));
  const setExpanded = useForgeStore((s) => s.setExpanded);
  const requestMenu = useContext(MenuContext);

  useEffect(() => {
    if (!entry.isDir || !expanded) return;
    let cancelled = false;
    setError(null);
    listForgeDir(entry.relpath)
      .then((next) => {
        if (!cancelled) setChildren(next);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [entry.isDir, entry.relpath, expanded, fsVersion]);

  const onToggle = useCallback(() => {
    if (!entry.isDir) return;
    // `.bases` directories are databases, not folders — open them in
    // a base-view tab instead of expanding the tree. The renderer
    // reads schema.json / records.json / views.toml through the
    // `load_forge_base` Tauri command.
    if (entry.name.endsWith(".bases")) {
      useLayoutStore
        .getState()
        .openContentTab(`base-file:${entry.relpath}`, entry.name, "database");
      return;
    }
    setExpanded(entry.relpath, !expanded);
  }, [entry.isDir, entry.name, entry.relpath, expanded, setExpanded]);

  const onContextMenu = useCallback(
    (e: ReactMouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      requestMenu?.(entry, e.clientX, e.clientY);
    },
    [entry, requestMenu],
  );

  const indent = { paddingInlineStart: `${depth * 12 + 4}px` } as const;

  if (!entry.isDir) {
    const active = openRelpath === entry.relpath;
    return (
      <li
        role="treeitem"
        aria-selected={active}
        className="file-tree-row is-file"
      >
        <button
          type="button"
          className={active ? "file-tree-file is-active" : "file-tree-file"}
          style={indent}
          onClick={() => void openFile(entry.relpath)}
          onContextMenu={onContextMenu}
        >
          <span className="file-tree-twisty" aria-hidden="true" />
          <File size={14} className="file-tree-icon" aria-hidden="true" />
          <span className="file-tree-name">{entry.name}</span>
        </button>
      </li>
    );
  }

  return (
    <li
      role="treeitem"
      aria-expanded={expanded}
      className="file-tree-row is-dir"
    >
      <button
        type="button"
        className="file-tree-toggle"
        onClick={onToggle}
        onContextMenu={onContextMenu}
        style={indent}
      >
        {expanded ? (
          <ChevronDown
            size={12}
            className="file-tree-twisty"
            aria-hidden="true"
          />
        ) : (
          <ChevronRight
            size={12}
            className="file-tree-twisty"
            aria-hidden="true"
          />
        )}
        <Folder size={14} className="file-tree-icon" aria-hidden="true" />
        <span className="file-tree-name">{entry.name}</span>
      </button>
      {expanded && (
        <ul role="group" className="file-tree-children">
          {error && <li className="file-tree-error">{error}</li>}
          {children === null && !error && (
            <li
              className="file-tree-loading"
              style={{ paddingInlineStart: `${(depth + 1) * 12 + 4}px` }}
            >
              loading…
            </li>
          )}
          {children?.map((child) => (
            <TreeNode key={child.relpath} entry={child} depth={depth + 1} />
          ))}
          {children?.length === 0 && (
            <li
              className="file-tree-empty"
              style={{ paddingInlineStart: `${(depth + 1) * 12 + 4}px` }}
            >
              (empty)
            </li>
          )}
        </ul>
      )}
    </li>
  );
}

// ── Menu actions ──────────────────────────────────────────────────────────

/** Parent directory of `relpath`, or "" for top-level entries. */
function dirname(relpath: string): string {
  const i = relpath.lastIndexOf("/");
  return i === -1 ? "" : relpath.slice(0, i);
}

/** Join a parent relpath and a filename, skipping the separator at root. */
function joinRel(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name;
}

function buildMenuItems(
  target: ForgeDirEntry | null,
  openFile: (relpath: string) => Promise<void>,
  closeFile: () => void,
): ContextMenuItem[] {
  const openRelpath = useOpenFileStore.getState().file?.relpath ?? null;
  // The directory new entries should land in: the target itself if a
  // folder, the target's parent if a file, the root if no target.
  const parentDir =
    target === null ? "" : target.isDir ? target.relpath : dirname(target.relpath);

  const items: ContextMenuItem[] = [
    {
      id: "new-file",
      label: "New file",
      onSelect: async () => {
        const name = window.prompt("File name:", "untitled.md");
        if (!name) return;
        try {
          const rel = joinRel(parentDir, name);
          await createForgeFile(rel);
          await openFile(rel);
        } catch (e) {
          window.alert(`Failed to create file: ${e}`);
        }
      },
    },
    {
      id: "new-folder",
      label: "New folder",
      onSelect: async () => {
        const name = window.prompt("Folder name:", "");
        if (!name) return;
        try {
          await createForgeDir(joinRel(parentDir, name));
        } catch (e) {
          window.alert(`Failed to create folder: ${e}`);
        }
      },
    },
  ];

  if (target) {
    items.push({
      id: "rename",
      label: "Rename…",
      separatorBefore: true,
      onSelect: async () => {
        const next = window.prompt("Rename to:", target.name);
        if (!next || next === target.name) return;
        try {
          const dst = joinRel(dirname(target.relpath), next);
          await renameForgeEntry(target.relpath, dst);
        } catch (e) {
          window.alert(`Failed to rename: ${e}`);
        }
      },
    });
    items.push({
      id: "delete",
      label: target.isDir ? "Delete folder…" : "Delete file…",
      onSelect: async () => {
        const ok = window.confirm(
          target.isDir
            ? `Delete folder "${target.name}" and everything inside? This cannot be undone.`
            : `Delete file "${target.name}"? This cannot be undone.`,
        );
        if (!ok) return;
        try {
          await deleteForgeEntry(target.relpath);
          // Close the viewer eagerly only if the deleted entry was (or
          // contained) the open file; the watcher's refresh handles
          // any other staleness.
          if (
            openRelpath !== null &&
            (openRelpath === target.relpath ||
              openRelpath.startsWith(`${target.relpath}/`))
          ) {
            closeFile();
          }
        } catch (e) {
          window.alert(`Failed to delete: ${e}`);
        }
      },
    });
  }

  // Append plugin-contributed context menu items for the appropriate scope.
  // Scopes: "file-tree:file", "file-tree:directory", "file-tree:root".
  const scope =
    target === null
      ? "file-tree:root"
      : target.isDir
        ? "file-tree:directory"
        : "file-tree:file";
  const pluginItems = contributions.listContextMenuItems(scope);
  if (pluginItems.length > 0) {
    // Mark the first plugin item with separatorBefore if it doesn't already
    // have one, so there's a visual break between built-in and plugin items.
    const [first, ...rest] = pluginItems;
    items.push({ ...first, separatorBefore: first.separatorBefore ?? true }, ...rest);
  }

  return items;
}
