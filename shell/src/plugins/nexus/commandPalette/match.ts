import type { CommandEntry } from '../../../types/plugin'
import { configStore } from '../../../stores/configStore'

export interface ScoredCommand {
  command: CommandEntry
  score: number
}

/** Cap; long lists are noise in a palette. */
const MAX_PALETTE_RESULTS = 50
const CONFIG_KEY_PALETTE_LIMIT = 'commandPalette.maxResultsLimit'

/**
 * Subsequence fuzzy match over `<category> <title>`. Lower score is
 * better — we use the index of the last matched character, so tighter
 * clusters near the start of the haystack rank above scattered late
 * matches. Ties break on title ascending.
 *
 * Empty query → all commands, sorted alphabetically by title.
 *
 * Deliberately simple — no library, no per-character bonuses, no
 * highlighting. Adding fzf-style scoring is a follow-up if/when this
 * proves too coarse.
 */
export function filterCommands(
  commands: CommandEntry[],
  query: string,
): ScoredCommand[] {
  const limit = configStore.get<number>(CONFIG_KEY_PALETTE_LIMIT, MAX_PALETTE_RESULTS) ?? MAX_PALETTE_RESULTS
  const q = query.toLowerCase().trim()

  if (q.length === 0) {
    return commands
      .slice()
      .sort((a, b) => a.title.localeCompare(b.title))
      .slice(0, limit)
      .map((command) => ({ command, score: 0 }))
  }

  const scored: ScoredCommand[] = []
  for (const command of commands) {
    const haystack = `${command.category ?? ''} ${command.title}`.toLowerCase()
    let qi = 0
    let lastMatchIdx = -1
    for (let hi = 0; hi < haystack.length && qi < q.length; hi++) {
      if (haystack[hi] === q[qi]) {
        lastMatchIdx = hi
        qi++
      }
    }
    if (qi === q.length) {
      scored.push({ command, score: lastMatchIdx })
    }
  }

  scored.sort((a, b) => {
    if (a.score !== b.score) return a.score - b.score
    return a.command.title.localeCompare(b.command.title)
  })

  return scored.slice(0, limit)
}
