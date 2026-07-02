// C1 (#354) — image & attachment pipeline shared helpers.
//
// Three consumers:
//   - the reading-view hydrator (`hydrateForgeImages`) that swaps
//     forge-relative `<img>` srcs for data: URLs after DOMPurify ran,
//   - the live-preview image widget (`cm/livePreviewDecorations.ts`),
//   - the paste/drop attachment importer (`cm/attachmentPaste.ts`).
//
// Images travel as data: URLs built from `com.nexus.storage::read_file`
// bytes — the same proven path the canvas file-node overlay uses
// (`canvas/CanvasOverlay.tsx`) — so no Tauri asset-protocol or CSP
// changes are needed and the browser dev-server behaves identically to
// the packaged shell.

import type { KernelAPI } from '../../../types/plugin.ts'
import { configStore } from '../../../stores/configStore'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'

/** Image MIME lookup — keyed by lowercase extension. Mirrors the
 *  canvas overlay's table; unknown extensions are treated as
 *  non-images. */
export const IMAGE_EXT_MIME: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  svg: 'image/svg+xml',
  bmp: 'image/bmp',
  ico: 'image/x-icon',
  avif: 'image/avif',
}

/** Reverse of [`IMAGE_EXT_MIME`] for naming pasted blobs. First
 *  extension wins for MIME types with aliases (jpeg → jpg). */
const MIME_EXT: Record<string, string> = {
  'image/png': 'png',
  'image/jpeg': 'jpg',
  'image/gif': 'gif',
  'image/webp': 'webp',
  'image/svg+xml': 'svg',
  'image/bmp': 'bmp',
  'image/x-icon': 'ico',
  'image/avif': 'avif',
}

export function isImagePath(path: string): boolean {
  const ext = path.toLowerCase().split('.').pop() ?? ''
  return ext in IMAGE_EXT_MIME
}

export function mimeForPath(path: string): string | null {
  const ext = path.toLowerCase().split('.').pop() ?? ''
  return IMAGE_EXT_MIME[ext] ?? null
}

/** Build a base64 data: URL from raw bytes. Chunked so we don't hit
 *  the `String.fromCharCode` call-stack cap on large images. */
export function bytesToDataUrl(bytes: Uint8Array, mime: string): string {
  const chunk = 0x8000
  let binary = ''
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return `data:${mime};base64,${btoa(binary)}`
}

/** Collapse `.` / `..` segments in a forge-relative path. Returns
 *  `null` when the path escapes the forge root (more `..` than
 *  ancestors) — the storage layer would reject it anyway, so callers
 *  can drop the candidate early. */
export function normalizeRelpath(path: string): string | null {
  const out: string[] = []
  for (const seg of path.split('/')) {
    if (seg === '' || seg === '.') continue
    if (seg === '..') {
      if (out.length === 0) return null
      out.pop()
      continue
    }
    out.push(seg)
  }
  return out.join('/')
}

/** `true` for srcs the hydrator must leave alone: absolute URLs,
 *  data:/blob: payloads, and anchor fragments. */
export function isExternalSrc(src: string): boolean {
  return /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(src)
}

/**
 * Resolve a markdown image src against the note that references it.
 * Returns forge-relative candidate paths in probe order:
 *   1. relative to the note's directory (the common authored form),
 *   2. relative to the forge root (Obsidian's "shortest path" form).
 * The src arrives URI-encoded from the markdown pipeline (`%20` etc.);
 * decode before resolving. Duplicates collapse to one candidate.
 */
