# Comments and annotations

Nexus comments are inline thread annotations attached to a **block**,
not to a line range. Edits around the block don't break the thread.

## Add a comment

1. Highlight the block (or place the cursor on it).
2. Press `Ctrl+K Ctrl+C` (or `Cmd+K Cmd+C` on macOS).
3. A thread opens in the right-panel **Comments** tab.

## Reply, resolve, delete

Each thread has a header with **Resolve**, **Reopen**, and **Delete**.
Resolved threads collapse but stay attached so you can audit them
later.

## Where comments live

Comments are stored in the block's properties — invisible in the
markdown body, but persisted on the file. This means:

- They travel with the file when you rename or move it.
- They show up in `git diff` as YAML frontmatter changes (clean diffs).
- A future merge tool sees them as ordinary text.

## Mentions

`@username` inside a comment renders as a mention. (Notification
delivery depends on configured plugins — by default mentions are
visual-only.)

## Filter and navigate

The right-panel Comments tab lists all threads in the current note.
The status-bar comment-count shows totals across the workspace.
