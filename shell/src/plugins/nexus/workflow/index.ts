import { createElement } from 'react'
import type { Plugin, PluginAPI } from '../../../types/plugin'
import { viewRegistry, workspace } from '../../../workspace'
import { WorkflowView } from './WorkflowView'
import { workflowPaneViewCreator } from './WorkflowPaneView'
import { useWorkflowStore, type WorkflowEntry } from './workflowStore'
import { LONG_RUNNING_OP_TIMEOUT_MS } from '../constants'

const VIEW_ID = 'nexus.workflow.view'

const EVENT_WORKSPACE_OPENED = 'workspace:opened'
const EVENT_WORKSPACE_CLOSED = 'workspace:closed'

const COMMAND_REFRESH = 'nexus.workflow.refresh'
const COMMAND_SHOW = 'nexus.workflow.show'
const COMMAND_VALIDATE = 'nexus.workflow.validate'

const WORKFLOW_PLUGIN_ID = 'com.nexus.workflow'
// Verified against crates/nexus-workflow/src/core_plugin.rs::dispatch_async:
//   `list`     args `{}`              → `Workflow[]` (full struct per lib.rs::Workflow).
//   `run`      args `{ name, variables? }` → `WorkflowRun` (final outcome).
//   `validate` args `{ text }`        → parsed `Workflow` JSON, or
//                                       `ExecutionFailed { reason: "invalid workflow: <serde err>" }`.
const LIST_COMMAND = 'list'
const RUN_COMMAND = 'run'
const VALIDATE_COMMAND = 'validate'

// Validate is a synchronous TOML parse on the kernel side. Five
// seconds is plenty of headroom even for very large definitions and
// keeps the UI from hanging if the bridge ever gets wedged.
const WORKFLOW_VALIDATE_TIMEOUT_MS = 5_000

// Long-running runs would otherwise hit the 30s default timeout in the
// kernel bridge. Pick a generous ceiling — workflows can spawn agent
// runs, terminal commands, AI calls.
const RUN_TIMEOUT_MS = LONG_RUNNING_OP_TIMEOUT_MS

/**
 * Decode `Workflow[]` from the kernel into the sidebar's `WorkflowEntry`
 * projection. Tolerant of missing fields — older `.workflow.toml`
 * files predate some keys, and the kernel preserves them as-is.
 */
function decode(raw: unknown): WorkflowEntry[] {
  if (!Array.isArray(raw)) return []
  const out: WorkflowEntry[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const wf = item as Record<string, unknown>
    const meta = (wf.workflow ?? {}) as Record<string, unknown>
    const trigger = (wf.trigger ?? {}) as Record<string, unknown>
    const inputs = (wf.inputs ?? {}) as Record<string, unknown>
    const steps = Array.isArray(wf.steps) ? wf.steps : []
    const name = typeof meta.name === 'string' ? meta.name : null
    if (!name) continue
    out.push({
      name,
      description: typeof meta.description === 'string' ? meta.description : '',
      triggerType: typeof trigger.type === 'string' ? trigger.type : 'unknown',
      stepCount: steps.length,
      hasInputs: Object.keys(inputs).length > 0,
    })
  }
  return out.sort((a, b) => a.name.localeCompare(b.name))
}

/**
 * Pull `workflow.name` out of the kernel's `validate` response so the
 * UI can confirm which workflow just parsed. The handler always
 * returns the full `Workflow` JSON; the `name` field is required by
 * the parser, so a successful response will always have it. Defensive
 * fallback: an empty string keeps the success pill rendering even if
 * the shape ever drifts.
 */
function extractWorkflowName(raw: unknown): string {
  if (!raw || typeof raw !== 'object') return ''
  const meta = (raw as Record<string, unknown>).workflow
  if (!meta || typeof meta !== 'object') return ''
  const name = (meta as Record<string, unknown>).name
  return typeof name === 'string' ? name : ''
}

