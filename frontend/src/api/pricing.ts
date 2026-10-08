import { requestJson } from './client'

export interface HolidayRule {
  name: string
  startDate: string
  endDate: string
  price: number
  includePreviousDay?: boolean
}

export interface PricingConfig {
  baseWeekdayPrice: number
  baseWeekendPrice: number
  holidayRules: HolidayRule[]
  receiveShippingFees?: Record<string, number>
  dynamicPriceMap: Record<string, number>
  updatedBy?: string
  createdAt?: string
  updatedAt?: string
}

export async function fetchConfig(): Promise<{ ok: boolean; config: PricingConfig }> {
  return requestJson('/api/pricing/config', '获取价格配置失败')
}

export async function saveConfig(payload: Partial<PricingConfig>): Promise<{ ok: boolean; config: PricingConfig }> {
  return requestJson('/api/pricing/config', {
    method: 'PUT',
    body: JSON.stringify(payload),
  }, '保存价格配置失败')
}
