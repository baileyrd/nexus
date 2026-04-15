/**
 * Tiny fuzzy subsequence matcher. Returns a numeric score or `null`
 * if `query` doesn't match `target` as an in-order subsequence.
 *
 * Scoring rewards consecutive matches, matches at word boundaries, and
 * early matches. Good enough for a command palette of O(100) entries;
 * swap for fuse.js / fzf if the catalog grows.
 */
export function fuzzyScore(query: string, target: string): number | null {
  if (!query) return 0;
  const q = query.toLowerCase();
  const t = target.toLowerCase();

  let score = 0;
  let qi = 0;
  let prevMatchIndex = -2;

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] !== q[qi]) continue;

    // Base: each matched character.
    score += 1;

    // Consecutive match bonus.
    if (ti === prevMatchIndex + 1) score += 4;

    // Word-boundary bonus (match at start or after separator).
    const prev = ti === 0 ? " " : t[ti - 1];
    if (/[\s\-._/:]/.test(prev)) score += 3;

    // Early-in-string bonus, tapering.
    if (ti < 8) score += 1;

    prevMatchIndex = ti;
    qi++;
  }

  if (qi < q.length) return null; // query not fully consumed
  return score;
}

export interface FuzzyRanked<T> {
  item: T;
  score: number;
}

/**
 * Rank items by `fuzzyScore(query, pick(item))`, filtering out
 * non-matches. Empty query returns all items in original order.
 */
export function fuzzyRank<T>(
  items: readonly T[],
  query: string,
  pick: (item: T) => string,
): FuzzyRanked<T>[] {
  if (!query) return items.map((item) => ({ item, score: 0 }));
  const ranked: FuzzyRanked<T>[] = [];
  for (const item of items) {
    const score = fuzzyScore(query, pick(item));
    if (score !== null) ranked.push({ item, score });
  }
  ranked.sort((a, b) => b.score - a.score);
  return ranked;
}
