import { create } from 'zustand'
import { clientLogger } from '../../../clientLogger'

/** BL-022 — change-notification surface. The plugin's `index.ts`
 *  installs a listener via `subscribeSkillsChanged` so it can refetch
 *  the listing after a save / delete without reaching into the
 *  shell's host EventBus (which the plugin-import hygiene test
 *  guards against). Module-scope subscriber list mirrors the
 *  pattern used by `contextContributors` in nexus.ai. */
type ChangeAction = 'saved' | 'deleted'
type ChangeListener = (event: { id: string; action: ChangeAction }) => void
const changeListeners: Set<ChangeListener> = new Set()
export function subscribeSkillsChanged(listener: ChangeListener): () => void {
  changeListeners.add(listener)
  return () => {
    changeListeners.delete(listener)
  }
}
function emitChange(event: { id: string; action: ChangeAction }): void {
  for (const listener of changeListeners) {
    try {
      listener(event)
    } catch (err) {
      // eslint-disable-next-line no-console
      clientLogger.warn('[nexus.skills] change listener threw', err)
    }
  }
}

/**
 * Subset of `PluginAPI` used by render actions. Typed structurally so
 * `skillsStore` unit tests can drive the render flow with a minimal
 * mock kernel — same pattern as `SavedKernelAPI` in
 * terminal/savedCommandsStore.ts.
 */
export interface SkillsKernelAPI {
  invoke<T = unknown>(pluginId: string, commandId: string, args?: unknown): Promise<T>
}

/**
 * One declared parameter on a skill, projected from the kernel's
 * `SkillParameter` (crates/nexus-skills/src/lib.rs L114-134).
 *
 * `values` carries the enum allowed-list (when `type === "enum"`).
 * `default` is whatever the frontmatter declared — we keep it as
 * `unknown` because YAML scalars / sequences flow through serde as
 * arbitrary JSON.
 */
export interface SkillParameter {
  name: string
  /** `"string" | "number" | "boolean" | "enum" | "list"` or custom. */
  type: string
  description: string
  /** Allowed values for `type === "enum"`. */
  values: string[]
  /** Element type for `type === "list"`. */
  items: string | null
  default: unknown
}

/**
 * One skill row, projected from `com.nexus.skills::list`.
 *
 * The kernel returns the full `Skill` struct (frontmatter merged with
 * body) per crates/nexus-skills/src/lib.rs::Skill. We only render
 * what fits in a sidebar row + an inline expand panel; the full
 * `body` is kept so the expanded view doesn't need a second `get`.
 */
export interface SkillEntry {
  id: string
  name: string
  description: string
  version: string
  author: string
  /** ISO 8601 creation date (`created` in the frontmatter). Carried
   *  through so the editor doesn't have to guess on save. */
  created: string
  tags: string[]
  applicableContexts: string[]
  triggers: string[]
  parameters: SkillParameter[]
  /** BL-021 — depends_on chain (raw skill ids; the resolver in
   *  nexus-skills drives the actual layering). */
  dependsOn: string[]
  body: string
  /** BL-022 — forge-relative path the kernel loaded this skill from
   *  (e.g. `.forge/skills/code-reviewer.skill.md`). Empty for skills
   *  loaded out-of-tree. The in-app editor uses it to drive
   *  `com.nexus.storage::write_file` / `delete_file`. */
  relpath: string
}

/**
 * Result of `com.nexus.skills::render`. The kernel returns the
 * substituted prompt body alongside the canonical id/name so the UI
 * doesn't need to keep its own copy in the request payload.
 */
export interface RenderResult {
  id: string
  name: string
  body: string
}

const SKILLS_PLUGIN_ID = 'com.nexus.skills'
const STORAGE_PLUGIN_ID = 'com.nexus.storage'
// Verified against crates/nexus-skills/src/core_plugin.rs HANDLER_RENDER (id 6,
// command name `render`, args `{ id, values? }`).
const CMD_RENDER = 'render'
const CMD_RELOAD = 'reload'
// Verified against crates/nexus-storage/src/core_plugin.rs:
//   write_file: { path, bytes: number[] } -> FileMetadata
//   delete_file: { path } -> {}
const CMD_WRITE_FILE = 'write_file'
const CMD_DELETE_FILE = 'delete_file'

