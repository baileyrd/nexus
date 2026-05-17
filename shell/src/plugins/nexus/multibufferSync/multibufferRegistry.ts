// BL-141 Approach B step 3b — multibuffer registry helpers.
//
// Tracks which source files each open `multibuffer://<uuid>` tab
// covers, so the change-event subscriber knows which multibuffers
// to refresh when a source file's session reports a `changed` event.
// The registry is a plain `Map`; pure helpers around it keep the
// subscriber code easy to unit-test.

import type { EditorSnapshot } from '../editor/types.ts'

export const MULTIBUFFER_RELPATH_PREFIX = 'multibuffer://'

export interface MultibufferEntry {
  /** Unique source relpaths the multibuffer's Excerpt blocks cover. */
  sources: Set<string>
}

export type MultibufferRegistry = Map<string, MultibufferEntry>

/** Returns the unique source relpaths referenced by every `Excerpt`
 *  block in `snapshot.tree`, in first-appearance order across
 *  `root_blocks`. Pure — mirrors `excerpt_sources` on the Rust side. */
export function extractSources(snapshot: EditorSnapshot): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const id of snapshot.tree.root_blocks) {
    const block = snapshot.tree.blocks[id]
    if (!block) continue
    const ty = block.ty as { kind?: string; source_relpath?: string }
    if (ty?.kind !== 'excerpt') continue
    const src = typeof ty.source_relpath === 'string' ? ty.source_relpath : null
    if (!src) continue
    if (!seen.has(src)) {
      seen.add(src)
      out.push(src)
    }
  }
  return out
}

/** Returns the multibuffer relpaths whose Excerpt blocks cover
 *  `sourceRelpath`. Empty result is the steady state — the
 *  subscriber only fires the registry walk when a `changed` event
 *  matches a key it cares about. */
export function multibuffersWatchingSource(
  registry: MultibufferRegistry,
  sourceRelpath: string,
): string[] {
  const out: string[] = []
  for (const [relpath, entry] of registry) {
    if (entry.sources.has(sourceRelpath)) {
      out.push(relpath)
    }
  }
  return out
}

/** Pull the `<relpath>` suffix from a
 *  `com.nexus.editor.changed.<relpath>` topic, or `null` if the
 *  topic isn't shaped that way. Doubles as a guard against the
 *  refresh-loop where our own refresh fires another changed event:
 *  callers ignore the suffix when it starts with `multibuffer://`. */
export const CHANGED_TOPIC_PREFIX = 'com.nexus.editor.changed.'
export function changedTopicRelpath(topic: string): string | null {
  if (!topic.startsWith(CHANGED_TOPIC_PREFIX)) return null
  const suffix = topic.slice(CHANGED_TOPIC_PREFIX.length)
  if (!suffix) return null
  return suffix
}

/** `true` when `relpath` is a synthetic multibuffer relpath we
 *  created (i.e. starts with `multibuffer://`). */
export function isMultibufferRelpath(relpath: string): boolean {
  return relpath.startsWith(MULTIBUFFER_RELPATH_PREFIX)
}
