// ── Snapshot / Diff / Prompt 输出 ────────────────────────────────────────
// 三者必须来自同一状态 revision：Snapshot 使用当前权威 session，Diff 比较
// baseline 与 current，Prompt 基于同一 Diff 生成。禁止维护第二套隐式状态。

import type { UiComponentDefinition } from '@/ui'
import type { LabSessionState } from '../state/lab-session'
import type { LabJournalEntry } from '../state/change-journal'
import { LAB_SCHEMA_VERSION } from '../state/lab-session'

export interface LabExportBundle {
  schemaVersion: string
  labVersion: string
  componentId: string
  baselineRevision: number
  currentRevision: number
  baseline: LabSessionState
  current: LabSessionState
  changes: LabJournalEntry[]
  auditHistory: LabJournalEntry[]
  diagnostics: string[]
}

export interface CapturedExportBundle {
  snapshot: LabExportBundle
  diff: ReturnType<typeof createDiff>
  prompt: string
}

function deepFreeze<T>(value: T): T {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    for (const child of Object.values(value as Record<string, unknown>)) deepFreeze(child)
    Object.freeze(value)
  }
  return value
}

export function createSnapshot(
  component: UiComponentDefinition,
  session: LabSessionState,
  baseline: LabSessionState,
  diagnostics: string[],
  changes: LabJournalEntry[] = [],
  auditHistory: LabJournalEntry[] = changes,
): LabExportBundle {
  const frozenBaseline = JSON.parse(JSON.stringify(baseline)) as LabSessionState
  const frozenCurrent = JSON.parse(JSON.stringify(session)) as LabSessionState
  const frozenChanges = JSON.parse(JSON.stringify(changes)) as LabJournalEntry[]
  return {
    schemaVersion: LAB_SCHEMA_VERSION,
    labVersion: '0.7.0',
    componentId: component.id,
    baselineRevision: baseline.revision,
    currentRevision: session.revision,
    baseline: frozenBaseline,
    current: frozenCurrent,
    changes: frozenChanges,
    auditHistory: JSON.parse(JSON.stringify(auditHistory)) as LabJournalEntry[],
    diagnostics: [...diagnostics],
  }
}

export function createDiff(baseline: LabSessionState, current: LabSessionState, changes: LabJournalEntry[]) {
  return {
    schemaVersion: LAB_SCHEMA_VERSION,
    componentId: current.component.id,
    sceneId: current.scene.id,
    baselineRevision: baseline.revision,
    currentRevision: current.revision,
    changes: changes.map(({ path, before, after, source, revision }) => ({ path, before, after, source, revision })),
  }
}

export function captureExportBundle(
  component: UiComponentDefinition,
  baseline: LabSessionState,
  current: LabSessionState,
  changes: LabJournalEntry[],
  diagnostics: string[],
  auditHistory: LabJournalEntry[] = changes,
): CapturedExportBundle {
  const snapshot = createSnapshot(component, current, baseline, diagnostics, changes, auditHistory)
  const frozenBaseline = JSON.parse(JSON.stringify(snapshot.baseline)) as LabSessionState
  const frozenCurrent = JSON.parse(JSON.stringify(snapshot.current)) as LabSessionState
  const frozenChanges = JSON.parse(JSON.stringify(snapshot.changes)) as LabJournalEntry[]
  const diff = createDiff(frozenBaseline, frozenCurrent, frozenChanges)
  const prompt = createAgentPrompt(component, diff)
  return {
    snapshot: deepFreeze(snapshot),
    diff: deepFreeze(diff),
    prompt,
  }
}

export function createAgentPrompt(component: UiComponentDefinition, diff: ReturnType<typeof createDiff>) {
  const exportDefinition = component.lab?.agentExport
  const preserve = exportDefinition?.preserve ?? ['public props', 'accessibility semantics', 'reduced-motion behavior', 'TALOS design tokens']
  const lines = diff.changes.length
    ? diff.changes.map((change, index) => `${index + 1}. Set ${change.path} from ${JSON.stringify(change.before)} to ${JSON.stringify(change.after)}.`)
    : ['No changes are currently recorded.']
  return [
    `Target component: ${component.id}`,
    `Implementation path: ${component.implementation.path}`,
    `Scene: ${diff.sceneId}`,
    `Baseline revision: ${diff.baselineRevision}`,
    `Current revision: ${diff.currentRevision}`,
    '',
    'Preserve:',
    ...preserve.map((item) => `- ${item}`),
    '',
    'Apply:',
    ...lines,
    '',
    'Acceptance:',
    '- Works in zh-CN and en.',
    '- Does not read tenant data or call business APIs.',
    '- Preserves accessibility and reduced-motion behavior.',
    '- Registry, TypeScript, and production build pass.',
  ].join('\n')
}
