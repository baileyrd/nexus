// Starter schemas for the New-base template picker. Each template
// ships a schema + optional seed records the kernel stamps with v4
// UUIDs. Field definitions mirror what the shell cell editors
// understand — keep in sync with fieldTypes.ts.

import type { BaseRecord, BaseSchema } from './kernelClient'

export interface BaseTemplate {
  id: string
  label: string
  description: string
  schema: BaseSchema
  seedRecords: BaseRecord[]
}

const todayIso = (): string => new Date().toISOString().slice(0, 10)

export const BASE_TEMPLATES: BaseTemplate[] = [
  {
    id: 'blank',
    label: 'Blank',
    description: 'Title + notes; add columns as you go.',
    schema: {
      version: '1.0',
      fields: {
        title: { type: 'title', required: true, primary: true },
        notes: { type: 'long-text' },
      },
    },
    seedRecords: [],
  },
  {
    id: 'tasks',
    label: 'Tasks',
    description: 'Status · due date · priority · assignee.',
    schema: {
      version: '1.0',
      fields: {
        title: { type: 'title', required: true, primary: true },
        status: {
          type: 'select',
          options: ['Todo', 'Doing', 'Blocked', 'Done'],
          required: true,
        },
        priority: { type: 'select', options: ['Low', 'Med', 'High'] },
        due: { type: 'date' },
        assignee: { type: 'text' },
        notes: { type: 'long-text' },
      },
    },
    seedRecords: [
      { id: '', title: 'Write project brief', status: 'Todo', priority: 'High', due: todayIso() },
      { id: '', title: 'Review open PRs', status: 'Doing', priority: 'Med' },
    ],
  },
  {
    id: 'crm',
    label: 'CRM',
    description: 'Contacts · company · stage · last touch.',
    schema: {
      version: '1.0',
      fields: {
        name: { type: 'title', required: true, primary: true },
        company: { type: 'text' },
        email: { type: 'email' },
        stage: {
          type: 'select',
          options: ['Lead', 'Qualified', 'Proposal', 'Closed-Won', 'Closed-Lost'],
        },
        last_touch: { type: 'date' },
        notes: { type: 'long-text' },
      },
    },
    seedRecords: [],
  },
  {
    id: 'projects',
    label: 'Projects',
    description: 'Status · owner · start/end · progress.',
    schema: {
      version: '1.0',
      fields: {
        title: { type: 'title', required: true, primary: true },
        status: {
          type: 'select',
          options: ['Planning', 'Active', 'On hold', 'Shipped'],
        },
        owner: { type: 'text' },
        start: { type: 'date' },
        end: { type: 'date' },
        progress: { type: 'percent' },
        notes: { type: 'long-text' },
      },
    },
    seedRecords: [],
  },
  {
    id: 'notes',
    label: 'Notes',
    description: 'Title · tags · created · body.',
    schema: {
      version: '1.0',
      fields: {
        title: { type: 'title', required: true, primary: true },
        tags: { type: 'multi-select', options: [] },
        created: { type: 'date' },
        body: { type: 'long-text' },
      },
    },
    seedRecords: [],
  },
]
