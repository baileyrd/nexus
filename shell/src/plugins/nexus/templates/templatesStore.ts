import { create } from 'zustand'

/**
 * One declared parameter on a template. Mirrors
 * `crates/nexus-templates/src/template.rs::TemplateParameter`.
 */
export interface TemplateParameter {
  name: string
  type?: string
  default?: string | null
  required?: boolean
  description?: string | null
}

/**
 * One row in the templates listing, projected from
 * `com.nexus.templates::list`.
 */
export interface TemplateEntry {
  name: string
  description: string | null
  target_path: string | null
  parameters: TemplateParameter[]
}

interface TemplatesStoreState {
  templates: TemplateEntry[]
  loading: boolean
  loadError: string | null
  /** Currently-selected template (for inline parameter form). */
  selected: string | null
  /** Form values keyed by template name → param name → string. */
  formValues: Record<string, Record<string, string>>
  /** Last successful apply: forge-relative path. Used for the "Open"
   *  link in the panel after a successful render. */
  lastApplied: string | null
  setTemplates: (rows: TemplateEntry[]) => void
  setLoading: (loading: boolean) => void
  setLoadError: (err: string | null) => void
  select: (name: string | null) => void
  setFormValue: (template: string, param: string, value: string) => void
  clearForm: (template: string) => void
  setLastApplied: (path: string | null) => void
  reset: () => void
}

export const useTemplatesStore = create<TemplatesStoreState>((set) => ({
  templates: [],
  loading: false,
  loadError: null,
  selected: null,
  formValues: {},
  lastApplied: null,
  setTemplates: (templates) => set({ templates }),
  setLoading: (loading) => set({ loading }),
  setLoadError: (loadError) => set({ loadError }),
  select: (selected) => set({ selected }),
  setFormValue: (template, param, value) =>
    set((s) => ({
      formValues: {
        ...s.formValues,
        [template]: { ...(s.formValues[template] ?? {}), [param]: value },
      },
    })),
  clearForm: (template) =>
    set((s) => {
      const next = { ...s.formValues }
      delete next[template]
      return { formValues: next }
    }),
  setLastApplied: (lastApplied) => set({ lastApplied }),
  reset: () =>
    set({
      templates: [],
      loading: false,
      loadError: null,
      selected: null,
      formValues: {},
      lastApplied: null,
    }),
}))
