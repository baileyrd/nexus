import { useEffect, useState, useCallback } from "react";
import { ChevronDown, ChevronRight, File, Folder } from "lucide-react";
import { listForgeDir, type ForgeDirEntry } from "../../ipc/forge";
import { useForgeStore } from "../../stores/forge";

/**
 * File-tree renderer for panels with `contentType = "files"`. Lists the
 * active forge root, lazily loading subdirectories when expanded. No
 * file actions yet — clicking a file is a no-op until an editor exists
 * to open it into.
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

function FileTreeForForge() {
  const [rootEntries, setRootEntries] = useState<ForgeDirEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

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
  }, []);

  if (error) {
    return <div className="file-tree-error">Failed to list forge: {error}</div>;
  }

  if (!rootEntries) {
    return <div className="file-tree-loading">loading…</div>;
  }

  if (rootEntries.length === 0) {
    return <div className="file-tree-empty">Forge is empty.</div>;
  }

  return (
    <ul className="file-tree" role="tree">
      {rootEntries.map((entry) => (
        <TreeNode key={entry.relpath} entry={entry} depth={0} />
      ))}
    </ul>
  );
}

interface TreeNodeProps {
  entry: ForgeDirEntry;
  depth: number;
}

function TreeNode({ entry, depth }: TreeNodeProps) {
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<ForgeDirEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadChildren = useCallback(async () => {
    if (children !== null) return;
    try {
      const next = await listForgeDir(entry.relpath);
      setChildren(next);
    } catch (e) {
      setError(String(e));
    }
  }, [entry.relpath, children]);

  const onToggle = useCallback(() => {
    if (!entry.isDir) return;
    const next = !expanded;
    setExpanded(next);
    if (next) void loadChildren();
  }, [entry.isDir, expanded, loadChildren]);

  const indent = { paddingInlineStart: `${depth * 12 + 4}px` } as const;

  if (!entry.isDir) {
    return (
      <li role="treeitem" className="file-tree-row is-file" style={indent}>
        <span className="file-tree-twisty" aria-hidden="true" />
        <File size={14} className="file-tree-icon" aria-hidden="true" />
        <span className="file-tree-name">{entry.name}</span>
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
        style={indent}
      >
        {expanded ? (
          <ChevronDown size={12} className="file-tree-twisty" aria-hidden="true" />
        ) : (
          <ChevronRight size={12} className="file-tree-twisty" aria-hidden="true" />
        )}
        <Folder size={14} className="file-tree-icon" aria-hidden="true" />
        <span className="file-tree-name">{entry.name}</span>
      </button>
      {expanded && (
        <ul role="group" className="file-tree-children">
          {error && <li className="file-tree-error">{error}</li>}
          {children === null && !error && (
            <li className="file-tree-loading" style={{ paddingInlineStart: `${(depth + 1) * 12 + 4}px` }}>
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
