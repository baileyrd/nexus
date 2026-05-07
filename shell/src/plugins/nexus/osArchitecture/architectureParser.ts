// BL-054 Phase 2 — architecture.md parser.
//
// Tolerant by design. Per the open-question §5 Q2 in
// docs/PRDs/BL-054-agentic-os-mode.md, the parser should accept
// whatever it can interpret and silently skip the rest so a half-
// authored architecture.md still yields a useful render. Strict
// validation can layer on top later if drift detection turns out to
// need it.
//
// Recognised shape:
//   ## <domain name>             ← H2 starts a domain
//   - <task-id>  [type | class | dest | automation]
//                                ← list item; trailing four-attribute tag
//
// Anything outside that pattern is ignored. Tags inside fenced code
// blocks are explicitly skipped so the example in the seeded
// architecture.md placeholder doesn't pollute the parse.

/** Execution-type for a task. Matches the BL-054 §1 four-attribute spec. */
export type TaskType = 'skill' | 'agent' | 'command' | 'manual' | 'unknown'

/** Class for a task. Foundations are recurring; capabilities run on demand. */
export type TaskClass = 'foundation' | 'capability' | 'unknown'

/** Memory destination — where the task writes output, if anywhere. */
export type TaskMemoryDest =
  | 'raw'
  | 'wiki'
  | 'project'
  | 'output'
  | 'none'
  | 'unknown'

/** Automation: cron schedule (`HHMM`), webhook, or none. Free-form
 *  preserved verbatim so the panel can render it without round-tripping
 *  through a strict enum. */
export interface TaskAutomation {
  /** `cron` / `webhook` / `none` / `unknown`. */
  kind: 'cron' | 'webhook' | 'none' | 'unknown'
  /** Raw automation text from the tag (`local cron 0700`). */
  raw: string
}

export interface ArchitectureTask {
  /** The slug before the bracketed tag (`daily-trend-scan`). */
  id: string
  /** Free-form description text after the slug if present. */
  description: string
  type: TaskType
  class: TaskClass
  memoryDest: TaskMemoryDest
  automation: TaskAutomation
  /** The unparsed bracket contents (`skill | foundation | raw | local cron 0700`). */
  rawTag: string
}

export interface ArchitectureDomain {
  name: string
  tasks: ArchitectureTask[]
}

export interface Architecture {
  /** Free-form intro paragraph(s) before the first H2, if any. */
  preamble: string
  domains: ArchitectureDomain[]
}

/** Pulls the domain → task hierarchy out of an architecture.md source.
 *  Empty / placeholder input returns an empty `domains` array. */
export function parseArchitecture(src: string): Architecture {
  const lines = src.replace(/\r\n/g, '\n').split('\n')
  const domains: ArchitectureDomain[] = []
  const preambleLines: string[] = []
  let current: ArchitectureDomain | null = null
  let inFence = false
  let inDomainScope = false

  for (const line of lines) {
    // Toggle fenced-code-block state. Anything inside is opaque.
    if (/^```/.test(line)) {
      inFence = !inFence
      continue
    }
    if (inFence) continue

    const h2 = line.match(/^##\s+(.+?)\s*$/)
    if (h2) {
      current = { name: h2[1].trim(), tasks: [] }
      domains.push(current)
      inDomainScope = true
      continue
    }

    if (!inDomainScope) {
      // Capture pre-domain prose so the panel can render an intro.
      preambleLines.push(line)
      continue
    }

    if (!current) continue

    const task = parseTaskLine(line)
    if (task) current.tasks.push(task)
  }

  return {
    preamble: preambleLines.join('\n').trim(),
    domains,
  }
}

/** Match a single task list item. Returns null when the line doesn't
 *  carry the four-attribute bracketed tag. Public so the unit tests
 *  can pin individual line shapes. */
export function parseTaskLine(line: string): ArchitectureTask | null {
  const m = line.match(/^\s*[-*+]\s+(.+?)\s*\[([^\]]+)\]\s*$/)
  if (!m) return null
  const head = m[1].trim()
  const rawTag = m[2].trim()
  const fields = rawTag.split('|').map((s) => s.trim())
  if (fields.length < 4) return null

  // The slug is the first whitespace-separated token of the head; the
  // rest is description (trimmed, optional).
  const headParts = head.split(/\s+/)
  const id = headParts[0]
  const description = headParts.slice(1).join(' ').trim()
  if (!id) return null

  return {
    id,
    description,
    type: parseType(fields[0]),
    class: parseClass(fields[1]),
    memoryDest: parseMemoryDest(fields[2]),
    automation: parseAutomation(fields[3]),
    rawTag,
  }
}

function parseType(s: string): TaskType {
  switch (s.toLowerCase()) {
    case 'skill': return 'skill'
    case 'agent': return 'agent'
    case 'command': return 'command'
    case 'manual': return 'manual'
    default: return 'unknown'
  }
}

function parseClass(s: string): TaskClass {
  switch (s.toLowerCase()) {
    case 'foundation': return 'foundation'
    case 'capability': return 'capability'
    default: return 'unknown'
  }
}

function parseMemoryDest(s: string): TaskMemoryDest {
  switch (s.toLowerCase()) {
    case 'raw': return 'raw'
    case 'wiki': return 'wiki'
    case 'project': return 'project'
    case 'output': return 'output'
    case 'none': return 'none'
    default: return 'unknown'
  }
}

function parseAutomation(s: string): TaskAutomation {
  const lower = s.toLowerCase()
  if (lower === 'none') return { kind: 'none', raw: s }
  if (lower === 'webhook') return { kind: 'webhook', raw: s }
  if (/cron/.test(lower)) return { kind: 'cron', raw: s }
  return { kind: 'unknown', raw: s }
}
