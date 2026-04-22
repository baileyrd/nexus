// Phase-5a DOM overlay on top of the 2D canvas. The overlay's root
// mirrors the camera transform so child divs positioned in world
// coordinates line up with the 2D-canvas render underneath.
//
// Pointer events on the overlay root are disabled so the underlying
// canvas still receives pan/drag/marquee gestures. Individual overlay
// children may opt back in via `pointer-events: auto` once future
// phases need scrollbars, links, or input focus — but Phase 5a keeps
// everything passive so text nodes can't eat clicks.

import { forwardRef, useEffect, useMemo, useState } from 'react'
import type { BaseSummary, CanvasKernelClient, CanvasNode, LinkPreview } from './kernelClient'
import { renderMarkdown } from '../editor/markdownRender'

interface Props {
  /** Nodes to overlay. Callers filter to the types the overlay handles
   *  so the 2D renderer can keep drawing the rest. */
  nodes: readonly CanvasNode[]
  /** Kernel client — passed down so link nodes can fetch OG metadata
   *  via `com.nexus.linkpreview::fetch`. */
  client: CanvasKernelClient
}

/**
 * The forwarded ref points at the **transformed layer** (not the
 * outer clipping wrapper), which is what the RAF tick updates with
 * the camera transform each frame.
 */
export const CanvasOverlay = forwardRef<HTMLDivElement, Props>(function CanvasOverlay(
  { nodes, client },
  layerRef,
) {
  return (
    <div
      style={{
        position: 'absolute',
        inset: 0,
        overflow: 'hidden',
        pointerEvents: 'none',
      }}
    >
      <div
        ref={layerRef}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          transformOrigin: '0 0',
          // transform is written imperatively on every RAF tick; see
          // CanvasView's render loop.
        }}
      >
        {nodes.map((n) => {
          if (n.type === 'text') return <TextNodeOverlay key={n.id} node={n} />
          if (n.type === 'link') return <LinkNodeOverlay key={n.id} node={n} client={client} />
          if (n.type === 'file') return <FileNodeOverlay key={n.id} node={n} client={client} />
          if (n.type === 'database') return <DatabaseNodeOverlay key={n.id} node={n} client={client} />
          return null
        })}
      </div>
    </div>
  )
})

/** Cap on the number of rows the database-node mini-grid shows at
 *  once. Past this we render a "+ N more" footer — nobody wants a
 *  10k-row table scroll-locked inside a card. */
const DB_NODE_ROW_CAP = 50
/** Cap on columns in the mini-grid. First four schema fields are
 *  usually the primary-ish ones; the rest get elided. */
const DB_NODE_COL_CAP = 4

const basePreviewCache = new Map<string, BaseSummary>()
const basePreviewPending = new Map<string, Promise<BaseSummary>>()

function loadBaseCached(
  client: CanvasKernelClient,
  relpath: string,
): Promise<BaseSummary> {
  const hit = basePreviewCache.get(relpath)
  if (hit) return Promise.resolve(hit)
  const pending = basePreviewPending.get(relpath)
  if (pending) return pending
  const p = client
    .loadBase(relpath)
    .then((base) => {
      basePreviewCache.set(relpath, base)
      basePreviewPending.delete(relpath)
      return base
    })
    .catch((err) => {
      basePreviewPending.delete(relpath)
      throw err
    })
  basePreviewPending.set(relpath, p)
  return p
}

