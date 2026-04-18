import { useMemo } from "react";
import type { StatusBarItem } from "../../bindings";
import { contributions } from "../../contributions";
import { useOpenFileStore } from "../../stores/openFile";
import { Icon } from "../Icon";

interface StatusBarProps {
  items: StatusBarItem[];
}

/**
 * Floating status bar pinned to the bottom-right of the workspace
 * frame. Mixes plain-text counters (no `action`) with icon buttons
 * (`action` set). Click handling routes through the same UI
 * contribution registry as ribbon / panel-toolbar dispatch.
 */
/** Sentinel item id: rendered as a flex:1 spacer so preset authors can
 *  split the status bar into left / right clusters without a schema
 *  change. Keeps the StatusBarItem Rust type contribution-compatible. */
const STATUS_SPACER_ID = "status.spacer";

/**
 * Item ids whose `text` is overridden from live stores. The preset
 * TOML still declares them (so the ordering + icon + action remain
 * preset-driven); this map just replaces the stale static text with
 * whatever the editor / forge currently knows. Keyed by id so new
 * feeds drop in without changing the component shape.
 */
function useLiveStatusText(): Record<string, string> {
  const file = useOpenFileStore((s) => s.file);
  return useMemo(() => {
    const content = file?.content ?? "";
    const words = countWords(content);
    const chars = content.length;
    const outLinks = countOutgoingLinks(content);
    return {
      "editor.word-count": `${words.toLocaleString()} words`,
      "editor.character-count": `${chars.toLocaleString()} characters`,
      // `editor.backlinks-count` would need an index query; surface the
      // outgoing-link count here as a live proxy until the IPC lands.
      "editor.backlinks-count": `${outLinks} outgoing`,
    };
  }, [file?.relpath, file?.content]);
}

function countWords(text: string): number {
  const m = text.trim().match(/\S+/g);
  return m ? m.length : 0;
}

function countOutgoingLinks(text: string): number {
  const m = text.match(/\[\[[^\]]+\]\]/g);
  return m ? m.length : 0;
}

export function StatusBar({ items }: StatusBarProps) {
  const liveText = useLiveStatusText();
  if (items.length === 0) return null;
  return (
    <div className="status-bar" role="status" aria-label="Workspace status">
      {items.map((item) =>
        item.id === STATUS_SPACER_ID ? (
          <span
            key={item.id}
            className="status-bar-spacer"
            aria-hidden="true"
          />
        ) : (
          <StatusBarEntry
            key={item.id}
            item={liveText[item.id] !== undefined
              ? { ...item, text: liveText[item.id] }
              : item}
          />
        ),
      )}
    </div>
  );
}

function StatusBarEntry({ item }: { item: StatusBarItem }) {
  const icon = item.icon ? (
    <Icon name={item.icon} size={14} className="status-bar-icon" />
  ) : null;
  const text = item.text ? <span className="status-bar-text">{item.text}</span> : null;

  if (item.action) {
    return (
      <button
        type="button"
        className="status-bar-item interactive"
        onClick={() => handleClick(item)}
        title={item.text ?? item.id}
      >
        {icon}
        {text}
      </button>
    );
  }

  return (
    <span className="status-bar-item" title={item.text ?? item.id}>
      {icon}
      {text}
    </span>
  );
}

function handleClick(item: StatusBarItem) {
  if (!item.action) return;
  switch (item.action.kind) {
    case "togglePanel":
      // Status-bar togglePanel has no target side (same as ribbon/footer);
      // left as a log until the action carries a side.
      // eslint-disable-next-line no-console
      console.log(
        `[status-bar] togglePanel '${item.action.panelId}' from ${item.id} (target side panel pending)`,
      );
      return;
    case "invokeCommand":
      contributions.invokeCommand(item.action.command);
      return;
    case "openView":
      contributions.openView(item.action.viewId);
      return;
  }
}
