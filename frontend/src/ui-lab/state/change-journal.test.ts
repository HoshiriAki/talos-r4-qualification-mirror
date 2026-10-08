import { describe, expect, it } from 'vitest'
import { mergeJournalChange, type LabJournalEntry } from './change-journal'

function entry(revision: number, before: unknown, after: unknown, source: LabJournalEntry['source'] = 'inspector'): LabJournalEntry {
  return { id: `r${revision}`, timestamp: `2026-07-31T00:00:0${revision}.000Z`, revision, source, path: 'component.props.label', before, after, componentId: 'talos.ui.button' }
}

describe('effective journal merge', () => {
  it('preserves A as the baseline across A to B to C even when sources change', () => {
    let changes = mergeJournalChange([], entry(1, 'A', 'B', 'inspector'))
    changes = mergeJournalChange(changes, entry(2, 'B', 'C', 'sandbox'))
    expect(changes).toHaveLength(1)
    expect(changes[0]).toMatchObject({ before: 'A', after: 'C', revision: 2, source: 'sandbox' })
  })

  it('removes an effective diff when a path returns to its baseline', () => {
    let changes = mergeJournalChange([], entry(1, 'A', 'B'))
    changes = mergeJournalChange(changes, entry(2, 'B', 'A', 'timeline'))
    expect(changes).toEqual([])
  })
})
