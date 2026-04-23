// Phase 4 of docs/notion-block-ux-plan.md — markdown input rules.
//
// Most of the block-creation shortcuts from the plan (`#` / `##` /
// `###` / `-` / `1.` / `>` / `---`) already work because they're
// authored markdown — the kernel reparse turns them into the right
// `BlockType`. This extension fills the gaps where a user's
// expectation diverges from the markdown spec:
//
//   `[] ` → `- [ ] `     (todo: markdown needs the list-item prefix)
//   `[x] ` → `- [x] `    (checked todo)
//   `* ` → `- `          (normalize to the single bullet style we emit)
//   `+ ` → `- `          (likewise)
//
// The rules fire when the typed trigger character lands at the start
// of a line (only whitespace before the pattern) and the inserted
// text exactly matches the pattern. Running after the insert keeps
// the handler simple — we just replace the prefix range.

import type { Extension } from '@codemirror/state'
import { EditorView } from '@codemirror/view'

interface InputRule {
  /** Final character that triggers the rule check (so we can fast-skip
   *  unrelated inputs). Typically a space. */
  trigger: string
  /** Pattern that must match the text between line start and the
   *  caret (after the trigger lands). */
  match: RegExp
  /** Replacement for the matched segment. */
  replace: string
}

const RULES: InputRule[] = [
  { trigger: ' ', match: /^\s*\[\]\s$/, replace: '- [ ] ' },
  { trigger: ' ', match: /^\s*\[x\]\s$/, replace: '- [x] ' },
  { trigger: ' ', match: /^\s*\[X\]\s$/, replace: '- [x] ' },
  { trigger: ' ', match: /^\s*\*\s$/, replace: '- ' },
  { trigger: ' ', match: /^\s*\+\s$/, replace: '- ' },
]

export function inputRulesExt(): Extension {
  return EditorView.updateListener.of((update) => {
    if (!update.docChanged) return
    // Only react to single-char user inputs so paste / programmatic
    // edits don't accidentally trigger rules.
    let inserted = ''
    update.changes.iterChanges((_fA, _tA, _fB, _tB, text) => {
      inserted += text.toString()
    })
    if (inserted.length !== 1) return

    const view = update.view
    const caret = view.state.selection.main.head
    const line = view.state.doc.lineAt(caret)
    const prefix = line.text.slice(0, caret - line.from)

    for (const rule of RULES) {
      if (inserted !== rule.trigger) continue
      const m = prefix.match(rule.match)
      if (!m) continue
      // Replace the matched slice at the line start.
      const matchStart = line.from + prefix.length - m[0].length
      const matchEnd = line.from + prefix.length
      // Dispatch in a microtask so we don't reenter the update callback.
      queueMicrotask(() => {
        view.dispatch({
          changes: { from: matchStart, to: matchEnd, insert: rule.replace },
          userEvent: 'input.rule',
        })
      })
      return
    }
  })
}
