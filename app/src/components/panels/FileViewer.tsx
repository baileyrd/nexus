import { useEffect, useMemo, useRef } from "react";
import { X } from "lucide-react";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import {
  SCROLL_TO_HEADING_EVENT,
  type ScrollToHeadingDetail,
} from "./Outline";

/**
 * Read-only file viewer. Renders the currently-open forge file as plain
 * text, tagging each line with `data-line` so the outline panel can
 * scroll to a heading via a custom event. A real editor (PRD-08) will
 * replace this.
 */
export function FileViewer() {
  const file = useOpenFileStore((s) => s.file);
  const loading = useOpenFileStore((s) => s.loading);
  const error = useOpenFileStore((s) => s.error);
  const close = useOpenFileStore((s) => s.close);
  const refresh = useOpenFileStore((s) => s.refresh);
  const fsVersion = useForgeStore((s) => s.fsVersion);
  const bodyRef = useRef<HTMLPreElement>(null);

  // Re-read the open file whenever the watcher signals a change. If
  // the file has been deleted or renamed externally, `refresh` closes
  // cleanly so the viewer doesn't get stuck on stale content.
  useEffect(() => {
    void refresh();
  }, [fsVersion, refresh]);

  // Scroll to the heading's line when the outline panel requests it.
  useEffect(() => {
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<ScrollToHeadingDetail>).detail;
      if (!detail || !bodyRef.current) return;
      const target = bodyRef.current.querySelector<HTMLElement>(
        `[data-line="${detail.line}"]`,
      );
      target?.scrollIntoView({ behavior: "smooth", block: "start" });
    };
    window.addEventListener(SCROLL_TO_HEADING_EVENT, handler);
    return () => window.removeEventListener(SCROLL_TO_HEADING_EVENT, handler);
  }, []);

  const lines = useMemo(() => file?.content.split("\n") ?? [], [file?.content]);

  if (loading) {
    return <div className="file-viewer-status">opening…</div>;
  }
  if (error) {
    return (
      <div className="file-viewer-status is-error">
        Failed to open: {error}
      </div>
    );
  }
  if (!file) return null;

  return (
    <div className="file-viewer">
      <header className="file-viewer-header">
        <span className="file-viewer-name">{file.name}</span>
        <span className="file-viewer-relpath">{file.relpath}</span>
        <button
          type="button"
          className="file-viewer-close"
          aria-label="Close file"
          title="Close file"
          onClick={close}
        >
          <X size={14} aria-hidden="true" />
        </button>
      </header>
      <pre ref={bodyRef} className="file-viewer-body">
        {lines.map((line, i) => (
          <span key={i} data-line={i}>
            {line}
            {i < lines.length - 1 ? "\n" : ""}
          </span>
        ))}
      </pre>
    </div>
  );
}
