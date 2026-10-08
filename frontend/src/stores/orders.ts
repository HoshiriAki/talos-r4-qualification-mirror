import { shanghaiBusinessDate } from '@/utils/businessDate'
import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { createBaseTableStore } from './baseTable'
import * as ordersApi from '@/api/orders'
import type { Order, OrderFilters } from '@/api/orders'
import type { DataQuery, DataPage } from '@/types/data-table'

async function fetchAdapter(query: DataQuery<OrderFilters>, signal?: AbortSignal): Promise<DataPage<Order>> {
  if (query.mode === 'page') {
    const result: any = await ordersApi.fetchPage(
      query.filter || {},
      query.page || 1,
      query.pageSize || 30,
      undefined, undefined,
      signal,
    )
    return {
      rows: result.orders || result.users || [],
      total: result.pagination?.total || 0,
      totalPages: result.pagination?.totalPages || 0,
      hasMore: false,
    }
  }
  // Generic table offset requests are projected onto the canonical page contract.
  const pageSize = query.limit || 50
  const page = Math.floor((query.offset || 0) / pageSize) + 1
  const result: any = await ordersApi.fetchPage(
    query.filter || {},
    page,
    pageSize,
    query.sortBy,
    query.sortOrder,
    signal,
  )
  return {
    rows: result.orders || [],
    total: result.pagination?.total || 0,
    totalPages: result.pagination?.totalPages || 0,
    hasMore: page < (result.pagination?.totalPages || 0),
  }
}

const base = createBaseTableStore<Order, OrderFilters>(
  'orders-base',
  fetchAdapter,
  {} as OrderFilters,
  50,
)

export const useOrdersStore = defineStore('orders', () => {
  const table = base()
  const initialLoading = ref(true)

  const orders = computed(() => table.rows)

  async function fetchOrders(reset = true, signal?: AbortSignal) {
    if (reset) {
      table.rows = []
      table.hasMore = true
      table.total = 0
    }
    if (!table.hasMore) return

    initialLoading.value = reset
    try {
      const pageSize = table.pagination.pageSize
      const page = Math.floor(table.rows.length / pageSize) + 1
      const q: DataQuery<OrderFilters> = {
        mode: 'page',
        page,
        pageSize,
        filter: table.filter,
        sortBy: undefined,
        sortOrder: undefined,
      }
      const result = await fetchAdapter(q, signal)
      table.rows = reset ? result.rows : [...table.rows, ...result.rows]
      table.total = result.total ?? 0
      table.hasMore = result.hasMore ?? false
    } catch (e: any) {
      if (e?.name === 'AbortError') return
      table.error = e.message || 'Failed to fetch orders'
      throw e
    } finally {
      initialLoading.value = false
    }
  }

  async function refreshAfterMutation() {
    await fetchOrders(true)
  }

  async function update(id: string, data: Partial<Order>) {
    table.error = null
    try {
      const idx = table.rows.findIndex((o: Order) => o.id === id)
      const current = idx === -1 ? undefined : table.rows[idx]
      const expectedVersion = data.lifecycle?.version ?? current?.lifecycle?.version
      if (!expectedVersion) throw new Error('订单生命周期版本不可用，请刷新后重试')
      const update: Partial<ordersApi.NamedOrderUpdate> = {
        expectedVersion,
        ...(data.startDate !== undefined && { startDate: data.startDate }),
        ...(data.endDate !== undefined && { endDate: data.endDate }),
        ...(data.deliveryDate !== undefined && { deliveryDate: data.deliveryDate }),
        ...(data.address !== undefined && { address: data.address }),
        ...(data.province !== undefined && { province: data.province }),
        ...(data.pickupMethods !== undefined && { pickupMethods: data.pickupMethods }),
        ...(data.notes !== undefined && { notes: data.notes }),
      }
      const order = await ordersApi.updateOrder(id, update)
      if (idx !== -1) table.rows[idx] = order
      return order
    } catch (e: any) {
      table.error = e.message || 'Failed to update order'
      throw e
    }
  }

  async function remove(id: string) {
    table.error = null
    try {
      await ordersApi.deleteOrder(id)
      table.rows = table.rows.filter((o: Order) => o.id !== id)
    } catch (e: any) {
      table.error = e.message || 'Failed to delete order'
      throw e
    }
  }

  async function bulkDelete(ids: string[]) {
    table.error = null
    try {
      const result = await ordersApi.bulkDeleteOrders(ids)
      await refreshAfterMutation()
      return result
    } catch (e: any) {
      table.error = e.message || 'Failed to bulk delete orders'
      throw e
    }
  }

  async function batchNotes(ids: string[], notes: string) {
    table.error = null
    try {
      await ordersApi.batchUpdateNotes(ids, notes)
      await refreshAfterMutation()
    } catch (e: any) {
      table.error = e.message || 'Failed to batch update notes'
      throw e
    }
  }

  async function exportOrders(exportFilters?: OrderFilters, ids?: string[]) {
    table.error = null
    try {
      const blob = await ordersApi.exportOrders(exportFilters || table.filter, ids)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `orders-${shanghaiBusinessDate()}.xlsx`
      a.click()
      URL.revokeObjectURL(url)
    } catch (e: any) {
      table.error = e.message || 'Failed to export orders'
      throw e
    }
  }

  async function importOrders(file: File) {
    table.error = null
    try {
      return await ordersApi.importOrders(file)
    } catch (e: any) {
      table.error = e.message || 'Failed to import orders'
      throw e
    }
  }

  async function importDevices(file: File) {
    table.error = null
    try {
      return await ordersApi.importDevices(file)
    } catch (e: any) {
      table.error = e.message || 'Failed to import devices'
      throw e
    }
  }

  async function importNotes(file: File) {
    table.error = null
    try {
      return await ordersApi.importNotes(file)
    } catch (e: any) {
      table.error = e.message || 'Failed to import notes'
      throw e
    }
  }

  async function linkDevice(orderId: string, serialNo: string) {
    table.error = null
    try {
      return await ordersApi.linkDevice(orderId, serialNo)
    } catch (e: any) {
      table.error = e.message || 'Failed to link device'
      throw e
    }
  }

  async function unlinkDevice(orderId: string, serialNo: string) {
    table.error = null
    try {
      return await ordersApi.unlinkDevice(orderId, serialNo)
    } catch (e: any) {
      table.error = e.message || 'Failed to unlink device'
      throw e
    }
  }

  function resetFilters() {
    const keys = Object.keys(table.filter)
    keys.forEach(k => delete (table.filter as any)[k])
  }

  return {
    // Data
    orders,
    filters: table.filter,
    loading: table.loading,
    hasMore: table.hasMore,
    totalCount: table.total,
    initialLoading,
    pagination: table.pagination,
    error: table.error,

    // Base table methods
    fetchPage: table.fetchPage,
    fetchMore: table.fetchMore,

    // Custom fetch
    fetchOrders,
    refreshAfterMutation,

    // Business methods
    update,
    remove,
    bulkDelete,
    batchNotes,
    exportOrders,
    importOrders,
    importDevices,
    importNotes,
    linkDevice,
    unlinkDevice,

    // Filter
    resetFilters,
  }
})
