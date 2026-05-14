# BL-053 — Forge visual target: from current shell to the mockup

> **Source:** Forge Color System mockup + ember-on-slate design exploration (2026-05-06).
> **Status:** **All four phases shipped (1, 2, 3, 4a, 4b).** Closure verified 2026-05-14.
> **Related:** the bundled themes `nexus-ember-dark` / `nexus-ember-light` (delivered 2026-05-06) supply the token values this plan styles against.

## Decisions locked in (§5 open questions)

1. **Callout syntax** → Obsidian-style `> [!type] Title\n> body`. The PRD called this "the safest call"; no migration cost since none of the in-repo fixtures use the other dialect.
2. **Status-pill source** → YAML frontmatter `status:` key, plus inline `Complete` / `Partial` / `Substantial` / `Scaffolded` / `Not started` / `Deferred` keywords in table cells and inline code. No Bases dependency, no new inline syntax. Matches the tree-dots hypothesis in §1 row D.
3. **Font bundling** → skipped. `[typography].font_imports` in the bundled themes points at the Google Fonts URL; Georgia is the offline fallback. Bundling the woff2 is a separate offline-first workstream.

## Phase 1 — Chrome polish (shipped)

| Mockup element | Status |
|----|----|
| A. Cool-slate chrome with single ember accent | ✅ Already shipped (bundled ember themes; this BL preserves) |
| B. Pill-shaped editor tabs + ember underline + soft fill on active | ✅ `shell.css` `.forge-tab` + `.forge-tab.active::after` |
| C. Active sidebar row + ember left rail | ✅ Pre-existing |
| M. Inspector segmented control (Outline / Backlinks / Graph) | ✅ `shell.css` `.rtab` / `.rtab.active` |
| P. Status bar bottom-right: forge name + ember dot | ✅ `WorkspaceStatus` registered into `statusBarRight` at priority 5; dot uses `--interactive-accent` |
| E. Fraunces serif H1 / H2 in editor | ✅ `--font-serif` declared in `shell.css :root`; applied to `.cm-content .cm-md-h1/h2` (live preview) + `.nexus-markdown-body h1/h2` (rendered viewer) |

## Phase 2 — Inline rendering (shipped)

| Mockup element | Status |
|----|----|
| F. Frontmatter metadata bar under H1 | ✅ `parseFrontmatter` + `renderFrontmatterBar` in `markdownRender.ts`; spliced after the first `<h1>` |
| G. Path / version-shaped inline `code` tinted ember | ✅ `codespan` renderer override; `nx-codepath` class in `markdown.css` |
| H. `[[wikilinks]]` rendered ember | ✅ Custom marked inline tokenizer; emits `<a class="nx-wikilink">` with `[[target\|alias#fragment]]` support |

## Phase 3 — Callouts (shipped)

| Mockup element | Status |
|----|----|
| I. `> [!info] Title\n> body` callout boxes | ✅ Custom marked block tokenizer; renders `<div class="nx-callout nx-callout--{kind}">` with dot + head + body slots |

Supported kinds: `info`, `note`, `tip`, `success` / `ok`, `warn` / `warning` / `todo`, `risk` / `danger` / `error`, `update`. Unknown kinds stay as plain blockquotes so a typo doesn't silently swallow the content.

## Phase 4 — Status pills + tree dots (Phase 4a shipped; Phase 4b deferred)

| Mockup element | Status |
|----|----|
| K. Status pills in table cells (Complete / Substantial / Partial / Scaffolded / Not started / Deferred) | ✅ `tablecell` renderer override — known status labels render as `<span class="nx-status-pill nx-status-pill__chip--{tone}">`; unknown cells fall through unchanged |
| K-inline. Status keywords in inline `code` | ✅ Same `STATUS_KIND` table is consulted by the `codespan` renderer |
| K-frontmatter. Frontmatter `status:` value as a pill in the metadata bar | ✅ `renderFrontmatterBar` swaps the chip for a pill when the value matches a known status |
| D. File-tree status dots driven by frontmatter `status:` | ✅ Shipped via the `nexus.status` plugin: `statusStore.ts` (zustand cache, FIFO-bounded at 256 entries, in-flight-promise coalescing, files:saved/modified/deleted/renamed invalidation), `useFileStatus.ts` (read-through hook gated to markdown extensions), `StatusPill.tsx` (`StatusDot` component used by `RowStatusDot` inside `FilesTree`). Reads route through `com.nexus.storage::read_frontmatter` (handler 59). |

**Tail items also shipped (2026-05-14):**

