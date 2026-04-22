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
import type { CanvasKernelClient, CanvasNode, LinkPreview } from './kernelClient'
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
          return null
        })}
      </div>
    </div>
  )
})

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
