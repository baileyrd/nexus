import { useEffect } from "react";
import { X } from "lucide-react";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";

/**
 * Read-only file viewer. Renders the currently-open forge file as plain
 * text. A real editor (PRD-08) will replace this — the viewer exists
 * so the file tree has a meaningful target before that lands.
 */
export function FileViewer() {
  const file = useOpenFileStore((s) => s.file);
  const loading = useOpenFileStore((s) => s.loading);
  const error = useOpenFileStore((s) => s.error);
  const close = useOpenFileStore((s) => s.close);
  const refresh = useOpenFileStore((s) => s.refresh);
  const fsVersion = useForgeStore((s) => s.fsVersion);

  // Re-read the open file whenever the watcher signals a change. If
  // the file has been deleted or renamed externally, `refresh` closes
  // cleanly so the viewer doesn't get stuck on stale content.
  useEffect(() => {
    void refresh();
  }, [fsVersion, refresh]);

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
      <pre className="file-viewer-body">{file.content}</pre>
    </div>
  );
}
