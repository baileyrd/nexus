# Git integration

Nexus has read-only git integration: status, log, blame, diff. The
goal is to surface what's changing in your forge without trying to be
a git client. For staging, committing, and pushing, use your
preferred git tool.

## Status panel

If your forge is inside a git repo, the **Source Control** panel in
the sidebar shows:

- Untracked files (`?`)
- Modified files (`M`)
- Staged files (`A`, `M`, `D`)
- Branch name and ahead/behind counts

File rows are clickable — they open the file in the editor.

## CLI

```bash
nexus git status
nexus git log --limit 20
nexus git log path/to/note.md
nexus git blame path/to/note.md
nexus git diff path/to/note.md          # unstaged changes
nexus git diff --staged
```

Output respects the global `--format` flag (`text`, `json`, `jsonl`,
`table`).

## Per-file history

Right-click a note in the file tree → **Show file history**. You'll
see every commit that touched the file with author, date, and message.
Click a commit to see the diff.

## Blame

Right-click in the editor → **Blame current line**. Shows the commit
that introduced the line at the cursor.

## Why read-only?

Three reasons:

1. **Git is finicky.** Merge conflicts, hooks, GPG signing, LFS — it's
   a category of tool unto itself. Doing it badly is worse than not
   doing it.
2. **The forge is your repo, not Nexus's.** Git authorship and history
   should reflect what you do in your normal git tool, not magic
   commits Nexus made on your behalf.
3. **No surprises.** Read-only means Nexus can never accidentally push
   the wrong thing or rewrite history.

## Recommended workflow

- Add `.forge/` to `.gitignore` (or commit only `.forge/config.toml`
  and `.forge/skills/`).
- Use your normal git client to stage, commit, and push.
- Watch the Nexus status panel for visual feedback.

## .gitignore template

```gitignore
# Nexus internal state — rebuildable from files
.forge/index.db
.forge/index.db-wal
.forge/index.db-shm
.forge/search/
.forge/temp/
.forge/logs/
.forge/kv.sqlite3
.forge/chat/

# Optional: keep your skills and config under version control
# (don't ignore .forge/skills/ or .forge/app.toml if you want to share them)
```

## Implementation

Backed by `git2` (libgit2 bindings). No `git` subprocess is invoked —
operations are pure-Rust and fast even on huge repos.
