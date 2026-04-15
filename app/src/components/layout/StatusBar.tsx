import type { StatusBarItem } from "../../bindings";

interface StatusBarProps {
  items: StatusBarItem[];
}

/**
 * Floating status bar pinned to the bottom-right of the workspace
 * frame. Mixes plain-text counters (no `action`) with icon buttons
 * (`action` set). Click handling routes through the same UI
 * contribution registry as ribbon / panel-toolbar dispatch.
 */
export function StatusBar({ items }: StatusBarProps) {
  if (items.length === 0) return null;
  return (
    <div className="status-bar" role="status" aria-label="Workspace status">
      {items.map((item) => (
        <StatusBarEntry key={item.id} item={item} />
      ))}
    </div>
  );
}

function StatusBarEntry({ item }: { item: StatusBarItem }) {
  const icon = item.icon ? (
    <span aria-hidden="true" className="status-bar-icon">
      {placeholderGlyph(item.icon)}
    </span>
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
      // eslint-disable-next-line no-console
      console.log(
        `[status-bar] togglePanel '${item.action.panelId}' from ${item.id} (target side panel pending)`,
      );
      return;
    case "invokeCommand":
      // eslint-disable-next-line no-console
      console.log(`[status-bar] invoke command '${item.action.command}' (registry pending)`);
      return;
    case "openView":
      // eslint-disable-next-line no-console
      console.log(`[status-bar] open view '${item.action.viewId}' (registry pending)`);
      return;
  }
}

function placeholderGlyph(icon: string): string {
  const trimmed = icon.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
