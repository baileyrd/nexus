import { useCallback, useEffect, useRef } from "react";
import { X } from "lucide-react";
import { useForgeStore } from "../../stores/forge";
import { useOpenFileStore } from "../../stores/openFile";
import { useOpenFile, useOpenFilesStore } from "../../stores/openFiles";
import { useLayoutStore } from "../../stores/layout";
import { EditorSurface } from "../surfaces/EditorSurface";
import { editorSyncContent } from "../../ipc/editor";

interface FileViewerProps {
  /** If present, view is tab-scoped: reads/writes through the keyed
   *  openFiles store and doesn't touch the global single-file store. */
  relpath?: string;
  /** Id of the tab this viewer is mounted in, for dirty-flag mirroring
   *  into the tab strip. */
  tabId?: string;
}

/**
 * File viewer with live CodeMirror 6 editor.
 *
 * When `relpath` is supplied the viewer is tab-scoped (multi-tab flow —
 * PRD 07 §5, §7). Without a relpath it falls back to the legacy single
 * `useOpenFileStore` flow so existing callers (no-op preset with an
 * empty tab list, `file.open` via the plugin bridge) keep working.
 */
export function FileViewer({ relpath, tabId }: FileViewerProps = {}) {
  if (relpath) return <TabScopedViewer relpath={relpath} tabId={tabId} />;
  return <LegacyViewer />;
}

interface TabScopedProps {
  relpath: string;
  tabId: string | undefined;
}

function TabScopedViewer({ relpath, tabId }: TabScopedProps) {
  const entry = useOpenFile(relpath);
  const openEntry = useOpenFilesStore((s) => s.open);
  const setContent = useOpenFilesStore((s) => s.setContent);
  const saveFile = useOpenFilesStore((s) => s.save);
  const refresh = useOpenFilesStore((s) => s.refresh);
  const fsVersion = useForgeStore((s) => s.fsVersion);
  const setTabDirty = useLayoutStore((s) => s.setTabDirty);
  const closeTab = useLayoutStore((s) => s.closeTab);

  // Lazy-load: a tab may be created for a file that isn't yet in the
  // keyed store (e.g. a layout restored across reloads, once we wire
  // persistence). `open` is idempotent on already-loaded files.
  useEffect(() => {
    if (!entry.file && !entry.loading && !entry.error) {
      void openEntry(relpath);
    }
  }, [entry.file, entry.loading, entry.error, openEntry, relpath]);

  // Pull fresh content on external FS bumps, but don't clobber dirty edits.
  useEffect(() => {
    if (entry.file && !entry.isDirty) void refresh(relpath);
  }, [fsVersion, entry.file, entry.isDirty, refresh, relpath]);

  // Mirror dirty flag into the tab strip so the • indicator tracks state.
  useEffect(() => {
    if (tabId) setTabDirty(tabId, entry.isDirty);
  }, [tabId, entry.isDirty, setTabDirty]);

  // Debounced push to the Rust block tree.
  const syncTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const latestContentRef = useRef<string>("");

  const handleChange = useCallback(
    (content: string) => {
      latestContentRef.current = content;
      setContent(relpath, content);
      if (syncTimerRef.current) clearTimeout(syncTimerRef.current);
      syncTimerRef.current = setTimeout(() => {
        void editorSyncContent(relpath, latestContentRef.current);
      }, 800);
    },
    [relpath, setContent],
  );

  useEffect(() => {
    return () => {
      if (syncTimerRef.current) clearTimeout(syncTimerRef.current);
    };
  }, [relpath]);

  const handleSave = useCallback(
    (content: string) => {
      void saveFile(relpath, content);
    },
    [relpath, saveFile],
  );

  const handleClose = useCallback(() => {
    if (tabId) closeTab(tabId);
  }, [tabId, closeTab]);

  if (entry.loading && !entry.file) {
    return <div className="file-viewer-status">opening…</div>;
  }
  if (entry.error) {
    return (
      <div className="file-viewer-status is-error">
        Failed to open: {entry.error}
      </div>
    );
  }
  if (!entry.file) return null;

  return (
    <div className="file-viewer">
      <header className="file-viewer-header">
        <span className="file-viewer-name">
          {entry.file.name}
          {entry.isDirty && (
            <span className="file-viewer-dirty" title="Unsaved changes" />
          )}
        </span>
        <span className="file-viewer-relpath">{entry.file.relpath}</span>
        {tabId && (
          <button
            type="button"
            className="file-viewer-close"
            aria-label="Close file"
            title="Close file"
            onClick={handleClose}
          >
            <X size={14} aria-hidden="true" />
          </button>
        )}
      </header>
      <EditorSurface
        initialContent={entry.file.content}
        filePath={entry.file.relpath}
        onChange={handleChange}
        onSave={handleSave}
      />
    </div>
  );
}

function LegacyViewer() {
  const file = useOpenFileStore((s) => s.file);
  const loading = useOpenFileStore((s) => s.loading);
  const error = useOpenFileStore((s) => s.error);
  const isDirty = useOpenFileStore((s) => s.isDirty);
  const close = useOpenFileStore((s) => s.close);
  const markDirty = useOpenFileStore((s) => s.markDirty);
  const save = useOpenFileStore((s) => s.save);
  const refresh = useOpenFileStore((s) => s.refresh);
  const fsVersion = useForgeStore((s) => s.fsVersion);

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

  if (loading) return <div className="file-viewer-status">opening…</div>;
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
          {isDirty && (
            <span className="file-viewer-dirty" title="Unsaved changes" />
          )}
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