export function resolveImageCandidates(
  noteRelpath: string,
  src: string,
): string[] {
  if (isExternalSrc(src)) return []
  let decoded = src
  try {
    decoded = decodeURIComponent(src)
  } catch {
    /* malformed escape — probe the raw string */
  }
  decoded = decoded.replace(/[?#].*$/, '')
  if (decoded.length === 0) return []
  const noteDir = noteRelpath.includes('/')
    ? noteRelpath.slice(0, noteRelpath.lastIndexOf('/'))
    : ''
  const candidates: string[] = []
  const fromNoteDir = normalizeRelpath(noteDir ? `${noteDir}/${decoded}` : decoded)
  if (fromNoteDir) candidates.push(fromNoteDir)
  const fromRoot = normalizeRelpath(decoded)
  if (fromRoot && !candidates.includes(fromRoot)) candidates.push(fromRoot)
  return candidates
}

// ── forge image loader (module-scope cache) ──────────────────────────

/** Resolved data: URLs keyed by forge relpath. Module-scope so the
 *  cache survives tab remounts and is shared between the reading view
 *  and the live-preview widget. External edits to an image are stale
 *  until reload — acceptable v1; a `file_written` watcher hook can
 *  call [`invalidateForgeImage`] later. */
const imageCache = new Map<string, string>()
const imagePending = new Map<string, Promise<string | null>>()

export function invalidateForgeImage(relpath: string): void {
  imageCache.delete(relpath)
  imagePending.delete(relpath)
}

/** Test seam: reset all cached image state. */
export function clearForgeImageCache(): void {
  imageCache.clear()
  imagePending.clear()
}

interface ReadFileResult {
  bytes: number[] | null
}

async function readImage(
  kernel: KernelAPI,
  relpath: string,
): Promise<string | null> {
  const mime = mimeForPath(relpath)
  if (!mime) return null
  const resp = await kernel.invoke<ReadFileResult>(
    STORAGE_PLUGIN_ID,
    'read_file',
    { path: relpath },
  )
  if (resp.bytes == null) return null
  return bytesToDataUrl(Uint8Array.from(resp.bytes), mime)
}

/**
 * Load the first candidate that exists as a data: URL, or `null` when
 * none resolves. Results (including per-candidate misses) are cached;
 * concurrent loads of the same relpath share one in-flight promise.
 */
export async function loadForgeImage(
  kernel: KernelAPI,
  candidates: string[],
): Promise<string | null> {
  for (const relpath of candidates) {
    const hit = imageCache.get(relpath)
    if (hit) return hit
    let pending = imagePending.get(relpath)
    if (!pending) {
      pending = readImage(kernel, relpath).catch(() => null)
      imagePending.set(relpath, pending)
    }
    const url = await pending
    imagePending.delete(relpath)
    if (url) {
      imageCache.set(relpath, url)
      return url
    }
  }
  return null
}

// ── reading-view hydrator ─────────────────────────────────────────────

export interface HydrateForgeImagesOptions {
  /** Relpath of the note whose rendered HTML we're hydrating —
   *  relative srcs resolve against its directory. */
  noteRelpath: string
  kernel: KernelAPI | null
}

/**
 * Walk a rendered (sanitized) markdown tree and swap forge-relative
 * `<img>` srcs for data: URLs. Handles both marked's native
 * `![alt](src)` output and the `![[embed]]` extension's placeholder
 * (`data-forge-src`, emitted src-less so a broken-image glyph never
 * flashes). Runs *after* DOMPurify — we only ever assign data: URLs
 * built from forge bytes, never markup.
 */
export function hydrateForgeImages(
  root: HTMLElement | null,
  options: HydrateForgeImagesOptions,
): void {
  if (!root) return
  const { noteRelpath, kernel } = options
  if (!kernel) return
  const images = root.querySelectorAll<HTMLImageElement>(
    'img[src], img[data-forge-src]',
  )
  for (const img of Array.from(images)) {
    const raw = img.getAttribute('data-forge-src') ?? img.getAttribute('src') ?? ''
    if (raw.length === 0 || isExternalSrc(raw)) continue
    const candidates = resolveImageCandidates(noteRelpath, raw)
    if (candidates.length === 0) continue
    img.classList.add('nx-forge-image')
    void loadForgeImage(kernel, candidates).then((url) => {
      if (!img.isConnected) return
      if (url) {
        img.src = url
        img.classList.remove('nx-forge-image--missing')
      } else {
        // Leave the relative src in place (it renders the alt text /
        // broken glyph) but tag it so the CSS can style the miss.
        img.classList.add('nx-forge-image--missing')
        if (!img.title) img.title = `Not found in forge: ${raw}`
      }
    })
  }
}

/** Assemble the live-preview widget's image-resolution context (see
 *  `cm/livePreviewDecorations.ts::forgeImageContext`) for a note. */
export function makeForgeImageContext(
  noteRelpath: string,
  kernel: KernelAPI,
): { noteRelpath: string; loadImage: (src: string) => Promise<string | null> } {
  return {
    noteRelpath,
    loadImage: (src: string) =>
      loadForgeImage(kernel, resolveImageCandidates(noteRelpath, src)),
  }
}

// ── attachment placement (paste / drop importer) ─────────────────────

/** Settings keys owned by the core settings plugin
 *  (`SettingsPanelView.tsx` — Files & Links tab). Consumed here for
 *  the first time; before C1 the location select persisted a value
 *  no code read. */
export const CONFIG_ATTACHMENT_LOCATION =
  'nexus.settings.files.defaultAttachmentLocation'
export const CONFIG_ATTACHMENT_FOLDER_PATH =
  'nexus.settings.files.attachmentFolderPath'

/** Fallback folder for the `specific` location mode. `attachments/`
 *  is the forge convention — it's one of the two directories the
 *  storage watcher covers (`crates/nexus-storage/src/watcher.rs`). */
export const DEFAULT_ATTACHMENT_FOLDER = 'attachments'

/**
 * Directory (forge-relative, no trailing slash, '' = forge root) where
 * a new attachment should land for a given location mode:
 *   - `root` → forge root (the labelled behaviour),
 *   - `same` → the note's own directory,
 *   - `specific` → the configured `folder` (default `attachments`).
 * Pure core of [`attachmentDirFor`], exported for tests.
 */
export function attachmentDirForMode(
  mode: string,
  noteRelpath: string,
  folder: string,
): string {
  if (mode === 'same') {
    return noteRelpath.includes('/')
      ? noteRelpath.slice(0, noteRelpath.lastIndexOf('/'))
      : ''
  }
  if (mode === 'specific') {
    const cleaned = folder.trim().replace(/^\/+|\/+$/g, '')
    return cleaned.length > 0 ? cleaned : DEFAULT_ATTACHMENT_FOLDER
  }
  return ''
}

/** [`attachmentDirForMode`] fed from the live settings store. */
export function attachmentDirFor(noteRelpath: string): string {
  return attachmentDirForMode(
    configStore.get<string>(CONFIG_ATTACHMENT_LOCATION, 'root'),
    noteRelpath,
    configStore.get<string>(
      CONFIG_ATTACHMENT_FOLDER_PATH,
      DEFAULT_ATTACHMENT_FOLDER,
    ),
  )
}

/** Strip path separators / control chars from a user-supplied file
 *  name so a hostile clipboard payload can't traverse directories.
 *  Collapses whitespace runs; falls back to `file` when nothing
 *  survives. */
export function sanitizeAttachmentName(name: string): string {
  const cleaned = name
    .replace(/[\\/]+/g, '-')
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f]/g, '')
    .replace(/\s+/g, ' ')
    .trim()
  return cleaned.length > 0 && cleaned !== '.' && cleaned !== '..'
    ? cleaned
    : 'file'
}

