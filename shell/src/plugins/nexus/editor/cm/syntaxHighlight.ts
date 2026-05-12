// Theme-aware syntax highlight style for the editor's baseline.
//
// CM6 ships `defaultHighlightStyle` which produces sensible colours,
// but its palette is fixed at import time and doesn't follow the
// active Nexus theme. This style maps `tags.*` to `var(--syntax-*)`
// custom properties declared in `shell/index.html` so theme overrides
// reach syntax tokens automatically.

import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags as t } from '@lezer/highlight'

export const nexusHighlightStyle = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.operatorKeyword, t.definitionKeyword],
    color: 'var(--syntax-keyword)' },
  { tag: [t.string, t.special(t.string), t.escape, t.regexp],
    color: 'var(--syntax-string)' },
  { tag: [t.number, t.integer, t.float],
    color: 'var(--syntax-number)' },
  { tag: [t.bool, t.null, t.atom],
    color: 'var(--syntax-atom)' },
  { tag: [t.lineComment, t.blockComment, t.docComment],
    color: 'var(--syntax-comment)',
    fontStyle: 'italic' },
  { tag: [t.propertyName, t.attributeName, t.labelName],
    color: 'var(--syntax-property)' },
  { tag: t.function(t.variableName),
    color: 'var(--syntax-function)' },
  { tag: [t.typeName, t.className, t.namespace],
    color: 'var(--syntax-type)' },
  { tag: [t.operator, t.compareOperator, t.arithmeticOperator, t.logicOperator,
          t.bitwiseOperator, t.updateOperator],
    color: 'var(--syntax-operator)' },
  { tag: [t.punctuation, t.bracket, t.paren, t.brace, t.squareBracket, t.angleBracket, t.separator],
    color: 'var(--syntax-punctuation)' },
  { tag: [t.meta, t.processingInstruction, t.annotation],
    color: 'var(--syntax-meta)' },
  { tag: [t.url, t.link],
    color: 'var(--syntax-link)',
    textDecoration: 'underline' },
  { tag: t.invalid,
    color: 'var(--syntax-invalid)' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.strong, fontWeight: 'bold' },
])

/** Drop-in extension for `baselineExtensions`. `fallback: true` keeps
 *  CM's defaults active for tags we don't override, so adding a new
 *  language won't suddenly render its tokens uncoloured. */
export const nexusSyntaxHighlighting = syntaxHighlighting(nexusHighlightStyle, {
  fallback: true,
})