/** BL-022 — editable shape of a skill, decoupled from the on-wire
 *  `SkillEntry` so the editor's own validation / dirty tracking
 *  doesn't muddle the read-only listing. Mirrors the kebab-cased
 *  frontmatter keys. */
export interface SkillDraft {
  /** Forge-relative path the draft will save to. Empty for a new
   *  skill until the user picks a slug. */
  relpath: string
  /** True when the draft was minted via "New skill" rather than
   *  "Edit". Drives the slug input + create-vs-overwrite branch on
   *  save. */
  isNew: boolean
  name: string
  id: string
  description: string
  version: string
  author: string
  created: string
  tags: string[]
  applicableContexts: string[]
  triggers: string[]
  dependsOn: string[]
  /** Raw body markdown after the frontmatter `---` block. */
  body: string
}

/** Render the editor draft back into the on-disk `.skill.md` file
 *  format: `---\n<yaml frontmatter>\n---\n<body>`. Pure — exposed for
 *  unit tests. The output uses a stable key order (id / name first,
 *  then descriptive fields, then arrays, then dependencies) so a
 *  save round-trip is deterministic and diff-friendly. */
export function serializeDraft(draft: SkillDraft): string {
  const lines: string[] = ['---']
  // Frontmatter ordering — id-like fields first, then descriptive,
  // then list fields. Mirrors the order the seed_builtins generator
  // emits so a saved skill matches a hand-written one verbatim.
  if (draft.name) lines.push(`name: ${yamlScalar(draft.name)}`)
  if (draft.id) lines.push(`id: ${yamlScalar(draft.id)}`)
  if (draft.description) lines.push(`description: ${yamlScalar(draft.description)}`)
  if (draft.version) lines.push(`version: ${yamlScalar(draft.version)}`)
  if (draft.author) lines.push(`author: ${yamlScalar(draft.author)}`)
  if (draft.created) lines.push(`created: ${yamlScalar(draft.created)}`)
  if (draft.tags.length > 0) lines.push(`tags: ${yamlList(draft.tags)}`)
  if (draft.applicableContexts.length > 0) {
    lines.push(`applicable_contexts: ${yamlList(draft.applicableContexts)}`)
  }
  if (draft.triggers.length > 0) lines.push(`triggers: ${yamlList(draft.triggers)}`)
  if (draft.dependsOn.length > 0) lines.push(`depends_on: ${yamlList(draft.dependsOn)}`)
  lines.push('---')
  lines.push('')
  lines.push(draft.body.replace(/\r\n/g, '\n').trimEnd())
  lines.push('')
  return lines.join('\n')
}

/** Quote a YAML scalar conservatively. We only need to handle the
 *  set of frontmatter values the editor produces (single-line strings,
 *  no special chars beyond `:` / `#` / leading whitespace). Anything
 *  ambiguous gets double-quoted with backslash escapes. */
function yamlScalar(s: string): string {
  if (s.length === 0) return '""'
  if (/[\n\r]/.test(s)) {
    // Multi-line — fall back to a JSON-style quoted scalar. The
    // editor doesn't emit these today; this is defence-in-depth.
    return JSON.stringify(s)
  }
  if (/^[A-Za-z0-9._/\- ]+$/.test(s) && !s.startsWith(' ') && !s.endsWith(' ')) {
    // Plain scalar — but avoid YAML-reserved bare strings.
    if (
      !['true', 'false', 'null', 'yes', 'no', 'on', 'off'].includes(s.toLowerCase()) &&
      !/^-?\d/.test(s) // numeric-looking → quote so it stays a string
    ) {
      return s
    }
  }
  return JSON.stringify(s)
}

function yamlList(items: string[]): string {
  return `[${items.map((s) => yamlScalar(s)).join(', ')}]`
}

