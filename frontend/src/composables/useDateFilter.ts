import { computed, type ComputedRef, type WritableComputedRef } from 'vue'

function toDateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function toDate(s: string | undefined): Date | null {
  return s ? new Date(s + 'T00:00:00') : null
}

interface DateBridgeOptions {
  get: () => string | undefined
  set: (dateKey: string) => void
}

function dateBridge(opts: DateBridgeOptions): WritableComputedRef<Date | null> {
  return computed<Date | null>({
    get: () => toDate(opts.get()),
    set: (v: Date | null) => { opts.set(v ? toDateKey(v) : '') },
  })
}

export function useDateFilter() {
  return { toDateKey, toDate, dateBridge }
}
