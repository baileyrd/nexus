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
import jsxA11y from 'eslint-plugin-jsx-a11y'

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
      'jsx-a11y': jsxA11y,
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
      // OI-17 — plugin-API deprecation gate. The CI-enforced half of
      // the deprecation policy: each `importNames` entry below is
      // mirrored by an `@deprecated` JSDoc tag in the package source
      // and a row in `packages/nexus-extension-api/DEPRECATED.md`.
      // Adding one of those three without the others is a process
      // bug — see DEPRECATED.md for the protocol. The list is empty
      // today; the rule still loads so a future deprecation only has
      // to add a name, not stand up the rule.
      //
      // We use `no-restricted-imports` rather than the type-aware
      // `@typescript-eslint/no-deprecated` because the latter requires
      // `parserOptions.project`, which the comment at the top of this
      // file deliberately defers on lint-cost grounds. The trade-off
      // is that this list is hand-maintained instead of derived from
      // the JSDoc; it's worth the manual step until type-aware lint
      // becomes affordable.
      'no-restricted-imports': ['error', {
        paths: [
          {
            name: '@nexus/extension-api',
            importNames: [],
            message:
              'This export is deprecated — see packages/nexus-extension-api/DEPRECATED.md for the replacement.',
          },
        ],
      }],
      // R19 / #202 — raw `console.*` bypasses `clientLogger`, which is the
      // single sink that timestamps, level-tags, and (in prod) ships log
      // lines through the Tauri bridge to `~/.nexus-shell/logs/`. `warn`/
      // `error` are allowed (matches the eslint default and the ~75 sites
      // that already surface user-visible warnings) but `log`/`info`/
      // `debug`/`trace` must go through `getClientLogger()` or an explicit
      // `eslint-disable-next-line no-console` with a comment justifying it.
      // `clientLogger.ts` is the legitimate owner of `console.*` and is
      // exempted in the override block below.
      'no-console': ['error', { allow: ['warn', 'error'] }],
      // #197 / R14 — a11y baseline via `eslint-plugin-jsx-a11y`. The
      // recommended preset covers the high-signal markup mistakes
      // (missing `alt`, label-for-control, keyboard handlers on
      // non-interactive elements, role validity, …). Set to `warn`
      // rather than `error` on first introduction: about half of the
      // shell's JSX surface has not been audited for keyboard /
      // screen-reader access yet, and gating PRs on a hard error
      // would force every contributor to land a wide cleanup. As
      // sites get fixed, individual rules can graduate to `error`
      // via per-rule overrides, and the preset itself can flip to
      // `error` once the warning count is at zero.
      ...Object.fromEntries(
        Object.keys(jsxA11y.configs.recommended.rules).map((rule) => [
          rule,
          'warn',
        ]),
      ),
    },
  },
  {
    // The two `clientLogger.ts` modules legitimately dispatch to `console.*`
    // — they ARE the indirection #202 wants every other caller routed
    // through. Without this override the lint rule would forbid its own
    // sink.
    files: [
      'src/clientLogger.ts',
      'src/host/clientLogger.ts',
    ],
    rules: {
      'no-console': 'off',
    },
  },
)
