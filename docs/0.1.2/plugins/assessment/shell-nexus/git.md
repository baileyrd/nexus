# Git

Two shell plugins surface the kernel `com.nexus.git` service in the UI: a full
source-control sidebar (`gitPanel`) and a status-bar branch/dirty indicator
(`gitStatus`). Together they cover the "commit and push" leg of the basic
capability set; the heavy lifting (branches, log, stash, conflicts) lives in
`gitPanel`, while `gitStatus` is the always-on at-a-glance signal that a forge
is even a git repo.

### gitPanel

- **Path:** `shell/src/plugins/nexus/gitPanel/`
- **Surface:**
  - Registers view type `git-panel` and view id `nexus.gitPanel.view` (the
    sidebar leaf with file list, commit UI, branch picker, log, stash, and
    conflict resolver).
  - Activity-bar item `nexus.gitPanel.activityItem` (priority 25, icon `git`).
  - Command `nexus.gitPanel.focus` (category `Git`).
  - Loads `file_statuses`, `branches`, `log`, and `stash_list` from
    `com.nexus.git` on `workspace:opened` and refreshes on every
    `com.nexus.git.*` event (branch / commit / dirty transitions).
- **Depends on:** `nexus.workspace`, `nexus.activityBar`, `nexus.gitStatus`;
  every IPC goes to the `com.nexus.git` core plugin.
- **Verdict:** Essential
- **Rationale:** Commit and push is in the basic-capability scope, and this is
  the only UI surface that lets a user stage, commit, switch branches, view the
  log, and resolve conflicts. Without it the only commit path is an external
  terminal.

### gitStatus

- **Path:** `shell/src/plugins/nexus/gitStatus/`
- **Surface:**
  - Status-bar item `nexus.gitStatus.item` in the `statusBarLeft` slot
    (priority 20) — renders current branch + dirty marker.
  - Polls `com.nexus.git::status` on `workspace:opened`, then subscribes to
    the `com.nexus.git.` topic prefix (`state`, `branch_changed`, `commit`,
    `dirty_changed`) and re-queries on each.
- **Depends on:** `nexus.workspace`; consumed by `nexus.gitPanel` via
  `useGitStatusStore`.
- **Verdict:** Useful
- **Rationale:** Not strictly required to commit, but the workspace's git state
  is otherwise invisible until the user opens the Git panel. It's the canonical
  "am I on a feature branch / are there unsaved changes" indicator and is
  declared `dependsOn` by `gitPanel`, so removing it would also break the
  Essential plugin.

## Category verdict

| Plugin    | Verdict   | Required for basic capabilities |
|-----------|-----------|---------------------------------|
| gitPanel  | Essential | Yes — only UI path to commit/push/branch |
| gitStatus | Useful    | No — quality-of-life, but `gitPanel` depends on it |