| Mockup element | Status |
|----|----|
| J. Markdown table chrome (rounded surface, dashed row separators, header band) | ✅ `markdown.css` `.nexus-markdown-body table` rebuild: border-separate + rounded outer container, dashed row separators via `border-bottom: 1px dashed`, header band with uppercased / tracked label styling, alternating row tint via `color-mix(--background-secondary-alt)`. |
| N. Outline numbered prefix (`01` / `02` ember stripe on top-level rows) | ✅ `OutlineView.tsx::formatPrefix` + `.nx-outline__prefix` CSS in `shell.css`. Only top-level headings (depth=0) carry the prefix so the visual band anchors at section starts. |
| N tail. Word-count badge per outline row | ✅ `parse.ts::countWordsIn` + per-section sum in both `parseHeadings` and `treeToHeadings`; `OutlineHeading.wordCount` field; `.nx-outline__count` CSS with `compactCount` (950 → "950", 1240 → "1.2k", 12400 → "12k"). Zero-word sections hide the badge so an empty heading doesn't clutter the row. |

**Still deferred:**

- **Font bundling** — `font_imports` pulls Fraunces from Google Fonts; first-boot-offline launches see Georgia. Bundling woff2 is a separate offline-first workstream (binary commit + cargo features needed).

## Tests

23 new live-preview pipeline tests in `markdownRender.test.ts` cover Phase 2/3/4: path-shaped inline code detection, wikilink rendering including pipe-aliases + fragments, frontmatter parse + CRLF + list values + unclosed blocks, metadata bar splicing position, status-pill substitution in table cells / inline code / frontmatter, and callout rendering including unknown-kind rejection. Legacy `MarkdownDoc` tests at `tests/markdown-doc-bl053.test.ts` continue to pin the dormant `core/editorArea` path. Shell suite green at 1370/0 fail.

The bundled ember themes ship the token values the mockup uses, but the shell renders a much plainer surface than the mockup. Closing the gap is partly theme/CSS work and partly markdown-rendering / plugin work. This document inventories the gap, splits it into phases by ROI, and lists the decisions that have to land before code does.

## 1. Inventory of what the mockup actually contains

| # | Element                                                                                              | Subsystem                                |
| - | ---------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| A | Cool-slate chrome with single ember accent                                                           | **Theme**                                |
| B | Pill-shaped editor tabs with ember underline + tint when active                                      | **Shell CSS**                            |
| C | Active sidebar row: ember-soft fill + 2px ember left rail                                            | **Shell CSS** (mostly done)              |
| D | File-tree status dots (e.g. green dot next to `Backlog-Current`)                                     | **Shell + frontmatter wiring**           |
| E | Big serif H1 ("Nexus PRD Implementation Status")                                                     | **Editor typography**                    |
| F | Metadata bar under H1: `FORGE · NEXUS_WORK · UPDATED 2026-04-17 · Rolling tracking doc`              | **Markdown extension** (frontmatter pills) |
| G | Inline code styled differently when it looks like a file path or version (`crates/**`, `01-17`)      | **Markdown renderer**                    |
| H | Wikilinks rendered ember (`[[BACKLOG.md]]`)                                                          | **Markdown renderer**                    |
| I | Callout box ("Update cadence") with ember dot, raised surface                                        | **Markdown extension**                   |
| J | Markdown table rendered with rounded surface + dashed row separators                                 | **Markdown renderer**                    |
| K | Status pills inside the table cells (Complete / Substantial / Partial / Scaffolded / Not started / Deferred) | **Generalized component + content layer** |
| L | "LEGEND" section header in small-caps muted                                                          | **Editor typography**                    |
| M | Inspector panel: Outline / Backlinks / Graph segmented control                                       | **Inspector plugin**                     |
| N | Outline rows with `01` / `02` ember-numbered prefix + faint word-count badge                         | **Outline plugin**                       |
| O | Active outline row ember rail                                                                        | **Outline plugin** (mostly done)         |
| P | Status bar at bottom: `lap-working` with ember dot                                                   | **Shell statusbar**                      |
| Q | macOS-style traffic-light buttons inline with tabs                                                   | **Tauri OS chrome** (out of scope)       |

## 2. What's reachable vs. not

