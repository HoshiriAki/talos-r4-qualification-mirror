// ── UI Lab Change Journal ────────────────────────────────────────────────
// 所有实验变更（inspector / scene / environment / timeline / sandbox /
// system）都从这里进入审计记录。before/after 来自权威状态树，Snapshot、
// Diff、Prompt 共用同一 revision 语义。Journal 不充当唯一状态来源。

import type { UiLabPatchSource } from './lab-session'

export interface LabJournalEntry {
  id: string
  timestamp: string
  revision: number
  source: UiLabPatchSource
  path: string
  before: unknown
  after: unknown
  componentId: string
}

export interface LabJournalHistoryEntry extends LabJournalEntry {
  kind: 'mutation'
}

export function mergeJournalChange(
  entries: LabJournalEntry[],
  next: LabJournalEntry,
  options: { keepFirstChangedAt?: boolean } = { keepFirstChangedAt: true },
): LabJournalEntry[] {
  const index = entries.findIndex(
    (entry) =>
      entry.componentId === next.componentId &&
      entry.path === next.path,
  )
  if (index < 0) {
    return [...entries, next]
  }
  const merged = entries.map((entry, current) => {
    if (current !== index) return entry
    const before = options.keepFirstChangedAt === false ? next.before : entry.before
    return { ...next, id: entry.id, before }
  })
  return merged.filter((entry) => JSON.stringify(entry.before) !== JSON.stringify(entry.after))
}
