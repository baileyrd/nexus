# Decision log

A chronological list of choices that shaped Forge. New entries go
on top; old entries stay put so future readers can see the thinking
at the time, not just the current answer.

## 2026-04 · Microkernel over monolith

**Context.** We needed an editor that could grow — markdown today,
canvases + databases + AI workflows over time.

**Decision.** Adopt a microkernel shape (VS Code / IntelliJ
pattern). A tiny core owns the event bus, capability system, and
plugin lifecycle; every feature is a contribution.

**Consequence.** Slower to ship the *first* feature. Everything
after it is incremental and unlocked for plugin authors.

## 2026-04 · Tauri over Electron

**Context.** Desktop shell required.

**Decision.** Tauri — smaller binary, lighter memory footprint,
native OS integration where it matters (filesystem, notifications).

**Trade-off.** Smaller ecosystem than Electron; some plugins
(especially those that bundle Chromium quirks) need adaptation.

## 2026-04 · Forge-as-directory

**Context.** Where do notes live?

**Decision.** A *forge* is a plain directory on disk — `notes/`,
`attachments/`, `.forge/`. No database wraps the files; the index
is a rebuild-on-crash cache.

**Consequence.** Users can `rsync`, `git`, `Dropbox` a forge
without Nexus running. The format outlives the app.

## 2026-04 · IBM Plex Serif for the body

**Context.** What should long-form reading feel like?

**Decision.** IBM Plex Serif for document body, Inter for UI,
JetBrains Mono for code. The serif gives the editor a paper-like
warmth that a pure-sans UI doesn't.

**Consequence.** Heavier font payload (3 families); mitigated by
`font-display: swap` on the Google Fonts `<link>`.
