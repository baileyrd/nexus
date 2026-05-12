// BL-068 Phase 4 — split-view preview pane for the Theme Builder.
//
// Renders a representative forge document inside a CSS-variable
// scope. The wrapper applies `composeOverridesForPreview(...)` as
// inline style, so every `var(--nx-*)` reference inside resolves
// against the current builder edits — independent of whatever mode
// the live shell is in.
//
// Two render modes:
//   • single — one preview column scoped to the active set of
//     builder overrides (the kernel-mode-respecting flow)
//   • dual   — two columns side-by-side scoped to the per-mode
//     light + dark overrides, useful when authoring a theme that
//     ships both modes in one file
//
// All token math lives in `previewTokens.ts` (pure module, unit-
// tested). This component is rendering only.

import type { CSSProperties, ReactElement } from 'react'

import { composeOverridesForPreview } from './previewTokens'

interface PreviewSnippetProps {
  baseVars: Record<string, string>
  overrides: Record<string, string>
  /** Optional column label rendered above the snippet — used in dual
   *  mode so the user knows which side they're looking at. */
  label?: string
}

function PreviewSnippet({
  baseVars,
  overrides,
  label,
}: PreviewSnippetProps): ReactElement {
  const vars = composeOverridesForPreview(baseVars, overrides) as CSSProperties
  return (
    <div
      // The CSS-variable scope is the whole point of this wrapper —
      // every `var(--nx-*)` inside resolves against the spread vars,
      // shadowing the document-level :root values for this subtree.
      style={{
        ...vars,
        background: 'var(--nx-bg-primary)',
        color: 'var(--nx-text-primary)',
        border: '1px solid var(--background-modifier-border)',
        borderRadius: 'var(--radius-s, 4px)',
        overflow: 'hidden',
        fontFamily: 'var(--nx-editor-font-family, var(--font-text, inherit))',
        fontSize: 13,
        lineHeight: 1.55,
        display: 'flex',
        flexDirection: 'column',
      }}
      data-testid={label ? `theme-preview-${label.toLowerCase()}` : 'theme-preview'}
    >
      {label && (
        <div
          style={{
            padding: '6px 10px',
            background: 'var(--nx-bg-secondary)',
            color: 'var(--nx-text-secondary)',
            fontFamily: 'var(--font-interface)',
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: '0.04em',
            textTransform: 'uppercase',
            borderBottom: '1px solid var(--background-modifier-border)',
          }}
        >
          {label}
        </div>
      )}
      <div
        style={{
          padding: '14px 18px',
          background: 'var(--nx-editor-bg, var(--nx-bg-primary))',
          flex: 1,
          overflow: 'auto',
        }}
      >
        <h1
          style={{
            color: 'var(--nx-prose-heading-color, var(--nx-text-primary))',
            fontSize: 22,
            margin: '0 0 6px 0',
            fontWeight: 700,
          }}
        >
          Forge document
        </h1>
        <p
          style={{
            color: 'var(--nx-text-secondary)',
            margin: '0 0 12px 0',
            fontSize: 11,
          }}
        >
          A representative slice — every token the builder edits has a
          surface here.
        </p>

        <h2
          style={{
            color: 'var(--nx-prose-heading-color, var(--nx-text-primary))',
            fontSize: 16,
            margin: '14px 0 6px 0',
            fontWeight: 600,
          }}
        >
          Body and inline marks
        </h2>
        <p style={{ margin: '0 0 8px 0' }}>
          The quick brown fox follows{' '}
          <a
            href="#"
            onClick={(e) => e.preventDefault()}
            style={{
              color: 'var(--nx-prose-link-color, var(--nx-color-primary))',
              textDecoration: 'underline',
            }}
          >
            a wikilink
          </a>{' '}
          and runs past <code style={inlineCodeStyle}>file_path:42</code> on
          the way home. Secondary text reads{' '}
          <span style={{ color: 'var(--nx-text-secondary)' }}>like this</span>{' '}
          and tertiary{' '}
          <span style={{ color: 'var(--nx-text-tertiary)' }}>like this</span>.
        </p>

        <h3
          style={{
            color: 'var(--nx-prose-heading-color, var(--nx-text-primary))',
            fontSize: 13,
            margin: '14px 0 6px 0',
            fontWeight: 600,
            textTransform: 'uppercase',
            letterSpacing: '0.05em',
          }}
        >
          Code block
        </h3>
        <pre style={codeBlockStyle}>
          <span style={{ color: 'var(--nx-syntax-keyword)' }}>fn </span>
          <span style={{ color: 'var(--nx-syntax-function)' }}>greet</span>
          <span>(</span>
          <span>name</span>
          <span>: </span>
          <span style={{ color: 'var(--nx-syntax-type)' }}>&str</span>
          <span>) {'{'}</span>
          {'\n  '}
          <span style={{ color: 'var(--nx-syntax-comment)' }}>// hello</span>
          {'\n  '}
          <span style={{ color: 'var(--nx-syntax-function)' }}>println!</span>
          <span>(</span>
          <span style={{ color: 'var(--nx-syntax-string)' }}>"hi {'{}'}, "</span>
          <span>, name);</span>
          {'\n  '}
          <span style={{ color: 'var(--nx-syntax-keyword)' }}>let </span>
          <span>n = </span>
          <span style={{ color: 'var(--nx-syntax-number)' }}>42</span>
          <span>;</span>
          {'\n}'}
        </pre>

        <h3
          style={{
            color: 'var(--nx-prose-heading-color, var(--nx-text-primary))',
            fontSize: 13,
            margin: '14px 0 6px 0',
            fontWeight: 600,
            textTransform: 'uppercase',
            letterSpacing: '0.05em',
          }}
        >
          Callout
        </h3>
        <div style={calloutStyle}>
          <strong style={{ color: 'var(--nx-color-primary)' }}>Note:</strong>{' '}
          Callouts use <code style={inlineCodeStyle}>--nx-callout-bg</code>{' '}
          and <code style={inlineCodeStyle}>--nx-callout-border</code>.
        </div>

        <h3
          style={{
            color: 'var(--nx-prose-heading-color, var(--nx-text-primary))',
            fontSize: 13,
            margin: '14px 0 6px 0',
            fontWeight: 600,
            textTransform: 'uppercase',
            letterSpacing: '0.05em',
          }}
        >
          Table
        </h3>
        <table style={tableStyle}>
          <thead>
            <tr>
              <th style={tableHeadCellStyle}>Token</th>
              <th style={tableHeadCellStyle}>Surface</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td style={tableCellStyle}>--nx-bg-primary</td>
              <td style={tableCellStyle}>Body background</td>
            </tr>
            <tr>
              <td style={tableCellStyle}>--nx-bg-secondary</td>
              <td style={tableCellStyle}>Sidebar / chrome</td>
            </tr>
            <tr>
              <td style={tableCellStyle}>--nx-bg-tertiary</td>
              <td style={tableCellStyle}>Hover</td>
            </tr>
          </tbody>
        </table>

        <div
          style={{
            display: 'flex',
            gap: 6,
            marginTop: 12,
            flexWrap: 'wrap',
          }}
        >
          <button type="button" style={primaryButtonStyle}>
            Primary
          </button>
          <button type="button" style={secondaryButtonStyle}>
            Secondary
          </button>
          <button type="button" style={mutedTagStyle}>
            Muted tag
          </button>
        </div>
      </div>
    </div>
  )
}

