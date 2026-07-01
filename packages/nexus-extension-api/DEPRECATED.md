# Deprecated — `@nexus/extension-api`

This file is the **live registry** of deprecations that are currently active
in `@nexus/extension-api`. Removed surfaces graduate to the historical
archive in [`/DEPRECATED.md`](../../DEPRECATED.md) at the repo root, which
also holds the broader deprecation policy (window, runtime warning, etc.).

## How a deprecation lands

When you deprecate a name (DTO field, exported type, method, property),
do all three of the following in the **same PR**:

1. **JSDoc.** Add a `@deprecated` block on the symbol in `src/index.ts`
   spelling out the replacement and the target removal version. The
   TypeScript language service surfaces this as a strikethrough + warning
   in every plugin author's IDE — that's the primary author-time signal.

   ```ts
   /**
    * @deprecated Since 1.1 — use `NewThing` instead. Will be removed in 1.2.
    *
    * The shape diverged from the runtime contract; see DEPRECATED.md.
    */
   readonly oldField?: string;
   ```

2. **This file.** Add a row to the table below with the symbol path, the
   announce version, the target removal version, and the migration
   pointer. Keep entries sorted by *announce version* descending (newest
   first), then by symbol path. When the removal lands, move the row to
   the [historical archive](../../DEPRECATED.md) and delete the
   `@deprecated` JSDoc.

3. **ESLint rule.** Add an `importNames` entry to
   [`shell/eslint.config.js`](../../shell/eslint.config.js)'s
   `no-restricted-imports` block keyed on `@nexus/extension-api`. That
   rule fires at lint time when shell code imports the deprecated name —
   the type-aware `@typescript-eslint/no-deprecated` rule would do this
   automatically, but enabling it workspace-wide currently costs more
   than the lint budget allows (see the comment at the top of the
   eslint config). Until that changes, the manual `importNames` table is
   the CI-enforced hand-off; the JSDoc is the IDE-time hand-off.

## Currently deprecated

| Symbol | Announced | Removal target | Replacement |
|--------|-----------|----------------|-------------|
| `ScriptPlugin` (`src/index.ts`) | 0.1.0 — 2026-07-01 | 0.2.0 | `SandboxedPlugin` (`src/sandbox/plugin.ts`) for sandboxed community plugins, or the shell-side `Plugin` interface (`shell/src/types/plugin.ts`) for first-party plugins. No runtime ever implemented the four-hook `ScriptPlugin` lifecycle; #187 resolved the entry-point contract to the two-hook `activate`/`deactivate` shape both runtimes already use. |

## Why three signals?

- **JSDoc** catches the developer in their editor *while they're typing
  the import* — the highest-leverage moment.
- **DEPRECATED.md** is the human-readable migration guide that's findable
  via `git grep` and from the package README.
- **ESLint rule** is the CI gate. A plugin author who imports a
  deprecated name fails their lint run instead of finding out at
  publish time.

The cost of the third signal is one config entry per deprecation. The
benefit is that nothing slips through if the IDE warning gets ignored or
the docs go un-read.
