# ADR 0019: Read-Only Support for Obsidian `.base` Format

**Date:** 2026-04-28
**Status:** Accepted

## Context

Nexus's bases subsystem (ADR-implicit; see `docs/bases-shell-plan.md`)
stores each base as a **`.bases` directory** containing `schema.json`,
`metadata.json`, and a `records/` tree of one TOML file per row. The
storage path (`crates/nexus-storage/src/lib.rs::base_create` and
siblings) and the shell plugin (`shell/src/plugins/nexus/bases/`) are
both built around this layout. Records live with the base.

Obsidian (since v1.7) ships a different format: a **single `.base`
YAML file** with three top-level keys — `filters`, `properties`, and
`views`. Records are *not* stored; rows are computed at view time by
querying every note in the vault, applying the filter expression
against frontmatter + file intrinsics, and projecting the configured
properties as columns. A `.base` file is a saved query, not a table.

Today a user dropping an Obsidian vault into a Nexus forge sees their
`.base` files render as raw YAML in the text editor — the bases plugin
only registers the `bases` extension (`shell/src/plugins/nexus/bases/index.ts:68`),
so single-file `.base` falls through to CodeMirror. Reported as a
display bug; root cause is a feature gap.

The two formats are not interchangeable. Obsidian's stores no rows;
Nexus's stores all rows. A round-trip in either direction is lossy
(losing computed-from-filter rows in one direction; losing user-entered
rows in the other).

## Decision

**Add a parallel read-only path for the Obsidian `.base` format,
alongside the existing read/write `.bases` directory format. Do not
unify the two.**

Concretely:

1. New module `crates/nexus-types/src/obsidian_base.rs` defines an
   `ObsidianBase` struct with `serde_yaml` round-trip. Reuses
   `BaseView`/`SortRule` shapes where they overlap; adds a `FilterNode`
   tree (`And`/`Or`/`Not`/`Expr(String)`) for the filter section.
2. New filter expression evaluator
   (`crates/nexus-storage/src/obsidian_base/filter.rs`) implements a
   documented v1 grammar subset (see *Grammar* below). Unsupported
   expressions are surfaced as structured `UnsupportedFilter` values,
   not silent failures.
3. New IPC handler `obsidian_base_query` on `com.nexus.storage`. Args
   `{ path: string }`, returns `{ schema, records, views,
   unsupported_filters }`. Records are shaped like the existing
   `BaseRecord` so the existing view layer (`BasesTable`,
   `BasesGallery`, etc.) consumes them unchanged.
4. Shell plugin registers extension `'base'` mapped to the same view
   type as `'bases'`. `kernelClient.ts` branches on extension:
   `.bases` → existing `base_*` IPC; `.base` → new
   `obsidian_base_query` (no record / schema CRUD methods bound).
   `basesStore` carries a per-tab `readOnly: boolean`; mutation
   actions become no-ops when set.
5. UI hides add-row, add-column, delete, and the schema editor when
   `readOnly` is true. An `unsupported_filters` banner renders above
   the table when non-empty.

### Grammar

v1 supports the subset that covers ~all `.base` files we've inspected
in real Obsidian vaults:

```
expr      := lhs op rhs
           | lhs '.' method '(' literal ')'
           | '!' expr
lhs       := identifier ('.' identifier)*
op        := '==' | '!=' | '>' | '<' | '>=' | '<='
method    := contains | startsWith | endsWith
rhs       := literal
literal   := string | number | boolean | null
```

LHS resolves against:

- Bare identifier → frontmatter property of that name.
- `file.name` → filename without extension.
- `file.path` → forge-relative path.
- `file.ext` → extension without leading dot.
- `file.folder` → containing folder.
- `file.ctime` / `file.mtime` → Unix seconds (compared as numbers).
- `file.tags` → array of tags from frontmatter + inline.

Anything outside this grammar — Obsidian formulas, `taggedWith()`,
nested method chains, computed properties — is collected into
`unsupported_filters` and excluded from row evaluation. The user sees
a banner; the table shows whatever rows the supported subset accepts.

## Alternatives considered

### A. Convert `.base` → `.bases` on open

Auto-migrate the YAML into a directory layout the existing code
already handles. Rejected: the formats are semantically different.
A `.base` file is a *query* over the vault; converting it to a
`.bases` directory snapshots the current result set into stored rows
that immediately diverge from the live notes. The user's intent
("show me all notes where `type == literature`") is destroyed at the
moment of conversion. Migration is also one-way — round-trip is lossy.

### B. Render `.base` as a code-only editor with a "preview" toggle

Treat `.base` as a YAML file with a side panel showing computed rows.
Rejected: doubles the maintenance surface (two view paths to keep in
sync) and gives a worse default UX than the table view the user
expects from Obsidian.

### C. Full feature parity (read-write `.base` with formula support)

Rejected for v1. Obsidian's expression language has computed
properties, formulas, embeds (`![[x.base]]`), and ongoing additions —
implementing it in full is open-ended scope. The grammar subset
above covers the common case; we extend it driven by what real files
in the wild actually need, not speculation.

### D. Unify the two formats behind a single `Base` abstraction

Rejected. The storage models diverge fundamentally (rows-as-files
vs. rows-as-query-results). A unified abstraction has to handle the
union of both update models, which leaks complexity into every
caller. The view layer is the correct unification point — both
formats produce `BaseRecord`-shaped data at the boundary, and
everything below the view consumes it the same way.

## Consequences

### Positive

- **Obsidian vaults render correctly out of the box.** Drop-in
  compatibility for the most common `.base` files removes a visible
  papercut.
- **No risk to the existing `.bases` path.** New code is parallel;
  existing handlers, schemas, and tests are untouched.
- **View layer reuse.** `BasesTable`/`BasesGallery`/`BasesBoard` and
  the rest of the rendering tree are shared, so improvements to the
  `.bases` view automatically benefit `.base`.
- **Visible failure mode for unsupported filters.** Users see a
  banner naming the expression that didn't evaluate, instead of an
  empty table or a wrong-looking one.

### Negative

- **Two formats in the codebase.** Future bases features need to
  decide: `.bases`-only, `.base`-only (read), or both. Most editing
  features will be `.bases`-only.
- **Filter grammar maintenance.** v1 supports a subset; we'll accrue
  feature requests to extend it. Mitigation: the grammar is one
  module with table-tested operators — additions are local. The
  unsupported-filter banner converts user demand into an actionable
  signal.
- **Query cost on large vaults.** A `.base` query touches every note.
  v1 evaluates per-row in Rust against the existing SQLite-indexed
  frontmatter. For `prop == value` shapes we can push the predicate
  into SQL later; defer until benchmarks justify it.
- **No editing in v1.** Users who want to *modify* a `.base` file
  edit YAML directly in a text editor. Acceptable: Obsidian's own
  UX for these files is also primarily YAML-edit; in-place visual
  editors arrived only recently.

### Out of scope (v1 — revisit when justified)

- Writing `.base` files from the bases UI.
- Obsidian formulas (`formula.totalPages = pages * 1.0`).
- Embedded bases (`![[x.base]]` inside a markdown note).
- Conversion between `.base` and `.bases` formats.
- Predicate pushdown into SQL.