interface SkillsStoreState {
  loading: boolean
  loadError: string | null
  skills: SkillEntry[]
  /** Skill id whose row is currently expanded, or null. */
  expandedId: string | null
  /** Skill id currently in render-form mode (within the expanded row). */
  renderingId: string | null
  /** Per-skill draft form values, keyed by skill id then param name. */
  paramDrafts: Record<string, Record<string, unknown>>
  /** Per-skill last render result. */
  renderResults: Record<string, RenderResult>
  /** Per-skill last render error. */
  renderErrors: Record<string, string>
  /** Skill id currently mid-render (single-flight). */
  rendering: string | null

  /** BL-022 — id of the skill currently in edit mode (or
   *  `'__new__'` when creating). Null when no editor is open. */
  editingId: string | null
  /** Active draft, hydrated from the existing skill on edit and from
   *  the starter template on new. */
  draft: SkillDraft | null
  /** Sticky save error from the last persist attempt. */
  saveError: string | null
  /** True while a write_file + reload pair is in flight. */
  saving: boolean

  setLoading(b: boolean): void
  setLoadError(e: string | null): void
  setSkills(skills: SkillEntry[]): void
  toggleExpanded(id: string): void
  /**
   * Toggle the per-skill render-form panel. Opening seeds the draft
   * with each parameter's `default` (so the form starts in a runnable
   * state for the no-input case).
   */
  toggleRenderForm(id: string): void
  setParamValue(skillId: string, name: string, value: unknown): void
  /**
   * Submit the draft to the kernel's `render` handler. On success
   * stashes the result on `renderResults`; on failure stashes the
   * message on `renderErrors`. Single-flight per skill — concurrent
   * calls for the same id are coalesced to whichever finishes first.
   */
  renderSkill(api: SkillsKernelAPI, id: string): Promise<void>
  clearRenderResult(id: string): void

  /** BL-022 — open the inline editor for an existing skill id. The
   *  draft is hydrated from the listing snapshot (the kernel returns
   *  the full body in `list`, so no extra round trip). */
  openEditor(id: string): void
  /** BL-022 — open the editor with a starter template for a fresh
   *  skill. The user picks the slug from the `id` field; save mints
   *  the relpath under `.forge/skills/<id>.skill.md`. */
  openNewSkill(): void
  /** Close the editor without saving. */
  cancelEditor(): void
  /** Patch the in-flight draft. Cheap shallow merge so leaf-form
   *  inputs can fire on every keystroke. */
  patchDraft(patch: Partial<SkillDraft>): void
  /** Save the current draft via `write_file` + `reload`. Returns
   *  `true` on success. */
  saveDraft(api: SkillsKernelAPI): Promise<boolean>
  /** Delete a skill via `delete_file` + `reload`. */
  deleteSkill(api: SkillsKernelAPI, id: string): Promise<boolean>
  reset(): void
}

/** Seed a draft with each parameter's declared default. Missing
 *  defaults stay absent so the kernel falls through to its own
 *  default-resolution path. */
function seedDraft(params: SkillParameter[]): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const p of params) {
    if (p.default !== undefined && p.default !== null) {
      out[p.name] = p.default
    }
  }
  return out
}

