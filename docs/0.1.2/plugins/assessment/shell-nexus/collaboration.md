# Collaboration

This category covers the real-time and asynchronous collaboration surfaces:
the peer presence panel, block-anchored comment threads, and the CRDT
conflict resolver modal. All three depend on backend Rust services
(`com.nexus.collab`, `com.nexus.comments`, the CRDT publisher in
`nexus-bootstrap`) and are independent of the basic single-user markdown
workflow. `nexus.crdtConflict` is the most narrowly scoped — it only fires
when collab-driven git pulls land conflicting CRDT ops on an open session.

### nexus.collab

- **Path:** `shell/src/plugins/nexus/collab/`
- **Surface:** Registers `collab-panel` view (left rail) and an activity-bar
  entry (`users` icon); command `nexus.collab.focus`. Subscribes to the
  `com.nexus.collab.` topic prefix (peers joined/left, presence,
  connection state, relay started/stopped) and hydrates the
  `useCollabStore` Zustand store the panel renders. Calls
  `com.nexus.collab::relay_status` at activation to recover existing
  relay state across shell reloads.
- **Depends on:** `nexus.workspace`, `nexus.activityBar`; backend
  `com.nexus.collab` (relay control, peer state, presence events).
- **Verdict:** Optional
- **Rationale:** Real-time multiplayer feature; requires the collab Rust
  service to be configured. The default single-user install never sees a
  peer event and the panel just shows its idle empty state.

### nexus.comments

- **Path:** `shell/src/plugins/nexus/comments/`
- **Surface:** Registers `comments` view, advertises a right-panel tab via
  `rightPanel:registerTab`, command `nexus.comments.focus`. Loads
  threads per active editor relpath through a monotonic request-id
  guard (drops late responses across fast tab switches); listens for
  `nexus.comments:reload` so the editor margin gutter can refresh after
  creating a thread. Restricts to `.md` / `.markdown` files
  (`isCommentableRelpath`).
- **Depends on:** `nexus.rightPanel`; `nexus.editor` (`useEditorStore`); backend
  `com.nexus.comments` (`list`, `reply`, `resolve`, `edit`, `delete`,
  `create_thread`).
- **Verdict:** Optional
- **Rationale:** Block-anchored review threads — a discrete reviewer
  feature that overlaps with git's commit/PR workflow. Not on the basic
  edit-and-save path. Note: thread *creation* lives in the editor margin
  gutter, not this panel — the panel is read/reply only.

### nexus.crdtConflict

- **Path:** `shell/src/plugins/nexus/crdtConflict/`
- **Surface:** Registers an overlay view `nexus.crdtConflict.modal` at
  priority 90 (same bucket as `confirm` and `pick`); subscribes to the
  `com.nexus.editor.crdt.conflict.` topic prefix and pushes envelopes
  through `useConflictStore`'s queue. The modal offers "Keep local",
  "Use remote", "Open file" actions using the enriched payload
  (`local_content`, `remote_content`, `delete_origin`) and applies the
  user's choice via `com.nexus.editor::apply_transaction`.
- **Depends on:** Backend `crdt_publisher` in `nexus-bootstrap` and
  `com.nexus.editor`. No declared shell-plugin `dependsOn`.
- **Verdict:** Optional
- **Rationale:** Only triggers when collab's CRDT publisher emits a
  conflict — which itself requires real-time co-editing followed by a
  `git pull` landing an incompatible op. The plugin is dead weight when
  `nexus.collab` is not in use, and is only meaningful as a companion to
  collab.

## Category verdict

| Plugin            | Verdict  | Required for basic workflow |
|-------------------|----------|-----------------------------|
| `nexus.collab`      | Optional | No — multiplayer feature    |
| `nexus.comments`    | Optional | No — reviewer feature       |
| `nexus.crdtConflict`| Optional | No — only fires when collab is active |
