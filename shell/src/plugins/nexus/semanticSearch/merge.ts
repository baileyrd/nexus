// shell/src/plugins/nexus/semanticSearch/merge.ts
//
// BL-040 — combine keyword (com.nexus.storage::search) and semantic
// (com.nexus.ai::semantic_search) results into a single ranked list.
//
// Strategy (matches the BL-040 prompt):
//   1. Normalise each list to [0, 1] by dividing by the max raw score
//      in that list. Empty / all-zero list → all entries score 0.
//   2. score-blend = 0.5 * keyword + 0.5 * semantic for items that
//      appear in both lists (matched by `file_path`).
//   3. Items present in only one list keep that list's normalised
//      score, multiplied by 0.5 (so a co-occurrence wins over a
//      single-source hit).
//   4. Sort descending. Cap at `limit` (default 30).
//
// The merger is exposed as a pure function so it's trivially testable
// without spinning up the kernel.

/** A keyword hit as returned by `com.nexus.storage::search`. */
export interface KeywordHit {
  /** Forge-relative path of the source file. */
  file_path: string
  /** Snippet to show next to the result. */
  excerpt?: string
  /** Raw BM25-ish score from Tantivy. */
  score: number
}

/** A semantic hit (one element of `vector_query`'s response). */
export interface SemanticHit {
  /** Forge-relative path of the source file. */
  file_path: string
  /** Originating block id — kept so future UI can deep-link. */
  block_id?: number
  /** Chunk text — used as snippet when no keyword excerpt is available. */
  chunk_text?: string
  /** Cosine similarity in [-1, 1] (typically [0, 1] for normalised vectors). */
  score: number
}

/** A merged result row: file path, blended score, best snippet, source flags. */
export interface MergedHit {
  file_path: string
  score: number
  snippet: string
  /** True when this file appeared in the keyword list. */
  hasKeyword: boolean
  /** True when this file appeared in the semantic list. */
  hasSemantic: boolean
}

const DEFAULT_LIMIT = 30
const KEYWORD_WEIGHT = 0.5
const SEMANTIC_WEIGHT = 0.5
const SINGLE_SOURCE_DAMPING = 0.5

/** Normalise a list of scores to [0, 1] by dividing by the max. */
function normaliseByMax<T extends { score: number }>(
  hits: readonly T[],
): Map<string, { hit: T; norm: number }> {
  let max = 0
  for (const h of hits) {
    if (h.score > max) max = h.score
  }
  const out = new Map<string, { hit: T & { file_path: string }; norm: number }>()
  for (const h of hits as readonly (T & { file_path: string })[]) {
    const existing = out.get(h.file_path)
    const norm = max > 0 ? h.score / max : 0
    // Dedup by file_path: keep the highest pre-normalisation score.
    if (!existing || h.score > existing.hit.score) {
      out.set(h.file_path, { hit: h, norm })
    }
  }
  return out
}

/**
 * Merge keyword + semantic hits per the BL-040 ranking rule. Pure.
 *
 * @param keyword raw `com.nexus.storage::search` rows.
 * @param semantic raw `com.nexus.ai::semantic_search` matches.
 * @param limit cap on the returned length (default 30).
 */
export function mergeResults(
  keyword: readonly KeywordHit[],
  semantic: readonly SemanticHit[],
  limit: number = DEFAULT_LIMIT,
): MergedHit[] {
  const kw = normaliseByMax(keyword)
  const sm = normaliseByMax(semantic)
  const paths = new Set<string>([...kw.keys(), ...sm.keys()])

  const merged: MergedHit[] = []
  for (const path of paths) {
    const k = kw.get(path)
    const s = sm.get(path)
    let score: number
    if (k && s) {
      score = KEYWORD_WEIGHT * k.norm + SEMANTIC_WEIGHT * s.norm
    } else if (k) {
      score = SINGLE_SOURCE_DAMPING * k.norm
    } else if (s) {
      score = SINGLE_SOURCE_DAMPING * s.norm
    } else {
      // Unreachable — `paths` was assembled from kw ∪ sm.
      continue
    }
    // Prefer the keyword excerpt (FTS-highlighted) when present;
    // fall back to the semantic chunk text so the row is never blank.
    const snippet =
      (k?.hit.excerpt && k.hit.excerpt.length > 0
        ? k.hit.excerpt
        : s?.hit.chunk_text) ?? ''
    merged.push({
      file_path: path,
      score,
      snippet,
      hasKeyword: !!k,
      hasSemantic: !!s,
    })
  }

  merged.sort((a, b) => b.score - a.score)
  if (limit > 0 && merged.length > limit) merged.length = limit
  return merged
}