export const useSkillsStore = create<SkillsStoreState>((set, get) => ({
  loading: false,
  loadError: null,
  skills: [],
  expandedId: null,
  renderingId: null,
  paramDrafts: {},
  renderResults: {},
  renderErrors: {},
  rendering: null,

  editingId: null,
  draft: null,
  saveError: null,
  saving: false,

  setLoading: (b) => set({ loading: b }),
  setLoadError: (e) => set({ loadError: e }),
  setSkills: (skills) => set({ skills }),
  toggleExpanded: (id) =>
    set((s) => ({ expandedId: s.expandedId === id ? null : id })),
  toggleRenderForm: (id) =>
    set((s) => {
      if (s.renderingId === id) {
        return { renderingId: null }
      }
      const skill = s.skills.find((x) => x.id === id)
      const drafts = { ...s.paramDrafts }
      if (skill && drafts[id] === undefined) {
        drafts[id] = seedDraft(skill.parameters)
      }
      return { renderingId: id, paramDrafts: drafts }
    }),
  setParamValue: (skillId, name, value) =>
    set((s) => {
      const current = s.paramDrafts[skillId] ?? {}
      return {
        paramDrafts: {
          ...s.paramDrafts,
          [skillId]: { ...current, [name]: value },
        },
      }
    }),
  renderSkill: async (api, id) => {
    // Single-flight: drop concurrent render of the same skill. (Different
    // ids may run in parallel — they don't share kernel state.)
    if (get().rendering === id) return
    set({ rendering: id })
    const values = get().paramDrafts[id] ?? {}
    try {
      const raw = await api.invoke<unknown>(SKILLS_PLUGIN_ID, CMD_RENDER, {
        id,
        values,
      })
      const result = decodeRenderResult(raw, id)
      set((s) => {
        const nextErrors = { ...s.renderErrors }
        delete nextErrors[id]
        return {
          renderResults: { ...s.renderResults, [id]: result },
          renderErrors: nextErrors,
          rendering: null,
        }
      })
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set((s) => ({
        renderErrors: { ...s.renderErrors, [id]: message },
        rendering: null,
      }))
    }
  },
  clearRenderResult: (id) =>
    set((s) => {
      const results = { ...s.renderResults }
      const errors = { ...s.renderErrors }
      delete results[id]
      delete errors[id]
      return { renderResults: results, renderErrors: errors }
    }),
  openEditor: (id) =>
    set((s) => {
      const skill = s.skills.find((x) => x.id === id)
      if (!skill) return {}
      return {
        editingId: id,
        saveError: null,
        saving: false,
        draft: skillToDraft(skill),
      }
    }),
  openNewSkill: () =>
    set({
      editingId: '__new__',
      saveError: null,
      saving: false,
      draft: starterDraft(),
    }),
  cancelEditor: () =>
    set({
      editingId: null,
      draft: null,
      saveError: null,
      saving: false,
    }),
  patchDraft: (patch) =>
    set((s) => {
      if (!s.draft) return {}
      // For a new skill, keep the relpath in lockstep with the id
      // unless the user has explicitly overridden it.
      const next = { ...s.draft, ...patch }
      if (s.draft.isNew && patch.id !== undefined && patch.relpath === undefined) {
        next.relpath = newRelpathForId(patch.id ?? '')
      }
      return { draft: next }
    }),
  saveDraft: async (api) => {
    const { draft } = get()
    if (!draft) return false
    const validation = validateDraft(draft)
    if (validation) {
      set({ saveError: validation })
      return false
    }
    set({ saving: true, saveError: null })
    const text = serializeDraft(draft)
    const bytes = encodeUtf8(text)
    try {
      await api.invoke<unknown>(STORAGE_PLUGIN_ID, CMD_WRITE_FILE, {
        path: draft.relpath,
        bytes,
      })
      // Reload so the registry reflects the saved file. Failures
      // here aren't fatal — the file is on disk; surface as a soft
      // warning by logging only.
      try {
        await api.invoke<unknown>(SKILLS_PLUGIN_ID, CMD_RELOAD, {})
      } catch (err) {
        // eslint-disable-next-line no-console
        clientLogger.warn('[nexus.skills] reload after save failed', err)
      }
      set({ saving: false, editingId: null, draft: null, saveError: null })
      // Tell the plugin's `refresh` driver to re-fetch the listing so
      // the row reflects the saved frontmatter (and the `relpath`
      // appears for newly-created skills).
      emitChange({ id: draft.id, action: 'saved' })
      return true
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set({ saving: false, saveError: message })
      return false
    }
  },
  deleteSkill: async (api, id) => {
    const skill = get().skills.find((x) => x.id === id)
    if (!skill || !skill.relpath) return false
    set({ saving: true, saveError: null })
    try {
      await api.invoke<unknown>(STORAGE_PLUGIN_ID, CMD_DELETE_FILE, {
        path: skill.relpath,
      })
      try {
        await api.invoke<unknown>(SKILLS_PLUGIN_ID, CMD_RELOAD, {})
      } catch (err) {
        // eslint-disable-next-line no-console
        clientLogger.warn('[nexus.skills] reload after delete failed', err)
      }
      set((s) => ({
        saving: false,
        editingId: s.editingId === id ? null : s.editingId,
        draft: s.editingId === id ? null : s.draft,
        // Optimistically prune the listing so the row vanishes
        // before the next refresh round-trip lands.
        skills: s.skills.filter((x) => x.id !== id),
        expandedId: s.expandedId === id ? null : s.expandedId,
      }))
      emitChange({ id, action: 'deleted' })
      return true
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      set({ saving: false, saveError: message })
      return false
    }
  },

  reset: () =>
    set({
      loading: false,
      loadError: null,
      skills: [],
      expandedId: null,
      renderingId: null,
      paramDrafts: {},
      renderResults: {},
      renderErrors: {},
      rendering: null,
      editingId: null,
      draft: null,
      saveError: null,
      saving: false,
    }),
}))

