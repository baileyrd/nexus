// R8 / #191 — placeholder "stub" pages for core plugins that are
// manifest-declared but whose backing subsystems haven't shipped yet.
// Lifted out of `SettingsPanelView.tsx` (which exceeded 3,300 LoC) so
// the stable tab implementations and the placeholders aren't intermixed.
//
// Each `Stub*Page` renders a small read-mostly form whose values
// round-trip through `useConfigStore` (via the `Wired*` cells) so the
// keys exist the moment the corresponding feature ships. The
// `STUB_CORE_PLUGINS` registry below threads each page into the
// per-plugin rail via the `cp-stub:<feature>` ids the panel knows to
// look up against the contributed-tab fallback.
//
// Consumed by `SettingsPanelView.tsx`'s navigation switch
// (`STUB_CORE_BY_ID.get(navTab)?.render(api)`).

import type { PluginCategory } from '@nexus/extension-api'
import type { PluginAPI } from '../../../types/plugin'
import {
  StubRow,
  WiredNumber,
  WiredNumberRange,
  WiredSelect,
  WiredText,
  WiredToggle,
} from './SettingsCells'

export interface StubCorePluginEntry {
  id: string
  label: string
  category: PluginCategory
  render: (api: PluginAPI | undefined) => React.ReactNode
}

export const STUB_CORE_PLUGINS: ReadonlyArray<StubCorePluginEntry> = [
  {
    id: 'cp-stub:backlinks',
    label: 'Backlinks',
    category: 'files',
    render: (api) => <StubBacklinksPage api={api} />,
  },
  {
    id: 'cp-stub:canvas',
    label: 'Canvas',
    category: 'editor',
    render: (api) => <StubCanvasPage api={api} />,
  },
  {
    id: 'cp-stub:command-palette',
    label: 'Command palette',
    category: 'navigation',
    render: () => <StubCommandPalettePage />,
  },
  {
    id: 'cp-stub:daily-notes',
    label: 'Daily notes',
    category: 'files',
    render: (api) => <StubDailyNotesPage api={api} />,
  },
  {
    id: 'cp-stub:file-recovery',
    label: 'File recovery',
    category: 'files',
    render: (api) => <StubFileRecoveryPage api={api} />,
  },
  {
    id: 'cp-stub:note-composer',
    label: 'Note composer',
    category: 'editor',
    render: (api) => <StubNoteComposerPage api={api} />,
  },
  {
    id: 'cp-stub:page-preview',
    label: 'Page preview',
    category: 'editor',
    render: (api) => <StubPagePreviewPage api={api} />,
  },
  {
    id: 'cp-stub:quick-switcher',
    label: 'Quick switcher',
    category: 'navigation',
    render: (api) => <StubQuickSwitcherPage api={api} />,
  },
  {
    id: 'cp-stub:sync',
    label: 'Sync',
    category: 'files',
    render: () => <StubSyncPage />,
  },
  {
    id: 'cp-stub:templates',
    label: 'Templates',
    category: 'editor',
    render: (api) => <StubTemplatesPage api={api} />,
  },
]

export const STUB_CORE_BY_ID = new Map(STUB_CORE_PLUGINS.map((p) => [p.id, p]))

export function StubBacklinksPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Show backlinks at the bottom of notes"
        description="Make backlinks visible in new tabs by default."
        control={
          <WiredToggle
            settingKey="nexus.settings.backlinks.showAtBottom"
            defaultValue={false}
            label="Toggle backlinks at bottom"
          />
        }
      />
    </div>
  )
}

export function StubCanvasPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Default location for new canvas files"
        description="Where newly created canvases are placed."
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.defaultLocation"
            defaultValue="root"
            label="Default canvas location"
            options={[
              { value: 'root', label: 'Forge folder' },
              { value: 'same', label: 'Same folder as current file' },
              { value: 'specific', label: 'Specific folder…' },
            ]}
          />
        }
      />
      <StubRow
        title="Default mouse wheel behavior"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.mouseWheel"
            defaultValue="pan"
            label="Default mouse wheel behavior"
            options={[
              { value: 'pan', label: 'Pan' },
              { value: 'zoom', label: 'Zoom' },
            ]}
          />
        }
      />
      <StubRow
        title="Default Ctrl + Drag behavior"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.ctrlDrag"
            defaultValue="menu"
            label="Default Ctrl+Drag behavior"
            options={[
              { value: 'menu', label: 'Show menu' },
              { value: 'select', label: 'Select' },
              { value: 'zoom', label: 'Zoom' },
            ]}
          />
        }
      />
      <StubRow
        title="Show card names"
        description=""
        control={
          <WiredSelect
            settingKey="nexus.settings.canvas.showCardNames"
            defaultValue="always"
            label="Show card names"
            options={[
              { value: 'always', label: 'Always' },
              { value: 'hover', label: 'On hover' },
              { value: 'never', label: 'Never' },
            ]}
          />
        }
      />
      <StubRow
        title="Snap to grid"
        description="Snap cards to the background grid when moving and resizing."
        control={
          <WiredToggle
            settingKey="nexus.settings.canvas.snapToGrid"
            defaultValue={true}
            label="Toggle snap to grid"
          />
        }
      />
      <StubRow
        title="Snap to objects"
        description="Snap cards to nearby objects when moving and resizing."
        control={
          <WiredToggle
            settingKey="nexus.settings.canvas.snapToObjects"
            defaultValue={true}
            label="Toggle snap to objects"
          />
        }
      />
      <StubRow
        title="Zoom threshold for hiding card content"
        description="Lower values will increase performance but hide card content sooner when zooming out."
        control={
          <WiredNumberRange
            settingKey="nexus.settings.canvas.zoomHideThreshold"
            defaultValue={40}
            min={0}
            max={100}
            label="Zoom threshold for hiding card content"
          />
        }
      />
    </div>
  )
}

