import { useCallback, useEffect, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Icon } from "../Icon";
import {
  useTreeDataProvider,
  type ContentComponentProps,
  type TreeDataProvider,
  type TreeNode,
} from "../../contributions";
import { useForgeStore } from "../../stores/forge";

/**
 * Generic tree panel rendered by the host shell for plugin-contributed
 * tree-data providers. Plugins call `contributions.registerTreeDataProvider`
 * (or `ctx.ui.registerTreeDataProvider`) and this component handles the
 * render — no bespoke React code required in the plugin.
 *
 * Lazy-loads children on first expand. Calls `provider.onSelect` when the
 * user clicks a node row.
 */
export function GenericTreePanel({ panel }: ContentComponentProps) {
  const provider = useTreeDataProvider(panel.contentType ?? "");
  // SI-4: re-fetch on forge switch. The provider object identity doesn't
  // change across forge switches, but the data it returns almost certainly
  // does (most providers key off forge root). Including the root in the
  // dep array resets the tree state when the user opens a different forge.
  const forgeRoot = useForgeStore((s) => s.info?.root ?? null);
  const [roots, setRoots] = useState<TreeNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!provider) return;
    setRoots(null);
    setError(null);
    void Promise.resolve(provider.getChildren(null))
      .then(setRoots)
      .catch((err) => setError(String(err)));
  }, [provider, forgeRoot]);

  if (!provider) {
    return (
      <div className="generic-tree-empty" role="status">
        No tree provider for "{panel.contentType}".
      </div>
    );
  }

  if (error) {
    return (
      <div className="generic-tree-error" role="alert">
        {error}
      </div>
    );
  }

  if (roots === null) {
    return (
      <div className="generic-tree-loading" role="status">
        Loading…
      </div>
    );
  }

  if (roots.length === 0) {
    return (
      <div className="generic-tree-empty" role="status">
        (empty)
      </div>
    );
  }

  return (
    <ul className="generic-tree" role="tree">
      {roots.map((node) => (
        <TreeNodeRow key={node.id} node={node} provider={provider} depth={0} />
      ))}
    </ul>
  );
}

/** Builds a content component bound to a specific viewId for use in
 * `contributions.setTreePanelFactory`. */
export function makeGenericTreePanelFactory(
  _viewId: string,
): typeof GenericTreePanel {
  // The panel's `contentType` is the viewId, so GenericTreePanel already
  // resolves the correct provider via `useTreeDataProvider(panel.contentType)`.
  // This factory just returns the same component; `_viewId` is accepted
  // for symmetry and potential future per-view customisation.
  return GenericTreePanel;
}

// ─── Internal ────────────────────────────────────────────────────────────────

interface RowProps {
  node: TreeNode;
  provider: TreeDataProvider;
  depth: number;
}

function TreeNodeRow({ node, provider, depth }: RowProps) {
  const isLeaf = node.children === undefined;
  const [expanded, setExpanded] = useState(false);
  const [children, setChildren] = useState<TreeNode[] | null>(
    node.children ?? null,
  );
  const [loading, setLoading] = useState(false);

  const toggle = useCallback(async () => {
    if (isLeaf) return;
    if (!expanded && children === null) {
      setLoading(true);
      try {
        const loaded = await Promise.resolve(provider.getChildren(node.id));
        setChildren(loaded);
      } finally {
        setLoading(false);
      }
    }
    setExpanded((v) => !v);
  }, [isLeaf, expanded, children, provider, node.id]);

  const handleClick = useCallback(() => {
    void toggle();
    void provider.onSelect?.(node.id, node);
  }, [toggle, provider, node]);

  const handleKey = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleClick();
      }
    },
    [handleClick],
  );

  return (
    <li
      role="treeitem"
      aria-expanded={isLeaf ? undefined : expanded}
      style={{ paddingLeft: `${depth * 14}px` }}
    >
      <div
        className="generic-tree-row"
        onClick={handleClick}
        onKeyDown={handleKey}
        tabIndex={0}
        role="button"
      >
        <span className="generic-tree-chevron">
          {!isLeaf &&
            (loading ? (
              <span className="generic-tree-spinner" aria-hidden />
            ) : expanded ? (
              <ChevronDown size={12} />
            ) : (
              <ChevronRight size={12} />
            ))}
        </span>
        {node.icon && (
          <Icon name={node.icon} size={14} className="generic-tree-icon" />
        )}
        <span className="generic-tree-label">{node.label}</span>
      </div>

      {expanded && children && children.length > 0 && (
        <ul role="group">
          {children.map((child) => (
            <TreeNodeRow
              key={child.id}
              node={child}
              provider={provider}
              depth={depth + 1}
            />
          ))}
        </ul>
      )}
    </li>
  );
}