const inlineCodeStyle: CSSProperties = {
  fontFamily: 'var(--font-monospace)',
  background: 'var(--nx-bg-tertiary)',
  color: 'var(--nx-syntax-string, var(--nx-text-primary))',
  padding: '0 4px',
  borderRadius: 3,
  fontSize: '0.9em',
}

const codeBlockStyle: CSSProperties = {
  fontFamily: 'var(--font-monospace)',
  background: 'var(--nx-bg-secondary)',
  color: 'var(--nx-text-primary)',
  padding: '8px 10px',
  borderRadius: 3,
  fontSize: 12,
  lineHeight: 1.5,
  margin: 0,
  whiteSpace: 'pre',
  overflow: 'auto',
}

const calloutStyle: CSSProperties = {
  background: 'var(--nx-callout-bg)',
  border: '1px solid var(--nx-callout-border)',
  borderRadius: 4,
  padding: '8px 10px',
  fontSize: 12,
  color: 'var(--nx-text-primary)',
}

const tableStyle: CSSProperties = {
  width: '100%',
  borderCollapse: 'collapse',
  fontSize: 12,
  background: 'var(--nx-bg-secondary)',
  color: 'var(--nx-text-primary)',
  border: '1px solid var(--background-modifier-border)',
}

const tableHeadCellStyle: CSSProperties = {
  textAlign: 'left',
  padding: '4px 8px',
  background: 'var(--nx-bg-tertiary)',
  color: 'var(--nx-text-secondary)',
  fontWeight: 600,
  borderBottom: '1px solid var(--background-modifier-border)',
}