export function StubCommandPalettePage() {
  return (
    <div className="settings-section">
      <div className="settings-section-title">Pinned commands</div>
      <div
        style={{
          padding: '14px 16px',
          background: 'var(--background-modifier-hover)',
          borderRadius: 6,
        }}
      >
        <input
          type="search"
          className="settings-search"
          placeholder="Select a command to add..."
          disabled
          style={{ width: '100%', marginBottom: 8 }}
          title="Coming soon"
        />
        <div style={{ color: 'var(--text-muted)', fontSize: 13 }}>No commands found.</div>
      </div>
    </div>
  )
}

export function StubDailyNotesPage(_: { api?: PluginAPI }) {
  const today = new Date().toISOString().slice(0, 10)
  return (
    <div className="settings-section">
      <StubRow
        title="Date format"
        description="Choose how daily notes are named in your forge."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.dateFormat"
            defaultValue={today}
            label="Date format"
          />
        }
      />
      <StubRow
        title="New file location"
        description="New daily notes will be placed here."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.fileLocation"
            defaultValue=""
            placeholder="Example: folder 1/folder 2"
            label="Daily note location"
          />
        }
      />
      <StubRow
        title="Template file location"
        description="Choose the file to use as a template."
        control={
          <WiredText
            settingKey="nexus.settings.dailyNotes.templateLocation"
            defaultValue=""
            placeholder="Example: folder/note"
            label="Daily note template"
          />
        }
      />
    </div>
  )
}

export function StubFileRecoveryPage({ api }: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Snapshot interval"
        description="Minimal interval in minutes between two snapshots."
        control={
          <WiredNumber
            settingKey="nexus.settings.fileRecovery.snapshotIntervalMinutes"
            defaultValue={5}
            min={1}
            label="Snapshot interval"
          />
        }
      />
      <StubRow
        title="History length"
        description="Number of days the snapshots are kept for."
        control={
          <WiredNumber
            settingKey="nexus.settings.fileRecovery.historyDays"
            defaultValue={7}
            min={1}
            label="History length"
          />
        }
      />
      <StubRow
        title="Snapshots"
        description="View and restore saved snapshots. Until the file-recovery subsystem ships, the list is always empty — the values above are persisted so they take effect the moment snapshots come online."
        control={
          <button
            type="button"
            onClick={() =>
              api?.notifications.show({
                type: 'info',
                message: 'No snapshots — the file-recovery daemon is not yet built.',
              })
            }
            style={{
              background: 'var(--interactive-accent)',
              color: 'var(--interactive-accent-ink)',
              border: 'none',
              borderRadius: 4,
              padding: '4px 12px',
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            View
          </button>
        }
      />
      <StubRow
        title="Clear history"
        description="Delete all snapshots."
        control={
          <button
            type="button"
            onClick={() =>
              api?.notifications.show({
                type: 'info',
                message: 'Snapshot history is empty — nothing to clear.',
              })
            }
            style={{
              background: 'transparent',
              color: 'var(--text-error, #e06c75)',
              border: '1px solid var(--text-error, #e06c75)',
              borderRadius: 4,
              padding: '4px 12px',
              fontSize: 13,
              cursor: 'pointer',
            }}
          >
            Clear
          </button>
        }
      />
    </div>
  )
}

export function StubNoteComposerPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Text after extraction"
        description="What to show in place of the selected text after extracting it."
        control={
          <WiredSelect
            settingKey="nexus.settings.noteComposer.textAfterExtraction"
            defaultValue="link"
            label="Text after extraction"
            options={[
              { value: 'link', label: 'Link to new file' },
              { value: 'embed', label: 'Embed new file' },
              { value: 'nothing', label: 'Nothing' },
            ]}
          />
        }
      />
      <StubRow
        title="Template file location"
        description="Template file to use when merging or extracting. Available variables: {{content}}, {{fromTitle}}, {{newTitle}}, {{date:FORMAT}}, e.g. {{date:YYYY-MM-DD}}."
        control={
          <WiredText
            settingKey="nexus.settings.noteComposer.templateLocation"
            defaultValue=""
            placeholder="Example: folder/note"
            label="Note composer template"
          />
        }
      />
      <StubRow
        title="Confirm file merge"
        description="Prompt before merging two files."
        control={
          <WiredToggle
            settingKey="nexus.settings.noteComposer.confirmMerge"
            defaultValue={true}
            label="Toggle confirm file merge"
          />
        }
      />
    </div>
  )
}