export const workflowPlugin: Plugin = {
  manifest: {
    id: 'nexus.workflow',
    name: 'Workflows',
    version: '0.1.0',
    core: false,
    activationEvents: ['onStartup'],
    dependsOn: ['nexus.workspace', 'nexus.activityBar', 'nexus.sidebar'],
    contributes: {
      commands: [
        { id: COMMAND_REFRESH, title: 'Refresh Workflows', category: 'Workflows' },
        { id: COMMAND_SHOW, title: 'Show Workflows', category: 'Workflows' },
        { id: COMMAND_VALIDATE, title: 'Validate Workflow TOML', category: 'Workflows' },
      ],
    },
  },

  async activate(api: PluginAPI) {
    const refresh = async () => {
      const store = useWorkflowStore.getState()
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setLoading(false)
        store.setLoadError('Open a workspace to load workflows.')
        store.setWorkflows([])
        return
      }
      store.setLoading(true)
      store.setLoadError(null)
      try {
        const raw = await api.kernel.invoke<unknown>(WORKFLOW_PLUGIN_ID, LIST_COMMAND, {})
        useWorkflowStore.getState().setWorkflows(decode(raw))
        useWorkflowStore.getState().setLoading(false)
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useWorkflowStore.getState().setLoadError(message)
        useWorkflowStore.getState().setWorkflows([])
        useWorkflowStore.getState().setLoading(false)
      }
    }

    const runWorkflow = async (name: string) => {
      const store = useWorkflowStore.getState()
      store.setRunStatus(name, 'running')
      try {
        await api.kernel.invoke<unknown>(
          WORKFLOW_PLUGIN_ID,
          RUN_COMMAND,
          { name },
          RUN_TIMEOUT_MS,
        )
        useWorkflowStore.getState().setRunStatus(name, 'done')
        api.notifications.show({
          type: 'success',
          message: `Workflow "${name}" finished.`,
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        useWorkflowStore.getState().setRunStatus(name, 'error', message)
        api.notifications.show({
          type: 'error',
          message: `Workflow "${name}" failed: ${message}`,
        })
      }
    }

    const validateWorkflow = async (text: string) => {
      const store = useWorkflowStore.getState()
      // Empty text would just return a parser error from the kernel.
      // Catch it client-side so the user gets a clearer hint.
      if (text.trim().length === 0) {
        store.setValidateStatus('error', { error: 'Paste a workflow TOML to validate.' })
        return
      }
      let available = false
      try {
        available = await api.kernel.available()
      } catch {
        available = false
      }
      if (!available) {
        store.setValidateStatus('error', {
          error: 'Open a workspace before validating workflows.',
        })
        return
      }
      store.setValidateStatus('validating')
      try {
        const result = await api.kernel.invoke<unknown>(
          WORKFLOW_PLUGIN_ID,
          VALIDATE_COMMAND,
          { text },
          WORKFLOW_VALIDATE_TIMEOUT_MS,
        )
        const name = extractWorkflowName(result)
        useWorkflowStore.getState().setValidateStatus('ok', { validatedName: name })
      } catch (err) {
        // Kernel surfaces parse errors as "invalid workflow: <serde
        // message>" — strip the prefix when present so the inline
        // panel reads cleanly. Position hints (line/col) inside the
        // serde message are preserved.
        const raw = err instanceof Error ? err.message : String(err)
        const message = raw.replace(/^.*invalid workflow:\s*/, '')
        useWorkflowStore.getState().setValidateStatus('error', { error: message })
      }
    }

    const renderWorkflowView = () =>
      createElement(WorkflowView, {
        onRun: (name: string) => void runWorkflow(name),
        onRefresh: () => void refresh(),
        onValidate: (text: string) => void validateWorkflow(text),
      })

    // Phase 7: legacy SlotRegistry slot:'sidebarContent' entry removed.
    viewRegistry.register('workflow', workflowPaneViewCreator(renderWorkflowView))

    api.activityBar.addItem({
      id: 'nexus.workflow.activityItem',
      icon: '',
      iconName: 'bolt',
      title: 'Workflows',
      viewId: VIEW_ID,
      priority: 30,
      command: COMMAND_SHOW,
    })

    api.commands.register(COMMAND_REFRESH, () => {
      void refresh()
    })
    api.commands.register(COMMAND_SHOW, async () => {
      const leaf = await workspace.ensureLeafOfType('workflow', 'main')
      workspace.revealLeaf(leaf)
    })
    // Command-palette entry mirrors the inline Validate button so a
    // workflow author can run validation on the textarea contents
    // without leaving the keyboard.
    api.commands.register(COMMAND_VALIDATE, () => {
      const text = useWorkflowStore.getState().validate.text
      void validateWorkflow(text)
    })

    // Reload the list whenever a workspace opens; clear it on close so
    // a forge switch doesn't briefly show the previous forge's
    // workflows. nexus.workspace fires `opened` synchronously inside
    // its own activate, which can land before this listener registers
    // on first boot — cover that race by checking kernel.available().
    api.events.on(EVENT_WORKSPACE_OPENED, () => {
      void refresh()
    })
    api.events.on(EVENT_WORKSPACE_CLOSED, () => {
      useWorkflowStore.getState().reset()
    })
    if (await api.kernel.available()) {
      void refresh()
    }
  },
}
