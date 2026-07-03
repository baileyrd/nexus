// shell/src/plugins/nexus/dailyNotes/index.ts
//
// C6 (#359) — daily notes were CLI-only (`nexus content daily`,
// hardcoded `notes/daily/%Y-%m-%d.md`, bypassing the templates system)
// with no shell open-today command, hotkey, day navigation, or
// calendar, leaving only the generic multi-prompt "New from template…"
// flow as a non-idempotent workaround. Both existing settings surfaces
// were dead: the persisted `nexus.settings.dailyNotes.*` keys
// (SettingsStubPages.tsx) and the Rust `AppConfig.core.daily_note_format`
// field had zero consumers.
//
// This plugin adds an idempotent "Open today's daily note" command
// wired to the dateFormat/fileLocation settings (finally giving them a
// reader), Ctrl+Shift+J, prev/next day navigation, and a small
// month-calendar pane (paneMode view + activity-bar item, same
// structure as nexus.taskDashboard).
//
// `templateLocation` is deliberately NOT wired: the templates engine's
// only built-in daily template (`daily-journal`, target_path
// `daily/{{today}}.md`) substitutes `{{today}}` as an engine-computed
// "real today" value with no way for a caller to override it for an
// arbitrary date, so it can't correctly back-date content for
// prev/next-day navigation or a calendar click on a past/future day.
// Faithfully supporting a user-chosen template here would need the
// templates engine to accept a caller-supplied date override — a
// substantially larger change than this narrow fix, and a reasonable
// follow-up rather than something to bolt on half-correctly now. The
// skeleton this plugin writes instead mirrors the CLI's own daily()
// skeleton (crates/nexus-cli/src/commands/content.rs), so shell- and
// CLI-created daily notes read the same.

import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { usePaneModeStore } from '../../../stores/paneModeStore'
import { DailyCalendarView } from './DailyCalendarView'
import { openDailyNote } from './openDailyNote'
import { useDailyNotesStore } from './dailyNotesStore'
import { setApi } from './dailyNotesRuntime'
import { addDays, parseDate } from '../bases/dateUtils'

const VIEW_ID = 'nexus.dailyNotes.calendar'
const ACTIVITY_ITEM_ID = 'nexus.dailyNotes.activityItem'

const COMMAND_OPEN_TODAY = 'nexus.dailyNotes.openToday'
const COMMAND_OPEN_PREVIOUS = 'nexus.dailyNotes.openPrevious'
const COMMAND_OPEN_NEXT = 'nexus.dailyNotes.openNext'
const COMMAND_SHOW_CALENDAR = 'nexus.dailyNotes.showCalendar'

const COMMAND_PANE_MODE_ENTER = 'nexus.paneMode.enter'
const COMMAND_PANE_MODE_EXIT = 'nexus.paneMode.exit'

const EVENT_ACTIVITY_BAR_ACTIVE_CHANGED = 'activityBar:activeChanged'

/** Lucide "calendar" glyph. */
const CALENDAR_ICON_PATH =
  'M8 2v4M16 2v4M3 10h18M5 4h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2z'

/** The date prev/next should step from: the store's last-opened daily
 *  note, or today when nothing has been opened yet this session. */
function referenceDate(): Date {
  const iso = useDailyNotesStore.getState().currentDate
  return (iso && parseDate(iso)) || new Date()
}

export const dailyNotesPlugin: Plugin = {
  manifest: {
    id: 'nexus.dailyNotes',
    name: 'Daily Notes',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['com.nexus.storage', 'nexus.paneMode', 'nexus.activityBar'],
    contributes: {
      commands: [
        { id: COMMAND_OPEN_TODAY, title: "Open Today's Daily Note", category: 'Daily Notes' },
        { id: COMMAND_OPEN_PREVIOUS, title: 'Daily Notes: Previous Day', category: 'Daily Notes' },
        { id: COMMAND_OPEN_NEXT, title: 'Daily Notes: Next Day', category: 'Daily Notes' },
        { id: COMMAND_SHOW_CALENDAR, title: 'Daily Notes: Show Calendar', category: 'Daily Notes' },
      ],
      keybindings: [{ command: COMMAND_OPEN_TODAY, key: 'ctrl+shift+j', mac: 'cmd+shift+j' }],
    },
  },

  activate(api: PluginAPI) {
    setApi(api)

    api.commands.register(COMMAND_OPEN_TODAY, () => void openDailyNote(new Date()))
    api.commands.register(COMMAND_OPEN_PREVIOUS, () => void openDailyNote(addDays(referenceDate(), -1)))
    api.commands.register(COMMAND_OPEN_NEXT, () => void openDailyNote(addDays(referenceDate(), 1)))

    api.views.register(VIEW_ID, {
      slot: 'paneMode',
      component: () => createElement(DailyCalendarView),
      priority: 10,
    })
    api.activityBar.addItem({
      id: ACTIVITY_ITEM_ID,
      icon: '',
      iconPath: CALENDAR_ICON_PATH,
      title: 'Daily Notes',
      viewId: VIEW_ID,
      priority: 59,
    })
    api.events.on<{ viewId: string | null }>(EVENT_ACTIVITY_BAR_ACTIVE_CHANGED, ({ viewId }) => {
      if (viewId === VIEW_ID) {
        void api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
      } else if (usePaneModeStore.getState().activeViewId === VIEW_ID) {
        void api.commands.execute(COMMAND_PANE_MODE_EXIT)
      }
    })
    api.commands.register(COMMAND_SHOW_CALENDAR, async () => {
      await api.commands.execute(COMMAND_PANE_MODE_ENTER, VIEW_ID)
    })
  },
}