export function StubPagePreviewPage(_: { api?: PluginAPI }) {
  const surfaces: ReadonlyArray<{ key: string; label: string; on: boolean }> = [
    { key: 'search', label: 'Search, Backlinks, and Outgoing links', on: true },
    { key: 'reading', label: 'Reading view', on: false },
    { key: 'editing', label: 'Editing view', on: true },
    { key: 'tabs', label: 'Tab header', on: true },
    { key: 'files', label: 'Files', on: true },
    { key: 'properties', label: 'Properties view', on: true },
    { key: 'bookmarks', label: 'Bookmarks', on: true },
    { key: 'outline', label: 'Outline', on: true },
    { key: 'bases', label: 'Bases', on: true },
    { key: 'graph', label: 'Graph view', on: true },
  ]
  return (
    <div className="settings-section">
      <div className="settings-section-title">Require Ctrl to trigger page preview on hover</div>
      {surfaces.map((s) => (
        <StubRow
          key={s.key}
          title={s.label}
          description=""
          control={
            <WiredToggle
              settingKey={`nexus.settings.pagePreview.ctrlRequired.${s.key}`}
              defaultValue={s.on}
              label={`Toggle Ctrl-required on ${s.label}`}
            />
          }
        />
      ))}
    </div>
  )
}

export function StubQuickSwitcherPage(_: { api?: PluginAPI }) {
  return (
    <div className="settings-section">
      <StubRow
        title="Show existing only"
        description="Only show results from existing files. Links to files that are not yet created will be hidden."
        control={
          <WiredToggle
            settingKey="nexus.settings.quickSwitcher.showExistingOnly"
            defaultValue={false}
            label="Toggle show existing only"
          />
        }
      />
      <StubRow
        title="Show attachments"
        description="Show attachment files like images, videos, and PDFs."
        control={
          <WiredToggle
            settingKey="nexus.settings.quickSwitcher.showAttachments"
            defaultValue={true}
            label="Toggle show attachments"
          />
        }
      />
    </div>
  )
}

export function StubSyncPage() {
  return (
    <div className="settings-section">
      <p style={{ marginBottom: 12 }}>
        Nexus Sync is the add-on sync service with end-to-end encryption and version
        history.
      </p>
      <p style={{ marginBottom: 16 }}>
        To start syncing, please log in or create a new Nexus account.
      </p>
      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end' }}>
        <button
          type="button"
          onClick={() =>
            window.open('https://github.com/baileyrd/nexus#sync', '_blank')
          }
          style={{
            background: 'var(--interactive-accent)',
            color: 'var(--interactive-accent-ink)',
            border: 'none',
            borderRadius: 4,
            padding: '6px 14px',
            fontSize: 13,
            cursor: 'pointer',
          }}
        >
          Sign up
        </button>
        <button
          type="button"
          onClick={() =>
            window.open('https://github.com/baileyrd/nexus#sync', '_blank')
          }
          style={{
            background: 'var(--background-modifier-hover)',
            color: 'var(--text-normal)',
            border: 'none',
            borderRadius: 4,
            padding: '6px 14px',
            fontSize: 13,
            cursor: 'pointer',
          }}
        >
          Log in
        </button>
      </div>
    </div>
  )
}

export function StubTemplatesPage(_: { api?: PluginAPI }) {
  const now = new Date()
  const today = now.toISOString().slice(0, 10)
  const time = now.toTimeString().slice(0, 5)
  return (
    <div className="settings-section">
      <StubRow
        title="Template folder location"
        description="Files in this folder will be available as templates."
        control={
          <WiredText
            settingKey="nexus.settings.templates.folderLocation"
            defaultValue=""
            placeholder="Example: folder 1/folder 2"
            label="Template folder location"
          />
        }
      />
      <StubRow
        title="Date format"
        description={
          '{{date}} in the template file will be replaced with this value. ' +
          `Your current syntax looks like this: ${today}`
        }
        control={
          <WiredText
            settingKey="nexus.settings.templates.dateFormat"
            defaultValue=""
            placeholder="YYYY-MM-DD"
            label="Templates date format"
          />
        }
      />
      <StubRow
        title="Time format"
        description={
          '{{time}} in the template file will be replaced with this value. ' +
          `Your current syntax looks like this: ${time}`
        }
        control={
          <WiredText
            settingKey="nexus.settings.templates.timeFormat"
            defaultValue=""
            placeholder="HH:mm"
            label="Templates time format"
          />
        }
      />
    </div>
  )
}
