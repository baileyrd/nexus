import { X } from "lucide-react";
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
