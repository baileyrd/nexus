// BL-054 Phase 2 — drift detection between architecture.md and the
// kernel's actual skill/workflow registries.
//
// Three drift kinds:
//   - skillMissing       — task tagged `[skill | …]` with no
//                          matching `.skill.md`
//   - automationMissing  — task tagged `[… | foundation | … | local
//                          cron …]` with no matching `.workflow.toml`
//                          trigger
//   - undocumentedSkill  — `.skill.md` file present with no entry in
//                          architecture.md
//
// Tasks tagged `manual`, `command`, or `unknown` for type don't trigger
// the skill check; tasks whose class isn't `foundation` skip the
// automation check. The matching is purely on the task `id` against
// the skill `id`; case-sensitive — the skill registry is the canonical
// source of casing.

import type { Architecture, ArchitectureTask } from './architectureParser'

export type DriftKind = 'skillMissing' | 'automationMissing' | 'undocumentedSkill'

export interface DriftItem {
  kind: DriftKind
  /** Task or skill identifier the drift hangs off. */
  id: string
  /** Domain the task belongs to, when relevant. Omitted for
   *  undocumentedSkill since the skill has no domain affiliation. */
  domain?: string
  /** Human-readable summary the panel renders below the offending row. */
  message: string
}

export interface DriftInputs {
  architecture: Architecture
  /** All known skill ids — projection of `com.nexus.skills::list`. */
  skillIds: ReadonlySet<string>
  /** Workflow names known to the kernel — projection of
   *  `com.nexus.workflow::list`. The convention this BL adopts is that
   *  the workflow `name` matches the task id when one exists; finer
   *  matching can layer on later. */
  workflowNames: ReadonlySet<string>
}

export interface DriftReport {
  /** Per-task drift, keyed by `domain::task_id` so the panel can
   *  surface a warning inline next to the task. */
  byTask: Map<string, DriftItem[]>
  /** Top-level drift items not associated with a task — currently
   *  just `undocumentedSkill`. */
  unattached: DriftItem[]
}

/** Run the drift checks against the parsed architecture + live
 *  registry snapshots. Pure — no IPC, no side effects, runs on every
 *  re-render so panel feedback stays current. */
export function detectDrift({ architecture, skillIds, workflowNames }: DriftInputs): DriftReport {
  const byTask = new Map<string, DriftItem[]>()
  const documentedTaskIds = new Set<string>()

  for (const domain of architecture.domains) {
    for (const task of domain.tasks) {
      documentedTaskIds.add(task.id)
      const drifts = checkTaskDrift(task, domain.name, skillIds, workflowNames)
      if (drifts.length > 0) {
        byTask.set(taskKey(domain.name, task.id), drifts)
      }
    }
  }

  const unattached: DriftItem[] = []
  for (const skillId of skillIds) {
    if (!documentedTaskIds.has(skillId)) {
      unattached.push({
        kind: 'undocumentedSkill',
        id: skillId,
        message: `Skill "${skillId}" exists but has no entry in architecture.md`,
      })
    }
  }
  // Stable order so panel rendering doesn't shuffle on every refresh.
  unattached.sort((a, b) => a.id.localeCompare(b.id))

  return { byTask, unattached }
}

function checkTaskDrift(
  task: ArchitectureTask,
  domain: string,
  skillIds: ReadonlySet<string>,
  workflowNames: ReadonlySet<string>,
): DriftItem[] {
  const out: DriftItem[] = []
  if (task.type === 'skill' && !skillIds.has(task.id)) {
    out.push({
      kind: 'skillMissing',
      id: task.id,
      domain,
      message: `Task tagged \`skill\` but no \`.skill.md\` matches id "${task.id}"`,
    })
  }
  if (
    task.class === 'foundation'
    && task.automation.kind === 'cron'
    && !workflowNames.has(task.id)
  ) {
    out.push({
      kind: 'automationMissing',
      id: task.id,
      domain,
      message: `Foundation task with cron trigger has no matching workflow named "${task.id}"`,
    })
  }
  return out
}

/** Stable key for `byTask` lookup. Public so the view can read drift
 *  per task without re-implementing the join. */
export function taskKey(domain: string, taskId: string): string {
  return `${domain}::${taskId}`
}
