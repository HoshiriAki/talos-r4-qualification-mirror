import { requestJson, requestBlob } from './client'

export interface OrderFilters {
  keyword?: string
  orderNo?: string
  address?: string
  startDateFrom?: string
  startDateTo?: string
  startDate?: string
  endDateFrom?: string
  endDateTo?: string
  endDate?: string
  includedDate?: string
  deliveryDateFrom?: string
  deliveryDateTo?: string
  deliveryDate?: string
  pickupMethods?: string[]
  status?: string
}

export interface OrderLifecycleSummary {
  commercialStatus: string
  contractStatus: string
  financialStatus: string
  fulfilmentStatus: string
  riskStatus: string
  version: number
  updatedAt: string
  blockers: string[]
}

export interface OrderAllowedAction {
  action: string
  dimension: string
  targetStatus: string
  label: string
  expectedVersion: number
}

export interface Order {
  id: string
  orderNo: string
  startDate: string
  endDate: string
  deliveryDate: string
  pickupMethods: string[]
  address: string
  notes: string
  /** Junction table serial numbers (current many-to-many). */
  devices: string[]
  /** Legacy single-device-per-order field — keep for backward compat. */
  deviceSerialNo?: string | null
  deviceModels?: Record<string, number>
  accessories?: string[]
  province?: string
  receiveProvince?: string
  returnProvince?: string
  receiveArea?: string
  returnArea?: string
  modelId?: string
  totalPrice?: number
  sendWarehouseId?: string
  returnWarehouseId?: string
  status?: string
  trackingNo?: string
  createdAt: string
  lifecycle?: OrderLifecycleSummary
  allowedActions?: OrderAllowedAction[]
}

export type NamedOrderUpdate = Pick<Order, 'startDate' | 'endDate' | 'deliveryDate' | 'address' | 'province' | 'pickupMethods' | 'notes'> & {
  status?: 'submitted' | 'cancelled'
  reason?: string
  expectedVersion: number
}

export interface PaginatedResult {
  orders: Order[]
  pagination: {
    page: number
    pageSize: number
    total: number
    totalPages: number
  }
}

export async function fetchPage(filters: OrderFilters = {}, page = 1, pageSize = 30, sortBy?: string | null, sortOrder?: 'asc' | 'desc' | number | null, signal?: AbortSignal): Promise<PaginatedResult> {
  const params = new URLSearchParams()
  params.set('page', String(page))
  params.set('pageSize', String(pageSize))
  if (sortBy) params.set('sortBy', sortBy)
  if (sortOrder != null) {
    const normalizedSortOrder = typeof sortOrder === 'number'
      ? (sortOrder === 1 ? 'asc' : 'desc')
      : sortOrder
    params.set('sortOrder', normalizedSortOrder)
  }

  Object.entries(filters).forEach(([key, value]) => {
    if (value && Array.isArray(value)) {
      value.forEach(v => params.append(key, v))
    } else if (value) {
      params.set(key, String(value))
    }
  })

  const result = await requestJson(`/api/v2/orders?${params.toString()}`, signal ? { signal } : undefined, '获取订单列表失败')
  return { orders: result.items ?? [], pagination: result.pagination }
}

export async function updateOrder(id: string, data: Partial<NamedOrderUpdate>): Promise<Order> {
  return requestJson(`/users/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(data),
  }, '更新订单失败')
}

export async function deleteOrder(id: string): Promise<{ ok: boolean }> {
  return requestJson(`/users/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  }, '删除订单失败')
}

export async function bulkDeleteOrders(ids: string[]): Promise<{
  ok: boolean
  successCount: number
  failCount: number
  deletedItems: Array<{ id: string; orderNo: string }>
  failedItems: Array<{ id: string; reason: string }>
}> {
  return requestJson('/users/bulk-delete', {
    method: 'POST',
    body: JSON.stringify({ ids }),
  }, '批量删除失败')
}

export async function batchUpdateNotes(ids: string[], notes: string): Promise<{ updated: number }> {
  return requestJson('/users/batch-notes', {
    method: 'PUT',
    body: JSON.stringify({ ids, notes }),
  }, '批量更新备注失败')
}

export async function bulkUpdateOrders(ids: string[], fields: { status?: string; pickupMethods?: string[]; notes?: string }): Promise<{ ok: boolean; updates: Record<string, number> }> {
  return requestJson('/users/bulk-update', {
    method: 'PUT',
    body: JSON.stringify({ ids, ...fields }),
  }, '批量更新订单失败')
}

export async function exportOrders(filters: OrderFilters = {}, ids?: string[]): Promise<Blob> {
  const body: any = ids && ids.length > 0 ? { ids } : filters
  return requestBlob('/users/export', {
    method: 'POST',
    body: JSON.stringify(body),
  }, '导出订单失败')
}

