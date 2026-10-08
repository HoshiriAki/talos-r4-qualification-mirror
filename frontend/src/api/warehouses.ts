import { requestJson } from './client'

export interface Warehouse {
  id: string
  name: string
  type: 'owned' | 'partner'
  enabled: boolean
  address: string
  contactName: string
  contactPhone: string
  notes: string
  capacity: number
  createdAt: string
  updatedAt: string
}

export interface WarehouseStats extends Warehouse {
  totalDevices: number
  availableDevices: number
  rentedDevices: number
  repairingDevices: number
  utilizationPercent: number
}

export interface RegionRule {
  id: string
  warehouseId: string
  province: string
  shippingDays: number
  returnDays: number
  isPrimary: boolean
  createdAt: string
  updatedAt: string
}

export interface WarehouseDevice {
  id: string
  serialNo: string
  rentalStatus: string
  modelId: string
  notes: string
  currentWarehouseId: string
  expectedWarehouseId: string
  expectedAvailableDate: string
  createdAt: string
}

export async function fetchWarehouses(): Promise<{ ok: boolean; warehouses: Warehouse[] }> {
  return requestJson('/api/warehouses', '获取仓库列表失败')
}

export async function fetchWarehouseStats(): Promise<{ ok: boolean; warehouses: WarehouseStats[] }> {
  return requestJson('/api/warehouses/stats', '获取仓库统计失败')
}

export async function fetchWarehouse(id: string): Promise<{ ok: boolean; warehouse: Warehouse; regions: RegionRule[] }> {
  return requestJson(`/api/warehouses/${id}`, '获取仓库区域规则失败')
}

export async function fetchWarehouseDevices(id: string, query?: Record<string, string>): Promise<{ ok: boolean; data: WarehouseDevice[]; total: number; page: number; pageSize: number }> {
  const qs = query ? '?' + new URLSearchParams(query).toString() : ''
  return requestJson(`/api/warehouses/${id}/devices${qs}`, '获取仓库设备失败')
}

export async function createWarehouse(payload: Partial<Warehouse>): Promise<{ ok: boolean; warehouse: Warehouse }> {
  return requestJson('/api/warehouses', {
    method: 'POST',
    body: JSON.stringify(payload),
  }, '创建仓库失败')
}

export async function updateWarehouse(id: string, payload: Partial<Warehouse>): Promise<{ ok: boolean; warehouse: Warehouse }> {
  return requestJson(`/api/warehouses/${id}`, {
    method: 'PATCH',
    body: JSON.stringify(payload),
  }, '更新仓库失败')
}

export async function deleteWarehouse(id: string): Promise<{ ok: boolean }> {
  return requestJson(`/api/warehouses/${id}`, {
    method: 'DELETE',
  }, '删除仓库失败')
}

export async function upsertRegionRule(warehouseId: string, payload: Partial<RegionRule>): Promise<{ ok: boolean; regions: RegionRule[] }> {
  return requestJson(`/api/warehouses/${warehouseId}/regions`, {
    method: 'PUT',
    body: JSON.stringify(payload),
  }, '更新区域规则失败')
}

export async function deleteRegionRule(warehouseId: string, province: string): Promise<{ ok: boolean }> {
  return requestJson(`/api/warehouses/${warehouseId}/regions/${encodeURIComponent(province)}`, {
    method: 'DELETE',
  }, '删除区域规则失败')
}
