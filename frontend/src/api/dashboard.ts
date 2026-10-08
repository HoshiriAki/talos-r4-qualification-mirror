import { requestJson } from './client'
import { shanghaiBusinessDate } from '@/utils/businessDate'

// ── Types ────────────────────────────────────────────────────────────────

export interface DashboardStats {
  version: number
  asOf: string
  activeOrders: number
  devicesOut: number
  availableDevices: number
  totalDevices: number
  returnsDueToday: number
  overdueReturns: number
  todayNewOrders: number
  recentOrders: RecentOrder[]
  overdueDetails: OverdueDetail[]
  orderBuckets: OrderBucket[]
  sceneNodes: HudSceneNode[]
}

export interface OrderBucket {
  status: string
  count: number
}

export interface HudSceneNode {
  id: string
  kind: 'device-cluster' | 'return-risk' | string
  label: string
  status: 'normal' | 'warning' | 'critical' | string
  count: number
  route: { name: DashboardRouteName; query?: Record<string, string> }
}

export type DashboardRouteName = 'customers' | 'devices' | 'dashboard'

export interface RecentOrder {
  id: string
  orderNo: string
  startDate: string
  endDate: string
  province: string
  totalPrice: number
  createdAt: string
}

export interface OverdueDetail {
  serialNo: string
  orderNos: string[]
  endDate: string
  province: string
  daysOverdue: number
}

export interface DailyCount {
  dateKey: string
  count: number
}

export interface DailyCountsResult {
  days: number
  counts: DailyCount[]
}

export interface TrendDataPoint {
  date: string
  count: number
}

export interface RevenueTrendPoint {
  date: string
  amount: number
  count: number
}

export interface DeviceStatusItem {
  status: string
  count: number
}

export interface ProvinceStatItem {
  province: string
  count: number
  amount?: number
  date?: string
}

export interface ModelRankingItem {
  modelName: string
  amount: number
}

export interface WarehouseStatItem {
  warehouseName: string
  outgoing: number
  stock: number
}

// ── API Functions ────────────────────────────────────────────────────────

export async function fetchDashboardStats(signal?: AbortSignal): Promise<DashboardStats> {
  const res = await requestJson('/api/dashboard/stats', { signal })
  if (typeof res.orders === 'number' && typeof res.devices === 'number') {
    return {
      version: 1,
      asOf: `${shanghaiBusinessDate()}T00:00:00+08:00`,
      activeOrders: res.activeOrders ?? 0,
      devicesOut: Math.max(0, res.devices - (res.availableDevices ?? 0)),
      availableDevices: res.availableDevices ?? 0,
      totalDevices: res.devices,
      returnsDueToday: 0,
      overdueReturns: 0,
      todayNewOrders: 0,
      recentOrders: [],
      overdueDetails: [],
      orderBuckets: [],
      sceneNodes: [],
    }
  }
  return res.data as DashboardStats
}

export async function fetchDailyOrderCounts(days = 15): Promise<DailyCountsResult> {
  const res = await requestJson(`/api/dashboard/daily-counts?days=${days}`)
  return res.data as DailyCountsResult
}

export async function fetchOrderTrend(granularity = 'day', days = 30): Promise<{ days: number; granularity: string; data: TrendDataPoint[] }> {
  const res = await requestJson(`/api/dashboard/order-trend?granularity=${granularity}&days=${days}`)
  return res.data
}

export async function fetchRevenueTrend(granularity = 'day', days = 30): Promise<{ days: number; granularity: string; data: RevenueTrendPoint[] }> {
  const res = await requestJson(`/api/dashboard/revenue-trend?granularity=${granularity}&days=${days}`)
  return res.data
}

export async function fetchCancelTrend(days = 30): Promise<{ days: number; data: TrendDataPoint[] }> {
  const res = await requestJson(`/api/dashboard/cancel-trend?days=${days}`)
  return res.data
}

export async function fetchDeviceStatusDistribution(): Promise<{ data: DeviceStatusItem[] }> {
  const res = await requestJson('/api/dashboard/device-status-distribution')
  return res.data
}

export async function fetchProvinceStats(type: 'pie' | 'trend' = 'pie', days = 30): Promise<{ type: string; days: number; data: ProvinceStatItem[] }> {
  const res = await requestJson(`/api/dashboard/province-stats?type=${type}&days=${days}`)
  // Backend returns { ok: true, pie: [...], trend: [...], days } — pick the right key
  return { type, days: res.days, data: res[type] || [] }
}

export async function fetchModelRanking(days = 30): Promise<{ days: number; data: ModelRankingItem[] }> {
  const res = await requestJson(`/api/dashboard/model-ranking?days=${days}`)
  return res.data
}

export async function fetchWarehouseStats(): Promise<{ data: WarehouseStatItem[] }> {
  const res = await requestJson('/api/dashboard/warehouse-stats')
  return res.data
}
