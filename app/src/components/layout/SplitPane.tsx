import { useCallback, useRef } from "react";
import type { LayoutNode, PaneId } from "../../bindings";
import { useLayoutStore } from "../../stores/layout";
import { PaneView } from "./PaneView";

interface SplitPaneProps {
  node: LayoutNode;
  focusedPaneId: PaneId | null | undefined;
}

const MIN_SIZE_PX = 80;

/** Recursive renderer for the workspace layout tree.
 *  Dividers are interactive: pointer drag rebalances the adjacent
 *  children's proportional sizes via the layout store. */
export function SplitPane({ node, focusedPaneId }: SplitPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const setSplitSizes = useLayoutStore((s) => s.setSplitSizes);

  if (node.type === "leaf") {
    return <PaneView node={node} focused={node.id === focusedPaneId} />;
  }

  const isRow = node.direction === "row";

  return (
    <div
      ref={containerRef}
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
      {node.children.map((child, i) => {
        const basis = node.sizes[i] ?? 1 / node.children.length;
        const showDivider = i < node.children.length - 1;
        return (
          <SplitCell
            key={child.id}
            basis={basis}
            showDivider={showDivider}
            direction={node.direction}
            onDividerDrag={(delta) =>
              handleDividerDrag(
                containerRef.current,
                node.id,
                node.sizes,
                node.children.length,
                i,
                delta,
                isRow,
                setSplitSizes,
              )
            }
          >
            <SplitPane node={child} focusedPaneId={focusedPaneId} />
          </SplitCell>
        );
      })}
    </div>
  );
}

/** Given a drag delta (px) along the split axis, compute new proportions
 *  for the two sides of divider `index` and commit them. Enforces a
 *  minimum pixel size on both adjacent cells. */
function handleDividerDrag(
  container: HTMLDivElement | null,
  paneId: PaneId,
  currentSizes: number[],
  childCount: number,
  index: number,
  deltaPx: number,
  isRow: boolean,
  setSplitSizes: (id: PaneId, sizes: number[]) => void,
) {
  if (!container) return;
  const axisPx = isRow ? container.clientWidth : container.clientHeight;
  if (axisPx <= 0) return;

  const sizes =
    currentSizes.length === childCount
      ? [...currentSizes]
      : Array<number>(childCount).fill(1 / childCount);
  const total = sizes.reduce((a, b) => a + b, 0) || 1;

  // Work in pixels so the min-size clamp is intuitive.
  const px = sizes.map((s) => (s / total) * axisPx);
  const minFrac = MIN_SIZE_PX / axisPx;

  let left = px[index] + deltaPx;
  let right = px[index + 1] - deltaPx;
  if (left < MIN_SIZE_PX) {
    right -= MIN_SIZE_PX - left;
    left = MIN_SIZE_PX;
  }
  if (right < MIN_SIZE_PX) {
    left -= MIN_SIZE_PX - right;
    right = MIN_SIZE_PX;
  }
  px[index] = left;
  px[index + 1] = right;

  const nextFrac = px.map((p) => Math.max(p / axisPx, minFrac));
  const sum = nextFrac.reduce((a, b) => a + b, 0);
  const normalised = nextFrac.map((f) => f / sum);

  setSplitSizes(paneId, normalised);
}

interface SplitCellProps {
  children: React.ReactNode;
  basis: number;
  showDivider: boolean;
  direction: "row" | "column";
  onDividerDrag: (deltaPx: number) => void;
}

function SplitCell({
  children,
  basis,
  showDivider,
  direction,
  onDividerDrag,
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
      {showDivider && (
        <SplitDivider direction={direction} onDrag={onDividerDrag} />
      )}
    </>
  );
}

interface SplitDividerProps {
  direction: "row" | "column";
  onDrag: (deltaPx: number) => void;
}

function SplitDivider({ direction, onDrag }: SplitDividerProps) {
  // Track the last reported pointer coordinate on the split axis.
  // Using refs (not state) avoids re-rendering the whole tree 60×/sec
  // during a drag — only the store update inside onDrag triggers a render.
  const lastCoord = useRef<number | null>(null);
  const draggingRef = useRef(false);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (e.button !== 0) return;
      e.preventDefault();
      (e.target as HTMLDivElement).setPointerCapture(e.pointerId);
      lastCoord.current = direction === "row" ? e.clientX : e.clientY;
      draggingRef.current = true;
    },
    [direction],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!draggingRef.current || lastCoord.current === null) return;
      const coord = direction === "row" ? e.clientX : e.clientY;
      const delta = coord - lastCoord.current;
      if (delta === 0) return;
      lastCoord.current = coord;
      onDrag(delta);
    },
    [direction, onDrag],
  );

  const handlePointerUp = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (!draggingRef.current) return;
      draggingRef.current = false;
      lastCoord.current = null;
      try {
        (e.target as HTMLDivElement).releasePointerCapture(e.pointerId);
      } catch {
        // releasePointerCapture throws if the pointer was never captured;
        // harmless in practice.
      }
    },
    [],
  );

  return (
    <div
      className="split-divider"
      data-axis={direction}
      role="separator"
      aria-orientation={direction === "row" ? "vertical" : "horizontal"}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerUp}
    />
  );
}
