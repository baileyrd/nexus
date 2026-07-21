// C71 (#424) — pure URL → action parsing for the `nexus://` deep-link
// handler. Kept dependency-free (no CM6/React/store imports) so it's
// testable without a DOM, mirroring `remoteCursors.ts`'s split between
// pure data logic and side-effecting wiring.
//
// WHATWG URL parsing treats the segment right after `scheme://` as the
// host for *any* scheme (not just the "special" ones), so
// `nexus://open?path=a/b.md` parses as `hostname: 'open'`,
// `search: '?path=a/b.md'` — no custom parsing needed.

export type DeepLinkAction =
  | { kind: 'open'; path: string }
  | { kind: 'search'; query: string }
  | { kind: 'new'; path: string; content: string }

/**
 * Parse a `nexus://<action>?...` URL into a typed action. Returns
 * `null` for an unrecognized action or one missing its required
 * parameter — callers should log and drop, never throw (a malformed
 * or hostile deep link must not be able to crash the dispatch path).
 */
export function parseDeepLink(uri: URL): DeepLinkAction | null {
  switch (uri.hostname.toLowerCase()) {
    case 'open': {
      const path = uri.searchParams.get('path')
      return path ? { kind: 'open', path } : null
    }
    case 'search': {
      const query = uri.searchParams.get('q')
      return query === null ? null : { kind: 'search', query }
    }
    case 'new': {
      const path = uri.searchParams.get('path')
      if (!path) return null
      return { kind: 'new', path, content: uri.searchParams.get('content') ?? '' }
    }
    default:
      return null
  }
}