| Items   | Reach                  | Notes                                                                                                                                          |
| ------- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| A, B, C, E, L, P | **Theme + shell CSS** | Pure styling. Days, not weeks.                                                                                                                 |
| F, G, H, I       | **Markdown renderer** | Rendering features that don't exist in `nexus-editor` today. Each is a self-contained extension.                                               |
| J, K             | **Renderer + content schema** | Need to decide where status pills come from — frontmatter, Bases column, or new inline syntax. See §5 Q2.                                |
| M, N, O          | **Inspector / outline plugin** | The outline plugin exists; this is feature work on top.                                                                                  |
| D                | **File tree + frontmatter** | Need a way to project frontmatter status onto tree nodes. Prior art in the file explorer; mostly wiring.                                   |
| Q                | **Out of scope**       | Tauri uses OS-native window decorations. A custom titlebar already exists; faking traffic-lights inside React is portable across platforms but not connected to real window control.     |

## 3. Proposed phasing

Four phases ordered by ROI per hour. Each produces a visible improvement without depending on the next.

### Phase 1 — Chrome polish (~1 day)

The mockup's "feel" is ~70% chrome styling, ~30% rendering. Get the chrome to match first.

- Pill-shaped editor tabs with `--interactive-accent` underline + `--interactive-accent-soft` fill on active.
- Inspector panel segmented control (Outline / Backlinks / Graph) with ember active treatment.
- Status bar bottom-right: forge name + ember status dot.
- Fraunces serif for `h1` / `h2` in the editor; Inter sans body; JetBrains Mono code (theme already declares these — wire consumer CSS to use them).
- Bump ember accent visibility (already done; may fine-tune against real screenshots).

**Files touched:** `shell/src/shell/shell.css`, `shell/index.html` (font imports), `shell/src/plugins/core/editorArea/*`, `shell/src/plugins/nexus/outline/*`.
**Risk:** low. Existing components, restyling only.

### Phase 2 — Inline rendering: wikilinks + path-style code (~2 days)

Easiest of the markdown-renderer changes because the data model already exists.

- **Wikilinks.** The editor likely already parses `[[X]]`. Wrap them in `<a class="nx-wikilink">` and theme ember.
- **Inline code.** When `<code>`'s text matches `^[\w./*-]+$` AND contains `/`, add a `nx-codepath` class and tint ember. Keep neutral styling for prose code.
- **Frontmatter metadata bar.** Parse YAML frontmatter and render `tags`, `updated`, `category` as a pill row directly below H1. Small new component, no editor-engine changes.

**Files touched:** the editor markdown renderer (probably `shell/src/plugins/nexus/editor/`) + a small frontmatter-bar component.
**Risk:** medium. Depends on how invasive the editor's renderer is.

### Phase 3 — Callouts (~3–5 days)

Real markdown-extension work.

- **Pick a syntax.** Obsidian's `> [!info] Title\n> body` is the safest call — fixtures in this repo likely already use it, and migration cost is zero.
- **Add a parser hook** in the markdown pipeline. Support a small set of types: `info`, `warn`, `risk`, `update` (the mockup's "Update cadence" uses an ember dot).
- **Render** as a `<div class="nx-callout nx-callout--{type}">` with header + body slots, themed by the existing status tokens.
- Each type gets its own dot color: `--ok` / `--warn` / `--risk` / `--interactive-accent` / `--cool`.

**Files touched:** the editor markdown pipeline + a Callout component.
**Risk:** medium-high. Block-level extensions are real work.

### Phase 4 — Status pills + tree dots (~3–5 days)

The most specified piece in the mockup, and the most ambiguous about where data comes from. Decision required first (§5 Q2).

Working hypothesis pending that decision: tree dots come from each file's frontmatter (`status:`); the legend table renders pills via either typed inline syntax or a Bases query.

**Files touched:** new `<StatusPill>` component in shell, frontmatter projection into the file tree, possibly a Bases column type, and the renderer change to display values as pills in tables.
**Risk:** high. Touches three subsystems; needs a product decision before code.

## 4. Recommended starting point

If a single PR has to capture the mockup's first impression, **Phase 1 alone** does ~70% of the visible work. Phase 2 adds ~15% more. Phases 3 and 4 are the long tail — prioritize against the rest of the roadmap, not done speculatively.

Honest aggregate estimate: **3–4 weeks of focused work** for all four phases. **3 days** if Phase 1 is enough.

## 5. Open questions before any code lands

1. **Is callout syntax already used in this repo's fixtures?** If yes, which dialect? Driven answer commits us to it.
2. **Where do status pills get their value from — frontmatter, Bases, or a new inline syntax?** Gates Phase 4 entirely.
3. **Is bundling Fraunces acceptable?** Webfont = network call on first load; bundling the woff2 = ~100KB ship. The theme already declares the font; nobody's loaded it yet.
4. **Phase 1 only, or further?** No phase commits to the next.

## 6. Acceptance criteria (per phase, when scoped in)

To be filled in when a phase is greenlit. The current document is a plan, not a spec.
