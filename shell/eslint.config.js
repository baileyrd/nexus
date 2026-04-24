// shell/eslint.config.js
//
// OI-06 — ESLint 9 flat config.
//
// Two things this file does:
//
//   1. Pins a project-local config file at the shell package root.
//      Under flat-config mode, ESLint searches upward from the
//      target file and stops at the nearest `eslint.config.{js,ts,mjs}`
//      — it never falls through to a user-global `~/.eslintrc.json`.
//      That kills the shadowing failure the OI-06 audit flagged
//      (personal config referenced plugins that aren't installed in
//      the shell workspace, so `pnpm lint` crashed before it started).
//
//   2. Runs the `@typescript-eslint` recommended rule set against the
//      TypeScript + TSX sources under `src/`. No type-aware rules
//      yet — those require `parserOptions.project` which slows the
//      lint run substantially. A follow-up can opt specific
//      directories into the typed-linting preset if the cost is
//      acceptable at CI time.
//
// Tests (`**/*.test.ts`, `tests/`) and generated files are excluded
// to mirror the prior `eslint src --ext .ts,.tsx` invocation that
// only scanned the production tree. Tests live in `shell/tests/` and
// as siblings of their implementations (`src/**/*.test.ts`); both
// are skipped.

import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'

export default tseslint.config(
  {
    // Top-level ignore patterns. Applied before `files` matching so
    // a file excluded here is never linted regardless of any later
    // config object.
    ignores: [
      'dist/**',
      'dist-*/**',
      'node_modules/**',
      'e2e/**',
      'src-tauri/**',
      'src/**/*.test.ts',
      'src/**/*.test.tsx',
      'tests/**',
    ],
  },
  // typescript-eslint's flat-config "recommended" preset. Enables the
  // parser + a curated rule set that doesn't need type information.
  ...tseslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    plugins: {
      'react-hooks': reactHooks,
    },
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
    },
    rules: {
      // React hooks correctness rules. `rules-of-hooks` catches
      // conditional hook calls (a real bug); `exhaustive-deps` is
      // set to warn rather than error so intentional empty-deps
      // effects (e.g. one-shot subscribes in TerminalView) can
      // stand with an inline disable comment — the disables were
      // load-bearing under ESLint 8 too, just not enforced because
      // the plugin wasn't wired in.
      ...reactHooks.configs.recommended.rules,
      'react-hooks/exhaustive-deps': 'warn',
      // Shell convention: intentional-unused vars use a leading
      // underscore. Match the @typescript-eslint default without
      // having to add eslint-disable-next-line annotations at each
      // guard-only binding.
      '@typescript-eslint/no-unused-vars': [
        'warn',
        {
          argsIgnorePattern: '^_',
          varsIgnorePattern: '^_',
          caughtErrorsIgnorePattern: '^_',
        },
      ],
      // `any` shows up legitimately in the plugin API contract
      // (e.g. `ComponentType<any>` for plugin-supplied components
      // whose prop types aren't statically knowable). Drop to warn
      // so those sites don't block lint; sites in implementation
      // code still get flagged for review.
      '@typescript-eslint/no-explicit-any': 'warn',
    },
  },
)
