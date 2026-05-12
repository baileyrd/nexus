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

Comments are stored in a **JSON sidecar** next to your forge index,
not in the markdown file itself. For a note at `<forge>/notes/foo.md`,
its sidecar is `<forge>/.forge/comments/notes/foo.md.json`. The
markdown body stays untouched.

The sidecar's shape (one file per note):

```json
{
  "version": 1,
  "file_path": "notes/foo.md",
  "threads": [
    {
      "id": "...",
      "block_id": "...",            // anchor stamped on the block by the editor
      "resolved": false,
      "created_at": "2026-05-12T…",
      "comments": [
        {
          "id": "...",
          "author": "Ada",
          "body": "Looks good — one thought on the second paragraph.",
          "mentions": [],
          "created_at": "2026-05-12T…"
        }
      ]
    }
  ]
}
```

Threads anchor to a block via `block_id`, a UUID the editor stamps on
the block when the first comment is created (handled automatically;
authors don't see the id in the markdown). The sidecar is rewritten
on every thread mutation and is deleted when its last thread is
removed — so a forge with no comments has no `.forge/comments/`
clutter.

This has a few consequences:

- The markdown body and `git diff` of your note stay clean — comments
  never touch the file.
- The sidecar **is** under your forge tree, so it lands in version
  control alongside the note (commit `.forge/comments/**` to share
  threads with collaborators, or add it to `.gitignore` if you want
  comments to stay local).
- Renaming or moving the note does **not** automatically relocate the
  sidecar today — there's no in-tree rename hook on `com.nexus.comments`
  yet. After a rename, move the matching JSON file by hand, or accept
  that the threads are orphaned until tooling catches up.

## Mentions

`@username` inside a comment renders as a mention. (Notification
delivery depends on configured plugins — by default mentions are
visual-only.)

## Filter and navigate

The right-panel Comments tab lists all threads in the current note.
The status-bar comment-count shows totals across the workspace.
