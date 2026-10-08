/**
 * Shanghai business-date tests — to be run when vitest is integrated.
 *
 * Manual validation (run `npx tsx` on this file or copy assertions into console):
 *   import { shanghaiBusinessDate, shanghaiDateRange, shanghaiTimestampLabel } from './businessDate'
 *
 *   // Midnight boundary: 2026-07-14T16:30Z = 2026-07-15 00:30 Shanghai
 *   console.assert(shanghaiBusinessDate('2026-07-14T16:30:00Z') === '2026-07-15', 'crosses midnight')
 *
 *   // Within same day: 2026-07-14T10:00Z = 2026-07-14 18:00 Shanghai
 *   console.assert(shanghaiBusinessDate('2026-07-14T10:00:00Z') === '2026-07-14', 'same day')
 *
 *   // Cross-month: 2026-01-31T20:00Z = 2026-02-01 04:00 Shanghai
 *   console.assert(shanghaiBusinessDate('2026-01-31T20:00:00Z') === '2026-02-01', 'cross month')
 *
 *   // Date object input
 *   console.assert(shanghaiBusinessDate(new Date('2026-07-15T02:00:00Z')) === '2026-07-15', 'Date input')
 *
 *   // shanghaiTimestampLabel: 2026-07-14T16:30Z → includes 00:30 (Shanghai midnight)
 *   console.assert(shanghaiTimestampLabel('2026-07-14T16:30:00Z').includes('00:30'), 'timestamp label')
 */

export {}
