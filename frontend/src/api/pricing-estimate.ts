import { requestJson } from './client'

export interface PriceDetail {
  dateKey: string
  finalDailyPrice: number
  priceSource: string
  occupyType: 'shipping' | 'normal' | 'return'
  occupyFactor: number
  amount: number
}

export interface WarehouseRoute {
  sendWarehouseId: string
  sendWarehouseName: string
  returnWarehouseId: string
  returnWarehouseName: string
  shippingDays: number
  returnDays: number
}

export interface PriceEstimate {
  breakdown: PriceDetail[]
  totalPrice: number
  route: WarehouseRoute | null
}

export interface OccupancyCoefficients {
  shipping: number
  normal: number
  return: number
}

export async function fetchPriceEstimate(params: {
  modelId?: string
  startDate: string
  endDate: string
  province?: string
}): Promise<{ ok: boolean; estimate: PriceEstimate }> {
  const qs = new URLSearchParams()
  if (params.modelId) qs.set('modelId', params.modelId)
  qs.set('startDate', params.startDate)
  qs.set('endDate', params.endDate)
  if (params.province) qs.set('province', params.province)
  return requestJson(`/api/pricing/estimate?${qs.toString()}`, '获取价格估算失败')
}

export async function fetchWarehouseRoute(province: string): Promise<{ ok: boolean; route: WarehouseRoute }> {
  return requestJson(`/api/pricing/warehouse-route?province=${encodeURIComponent(province)}`, '获取仓库路由失败')
}

export async function fetchProvinces(): Promise<{ ok: boolean; provinces: string[] }> {
  return requestJson('/api/pricing/provinces', '获取省份列表失败')
}
