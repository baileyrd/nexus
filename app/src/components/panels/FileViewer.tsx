import { useCallback, useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import { EditorSurface } from "../surfaces/EditorSurface";
import { editorSyncContent } from "../../ipc/editor";

/**
 * File viewer with live CodeMirror 6 editor. Renders the currently-open
 * forge file with syntax-aware editing, dirty-state tracking, and
 * Mod+S save.
 */
export function FileViewer() {
  const file = useOpenFileStore((s) => s.file);
  const loading = useOpenFileStore((s) => s.loading);
  const error = useOpenFileStore((s) => s.error);
  const isDirty = useOpenFileStore((s) => s.isDirty);
  const close = useOpenFileStore((s) => s.close);
  const markDirty = useOpenFileStore((s) => s.markDirty);
  const save = useOpenFileStore((s) => s.save);
  const refresh = useOpenFileStore((s) => s.refresh);
  const fsVersion = useForgeStore((s) => s.fsVersion);

  // Debounced sync: after 800 ms of no typing, push the latest content to the
  // Rust block tree so AI / MCP / outline consumers stay reasonably fresh.
  const syncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestContentRef = useRef<string>("");

  useEffect(() => {
    void refresh();
  }, [fsVersion, refresh]);

  const handleChange = useCallback(
    (content: string) => {
      markDirty();
      latestContentRef.current = content;
      if (syncTimerRef.current) clearTimeout(syncTimerRef.current);
      const relpath = file?.relpath;
      if (!relpath) return;
      syncTimerRef.current = setTimeout(() => {
        void editorSyncContent(relpath, latestContentRef.current);
      }, 800);
    },
    [markDirty, file?.relpath],
  );

  // Cancel any pending sync when the component unmounts or the file changes.
  useEffect(() => {
    return () => {
      if (syncTimerRef.current) clearTimeout(syncTimerRef.current);
    };
  }, [file?.relpath]);

  const handleSave = useCallback(
    (content: string) => {
      void save(content);
    },
    [save],
  );

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
        <span className="file-viewer-name">
          {file.name}
          {isDirty && <span className="file-viewer-dirty" title="Unsaved changes" />}
        </span>
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
      <EditorSurface
        initialContent={file.content}
        filePath={file.relpath}
        onChange={handleChange}
        onSave={handleSave}
      />
    </div>
  );
}
