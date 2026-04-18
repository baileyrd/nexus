import type { StatusBarItem } from "../../bindings";
import { contributions } from "../../contributions";
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

export function StatusBar({ items }: StatusBarProps) {
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
          <StatusBarEntry key={item.id} item={item} />
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
