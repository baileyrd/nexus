import { useEffect, useMemo, useState } from "react";
import type { StatusBarItem } from "../../bindings";
import { contributions } from "../../contributions";
import { useOpenFileStore } from "../../stores/openFile";
import { onPluginEvent } from "../../plugins/events";
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
interface GitSnapshot {
  branch: string | null;
  head: string | null;
  isDirty: boolean;
}

/**
 * Subscribe to git state events from `com.nexus.git`. The git core
 * plugin emits an initial `com.nexus.git.state` snapshot on its first
 * poll and then delta events on branch / commit / dirty changes. We
 * keep the latest snapshot in component state and let the caller
 * format it. Short-circuit if the forge isn't a git repo — events
 * just never arrive and we fall back to the preset text.
 */
function useGitState(): GitSnapshot {
  const [state, setState] = useState<GitSnapshot>({
    branch: null,
    head: null,
    isDirty: false,
  });
  useEffect(() => {
    const handle = (payload: {
      branch?: string | null;
      head?: string | null;
      is_dirty?: boolean;
    }) =>
      setState({
        branch: payload.branch ?? null,
        head: payload.head ?? null,
        isDirty: Boolean(payload.is_dirty),
      });
    const unlistens: Array<Promise<() => void>> = [
      onPluginEvent<{ branch?: string; head?: string; is_dirty?: boolean }>(
        "com.nexus.git.state",
        (ev) => handle(ev.payload),
      ),
      onPluginEvent<{ to?: string; head?: string }>(
        "com.nexus.git.branch_changed",
        (ev) =>
          setState((s) => ({
            ...s,
            branch: ev.payload.to ?? s.branch,
            head: ev.payload.head ?? s.head,
          })),
      ),
      onPluginEvent<{ branch?: string; head?: string }>(
        "com.nexus.git.commit",
        (ev) =>
          setState((s) => ({
            ...s,
            head: ev.payload.head ?? s.head,
          })),
      ),
      onPluginEvent<{ is_dirty?: boolean }>(
        "com.nexus.git.dirty_changed",
        (ev) =>
          setState((s) => ({ ...s, isDirty: Boolean(ev.payload.is_dirty) })),
      ),
    ];
    return () => {
      for (const p of unlistens) void p.then((u) => u());
    };
  }, []);
  return state;
}

function useLiveStatusText(): Record<string, string> {
  const file = useOpenFileStore((s) => s.file);
  const git = useGitState();
  return useMemo(() => {
    const content = file?.content ?? "";
    const words = countWords(content);
    const chars = content.length;
    const outLinks = countOutgoingLinks(content);
    const liveGit: Record<string, string> = {};
    if (git.branch) {
      liveGit["git.branch"] = git.isDirty ? `${git.branch} *` : git.branch;
    }
    if (git.head) liveGit["git.sha"] = git.head;
    return {
      "editor.word-count": `${words.toLocaleString()} words`,
      "editor.character-count": `${chars.toLocaleString()} characters`,
      // `editor.backlinks-count` would need an index query; surface the
      // outgoing-link count here as a live proxy until the IPC lands.
      "editor.backlinks-count": `${outLinks} outgoing`,
      ...liveGit,
    };
  }, [file?.relpath, file?.content, git.branch, git.head, git.isDirty]);
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
