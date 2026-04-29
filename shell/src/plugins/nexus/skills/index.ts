import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { SkillsView } from './SkillsView'
import { skillsPaneViewCreator } from './SkillsPaneView'
import {
  subscribeSkillsChanged,
  useSkillsStore,
  type SkillEntry,
  type SkillParameter,
} from './skillsStore'

const VIEW_ID = 'nexus.skills.view'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const COMMAND_REFRESH = 'nexus.skills.refresh'
const COMMAND_SHOW = 'nexus.skills.show'

const SKILLS_PLUGIN_ID = 'com.nexus.skills'
// Verified against crates/nexus-skills/src/core_plugin.rs:
//   `list` args `{}` → `Skill[]` (frontmatter flatten + body, see lib.rs::Skill).
const LIST_COMMAND = 'list'

/**
 * Decode `Skill[]` from the kernel into `SkillEntry`. Frontmatter is
 * flattened on the wire (serde `#[serde(flatten)]`) so all the
 * `meta` fields live at the top level alongside `body`. Defensive
 * coercion: `tags` / `applicable_contexts` / `triggers` may be
 * absent on hand-written skill files.
 */
function decode(raw: unknown): SkillEntry[] {
  if (!Array.isArray(raw)) return []
  const out: SkillEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const sk = item as Record<string, unknown>
    const id = typeof sk.id === 'string' ? sk.id : null
    const name = typeof sk.name === 'string' ? sk.name : null
    if (!id || !name) continue
    out.push({
      id,
      name,
      description: typeof sk.description === 'string' ? sk.description : '',
      version: typeof sk.version === 'string' ? sk.version : '',
      author: typeof sk.author === 'string' ? sk.author : '',
      created: typeof sk.created === 'string' ? sk.created : '',
      tags: stringArray(sk.tags),
      applicableContexts: stringArray(sk.applicable_contexts),
      triggers: stringArray(sk.triggers),
      parameters: decodeParameters(sk.parameters),
      dependsOn: stringArray(sk.depends_on),
      body: typeof sk.body === 'string' ? sk.body : '',
      relpath: typeof sk.relpath === 'string' ? sk.relpath : '',
    })
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

function stringArray(v: unknown): string[] {
  if (!Array.isArray(v)) return []
  return v.filter((x): x is string => typeof x === 'string')
}

/**
 * Decode the `parameters:` block from `Skill.frontmatter`. Mirrors
 * `SkillParameter` in crates/nexus-skills/src/lib.rs L114-134:
 *
 *   { name, type, description?, values?, items?, default? }
 *
 * `values` is YAML — could be scalars or sequences — but the form
 * UI only uses string enum members, so we coerce to strings and
 * drop anything that isn't representable.
 */
function decodeParameters(v: unknown): SkillParameter[] {
  if (!Array.isArray(v)) return []
  const out: SkillParameter[] = []
  for (const item of v) {
    if (!item || typeof item !== 'object') continue
    const p = item as Record<string, unknown>
    const name = typeof p.name === 'string' ? p.name : null
    if (!name) continue
    const ptype = typeof p.type === 'string' ? p.type : 'string'
    const enumValues: string[] = []
    if (Array.isArray(p.values)) {
      for (const x of p.values) {
        if (typeof x === 'string') enumValues.push(x)
        else if (typeof x === 'number' || typeof x === 'boolean') enumValues.push(String(x))
      }
    }
    out.push({
      name,
      type: ptype,
      description: typeof p.description === 'string' ? p.description : '',
      values: enumValues,
      items: typeof p.items === 'string' ? p.items : null,
      default: 'default' in p ? p.default : undefined,
    })
  }
  return out
}

export const skillsPlugin: Plugin = {
  manifest: {
    id: 'nexus.skills',
    name: 'Skills',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [
        { id: COMMAND_REFRESH, title: 'Refresh Skills', category: 'Skills' },
        { id: COMMAND_SHOW, title: 'Show Skills', category: 'Skills' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const refresh = async () => {
      const store = useSkillsStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setLoading(false)
        store.setLoadError('Open a workspace to load skills.')
        store.setSkills([])
        return
      }
      store.setLoading(true)
      store.setLoadError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(SKILLS_PLUGIN_ID, LIST_COMMAND, {})
        useSkillsStore.getState().setSkills(decode(raw))
        useSkillsStore.getState().setLoading(false)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useSkillsStore.getState().setLoadError(message)
        useSkillsStore.getState().setSkills([])
        useSkillsStore.getState().setLoading(false)
      }
    }

    const renderSkillsView = () =>
      createElement(SkillsView, {
        onRefresh: () => void refresh(),
        kernel: api.kernel,
      })

    // Phase 7: legacy SlotRegistry slot:'sidebarContent' entry removed.
    viewRegistry.register('skills', skillsPaneViewCreator(renderSkillsView))

    api.activityBar.addItem({
      id: 'nexus.skills.activityItem',
      icon: '',
      iconName: 'book',
      title: 'Skills',
      viewId: VIEW_ID,
      priority: 40,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refresh()
    })
    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType('skills', 'main')
      workspace.revealLeaf(leaf)
    })

    // Same load-on-open / clear-on-close lifecycle as nexus.workflow.
    // Workspace restoration emits `opened` synchronously before this
    // listener registers on first boot — cover with a kernel.available()
    // probe.
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useSkillsStore.getState().reset()
    })
    // BL-022 — refresh the listing whenever the in-app editor saves
    // or deletes a skill. The store emits via a module-local
    // subscription so we don't have to reach into the host EventBus.
    subscribeSkillsChanged(() => {
      void refresh()
    })
    if (await api.kernel.available()) {
      void refresh()
    }
  },
}