const tableCellStyle: CSSProperties = {
  padding: '4px 8px',
  borderTop: '1px solid var(--background-modifier-border)',
  fontFamily: 'var(--font-monospace)',
  fontSize: 11,
}

const primaryButtonStyle: CSSProperties = {
  background: 'var(--nx-color-primary)',
  color: 'var(--text-on-accent, #fff)',
  border: 0,
  borderRadius: 3,
  padding: '4px 10px',
  fontFamily: 'var(--font-interface)',
  fontSize: 11,
  cursor: 'pointer',
}

const secondaryButtonStyle: CSSProperties = {
  background: 'var(--nx-color-secondary)',
  color: 'var(--text-on-accent, #fff)',
  border: 0,
  borderRadius: 3,
  padding: '4px 10px',
  fontFamily: 'var(--font-interface)',
  fontSize: 11,
  cursor: 'pointer',
}

const mutedTagStyle: CSSProperties = {
  background: 'var(--nx-bg-tertiary)',
  color: 'var(--nx-text-muted)',
  border: '1px solid var(--background-modifier-border)',
  borderRadius: 3,
  padding: '4px 10px',
  fontFamily: 'var(--font-interface)',
  fontSize: 11,
  cursor: 'pointer',
}

interface BuilderPreviewProps {
  baseVars: Record<string, string>
  /** Builder overrides for the active edit set. In single-mode this
   *  is `builderOverrides`; in dual-mode the parent passes
   *  `lightOverrides` and `darkOverrides` separately via
   *  `dualOverrides`. */
  overrides: Record<string, string>
  /** Set when the builder is in dual mode and the preview should
   *  render two columns side-by-side instead of one. */
  dualOverrides?: {
    light: Record<string, string>
    dark: Record<string, string>
  }
}

export function BuilderPreview({
  baseVars,
  overrides,
  dualOverrides,
}: BuilderPreviewProps): ReactElement {
  if (dualOverrides) {
    return (
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: 8,
          padding: 12,
          height: '100%',
          overflow: 'auto',
          background: 'var(--background-secondary)',
        }}
      >
        <PreviewSnippet
          baseVars={baseVars}
          overrides={dualOverrides.light}
          label="Light"
        />
        <PreviewSnippet
          baseVars={baseVars}
          overrides={dualOverrides.dark}
          label="Dark"
        />
      </div>
    )
  }
  return (
    <div
      style={{
        padding: 12,
        height: '100%',
        overflow: 'auto',
        background: 'var(--background-secondary)',
      }}
    >
      <PreviewSnippet baseVars={baseVars} overrides={overrides} />
    </div>
  )
}
