import { useMemo } from "react";
import { useOpenFileStore } from "../../stores/openFile";
import { parseHeadings } from "../../util/markdown";

/** Custom DOM event the FileViewer listens for to scroll to a heading. */
export const SCROLL_TO_HEADING_EVENT = "nx:scroll-to-heading";

export interface ScrollToHeadingDetail {
  line: number;
}

/**
 * Outline panel: parses ATX headings out of the currently-open file
 * and renders them as a clickable nav. Clicking a heading scrolls the
 * viewer to that line via a window-level custom event.
 */
export function Outline() {
  const file = useOpenFileStore((s) => s.file);

  const headings = useMemo(
    () => (file ? parseHeadings(file.content) : []),
    [file?.content],
  );

  if (!file) {
    return <div className="outline-empty">No file open.</div>;
  }
  if (headings.length === 0) {
    return <div className="outline-empty">No headings.</div>;
  }

  const onClick = (line: number) => {
    window.dispatchEvent(
      new CustomEvent<ScrollToHeadingDetail>(SCROLL_TO_HEADING_EVENT, {
        detail: { line },
      }),
    );
  };

  return (
    <ul className="outline" role="tree">
      {headings.map((h) => (
        <li key={h.line} className={`outline-item level-${h.level}`} role="treeitem">
          <button
            type="button"
            className="outline-link"
            style={{ paddingInlineStart: `${(h.level - 1) * 10 + 8}px` }}
            onClick={() => onClick(h.line)}
            title={h.text}
          >
            {h.text}
          </button>
        </li>
      ))}
    </ul>
  );
}