export async function importOrders(file: File, approved = true): Promise<{ ok: boolean; successCount: number; failCount: number; failedRows: any[] }> {
  const form = new FormData()
  form.append('file', file)
  form.append('approved', String(approved))
  return requestJson('/users/import-orders', {
    method: 'POST',
    body: form,
  }, '导入订单失败')
}

export async function importDevices(file: File, approved = true): Promise<{ ok: boolean; successCount: number; failCount: number; failedRows: any[] }> {
  const form = new FormData()
  form.append('file', file)
  form.append('approved', String(approved))
  return requestJson('/users/import-devices-by-orderno', {
    method: 'POST',
    body: form,
  }, '导入设备关联失败')
}

export async function importNotes(file: File, approved = true): Promise<{ ok: boolean; successCount: number; failCount: number; failedRows: any[] }> {
  const form = new FormData()
  form.append('file', file)
  form.append('approved', String(approved))
  return requestJson('/users/import-notes-by-orderno', {
    method: 'POST',
    body: form,
  }, '导入备注失败')
}

export interface BatchShipOrder {
  id: string
  trackingNo?: string
  deviceSerialNos: string[]
}

export interface BatchShipResult {
  ok: boolean
  successCount: number
  failCount: number
  results: Array<{ orderId: string; ok: boolean; error?: string; linkedDevices?: string[] }>
}

export async function submitBatchShip(orders: BatchShipOrder[]): Promise<BatchShipResult> {
  return requestJson('/users/submit-batch-ship', {
    method: 'POST',
    body: JSON.stringify({ orders }),
  }, '批量提交发货失败')
}

export async function batchOrderNos(ids: string[]): Promise<Record<string, string>> {
  if (ids.length === 0) return {}
  const data = await requestJson('/users/batch-order-nos', {
    method: 'POST',
    body: JSON.stringify({ ids }),
  }, '批量查询订单号失败')
  const map: Record<string, string> = {}
  for (const o of (data.orders ?? [])) {
    if (o.id && o.orderNo) map[o.id] = o.orderNo
  }
  return map
}

export async function linkDevice(orderId: string, serialNo: string): Promise<{ ok: boolean; relation: any; device: any }> {
  return requestJson(`/users/${encodeURIComponent(orderId)}/devices`, {
    method: 'POST',
    body: JSON.stringify({ serialNo }),
  }, '关联设备失败')
}

export async function unlinkDevice(orderId: string, serialNo: string): Promise<{ ok: boolean }> {
  return requestJson(`/users/${encodeURIComponent(orderId)}/devices/${encodeURIComponent(serialNo)}`, {
    method: 'DELETE',
  }, '移除设备关联失败')
}

export interface LifecycleActionResult {
  lifecycle: OrderLifecycleSummary
  allowedActions: OrderAllowedAction[]
  blockers: string[]
}

export async function applyLifecycleAction(
  orderId: string,
  action: string,
  expectedVersion: number,
  reason = '',
): Promise<LifecycleActionResult> {
  return requestJson(`/api/v2/orders/${encodeURIComponent(orderId)}/lifecycle/actions/${encodeURIComponent(action)}`, {
    method: 'POST',
    body: JSON.stringify({ expectedVersion, reason }),
  }, '订单生命周期操作失败')
}

// ── Legacy 021 compatibility adapter ─────────────────────────────────

export interface TransitionResult {
  ok: boolean
  from?: string
  to: string
  lifecycle?: LifecycleActionResult
}

export async function transitionOrder(orderId: string, toStatus: string, reason?: string): Promise<TransitionResult> {
  return requestJson('/users/transition', {
    method: 'POST',
    body: JSON.stringify({ orderId, toStatus, reason: reason || '' }),
  }, '状态转移失败')
}

/** Compatibility projection values stored in orders.status. Canonical policy is Lifecycle V2. */
export const ORDER_STATUSES = [
  'draft', 'confirmed', 'paid', 'shipped',
  'in_use', 'returned', 'inspected', 'completed',
  'closed', 'cancelled',
] as const

export type OrderStatus = typeof ORDER_STATUSES[number]

export const STATUS_LABELS: Record<string, string> = {
  draft: '草稿',
  confirmed: '已确认',
  paid: '已付款',
  shipped: '已发货',
  in_use: '使用中',
  returned: '已归还',
  inspected: '检查中',
  completed: '已完成',
  closed: '已关闭',
  cancelled: '已取消',
  reserved: '已预留',
  active: '进行中',
  repairing: '维修中',
} as const