function DatabaseNodeOverlay({
  node,
  client,
}: {
  node: CanvasNode
  client: CanvasKernelClient
}) {
  // Canvas PRD-06 says database nodes reference a `.bases` file via
  // `file`; older / Obsidian-ish canvases sometimes use `source`.
  // Prefer `file`, fall back to `source` so we cover both.
  const relpath = node.file ?? node.source ?? ''
  const [base, setBase] = useState<BaseSummary | null>(
    () => basePreviewCache.get(relpath) ?? null,
  )
  const [loading, setLoading] = useState(!!relpath && !basePreviewCache.has(relpath))
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!relpath) {
      setBase(null)
      setLoading(false)
      setError(null)
      return
    }
    const cached = basePreviewCache.get(relpath)
    if (cached) {
      setBase(cached)
      setLoading(false)
      setError(null)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    loadBaseCached(client, relpath)
      .then((b) => {
        if (!cancelled) {
          setBase(b)
          setLoading(false)
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(String(err))
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [relpath, client])

  const columns = useMemo(() => {
    if (!base) return [] as string[]
    return Object.keys(base.schema.fields).slice(0, DB_NODE_COL_CAP)
  }, [base])
  const rows = useMemo(() => {
    if (!base) return [] as BaseSummary['records']
    return base.records.slice(0, DB_NODE_ROW_CAP)
  }, [base])
  const title = base?.name || basenameOf(relpath) || 'Database'
  const totalRows = base?.records.length ?? 0
  const hiddenRows = Math.max(0, totalRows - rows.length)

  return (
    <div
      style={{
        position: 'absolute',
        left: node.x,
        top: node.y,
        width: node.width,
        height: node.height,
        padding: 10,
        boxSizing: 'border-box',
        overflow: 'hidden',
        color: 'var(--fg, #e5e7eb)',
        fontFamily: 'var(--font-family, system-ui, sans-serif)',
        fontSize: 12,
        lineHeight: 1.35,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 6,
          minHeight: 14,
        }}
      >
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            letterSpacing: 0.4,
            textTransform: 'uppercase',
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          DATABASE · {title}
          {loading && ' · loading…'}
        </div>
        {!loading && base && (
          <div
            style={{
              color: 'var(--fg-muted, #9ca3af)',
              fontSize: 10,
              fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
              flex: '0 0 auto',
            }}
          >
            {totalRows} row{totalRows === 1 ? '' : 's'}
          </div>
        )}
      </div>

      {error && (
        <div style={{ color: 'var(--risk, #ef4444)', fontSize: 11 }}>
          failed to load: {error}
        </div>
      )}

      {!error && !loading && base && columns.length > 0 && (
        <div style={{ flex: '1 1 auto', minHeight: 0, overflow: 'hidden' }}>
          <table
            style={{
              width: '100%',
              borderCollapse: 'collapse',
              fontSize: 11,
              tableLayout: 'fixed',
            }}
          >
            <thead>
              <tr>
                {columns.map((c) => (
                  <th
                    key={c}
                    style={{
                      textAlign: 'left',
                      padding: '3px 6px',
                      color: 'var(--fg-muted, #9ca3af)',
                      borderBottom: '1px solid var(--divider-color, #3f3f46)',
                      fontWeight: 500,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                    }}
                  >
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={r.id}>
                  {columns.map((c) => (
                    <td
                      key={c}
                      style={{
                        padding: '3px 6px',
                        borderBottom: '1px solid rgba(255,255,255,0.05)',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {formatCell(r[c])}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {!error && !loading && base && columns.length === 0 && (
        <div style={{ color: 'var(--fg-muted, #9ca3af)', fontSize: 11 }}>
          No schema fields defined.
        </div>
      )}

      {!error && !loading && !base && !relpath && (
        <div style={{ color: 'var(--fg-muted, #9ca3af)', fontSize: 11 }}>
          No database linked.
        </div>
      )}

      {hiddenRows > 0 && (
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            flex: '0 0 auto',
          }}
        >
          + {hiddenRows} more row{hiddenRows === 1 ? '' : 's'}
        </div>
      )}

      {relpath && (
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {relpath}
        </div>
      )}
    </div>
  )
}

/** Render a cell value as a short string. Objects / arrays get a
 *  compact JSON-ish display so the grid doesn't explode into
 *  multi-line layouts. */
function formatCell(v: unknown): string {
  if (v == null) return ''
  if (typeof v === 'string') return v
  if (typeof v === 'number' || typeof v === 'boolean') return String(v)
  try {
    return JSON.stringify(v)
  } catch {
    return String(v)
  }
}

/** How many bytes of a text file we render inside a node preview.
 *  Past this we clip with an ellipsis indicator — no-one wants a
 *  megabyte README painted across the canvas. */
const FILE_PREVIEW_TEXT_CAP = 64 * 1024
/** Image MIME lookup — keyed by lowercase extension. Unknown
 *  extensions fall back to the plain-text path. */
const IMAGE_EXT_MIME: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
}
/** Per-relpath cached file contents. Module-scope so it survives tab
 *  remounts. No eviction — file previews are small (capped above). */
interface FilePreviewData {
  kind: 'markdown' | 'image' | 'text' | 'binary'
  /** Rendered HTML for markdown, data: URL for images, raw preview
   *  string for text, empty for binary. */
  content: string
  /** Surfaced in the badge when content is truncated so users know
   *  they're looking at a partial view. */
  truncated?: boolean
}
const filePreviewCache = new Map<string, FilePreviewData>()
const filePreviewPending = new Map<string, Promise<FilePreviewData>>()

/** Classify a relpath by extension. Returns `null` for "no file". */
function classifyFile(relpath: string): 'markdown' | 'image' | 'text' | 'binary' | null {
  if (!relpath) return null
  const ext = relpath.toLowerCase().split('.').pop() ?? ''
  if (ext === 'md' || ext === 'mdx' || ext === 'markdown') return 'markdown'
  if (ext in IMAGE_EXT_MIME) return 'image'
  // Everything that's plausibly text gets the text path; unknowns go
  // to binary + a placeholder card.
  if (
    ['txt', 'json', 'yaml', 'yml', 'toml', 'csv', 'tsv', 'log', 'rs', 'ts', 'tsx', 'js', 'jsx',
     'py', 'go', 'sh', 'html', 'css', 'xml', 'conf', 'ini', 'env'].includes(ext)
  ) {
    return 'text'
  }
  return 'binary'
}

/** Build a base64 data: URL from raw bytes. Done in chunks so we
 *  don't hit the "maximum call stack" cap of `String.fromCharCode`
 *  on large images. */
function bytesToDataUrl(bytes: Uint8Array, mime: string): string {
  const chunk = 0x8000
  let binary = ''
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  const b64 = btoa(binary)
  return `data:${mime};base64,${b64}`
}

function loadFilePreview(
  client: CanvasKernelClient,
  relpath: string,
): Promise<FilePreviewData> {
  const hit = filePreviewCache.get(relpath)
  if (hit) return Promise.resolve(hit)
  const pending = filePreviewPending.get(relpath)
  if (pending) return pending
  const kind = classifyFile(relpath)
  const p = (async (): Promise<FilePreviewData> => {
    const bytes = await client.readFile(relpath)
    if (bytes == null) return { kind: 'binary', content: '' }
    if (kind === 'image') {
      const ext = relpath.toLowerCase().split('.').pop() ?? ''
      const mime = IMAGE_EXT_MIME[ext] ?? 'application/octet-stream'
      return { kind: 'image', content: bytesToDataUrl(bytes, mime) }
    }
    if (kind === 'markdown' || kind === 'text') {
      const truncated = bytes.length > FILE_PREVIEW_TEXT_CAP
      const slice = truncated ? bytes.subarray(0, FILE_PREVIEW_TEXT_CAP) : bytes
      let text: string
      try {
        text = new TextDecoder('utf-8', { fatal: true }).decode(slice)
      } catch {
        return { kind: 'binary', content: '' }
      }
      if (kind === 'markdown') {
        return { kind: 'markdown', content: renderMarkdown(text), truncated }
      }
      return { kind: 'text', content: text, truncated }
    }
    return { kind: 'binary', content: '' }
  })()
    .then((data) => {
      filePreviewCache.set(relpath, data)
      filePreviewPending.delete(relpath)
      return data
    })
    .catch((err) => {
      filePreviewPending.delete(relpath)
      throw err
    })
  filePreviewPending.set(relpath, p)
  return p
}

function FileNodeOverlay({
  node,
  client,
}: {
  node: CanvasNode
  client: CanvasKernelClient
}) {
  const relpath = node.file ?? ''
  const [data, setData] = useState<FilePreviewData | null>(
    () => filePreviewCache.get(relpath) ?? null,
  )
  const [loading, setLoading] = useState(!!relpath && !filePreviewCache.has(relpath))
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!relpath) {
      setData(null)
      setLoading(false)
      setError(null)
      return
    }
    const cached = filePreviewCache.get(relpath)
    if (cached) {
      setData(cached)
      setLoading(false)
      setError(null)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    loadFilePreview(client, relpath)
      .then((d) => {
        if (!cancelled) {
          setData(d)
          setLoading(false)
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(String(err))
          setLoading(false)
        }
      })
    return () => {
      cancelled = true
    }
  }, [relpath, client])

  const basename = basenameOf(relpath)

  return (
    <div
      style={{
        position: 'absolute',
        left: node.x,
        top: node.y,
        width: node.width,
        height: node.height,
        padding: 10,
        boxSizing: 'border-box',
        overflow: 'hidden',
        color: 'var(--fg, #e5e7eb)',
        fontFamily: 'var(--font-family, system-ui, sans-serif)',
        fontSize: 12,
        lineHeight: 1.35,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 6,
          minHeight: 14,
        }}
      >
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            letterSpacing: 0.4,
            textTransform: 'uppercase',
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          FILE · {basename || '(untitled)'}
          {loading && ' · loading…'}
          {data?.truncated && ' · truncated'}
        </div>
      </div>

      {error && (
        <div style={{ color: 'var(--risk, #ef4444)', fontSize: 11 }}>
          failed to read: {error}
        </div>
      )}

      {!error && !loading && data?.kind === 'markdown' && (
        <div
          className="nexus-markdown-body"
          style={{ flex: '1 1 auto', overflow: 'hidden', fontSize: 12 }}
          dangerouslySetInnerHTML={{ __html: data.content }}
        />
      )}

      {!error && !loading && data?.kind === 'text' && (
        <pre
          style={{
            flex: '1 1 auto',
            overflow: 'hidden',
            margin: 0,
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            fontSize: 11,
            lineHeight: 1.4,
            color: 'var(--fg, #e5e7eb)',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
          }}
        >
          {data.content}
        </pre>
      )}

      {!error && !loading && data?.kind === 'image' && (
        <img
          src={data.content}
          alt={basename}
          style={{
            flex: '1 1 auto',
            width: '100%',
            minHeight: 0,
            objectFit: 'contain',
            borderRadius: 4,
          }}
        />
      )}

      {!error && !loading && data?.kind === 'binary' && (
        <div style={{ color: 'var(--fg-muted, #9ca3af)', fontSize: 11 }}>
          {relpath
            ? 'Binary or unsupported file type — no preview available.'
            : 'No file linked.'}
        </div>
      )}

      {relpath && (
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {relpath}
        </div>
      )}
    </div>
  )
}

function basenameOf(path: string): string {
  if (!path) return ''
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return i >= 0 ? path.slice(i + 1) : path
}

/** In-memory cache of URL → preview so opening multiple tabs (or
 *  flipping away and back to the same canvas) doesn't refetch the
 *  same page. No eviction — desktop sessions are short and previews
 *  are small. Stored at module scope so it survives React
 *  remounts on leaf re-opens. */
const previewCache = new Map<string, LinkPreview>()
/** Pending fetches, so two link nodes pointing at the same URL
 *  share a single network trip. */
const pendingFetches = new Map<string, Promise<LinkPreview>>()

function fetchPreviewCached(
  client: CanvasKernelClient,
  url: string,
): Promise<LinkPreview> {
  const hit = previewCache.get(url)
  if (hit) return Promise.resolve(hit)
  const pending = pendingFetches.get(url)
  if (pending) return pending
  const p = client
    .fetchLinkPreview(url)
    .then((preview) => {
      previewCache.set(url, preview)
      pendingFetches.delete(url)
      return preview
    })
    .catch((err) => {
      pendingFetches.delete(url)
      throw err
    })
  pendingFetches.set(url, p)
  return p
}

function LinkNodeOverlay({
  node,
  client,
}: {
  node: CanvasNode
  client: CanvasKernelClient
}) {
  const url = node.url ?? ''
  const [preview, setPreview] = useState<LinkPreview | null>(() => previewCache.get(url) ?? null)
  const [loading, setLoading] = useState(!previewCache.has(url))
  useEffect(() => {
    if (!url) return
    if (previewCache.has(url)) {
      setPreview(previewCache.get(url) ?? null)
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    fetchPreviewCached(client, url)
      .then((p) => {
        if (!cancelled) {
          setPreview(p)
          setLoading(false)
        }
      })
      .catch(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [url, client])

  const hostname = useMemo(() => hostOf(url), [url])
  const title = preview?.title || node.label || hostname || url || '(no URL)'
  const description = preview?.description ?? null
  const siteName = preview?.site_name ?? hostname
  const favicon = preview?.favicon_url ?? null
  const image = preview?.image_url ?? null

  return (
    <div
      style={{
        position: 'absolute',
        left: node.x,
        top: node.y,
        width: node.width,
        height: node.height,
        padding: 10,
        boxSizing: 'border-box',
        overflow: 'hidden',
        color: 'var(--fg, #e5e7eb)',
        fontFamily: 'var(--font-family, system-ui, sans-serif)',
        fontSize: 12,
        lineHeight: 1.35,
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, minHeight: 16 }}>
        {favicon && (
          <img
            src={favicon}
            alt=""
            width={14}
            height={14}
            style={{ borderRadius: 2, flex: '0 0 auto' }}
            onError={(e) => {
              // Broken favicon should not leave a broken-image glyph sitting
              // in the card — hide the element and carry on.
              ;(e.currentTarget as HTMLImageElement).style.display = 'none'
            }}
          />
        )}
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 10,
            letterSpacing: 0.4,
            textTransform: 'uppercase',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
          }}
        >
          {siteName || 'LINK'}
          {loading && ' · loading…'}
        </div>
      </div>

      <div
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: 'var(--fg, #e5e7eb)',
          overflow: 'hidden',
          display: '-webkit-box',
          WebkitLineClamp: 2,
          WebkitBoxOrient: 'vertical',
          wordBreak: 'break-word',
        }}
      >
        {title}
      </div>

      {description && (
        <div
          style={{
            color: 'var(--fg-muted, #9ca3af)',
            fontSize: 11,
            overflow: 'hidden',
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
          }}
        >
          {description}
        </div>
      )}

      {image && (
        <img
          src={image}
          alt=""
          style={{
            marginTop: 'auto',
            width: '100%',
            maxHeight: 80,
            objectFit: 'cover',
            borderRadius: 4,
            flex: '0 0 auto',
          }}
          onError={(e) => {
            ;(e.currentTarget as HTMLImageElement).style.display = 'none'
          }}
        />
      )}

      <div
        style={{
          color: 'var(--accent, #3b82f6)',
          fontSize: 10,
          fontFamily: 'var(--font-monospace, ui-monospace, monospace)',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
        }}
      >
        {url}
      </div>
    </div>
  )
}

function hostOf(url: string): string {
  if (!url) return ''
  try {
    return new URL(url).hostname
  } catch {
    return ''
  }
}

function TextNodeOverlay({ node }: { node: CanvasNode }) {
  // Parse once per text change — resizing and moving don't re-parse,
  // only re-position.
  const html = useMemo(() => renderMarkdown(node.text ?? ''), [node.text])
  return (
    <div
      className="nexus-markdown-body"
      style={{
        position: 'absolute',
        left: node.x,
        top: node.y,
        width: node.width,
        height: node.height,
        padding: 10,
        boxSizing: 'border-box',
        overflow: 'hidden',
        color: 'var(--fg, #e5e7eb)',
        fontFamily: 'var(--font-family, system-ui, sans-serif)',
        fontSize: 13,
        lineHeight: 1.4,
      }}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}