/** Name for a pasted image blob (clipboard screenshots arrive as
 *  extensionless `image.png` blobs). Obsidian-style timestamped name
 *  so repeated pastes never collide within the same second either —
 *  the collision probe in [`writeAttachment`] covers the rest. */
export function pastedImageName(mime: string, now: Date): string {
  const ext = MIME_EXT[mime] ?? 'png'
  const pad = (n: number) => String(n).padStart(2, '0')
  const stamp =
    `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}` +
    `-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`
  return `pasted-image-${stamp}.${ext}`
}

/** Split `name.ext` → `["name", ".ext"]` (extension optional). */
function splitExt(name: string): [string, string] {
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return [name, '']
  return [name.slice(0, dot), name.slice(dot)]
}

interface FileExistsResult {
  exists: boolean
}

/**
 * Write attachment bytes into `dir`, deduplicating the file name via
 * `com.nexus.storage::file_exists` (`name.png` → `name-1.png` → …).
 * Returns the forge-relative path written. Atomic-write semantics come
 * from the storage handler.
 */
export async function writeAttachment(
  kernel: KernelAPI,
  dir: string,
  name: string,
  bytes: Uint8Array,
): Promise<string> {
  const safe = sanitizeAttachmentName(name)
  const [stem, ext] = splitExt(safe)
  let relpath = dir ? `${dir}/${safe}` : safe
  for (let n = 1; n <= 99; n++) {
    const resp = await kernel.invoke<FileExistsResult>(
      STORAGE_PLUGIN_ID,
      'file_exists',
      { path: relpath },
    )
    if (!resp.exists) break
    const next = `${stem}-${n}${ext}`
    relpath = dir ? `${dir}/${next}` : next
  }
  await kernel.invoke(STORAGE_PLUGIN_ID, 'write_file', {
    path: relpath,
    bytes: Array.from(bytes),
  })
  return relpath
}

/**
 * Markdown to insert for a stored attachment: `![](path)` for images,
 * `[name](path)` for everything else. The path is URI-encoded so
 * spaces survive CommonMark's link-destination grammar; the hydrator
 * decodes on the way back in.
 */
export function attachmentMarkdown(relpath: string): string {
  const encoded = encodeURI(relpath)
  if (isImagePath(relpath)) return `![](${encoded})`
  const name = relpath.split('/').pop() ?? relpath
  return `[${name}](${encoded})`
}
