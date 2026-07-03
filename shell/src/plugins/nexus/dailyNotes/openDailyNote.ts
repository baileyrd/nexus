// C6 (#359) — the idempotent create-or-open flow shared by the "Open
// today" / prev / next commands and the calendar pane's click-a-day
// handler.

import { isoDate } from '../bases/dateUtils'
import { dailyNotePath, dailyNoteSkeleton, DEFAULT_DATE_FORMAT, DEFAULT_FILE_LOCATION } from './dailyNoteFormat'
import { useDailyNotesStore } from './dailyNotesStore'
import { getApi } from './dailyNotesRuntime'

const STORAGE_PLUGIN_ID = 'com.nexus.storage'
const EVENT_FILE_OPEN = 'files:open'

const utf8Encoder = new TextEncoder()

const CONFIG_KEY_DATE_FORMAT = 'nexus.settings.dailyNotes.dateFormat'
const CONFIG_KEY_FILE_LOCATION = 'nexus.settings.dailyNotes.fileLocation'

/** Basename of a forge-relative path. Forward-slash only. */
function basename(relpath: string): string {
  const i = relpath.lastIndexOf('/')
  return i === -1 ? relpath : relpath.slice(i + 1)
}

/**
 * Read the daily-notes settings, wired to the existing (previously dead)
 * `nexus.settings.dailyNotes.dateFormat` / `.fileLocation` stub keys
 * (SettingsStubPages.tsx). `templateLocation` is deliberately not wired
 * here — see the module doc in dailyNoteFormat.ts's neighbor,
 * index.ts, for why.
 */
function readSettings(): { dateFormat: string; fileLocation: string } {
  const api = getApi()
  const dateFormat = api.configuration.getValue<string>(CONFIG_KEY_DATE_FORMAT, DEFAULT_DATE_FORMAT).trim()
  const fileLocation = api.configuration.getValue<string>(CONFIG_KEY_FILE_LOCATION, '').trim()
  return {
    dateFormat: dateFormat || DEFAULT_DATE_FORMAT,
    fileLocation: fileLocation || DEFAULT_FILE_LOCATION,
  }
}

/**
 * Open `date`'s daily note, creating it from the standard skeleton first
 * if it doesn't exist yet. Idempotent: re-invoking for the same date
 * with the file already present just opens it. Updates
 * `dailyNotesStore.currentDate` so prev/next navigation continues from
 * wherever the user last landed.
 */
export async function openDailyNote(date: Date): Promise<void> {
  const api = getApi()
  const { dateFormat, fileLocation } = readSettings()
  const relpath = dailyNotePath(date, fileLocation, dateFormat)

  const exists = await fileExists(relpath)
  if (!exists) {
    try {
      const bytes = Array.from(utf8Encoder.encode(dailyNoteSkeleton(date)))
      await api.kernel.invoke(STORAGE_PLUGIN_ID, 'write_file', { path: relpath, bytes })
    } catch (e) {
      api.notifications.show({
        message: `Failed to create daily note "${relpath}": ${String(e)}`,
        type: 'error',
      })
      return
    }
  }

  api.events.emit(EVENT_FILE_OPEN, { relpath, name: basename(relpath) })
  useDailyNotesStore.getState().setCurrentDate(isoDate(date))
}

async function fileExists(relpath: string): Promise<boolean> {
  try {
    const raw = await getApi().kernel.invoke<unknown>(STORAGE_PLUGIN_ID, 'query_files', {
      prefix: relpath,
    })
    if (!Array.isArray(raw)) return false
    return raw.some((row) => {
      if (!row || typeof row !== 'object') return false
      return (row as Record<string, unknown>).path === relpath
    })
  } catch {
    return false
  }
}
