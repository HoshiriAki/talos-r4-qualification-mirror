/**
 * Asia/Shanghai business-date helpers.
 *
 * ALL business dates MUST use these functions. Never compute a date from
 * `new Date().toISOString().slice(0, 10)` or `new Date()` — both use the
 * host (local) timezone, which produces wrong dates when the machine is
 * not already set to UTC+8.
 */

const SHANGHAI_TZ = 'Asia/Shanghai'

/** ISO-8601 date string — always `YYYY-MM-DD`. */
export type BusinessDate = `${number}-${number}-${number}`

/**
 * Return the current business date in Asia/Shanghai.
 *
 * Accepts an optional input to convert a specific instant. Defaults to
 * `Date.now()`.
 */
export function shanghaiBusinessDate(
  input: Date | string | number = Date.now(),
): BusinessDate {
  const date = input instanceof Date ? input : new Date(input)
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: SHANGHAI_TZ,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  }).formatToParts(date)
  const value: Record<string, string> = {}
  for (const part of parts) {
    value[part.type] = part.value
  }
  return `${value.year}-${value.month}-${value.day}` as BusinessDate
}

export interface DateRange {
  start: BusinessDate
  end: BusinessDate
}

/**
 * Build a date range relative to the Shanghai business day.
 *
 * Presets:
 * - `today`    — start = end = today
 * - `tomorrow` — start = end = tomorrow
 * - `next7Days` — start = today, end = today + 6 days
 */
export function shanghaiDateRange(
  preset: 'today' | 'tomorrow' | 'next7Days',
): DateRange {
  const today = shanghaiBusinessDate()
  const [y, m, d] = today.split('-').map(Number)

  switch (preset) {
    case 'today':
      return { start: today, end: today }
    case 'tomorrow': {
      const t = shanghaiBusinessDate(Date.UTC(y, m - 1, d + 1))
      return { start: t, end: t }
    }
    case 'next7Days': {
      const end = shanghaiBusinessDate(Date.UTC(y, m - 1, d + 6))
      return { start: today, end }
    }
  }
}

/**
 * Format an ISO-8601 timestamp string into a Shanghai-local label.
 * Example: `2026-07-14T16:30:00.000Z` → `2026-07-14 23:30`
 */
export function shanghaiTimestampLabel(input: string): string {
  const d = new Date(input)
  const fmt = new Intl.DateTimeFormat('zh-CN', {
    timeZone: SHANGHAI_TZ,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  })
  return fmt.format(d)
}
