import { create } from 'zustand'

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
  tags: string[]
  applicableContexts: string[]
  triggers: string[]
  parameters: SkillParameter[]
  body: string
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
// Verified against crates/nexus-skills/src/core_plugin.rs HANDLER_RENDER (id 6,
// command name `render`, args `{ id, values? }`).
const CMD_RENDER = 'render'

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
    }),
}))

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