function skillToDraft(skill: SkillEntry): SkillDraft {
  return {
    relpath: skill.relpath,
    isNew: false,
    name: skill.name,
    id: skill.id,
    description: skill.description,
    version: skill.version,
    author: skill.author,
    created: skill.created,
    tags: [...skill.tags],
    applicableContexts: [...skill.applicableContexts],
    triggers: [...skill.triggers],
    dependsOn: [...skill.dependsOn],
    body: skill.body,
  }
}

function starterDraft(): SkillDraft {
  const today = new Date().toISOString().slice(0, 10)
  return {
    relpath: '',
    isNew: true,
    name: '',
    id: '',
    description: '',
    version: '0.1.0',
    author: '',
    created: today,
    tags: [],
    applicableContexts: [],
    triggers: [],
    dependsOn: [],
    body: '# Instructions\n\nDescribe how the model should behave when this skill is active.\n',
  }
}

function newRelpathForId(id: string): string {
  const slug = id.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '')
  return slug.length === 0 ? '' : `.forge/skills/${slug}.skill.md`
}

const ID_PATTERN = /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/

/** Returns an error message when the draft can't be saved; null when
 *  it's good to go. Mirrors the kernel-side `parse_skill_text`'s
 *  required-field set so the user catches problems before disk I/O. */
export function validateDraft(draft: SkillDraft): string | null {
  if (draft.id.trim().length === 0) return 'id is required'
  if (!ID_PATTERN.test(draft.id)) {
    return 'id must be kebab-case (lowercase letters, numbers, hyphens)'
  }
  if (draft.name.trim().length === 0) return 'name is required'
  if (draft.description.trim().length === 0) return 'description is required'
  if (draft.version.trim().length === 0) return 'version is required'
  if (draft.author.trim().length === 0) return 'author is required'
  if (draft.created.trim().length === 0) return 'created is required'
  if (draft.relpath.trim().length === 0) return 'relpath could not be derived from id'
  return null
}

function encodeUtf8(s: string): number[] {
  // Node + browsers both expose TextEncoder. The kernel's write_file
  // accepts a `Vec<u8>` via JSON array of byte ints.
  const enc = new TextEncoder()
  return Array.from(enc.encode(s))
}

/** Defensive decode of the kernel's render response — falls back to
 *  empty strings rather than throwing so the UI can always show
 *  *something* and the error path is reserved for IPC failures. */
function decodeRenderResult(raw: unknown, fallbackId: string): RenderResult {
  if (!raw || typeof raw !== 'object') {
    return { id: fallbackId, name: '', body: '' }
  }
  const r = raw as Record<string, unknown>
  return {
    id: typeof r.id === 'string' ? r.id : fallbackId,
    name: typeof r.name === 'string' ? r.name : '',
    body: typeof r.body === 'string' ? r.body : '',
  }
}
