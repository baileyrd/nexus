// C6 (#359) — pure date-formatting + skeleton logic for daily notes, kept
// separate from index.ts so it's testable without mocking the plugin
// stack (mirrors taskDashboard/taskGrouping.ts, quickSwitcher/fileMatch.ts).

import { MONTH_LABELS } from '../bases/dateUtils'

export const DEFAULT_DATE_FORMAT = 'YYYY-MM-DD'
export const DEFAULT_FILE_LOCATION = 'daily'

/**
 * Render `d` through a tiny token format: `YYYY`/`MM`/`DD` substitute the
 * zero-padded year/month/day; everything else in `format` passes through
 * literally. Deliberately not a full strftime — the two settings-page
 * stub fields this backs (`dateFormat`/`fileLocation`,
 * SettingsStubPages.tsx) only ever documented a filename shape, and a
 * minimal token set is enough to cover every reasonable daily-note
 * naming convention users actually ask for.
 */
export function formatDate(d: Date, format: string): string {
  const yyyy = String(d.getFullYear())
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  return format.replace(/YYYY/g, yyyy).replace(/MM/g, mm).replace(/DD/g, dd)
}

/** Forge-relative path for `d`'s daily note, joining `folder` + the formatted filename. */
export function dailyNotePath(d: Date, folder: string, dateFormat: string): string {
  const filename = `${formatDate(d, dateFormat)}.md`
  const trimmedFolder = folder.replace(/^\/+|\/+$/g, '')
  return trimmedFolder ? `${trimmedFolder}/${filename}` : filename
}

/**
 * Default daily-note skeleton for `d` — mirrors `nexus content daily`'s
 * CLI skeleton (crates/nexus-cli/src/commands/content.rs) so shell- and
 * CLI-created daily notes look the same, independent of where the file
 * lives (the CLI hardcodes `notes/daily/`; this plugin's folder is
 * configurable — see `dailyNotePath`).
 */
export function dailyNoteSkeleton(d: Date): string {
  const yyyy = d.getFullYear()
  const mm = String(d.getMonth() + 1).padStart(2, '0')
  const dd = String(d.getDate()).padStart(2, '0')
  const dateStr = `${yyyy}-${mm}-${dd}`
  const title = `${MONTH_LABELS[d.getMonth()]} ${d.getDate()}, ${yyyy}`
  return `---\ndate: ${dateStr}\ntags: [daily]\n---\n# ${title}\n\n## Tasks\n\n## Notes\n`
}
