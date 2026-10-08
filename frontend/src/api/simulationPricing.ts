import { requestJson } from './client'

export interface SimulationHolidayRule {
  name: string
  startDate: string
  endDate: string
  price: number
  includePreviousDay?: boolean
}

export interface SimulationPricingConfig {
  baseWeekdayPrice: number
  baseWeekendPrice: number
  holidayRules: SimulationHolidayRule[]
  receiveShippingFees: Record<string, number>
  dynamicPriceMap: Record<string, number>
}

export interface SimulationPricingDailyEntry {
  date?: string
  dateKey?: string
  price: number
  source?: string
}

export interface SimulationPricingEstimate {
  breakdown: SimulationPricingDailyEntry[]
  totalPrice: number
}

const defaults: SimulationPricingConfig = {
  baseWeekdayPrice: 8.5,
  baseWeekendPrice: 14,
  holidayRules: [],
  receiveShippingFees: { area1: 7, area2: 7, area3: 7, area4: 18 },
  dynamicPriceMap: {},
}

function configFrom(value: any): SimulationPricingConfig {
  const source = value?.config ?? value?.value ?? value ?? {}
  return {
    baseWeekdayPrice: Number(source.baseWeekdayPrice ?? source.base_weekday_price ?? defaults.baseWeekdayPrice),
    baseWeekendPrice: Number(source.baseWeekendPrice ?? source.base_weekend_price ?? defaults.baseWeekendPrice),
    holidayRules: Array.isArray(source.holidayRules ?? source.holiday_rules) ? (source.holidayRules ?? source.holiday_rules) : [],
    receiveShippingFees: { ...defaults.receiveShippingFees, ...(source.receiveShippingFees ?? source.receive_shipping_fees ?? {}) },
    dynamicPriceMap: source.dynamicPriceMap ?? source.dynamic_price_map ?? {},
  }
}

function estimateFrom(value: any): SimulationPricingEstimate {
  const source = value?.estimate ?? value ?? {}
  const breakdown = Array.isArray(source.breakdown) ? source.breakdown : (Array.isArray(source.days) ? source.days : [])
  return {
    breakdown: breakdown.map((item: any) => ({
      date: item.date ?? item.dateKey ?? item.date_key,
      dateKey: item.dateKey ?? item.date_key ?? item.date,
      price: Number(item.price ?? item.dailyPrice ?? item.daily_price ?? 0),
      source: item.source ?? item.priceSource ?? item.price_source,
    })),
    totalPrice: Number(source.totalPrice ?? source.total_price ?? source.total ?? 0),
  }
}

function basePath(simulationId: string) {
  return `/api/tenant-simulations/${encodeURIComponent(simulationId)}`
}

export const simulationPricingApi = {
  async getConfig(simulationId: string): Promise<SimulationPricingConfig> {
    return configFrom(await requestJson(`${basePath(simulationId)}/pricing-config`, '读取隔离定价配置失败'))
  },
  async saveConfig(simulationId: string, config: SimulationPricingConfig, idempotencyKey: string): Promise<SimulationPricingConfig> {
    return configFrom(await requestJson(`${basePath(simulationId)}/pricing-config`, {
      method: 'PUT', headers: { 'Idempotency-Key': idempotencyKey }, body: JSON.stringify({ value: config }),
    }, '写入隔离定价配置失败'))
  },
  async estimate(simulationId: string, startDate: string, endDate: string): Promise<SimulationPricingEstimate> {
    const query = new URLSearchParams({ startDate, endDate })
    return estimateFrom(await requestJson(`${basePath(simulationId)}/pricing-estimate?${query.toString()}`, '计算隔离价格失败'))
  },
}
