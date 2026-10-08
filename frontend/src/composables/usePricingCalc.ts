import { ref } from 'vue'
import { generateQuoteText } from '@/constants/quoteTemplates'
import { usePricingStore } from '@/stores/pricing'
import { fetchPriceEstimate, fetchWarehouseRoute, fetchProvinces } from '@/api/pricing-estimate'
import type { HolidayRule } from '@/api/pricing'
import type { PriceDetail, WarehouseRoute } from '@/api/pricing-estimate'

const DYNAMIC_ROLLING_DAYS = 15
const DAY_MS = 24 * 60 * 60 * 1000

export const DAY_LABELS = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']

export const OCCUPANCY_COEFFICIENTS = { shipping: 0.2, normal: 1.0, return: 0.2 }

export interface DateEntry {
  dateKey: string
  label: string
  dayOfWeek: number
  dayLabel: string
}

export interface RollingDateEntry {
  dateKey: string
  label: string
  dayLabel: string
  defaultPrice: number
  dynamicPrice: number | undefined
  effectivePrice: number
}

export function usePricingCalc() {
  const pricingStore = usePricingStore()
  const provinces = ref<string[]>([])
  const provincesLoading = ref(false)

  function pad2(n: number): string { return String(n).padStart(2, '0') }

  function toDateLocal(dateLike: any): Date {
    if (dateLike instanceof Date) return new Date(dateLike.getFullYear(), dateLike.getMonth(), dateLike.getDate())
    const text = String(dateLike || '').trim()
    if (!text) return new Date()
    const m = text.match(/^(\d{4})[/-](\d{1,2})[/-](\d{1,2})/)
    if (m) {
      const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
      if (d.getFullYear() === Number(m[1]) && d.getMonth() === Number(m[2]) - 1 && d.getDate() === Number(m[3])) return d
    }
    const fallback = new Date(text)
    return isNaN(fallback.getTime()) ? new Date() : new Date(fallback.getFullYear(), fallback.getMonth(), fallback.getDate())
  }

  function formatDate(dateLike: any): string {
    const d = toDateLocal(dateLike)
    return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`
  }

  function addDays(dateLike: any, days: number): Date {
    const d = toDateLocal(dateLike)
    return new Date(d.getTime() + days * DAY_MS)
  }

  function parseDateKey(dateKey: string): Date | null {
    const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(dateKey || '').trim())
    if (!m) return null
    const d = new Date(Number(m[1]), Number(m[2]) - 1, Number(m[3]))
    if (d.getFullYear() === Number(m[1]) && d.getMonth() === Number(m[2]) - 1 && d.getDate() === Number(m[3])) return d
    return null
  }

  function getRollingDateKeys(days = DYNAMIC_ROLLING_DAYS, fromDate?: string): string[] {
    const start = fromDate ? toDateLocal(fromDate) : toDateLocal(new Date())
    return Array.from({ length: days }, (_, i) => {
      const d = new Date(start.getTime() + i * DAY_MS)
      return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`
    })
  }

  // ── Price calculation ──

  function getBaseDailyPrice(
    dateLike: any,
    baseWeekdayOverride?: number,
    baseWeekendOverride?: number,
  ): number {
    const day = toDateLocal(dateLike).getDay()
    const isWeekend = day === 0 || day === 5 || day === 6
    const weekday = baseWeekdayOverride ?? pricingStore.baseWeekdayPrice
    const weekend = baseWeekendOverride ?? pricingStore.baseWeekendPrice
    return isWeekend ? weekend : weekday
  }

  function isDateInHolidayRange(dateKey: string, rule: HolidayRule): boolean {
    const checkDate = toDateLocal(dateKey)
    const start = toDateLocal(rule.startDate)
    const end = toDateLocal(rule.endDate)
    if (rule.includePreviousDay) {
      const prevDay = new Date(start.getTime() - DAY_MS)
      const prevKey = formatDate(prevDay)
      if (dateKey === prevKey) return true
    }
    return checkDate >= start && checkDate <= end
  }

  function isHoliday(dateLike: any, rulesOverride?: HolidayRule[]): boolean {
    const rules = rulesOverride ?? pricingStore.holidayRules
    const dateKey = formatDate(dateLike)
    return rules.some(rule => isDateInHolidayRange(dateKey, rule))
  }

  function getHolidayPrice(dateLike: any, rulesOverride?: HolidayRule[]): number | null {
    const rules = rulesOverride ?? pricingStore.holidayRules
    const dateKey = formatDate(dateLike)
    for (const rule of rules) {
      if (isDateInHolidayRange(dateKey, rule)) return rule.price
    }
    return null
  }

  function getDailyPrice(
    dateLike: any,
    dynamicMapOverride?: Record<string, number>,
    baseWeekdayOverride?: number,
    baseWeekendOverride?: number,
    rulesOverride?: HolidayRule[],
  ): number {
    const dateKey = formatDate(dateLike)
    const dynamicMap = dynamicMapOverride ?? pricingStore.dynamicPriceMap
    if (dynamicMap[dateKey] !== undefined && dynamicMap[dateKey] !== null) {
      return dynamicMap[dateKey]
    }
    const holidayPrice = getHolidayPrice(dateLike, rulesOverride)
    if (holidayPrice !== null) return holidayPrice
    return getBaseDailyPrice(dateLike, baseWeekdayOverride, baseWeekendOverride)
  }

  function getRollingDateEntries(
    days = DYNAMIC_ROLLING_DAYS,
    baseWeekdayOverride?: number,
    baseWeekendOverride?: number,
    rulesOverride?: HolidayRule[],
    dynamicMapOverride?: Record<string, number>,
  ): RollingDateEntry[] {
    const keys = getRollingDateKeys(days)
    const rules = rulesOverride ?? pricingStore.holidayRules
    const dynamicMap = dynamicMapOverride ?? pricingStore.dynamicPriceMap
    return keys.map(dateKey => {
      const d = parseDateKey(dateKey)
      const dayOfWeek = d ? d.getDay() : 0
      const label = d ? DAY_LABELS[dayOfWeek] : ''
      const basePrice = getBaseDailyPrice(dateKey, baseWeekdayOverride, baseWeekendOverride)
      const holidayPrice = getHolidayPrice(dateKey, rules)
      const defaultPrice = holidayPrice !== null ? holidayPrice : basePrice
      const dynamicPrice = dynamicMap[dateKey]
      const effectivePrice = dynamicPrice !== undefined && dynamicPrice !== null
        ? dynamicPrice
        : defaultPrice
      return { dateKey, label, dayLabel: label, defaultPrice, dynamicPrice, effectivePrice }
    })
  }

  // ── Warehouse routing ──

  async function resolveWarehouseRoute(province: string): Promise<WarehouseRoute | null> {
    if (!province) return null
    try {
      const data = await fetchWarehouseRoute(province)
      return data.route || null
    } catch {
      return null
    }
  }

  // ── Price estimate from backend ──

  async function getPriceEstimate(params: {
    modelId?: string
    startDate: string
    endDate: string
    province?: string
  }) {
    try {
      const data = await fetchPriceEstimate(params)
      return data.estimate || null
    } catch {
      return null
    }
  }

  // ── Provinces ──

  async function loadProvinces() {
    if (provinces.value.length > 0) return provinces.value
    provincesLoading.value = true
    try {
      const data = await fetchProvinces()
      provinces.value = data.provinces || []
    } catch {
      provinces.value = []
    } finally {
      provincesLoading.value = false
    }
    return provinces.value
  }

  // ── Delivery lead days (from warehouse route) ──

  function getDeliveryLeadDays(route: WarehouseRoute | null): number {
    if (!route) return 1
    return route.shippingDays
  }


  return {
    DYNAMIC_ROLLING_DAYS,
    OCCUPANCY_COEFFICIENTS,
    provinces,
    provincesLoading,
    formatDate,
    toDateLocal,
    addDays,
    parseDateKey,
    getRollingDateKeys,
    getBaseDailyPrice,
    isHoliday,
    getHolidayPrice,
    getDailyPrice,
    getRollingDateEntries,
    resolveWarehouseRoute,
    getPriceEstimate,
    loadProvinces,
    getDeliveryLeadDays,
    generateQuoteText,
  }
}
