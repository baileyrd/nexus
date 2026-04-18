import { useEffect, useMemo } from "react";
import { useOpenFileStore } from "../../stores/openFile";
import { usePanelCountsStore } from "../../stores/panelCounts";
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

  // Publish the heading count so the Inspector tab can render
  // "Outline 14" next to the label. Cleared on unmount.
  const setPanelCount = usePanelCountsStore((s) => s.setPanelCount);
  useEffect(() => {
    setPanelCount("outline", headings.length);
    return () => setPanelCount("outline", null);
  }, [headings.length, setPanelCount]);

  // Per-section word count — the chip at the right of each row in the
  // Forge design. Derived by slicing the file content between each
  // heading and the next (or EOF for the last one) and word-counting.
  const sectionWords = useMemo(() => {
    if (!file || headings.length === 0) return [] as number[];
    const lines = file.content.split("\n");
    return headings.map((h, i) => {
      const startLine = h.line + 1; // content after the heading line itself
      const endLine = i + 1 < headings.length ? headings[i + 1]!.line : lines.length;
      const slice = lines.slice(startLine, endLine).join(" ");
      const m = slice.trim().match(/\S+/g);
      return m ? m.length : 0;
    });
  }, [file?.content, headings]);

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

  // Only index top-level headings (h1/h2) so the number column stays
  // readable; nested h3/h4 keep the indent but drop the index slot.
  let indexCounter = 0;
  return (
    <>
      <div className="outline-section">
        <span className="outline-section-label">Document outline</span>
        <span className="outline-section-count">{headings.length} hdrs</span>
      </div>
      <ul className="outline" role="tree">
      {headings.map((h, i) => {
        const index = h.level <= 2 ? ++indexCounter : null;
        const words = sectionWords[i] ?? 0;
        return (
          <li
            key={h.line}
            className={`outline-item level-${h.level}`}
            role="treeitem"
          >
            <button
              type="button"
              className="outline-link"
              style={{ paddingInlineStart: `${(h.level - 1) * 10 + 8}px` }}
              onClick={() => onClick(h.line)}
              title={h.text}
            >
              <span className="outline-index" aria-hidden="true">
                {index !== null ? String(index).padStart(2, "0") : ""}
              </span>
              <span className="outline-text">{h.text}</span>
              {words > 0 && (
                <span className="outline-words" aria-hidden="true">
                  {formatWordChip(words)}
                </span>
              )}
            </button>
          </li>
        );
      })}
      </ul>
    </>
  );
}

function formatWordChip(n: number): string {
  if (n >= 10000) return `${(n / 1000).toFixed(1)}kw`;
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}kw`;
  return `${n}w`;
}
