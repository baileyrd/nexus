import type { LayoutNode, PaneId } from "../../bindings";
import { PaneView } from "./PaneView";

interface SplitPaneProps {
  node: LayoutNode;
  focusedPaneId: PaneId | null | undefined;
}

// Recursive renderer for the workspace layout tree. v1 is non-interactive:
// no drag-to-resize, no drag-to-split, no tab reorder. The split-pane
// system from PRD §6 layers those on top of this skeleton.
export function SplitPane({ node, focusedPaneId }: SplitPaneProps) {
  if (node.type === "leaf") {
    return <PaneView node={node} focused={node.id === focusedPaneId} />;
  }

  const isRow = node.direction === "row";
  return (
    <div
      className="split"
      data-direction={node.direction}
      style={{
        display: "flex",
        flexDirection: isRow ? "row" : "column",
        flex: "1 1 0",
        minWidth: 0,
        minHeight: 0,
      }}
    >
      {node.children.map((child, i) => (
        <SplitCell
          key={child.id}
          basis={node.sizes[i] ?? 1 / node.children.length}
          showDivider={i < node.children.length - 1}
          direction={node.direction}
        >
          <SplitPane node={child} focusedPaneId={focusedPaneId} />
        </SplitCell>
      ))}
    </div>
  );
}

interface SplitCellProps {
  children: React.ReactNode;
  basis: number;
  showDivider: boolean;
  direction: "row" | "column";
}

function SplitCell({
  children,
  basis,
  showDivider,
  direction,
}: SplitCellProps) {
  return (
    <>
      <div
        className="split-cell"
        style={{
          flex: `${basis} 1 0`,
          display: "flex",
          minWidth: 0,
          minHeight: 0,
        }}
      >
        {children}
      </div>
      {showDivider && <div className="split-divider" data-axis={direction} />}
    </>
  );
}
