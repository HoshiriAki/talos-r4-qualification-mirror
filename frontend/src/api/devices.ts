import { requestJson, requestBlob } from './client'

export interface DeviceFilters {
  keyword?: string
  rentalStatus?: string
  notes?: string
  warningStatus?: string
}

export interface Device {
  id?: string
  serialNo: string
  rentalStatus: string
  notes: string
  fallbackReturnNode?: string
  createdAt?: string
  returnNode?: string
  returnNodeSource?: string
  warningStatus?: string
  warningReason?: string
}

export interface PaginatedDevicesResult {
  devices: Device[]
  pagination: {
    page: number
    pageSize: number
    total: number
    totalPages: number
  }
}

export async function fetchPage(filters: DeviceFilters = {}, page?: number, pageSize?: number, sortBy?: string | null, sortOrder?: number | null, signal?: AbortSignal): Promise<PaginatedDevicesResult> {
  const params = new URLSearchParams()
  if (page !== undefined) params.set('page', String(page))
  if (pageSize !== undefined) params.set('pageSize', String(pageSize))
  if (sortBy) params.set('sortBy', sortBy)
  if (sortOrder != null) params.set('sortOrder', sortOrder === 1 ? 'asc' : 'desc')

  Object.entries(filters).forEach(([key, value]) => {
    if (value) params.set(key, String(value))
  })

  const result = await requestJson(`/devices?${params.toString()}`, signal ? { signal } : undefined, '获取设备列表失败')
  if (Array.isArray(result)) {
    return {
      devices: result,
      pagination: { page: 1, pageSize: result.length, total: result.length, totalPages: 1 },
    }
  }
  return result
}

export async function createDevice(data: { serialNo: string; rentalStatus: string; notes?: string }): Promise<Device> {
  return requestJson('/devices', {
    method: 'POST',
    body: JSON.stringify(data),
  }, '创建设备失败')
}

export async function updateDevice(serialNo: string, data: Partial<Device>): Promise<Device> {
  return requestJson(`/devices/${encodeURIComponent(serialNo)}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  }, '更新设备失败')
}

export async function deleteDevice(serialNo: string): Promise<{ ok: boolean }> {
  return requestJson(`/devices/${encodeURIComponent(serialNo)}`, {
    method: 'DELETE',
  }, '删除设备失败')
}

export async function bulkDeleteDevices(serialNos: string[]): Promise<{
  ok: boolean
  successCount: number
  failCount: number
  deletedItems: Array<{ serialNo: string }>
  failedItems: Array<{ serialNo: string; reason: string }>
}> {
  return requestJson('/devices/bulk-delete', {
    method: 'POST',
    body: JSON.stringify({ serialNos }),
  }, '批量删除失败')
}

export interface AutoCompletedOrder {
  orderId: string
  reason: string
  triggeredBySerialNo: string
}

export interface CheckinResult {
  ok: boolean
  action?: string
  message?: string
  device?: Device
  beforeStatus?: string
  afterStatus?: string
  autoCompletedOrders?: AutoCompletedOrder[]
}

export async function checkinScan(serialNo: string): Promise<CheckinResult> {
  return requestJson('/devices/checkin-scan', {
    method: 'POST',
    body: JSON.stringify({ serialNo }),
  }, '扫码入库失败')
}

export async function undoCheckin(serialNo: string, previousStatus: string): Promise<{ ok: boolean }> {
  return requestJson('/devices/checkin-undo', {
    method: 'POST',
    body: JSON.stringify({ serialNo, previousStatus }),
  }, '撤销入库失败')
}

export async function exportDevices(filters?: DeviceFilters, serialNos?: string[]): Promise<Blob> {
  const body: any = serialNos && serialNos.length > 0 ? { serialNos } : (filters || {})
  return requestBlob('/devices/export', {
    method: 'POST',
    body: JSON.stringify(body),
  }, '导出设备失败')
}

export async function bulkUpdateDevices(serialNos: string[], updates: Partial<Device>): Promise<{
  ok: boolean
  updatedCount: number
  failCount: number
  updatedItems: Array<{ serialNo: string }>
  failedItems: Array<{ serialNo: string; reason: string }>
}> {
  return requestJson('/devices/bulk-update', {
    method: 'POST',
    body: JSON.stringify({ serialNos, updates }),
  }, '批量更新失败')
}

export async function importExcel(file: File): Promise<{
  ok: boolean
  successCount: number
  failCount: number
  failedRows: any[]
}> {
  const form = new FormData()
  form.append('file', file)
  return requestJson('/devices/import-excel', {
    method: 'POST',
    body: form,
  }, '导入设备失败')
}
