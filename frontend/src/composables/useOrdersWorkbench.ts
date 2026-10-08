import { ref, readonly, onUnmounted } from 'vue'
import { useOrdersStore } from '@/stores/orders'
import * as ordersApi from '@/api/orders'
import type { Order, OrderFilters } from '@/api/orders'

/** Merge legacy deviceSerialNo and junction devices[] — deduplicated. */
export function getOrderSerialNos(order: Order): string[] {
  return [
    ...new Set(
      [
        ...(Array.isArray(order.devices) ? order.devices : []),
        ...((order as any).deviceSerialNo ? [(order as any).deviceSerialNo as string] : []),
      ].filter(Boolean),
    ),
  ]
}

export function useOrdersWorkbench(getFilters?: () => OrderFilters) {
  const store = useOrdersStore()

  let activeController: AbortController | undefined
  const rows = ref<Order[]>([])
  const total = ref(0)
  const page = ref(1)
  const pageSize = ref(10)
  const sortField = ref<string | null>(null)
  const sortOrder = ref<1 | -1 | null>(null)
  const loading = ref(false)
  const error = ref('')

  // ── Load with abort-controller ─────────────────────────────────────
  async function loadPage(nextPage = page.value, nextPageSize = pageSize.value) {
    activeController?.abort()
    const controller = new AbortController()
    activeController = controller

    loading.value = true
    error.value = ''

    try {
      const filters = getFilters?.() ?? (store.filters as OrderFilters)
      const result = await ordersApi.fetchPage(
        filters,
        nextPage,
        nextPageSize,
        sortField.value,
        sortOrder.value,
        controller.signal,
      )
      if (activeController !== controller) return
      rows.value = result.orders
      total.value = result.pagination?.total ?? 0
      page.value = nextPage
      pageSize.value = nextPageSize
      loading.value = false
    } catch (e: any) {
      if (activeController !== controller) return
      if (e?.name === 'AbortError') return
      error.value = e.message || 'Failed to load orders'
      loading.value = false
      throw e
    }
  }

  async function load() { await loadPage(1, pageSize.value) }
  async function refresh() { await loadPage(page.value, pageSize.value) }

  async function deleteOrder(id: string) {
    await store.remove(id)
    await refresh()
  }

  async function bulkDelete(ids: string[]) {
    await store.bulkDelete(ids)
    await refresh()
  }

  async function exportCurrent(ids?: string[]) {
    await store.exportOrders(getFilters?.() ?? (store.filters as OrderFilters), ids)
  }

  onUnmounted(() => { activeController?.abort() })

  return {
    rows,
    loading: readonly(loading),
    total,
    page,
    pageSize,
    sortField,
    sortOrder,
    error: readonly(error),
    filters: store.filters,
    load, loadPage, refresh, deleteOrder, bulkDelete, exportCurrent,
    // Passthrough — create is intentionally retired by R1-P2; Quote is the only create path.
    update: store.update, remove: store.remove,
    batchNotes: store.batchNotes, exportOrders: store.exportOrders,
    importOrders: store.importOrders, importDevices: store.importDevices,
    importNotes: store.importNotes, linkDevice: store.linkDevice,
    unlinkDevice: store.unlinkDevice, resetFilters: store.resetFilters,
  }
}