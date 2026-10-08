<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useOrdersStore } from '@/stores/orders'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import * as ordersApi from '@/api/orders'
import { useConfirm } from 'primevue/useconfirm'
import { useToast } from 'primevue/usetoast'
import { useInlineEdit } from '@/components/data-table'
import { useDateFilter } from '@/composables/useDateFilter'
import { useRouteQuery } from '@/composables/useRouteQuery'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Select from 'primevue/select'
import type { Order } from '@/api/orders'
import { useOrdersWorkbench } from '@/composables/useOrdersWorkbench'

const orders = useOrdersStore()
const tenantPreview = useTenantPreviewStore()
const confirm = useConfirm()
const toast = useToast()
const { dateBridge } = useDateFilter()

// URL-synced filter state
const filters = useRouteQuery({
  keyword: '',
  status: '',
  startDate: '',
  endDate: '',
  deliveryDate: '',
})

// Date bridges for filter date strings (URL-synced)
const filterStartDate = dateBridge({ get: () => filters.startDate, set: (v) => { filters.startDate = v } })
const filterEndDate = dateBridge({ get: () => filters.endDate, set: (v) => { filters.endDate = v } })
const filterDeliveryDate = dateBridge({ get: () => filters.deliveryDate, set: (v) => { filters.deliveryDate = v } })
// includedDate stays store-bound (not a primary filter, not synced to URL)
const filterIncludedDate = dateBridge({ get: () => orders.filters.includedDate, set: (v) => { orders.filters.includedDate = v } })

// Build API-level filter object from URL-synced filters + store-bound filters
function buildApiFilters(): any {
  const apiFilters: any = {}
  // URL-synced filters
  if (filters.keyword) apiFilters.keyword = filters.keyword
  if (filters.status) apiFilters.status = filters.status
  if (filters.startDate) apiFilters.startDate = filters.startDate
  if (filters.endDate) apiFilters.endDate = filters.endDate
  if (filters.deliveryDate) apiFilters.deliveryDate = filters.deliveryDate
  // Store-bound filters (not synced to URL)
  if (orders.filters.includedDate) apiFilters.includedDate = orders.filters.includedDate
  if (orders.filters.endDateFrom) apiFilters.endDateFrom = orders.filters.endDateFrom
  if (orders.filters.endDateTo) apiFilters.endDateTo = orders.filters.endDateTo
  if (orders.filters.pickupMethods?.length) apiFilters.pickupMethods = orders.filters.pickupMethods
  if (orders.filters.orderNo) apiFilters.orderNo = orders.filters.orderNo
  return apiFilters
}

// The workbench composable is the sole owner of list, pagination, sorting and
// request cancellation state. The Pinia store remains responsible for shared
// mutations and filter persistence.
const workbench = useOrdersWorkbench(buildApiFilters)
const { rows, total, page, pageSize, loading, sortField, sortOrder } = workbench

async function loadPage(p?: number, ps?: number) {
  const pg = p ?? page.value
  const pz = ps ?? pageSize.value
  await workbench.loadPage(pg, pz)
  // Merge selection only after the latest, non-aborted request wins.
  if (selectAll.value) {
    const existingIds = new Set(selectedOrders.value.map((o: Order) => o.id))
    const toAdd = rows.value.filter(row => !existingIds.has(row.id) && !uncheckedIds.value.has(row.id))
    selectedOrders.value = [...selectedOrders.value, ...toAdd]
  }
}

// ── Filter panel visibility ──
const showFilters = ref(false)

// ── Batch search ──
const batchSearchDialog = ref(false)
const batchOrderInput = ref('')

// ── Selection for bulk ops (PrimeVue DataTable selection) ──
const selectedOrders = ref<Order[]>([])
const selectAll = ref(false)
const uncheckedIds = ref<Set<string>>(new Set())
const selectedIds = computed(() => selectedOrders.value.map(o => o.id))
const selectedCount = computed(() => selectAll.value ? total.value - uncheckedIds.value.size : selectedIds.value.length)
const selectAllLoading = ref(false)
const selectAllAbort = ref<AbortController | null>(null)

watch(
  () => buildApiFilters(),
  () => {
    selectedOrders.value = []
    selectAll.value = false
    uncheckedIds.value = new Set()
  },
  { deep: true },
)

// ── Edit modal ──
const editVisible = ref(false)
const editOrder = ref<Order | null>(null)

// Date bridges for edit modal
const editStartDate = dateBridge({
  get: () => editOrder.value?.startDate || undefined,
  set: (v) => { if (editOrder.value) editOrder.value.startDate = v },
})
const editEndDate = dateBridge({
  get: () => editOrder.value?.endDate || undefined,
  set: (v) => { if (editOrder.value) editOrder.value.endDate = v },
})
const editDeliveryDate = dateBridge({
  get: () => editOrder.value?.deliveryDate || undefined,
  set: (v) => { if (editOrder.value) editOrder.value.deliveryDate = v },
})

// ── Link device modal ──
const linkVisible = ref(false)
const linkOrderId = ref('')
const linkSerialNo = ref('')

// ── Inline editing (composable) ──
const {
  editingCell,
  startEdit: startInlineEdit,
  saveEdit: saveInlineEdit,
  cancelEdit: cancelInlineEdit,
  isEditing: isCellEditing,
} = useInlineEdit({
  onSave: async (id, field, value) => {
    await orders.update(id, { [field]: value })
    await loadPage()
    toast.add({ severity: 'success', summary: '已更新', life: 2000 })
  },
})

// ── File input refs ──
const importOrdersRef = ref<HTMLInputElement | null>(null)
const importDevicesRef = ref<HTMLInputElement | null>(null)
const importNotesRef = ref<HTMLInputElement | null>(null)

// ── Pickup method filter options ──
const pickupFilterOptions = ['顺丰标快', '自取/跑腿', '半日达']

// ── Initial load ──
onMounted(async () => {
  try {
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 })
  }
})

// ── Pagination ──
function onPage(event: { page: number; rows: number }) {
  loadPage(event.page + 1, event.rows).catch(e =>
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 })
  )
}

// ── Sort ──
function onSort(event: { sortField?: string | ((item: unknown) => string); sortOrder?: number | null }) {
  sortField.value = typeof event.sortField === 'string' ? event.sortField : null
  sortOrder.value = event.sortOrder === 1 || event.sortOrder === -1 ? event.sortOrder : null
  loadPage(1).catch(e =>
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 })
  )
}

// ── Select All ──
function onSelectAllChange(event: { checked: boolean }) {
  selectAll.value = event.checked
  if (event.checked) {
    uncheckedIds.value = new Set()
    selectedOrders.value = [...rows.value]
  } else {
    selectedOrders.value = []
    uncheckedIds.value = new Set()
  }
}

function onRowSelect(event: { data: Order }) {
  if (selectAll.value && event.data?.id) {
    uncheckedIds.value.delete(event.data.id)
  }
}

function onRowUnselect(event: { data: Order }) {
  if (selectAll.value && event.data?.id) {
    uncheckedIds.value.add(event.data.id)
  }
}

const MAX_SELECT_ALL = 5000
const SELECT_ALL_PAGE_SIZE = 200 // query-builder caps pageSize at 200, must paginate

async function getAllMatchingOrderIds(): Promise<string[]> {
  const apiFilters = buildApiFilters()
  selectAllLoading.value = true
  const ctrl = new AbortController()
  selectAllAbort.value = ctrl
  try {
    const ids: string[] = []
    let pg = 1
    while (ids.length < MAX_SELECT_ALL) {
      const result = await ordersApi.fetchPage(apiFilters, pg, SELECT_ALL_PAGE_SIZE, undefined, undefined, ctrl.signal)
      const batch = (result.orders || []).map((o: any) => o.id)
      ids.push(...batch)
      const total = result.pagination?.total ?? 0
      if (total > MAX_SELECT_ALL) {
        toast.add({ severity: 'warn', summary: '已达上限', detail: `仅选中前 ${MAX_SELECT_ALL} 条，请缩小筛选范围`, life: 3000 })
        break
      }
      if (batch.length < SELECT_ALL_PAGE_SIZE || ids.length >= total) break
      pg++
    }
    return ids.slice(0, MAX_SELECT_ALL)
  } catch (e: any) {
    if (e?.name === 'AbortError') return selectedIds.value
    throw e
  } finally {
    selectAllLoading.value = false
    selectAllAbort.value = null
  }
}

function cancelSelectAll() {
  selectAllAbort.value?.abort()
  selectAllLoading.value = false
  selectAllAbort.value = null
}

async function getEffectiveOrderIds(): Promise<string[]> {
  if (!selectAll.value) return selectedIds.value
  const allIds = await getAllMatchingOrderIds()
  return allIds.filter(id => !uncheckedIds.value.has(id))
}

// ── Search ──
async function search() {
  await loadPage(1).catch(e => toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 }))
}

// ── Quick filter ──
async function applyQuickFilter(type: string) {
  const now = new Date()
  const fmt = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  const today = fmt(now)

  // Only modify deliveryDate and pickupMethods — preserve other filters (keyword, startDate, endDate, includedDate)
  if (type === 'today-sf') {
    filters.deliveryDate = today
    orders.filters.pickupMethods = ['顺丰标快']
  } else if (type === 'today-halfday') {
    filters.deliveryDate = today
    orders.filters.pickupMethods = ['半日达']
  } else if (type === 'today-self') {
    filters.deliveryDate = today
    orders.filters.pickupMethods = ['自取/跑腿']
  } else if (type === 'tomorrow') {
    const tomorrow = new Date(now.getTime() + 86400000)
    filters.deliveryDate = fmt(tomorrow)
    orders.filters.pickupMethods = []
  } else if (type === 'overdue') {
    const graceDeadline = new Date(now.getTime() - 5 * 86400000)
    orders.filters.endDateFrom = ''
    orders.filters.endDateTo = fmt(graceDeadline)
    filters.status = 'active'
    filters.deliveryDate = ''
    orders.filters.pickupMethods = []
  } else if (type === 'clear-quick') {
    filters.deliveryDate = ''
    orders.filters.pickupMethods = []
    filters.status = ''
    orders.filters.endDateFrom = ''
    orders.filters.endDateTo = ''
  }
  await loadPage().catch(e => toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 }))
}

// ── Batch search ──
async function executeBatchSearch() {
  const orderNos = batchOrderInput.value
    .split(/[\n,，]+/)
    .map(s => s.trim())
    .filter(Boolean)
  if (orderNos.length === 0) return
  // 单个订单号：精确匹配 orderNo
  // 多个订单号：清空 orderNo，用空格分隔的 keyword 做多值模糊匹配
  if (orderNos.length === 1) {
    orders.filters.orderNo = orderNos[0]
    filters.keyword = ''
  } else {
    orders.filters.orderNo = ''
    filters.keyword = orderNos.join(' ')
  }

  await loadPage().catch(e =>
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 })
  )
}

// ── Delete ──
function confirmDelete(id: string) {
  confirm.require({
    message: '确认删除此订单？',
    header: '确认',
    accept: async () => {
      try {
        await orders.remove(id)
        selectedOrders.value = selectedOrders.value.filter(o => o.id !== id)
        toast.add({ severity: 'success', summary: '已删除', life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

// ── Bulk delete ──
async function confirmBulkDelete() {
  const ids = await getEffectiveOrderIds()
  if (ids.length === 0) return
  confirm.require({
    message: `确认删除 ${ids.length} 个订单？`,
    header: '批量删除',
    accept: async () => {
      try {
        const result = await orders.bulkDelete(ids)
        selectedOrders.value = []
        selectAll.value = false
        uncheckedIds.value = new Set()
        toast.add({ severity: 'success', summary: `成功删除 ${result.successCount} 条`, life: 3000 })
        await loadPage()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '批量删除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

// ── Batch edit state ──
const batchStatus = ref('')
const batchDeliveryMethod = ref('')
const batchNotes = ref('')
const batchUpdating = ref(false)
const batchDeleting = ref(false)
const batchExporting = ref(false)

async function applyBatchEdit() {
  if (batchUpdating.value) return
  const ids = await getEffectiveOrderIds()
  if (ids.length === 0) return

  const fields: Record<string, any> = {}
  if (batchStatus.value) fields.status = batchStatus.value
  if (batchDeliveryMethod.value) fields.pickupMethods = [batchDeliveryMethod.value]
  if (batchNotes.value.trim()) fields.notes = batchNotes.value.trim()

  if (Object.keys(fields).length === 0) {
    toast.add({ severity: 'warn', summary: '请选择要修改的字段', life: 2000 })
    return
  }

  batchUpdating.value = true
  try {
    await ordersApi.bulkUpdateOrders(ids, fields)
    selectedOrders.value = []
    selectAll.value = false
    uncheckedIds.value = new Set()
    batchStatus.value = ''
    batchDeliveryMethod.value = ''
    batchNotes.value = ''
    toast.add({ severity: 'success', summary: `批量更新完成，已更新 ${ids.length} 条`, life: 2000 })
    await loadPage()
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '批量更新失败', detail: e.message, life: 4000 })
  } finally {
    batchUpdating.value = false
  }
}

async function applyBatchDelete() {
  const ids = await getEffectiveOrderIds()
  if (ids.length === 0) return
  batchDeleting.value = true
  confirm.require({
    message: `确认删除 ${ids.length} 个订单？此操作不可撤销。`,
    header: '批量删除确认',
    accept: async () => {
      try {
        const result = await ordersApi.bulkDeleteOrders(ids)
        selectedOrders.value = []
        selectAll.value = false
        uncheckedIds.value = new Set()
        toast.add({ severity: result.failCount > 0 ? 'error' : 'success', summary: result.failCount > 0 ? '部分失败' : '成功', detail: `批量删除完成：成功 ${result.successCount}，失败 ${result.failCount}`, life: result.failCount > 0 ? 4000 : 2000 })
        await orders.refreshAfterMutation()
        await loadPage()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '批量删除失败', detail: e.message, life: 4000 })
      } finally {
        batchDeleting.value = false
      }
    },
    reject: () => { batchDeleting.value = false },
  })
}

async function applyBatchExport() {
  batchExporting.value = true
  try {
    const ids = await getEffectiveOrderIds()
    await ordersApi.exportOrders({}, ids)
    toast.add({ severity: 'success', summary: '导出成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '导出失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    batchExporting.value = false
  }
}

// ── Edit modal ──
function openEdit(order: Order) {
  editOrder.value = { ...order }
  editVisible.value = true
}

function closeEdit() {
  editVisible.value = false
  editOrder.value = null
}

async function saveEdit() {
  if (!editOrder.value) return
  try {
    await orders.update(editOrder.value.id, editOrder.value)
    editVisible.value = false
    editOrder.value = null
    toast.add({ severity: 'success', summary: '订单已更新', life: 2000 })
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '更新失败', detail: e.message, life: 4000 })
  }
}

// ── Device linking ──
function openLinkDevice(orderId: string) {
  linkOrderId.value = orderId
  linkSerialNo.value = ''
  linkVisible.value = true
}

function closeLinkDevice() {
  linkVisible.value = false
  linkOrderId.value = ''
  linkSerialNo.value = ''
}

async function confirmLinkDevice() {
  if (!linkSerialNo.value.trim()) return
  try {
    await orders.linkDevice(linkOrderId.value, linkSerialNo.value.trim())
    linkVisible.value = false
    toast.add({ severity: 'success', summary: '设备已关联', life: 2000 })
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '关联失败', detail: e.message, life: 4000 })
  }
}

async function removeDevice(orderId: string, serialNo: string) {
  try {
    await orders.unlinkDevice(orderId, serialNo)
    toast.add({ severity: 'success', summary: '设备已移除', life: 2000 })
    await orders.refreshAfterMutation()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '移除失败', detail: e.message, life: 4000 })
  }
}

function confirmRemoveDevice(orderId: string, serialNo: string) {
  confirm.require({
    message: `确认移除设备 ${serialNo}？`,
    header: '移除设备',
    accept: async () => {
      try {
        await orders.unlinkDevice(orderId, serialNo)
        toast.add({ severity: 'success', summary: `已移除设备 ${serialNo}`, life: 2000 })
        await loadPage()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '移除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

// ── Import ──
async function handleImport(type: string, event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    let result
    if (type === 'orders') result = await orders.importOrders(file)
    else if (type === 'devices') result = await orders.importDevices(file)
    else result = await orders.importNotes(file)
    toast.add({
      severity: 'success',
      summary: `导入完成: ${result.successCount} 成功, ${result.failCount} 失败`,
      life: 2000,
    })
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '导入失败', detail: e.message, life: 4000 })
  } finally {
    input.value = ''
  }
}

// ── Export ──
const exporting = ref(false)

async function handleExport() {
  exporting.value = true
  try {
    await workbench.exportCurrent()
    toast.add({ severity: 'success', summary: '导出成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '导出失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    exporting.value = false
  }
}

// ── Toggle pickup filter ──
function togglePickupFilter(method: string) {
  const current = orders.filters.pickupMethods || []
  const idx = current.indexOf(method)
  if (idx === -1) orders.filters.pickupMethods = [...current, method]
  else orders.filters.pickupMethods = current.filter(m => m !== method)
}

function isPickupFilterSelected(method: string): boolean {
  const current = orders.filters.pickupMethods
  if (!current || !Array.isArray(current)) return false
  return current.includes(method)
}

// ── Reset all filters ──
async function resetAllFilters() {
  orders.resetFilters()
  filters.keyword = ''
  filters.status = ''
  filters.startDate = ''
  filters.endDate = ''
  filters.deliveryDate = ''
  await loadPage().catch(e => toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 }))
}

// ── Clear quick filter only ──
async function clearQuickFilter() {
  await applyQuickFilter('clear-quick')
}

// ── Helper: pickup method badge style ──
function pickupBadgeSeverity(method: string): string {
  if (method === '顺丰标快') return 'info'
  if (method === '半日达') return 'warn'
  if (method === '自取/跑腿') return 'success'
  return 'info'
}

// ── Helpers: parse area info from notes suffix ──
function parseAreaFromNotes(notes: string): { cleanNotes: string; receiveArea: string; returnArea: string } {
  if (!notes) return { cleanNotes: '', receiveArea: '', returnArea: '' }
  const match = notes.match(/\n---\n\[区域信息\] 收:(\S+)\s+还:(\S+)/)
  if (match) {
    return {
      cleanNotes: notes.replace(/\n---\n\[区域信息\] 收:\S+\s+还:\S+/, ''),
      receiveArea: match[1],
      returnArea: match[2],
    }
  }
  return { cleanNotes: notes, receiveArea: '', returnArea: '' }
}

function getCleanNotes(notes: string): string {
  return parseAreaFromNotes(notes).cleanNotes
}

function getAreaDisplay(notes: string): string {
  const { receiveArea, returnArea } = parseAreaFromNotes(notes)
  if (!receiveArea && !returnArea) return ''
  return `收:${receiveArea} 还:${returnArea}`
}

// ── Computed: is quick filter active ──
const quickFilterActive = computed(() => {
  const now = new Date()
  const fmt = (d: Date) =>
    `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
  const today = fmt(now)
  const tomorrow = fmt(new Date(now.getTime() + 86400000))
  const pm = orders.filters.pickupMethods
  const hasPickup = Array.isArray(pm) && pm.length > 0
  const dd = filters.deliveryDate

  if (dd === today && pm?.length === 1 && pm[0] === '顺丰标快') return 'today-sf'
  if (dd === today && pm?.length === 1 && pm[0] === '半日达') return 'today-halfday'
  if (dd === today && pm?.length === 1 && pm[0] === '自取/跑腿') return 'today-self'
  if (dd === tomorrow && !hasPickup) return 'tomorrow'
  return null
})

</script>

<template>
  <div class="page-root">

    <!-- MODULE-NN bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-02</span>
      <div class="structure-line" />
      <span class="module-page-label">订单工作台</span>
    </div>

    <div class="space-y-4">
    <!-- Header -->
    <div class="flex items-center justify-between">
      <h2 class="panel-title mb-0">订单工作台</h2>
      <span class="mono-label">
        共 {{ total || rows.length }} 条记录
        <template v-if="selectedCount > 0"> / 已选 {{ selectedCount }} 条</template>
      </span>
    </div>

    <!-- Top Toolbar -->
    <div class="flex flex-wrap items-center gap-2">
      <!-- Keyword search -->
      <div class="flex items-center gap-1 flex-1 min-w-0 max-w-80">
        <input
          v-model="filters.keyword"
          class="input flex-1"
          placeholder="关键字搜索..."
          @keydown.enter="search()"
        />
      </div>

      <!-- Search / Refresh -->
      <button class="btn-primary text-xs px-3 py-1.5" @click="search()">搜索</button>
      <button class="btn-secondary text-xs px-3 py-1.5" @click="loadPage()">刷新</button>

      <!-- Batch search -->
      <button class="btn-secondary text-xs px-3 py-1.5" @click="batchSearchDialog = true">批量搜索</button>

      <!-- Export -->
      <button v-if="!tenantPreview.active" class="btn-secondary text-xs px-3 py-1.5 ml-auto" :disabled="exporting" @click="handleExport()">
        {{ exporting ? '导出中...' : '导出 Excel' }}
      </button>
    </div>

    <!-- Batch Action Toolbar -->
    <Transition name="batch-panel">
    <div v-if="selectedCount > 0 && !tenantPreview.active" class="card overflow-hidden">
      <div class="flex items-center justify-between mb-3">
        <h3 class="font-mono text-sm font-bold text-text-primary">批量操作</h3>
        <span class="font-mono text-xs text-text-muted">已选 {{ selectedCount }} 个订单</span>
      </div>
      <div class="flex flex-wrap items-end gap-3">
        <!-- 修改状态 -->
        <div class="w-[130px]">
          <label class="mono-label block mb-1">修改状态</label>
          <Select v-model="batchStatus" :options="[{ label: '预约', value: 'reserved' }, { label: '进行中', value: 'active' }, { label: '已完成', value: 'completed' }]" option-label="label" option-value="value" placeholder="选择状态" class="w-full" />
        </div>
        <!-- 修改配送方式 -->
        <div class="w-[130px]">
          <label class="mono-label block mb-1">修改配送方式</label>
          <Select v-model="batchDeliveryMethod" :options="['顺丰标快', '自取/跑腿', '半日达'].map(m => ({ label: m, value: m }))" option-label="label" option-value="value" placeholder="选择方式" class="w-full" />
        </div>
        <!-- 修改备注 -->
        <div class="flex-1 min-w-[180px]">
          <label class="mono-label block mb-1">修改备注（覆盖）</label>
          <input v-model="batchNotes" class="input w-full" placeholder="输入新备注..." @keydown.enter="applyBatchEdit" />
        </div>
        <!-- 应用 -->
        <button class="btn-primary text-xs px-3 py-1.5" :disabled="batchUpdating || (!batchStatus && !batchDeliveryMethod && !batchNotes.trim())" @click="applyBatchEdit">
          {{ batchUpdating ? '应用中...' : '应用修改' }}
        </button>
        <!-- 分隔 -->
        <div class="w-px h-8 bg-border self-center" />
        <!-- 删除 -->
        <button class="btn-danger text-xs px-3 py-1.5" :disabled="batchDeleting || selectAllLoading" @click="applyBatchDelete">
          {{ batchDeleting ? '删除中...' : `删除 (${selectedCount})` }}
        </button>
        <!-- 导出 -->
        <button class="btn-secondary text-xs px-3 py-1.5" :disabled="batchExporting || selectAllLoading" @click="applyBatchExport">
          {{ batchExporting ? '导出中...' : `导出 (${selectedCount})` }}
        </button>
        <!-- 取消 -->
        <button class="btn-secondary text-xs px-3 py-1.5" @click="selectedOrders = []; selectAll = false; uncheckedIds = new Set(); selectAllLoading = false">取消选择</button>
      </div>
      <div v-if="selectAllLoading" class="flex items-center gap-2 mt-2">
        <button class="btn-secondary text-xs px-3 py-1.5" @click="cancelSelectAll()">取消加载</button>
        <span class="font-sans text-xs text-text-muted">正在获取所有匹配订单...</span>
      </div>
    </div>
    </Transition>

    <!-- Filter Toggle -->
    <div>
      <button class="btn-secondary text-xs px-3 py-1.5" @click="showFilters = !showFilters">
        {{ showFilters ? '收起筛选' : '展开筛选' }}
      </button>
    </div>

    <!-- Filter Panel -->
    <Transition name="collapse">
    <div
      v-if="showFilters"
      class="card space-y-4 overflow-hidden"
    >
      <!-- Date filters (matching original system single-date layout) -->
      <div class="space-y-1">
        <span class="mono-label">日期筛选</span>
        <div class="grid grid-cols-2 sm:grid-cols-4 gap-2">
          <div class="flex flex-col gap-1">
            <label class="mono-label">开始日期</label>
            <DatePicker v-model="filterStartDate" show-icon date-format="yy-mm-dd" class="w-full" />
          </div>
          <div class="flex flex-col gap-1">
            <label class="mono-label">结束日期</label>
            <DatePicker v-model="filterEndDate" show-icon date-format="yy-mm-dd" class="w-full" />
          </div>
          <div class="flex flex-col gap-1">
            <label class="mono-label">发货日期</label>
            <DatePicker v-model="filterDeliveryDate" show-icon date-format="yy-mm-dd" class="w-full" />
          </div>
          <div class="flex flex-col gap-1">
            <label class="mono-label">包含日期</label>
            <DatePicker v-model="filterIncludedDate" show-icon date-format="yy-mm-dd" class="w-full" />
          </div>
        </div>
      </div>

      <!-- Status filter -->
      <div class="flex flex-wrap items-end gap-3">
        <div class="w-[150px]">
          <label class="mono-label block mb-1">订单状态</label>
          <Select v-model="filters.status" :options="[{ label: '全部', value: '' }, { label: '预约', value: 'reserved' }, { label: '进行中', value: 'active' }, { label: '已完成', value: 'completed' }]" option-label="label" option-value="value" class="w-full" />
        </div>
      </div>

      <!-- Pickup method filters -->
      <div class="space-y-1">
        <span class="mono-label">配送方式</span>
        <div class="flex flex-wrap gap-2">
          <button
            v-for="method in pickupFilterOptions"
            :key="method"
            :class="isPickupFilterSelected(method) ? 'btn-primary text-xs px-3 py-1.5' : 'btn-secondary text-xs px-3 py-1.5'"
            @click="togglePickupFilter(method)"
          >{{ method }}</button>
        </div>
      </div>

      <!-- Quick filter buttons -->
      <div class="space-y-1">
        <span class="mono-label">快捷筛选</span>
        <div class="flex flex-wrap gap-2">
          <button :class="quickFilterActive === 'today-sf' ? 'btn-primary text-xs px-3 py-1.5' : 'btn-secondary text-xs px-3 py-1.5'" @click="applyQuickFilter('today-sf')">今天要发货</button>
          <button :class="quickFilterActive === 'today-halfday' ? 'btn-primary text-xs px-3 py-1.5' : 'btn-secondary text-xs px-3 py-1.5'" @click="applyQuickFilter('today-halfday')">今天要发半日达</button>
          <button :class="quickFilterActive === 'today-self' ? 'btn-primary text-xs px-3 py-1.5' : 'btn-secondary text-xs px-3 py-1.5'" @click="applyQuickFilter('today-self')">今天自取/跑腿</button>
          <button :class="quickFilterActive === 'tomorrow' ? 'btn-primary text-xs px-3 py-1.5' : 'btn-secondary text-xs px-3 py-1.5'" @click="applyQuickFilter('tomorrow')">明天要发货</button>
          <button class="btn-secondary text-xs px-3 py-1.5" @click="clearQuickFilter()">清除快捷筛选</button>
          <button class="btn-danger text-xs px-3 py-1.5" @click="applyQuickFilter('overdue')">逾期未完成</button>
          <button class="btn-danger text-xs px-3 py-1.5" @click="resetAllFilters()">重置</button>
        </div>
      </div>

      <!-- Apply filters -->
      <div>
        <button class="btn-primary text-xs px-3 py-1.5" @click="search()">应用筛选</button>
      </div>
    </div>
    </Transition>

    <!-- Paginated DataTable -->
    <div class="card table-panel overflow-hidden">
    <div v-if="loading &amp;&amp; rows.length" class="skeleton-progress" />
    <DataTable
      v-model:selection="selectedOrders"
      :value="rows"
      dataKey="id"
      :lazy="true"
      :loading="loading"
      :paginator="true"
      :rows="pageSize"
      :totalRecords="total"
      :first="(page - 1) * pageSize"
      :rowsPerPageOptions="[10, 20, 30, 50]"
      paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
      currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
      :resizableColumns="true"
      columnResizeMode="expand"
      :sortField="sortField || undefined"
      :sortOrder="sortOrder || undefined"
      removableSort
      :selectAll="selectAll"
      @page="onPage"
      @sort="onSort"
      @select-all-change="onSelectAllChange"
      @row-select="onRowSelect"
      @row-unselect="onRowUnselect"
    >
      <template #empty>
        <div class="py-6 text-center">
          <span class="font-sans text-sm text-text-muted">暂无数据</span>
        </div>
      </template>
      <Column v-if="!tenantPreview.active" selectionMode="multiple" headerStyle="width: 3rem" />
      <Column field="orderNo" header="订单号" :min-width="120" sortable>
        <template #body="{ data }: { data: Order }">
          <span v-if="data" class="font-mono whitespace-nowrap">{{ data.orderNo }}</span>
          <Skeleton v-else width="80%" height="1rem" />
        </template>
      </Column>

      <Column field="status" header="状态" :min-width="90" sortable>
        <template #body="{ data }: { data: Order }">
          <span v-if="data" :class="data.status === 'completed' ? 'badge-success' : data.status === 'reserved' ? 'badge-warning' : 'badge-info'">
            {{ data.status === 'completed' ? '已完成' : data.status === 'reserved' ? '预约' : '进行中' }}
          </span>
          <Skeleton v-else width="50%" height="1rem" />
        </template>
      </Column>

      <Column field="startDate" header="开始日期" :min-width="100" sortable>
        <template #body="{ data }: { data: Order }">
          <template v-if="data">
            <template v-if="!tenantPreview.active && editingCell?.rowId === data.id && editingCell?.field === 'startDate'">
              <input
                type="date"
                :value="data.startDate"
                class="input text-xs px-1 py-0 w-24"
                @blur="saveInlineEdit(data.id, 'startDate', ($event.target as HTMLInputElement).value)"
                @keydown.escape="cancelInlineEdit()"
                @keydown.enter="saveInlineEdit(data.id, 'startDate', ($event.target as HTMLInputElement).value)"
              />
            </template>
            <span
              v-else
              :class="!tenantPreview.active ? 'cursor-pointer hover:text-text-accent whitespace-nowrap' : 'whitespace-nowrap'"
              @click="!tenantPreview.active && startInlineEdit(data.id, 'startDate')"
            >
              {{ data.startDate || '-' }}
            </span>
          </template>
          <Skeleton v-else width="70%" height="1rem" />
        </template>
      </Column>

      <Column field="endDate" header="结束日期" :min-width="100" sortable>
        <template #body="{ data }: { data: Order }">
          <template v-if="data">
            <template v-if="!tenantPreview.active && editingCell?.rowId === data.id && editingCell?.field === 'endDate'">
              <input
                type="date"
                :value="data.endDate"
                class="input text-xs px-1 py-0 w-24"
                @blur="saveInlineEdit(data.id, 'endDate', ($event.target as HTMLInputElement).value)"
                @keydown.escape="cancelInlineEdit()"
                @keydown.enter="saveInlineEdit(data.id, 'endDate', ($event.target as HTMLInputElement).value)"
              />
            </template>
            <span
              v-else
              :class="!tenantPreview.active ? 'cursor-pointer hover:text-text-accent whitespace-nowrap' : 'whitespace-nowrap'"
              @click="!tenantPreview.active && startInlineEdit(data.id, 'endDate')"
            >
              {{ data.endDate || '-' }}
            </span>
          </template>
          <Skeleton v-else width="70%" height="1rem" />
        </template>
      </Column>

      <Column field="deliveryDate" header="发货日期" :min-width="100" sortable>
        <template #body="{ data }: { data: Order }">
          <template v-if="data">
            <template v-if="!tenantPreview.active && editingCell?.rowId === data.id && editingCell?.field === 'deliveryDate'">
              <input
                type="date"
                :value="data.deliveryDate"
                class="input text-xs px-1 py-0 w-24"
                @blur="saveInlineEdit(data.id, 'deliveryDate', ($event.target as HTMLInputElement).value)"
                @keydown.escape="cancelInlineEdit()"
                @keydown.enter="saveInlineEdit(data.id, 'deliveryDate', ($event.target as HTMLInputElement).value)"
              />
            </template>
            <span
              v-else
              :class="!tenantPreview.active ? 'cursor-pointer hover:text-text-accent whitespace-nowrap' : 'whitespace-nowrap'"
              @click="!tenantPreview.active && startInlineEdit(data.id, 'deliveryDate')"
            >
              {{ data.deliveryDate || '-' }}
            </span>
          </template>
          <Skeleton v-else width="70%" height="1rem" />
        </template>
      </Column>

      <Column header="配送方式" :min-width="110">
        <template #body="{ data }: { data: Order }">
          <div v-if="data" class="flex flex-wrap gap-1">
            <Tag
              v-for="method in data.pickupMethods"
              :key="method"
              :severity="pickupBadgeSeverity(method) as any"
              :value="method"
            />
            <span v-if="!data.pickupMethods || data.pickupMethods.length === 0" class="text-text-muted">-</span>
          </div>
          <Skeleton v-else width="80%" height="1rem" />
        </template>
      </Column>

      <Column header="设备" :min-width="160">
        <template #body="{ data }: { data: Order }">
          <div v-if="data" class="space-y-1">
            <div class="flex items-center gap-1">
              <span class="font-mono text-xs">{{ data.devices?.length || 0 }} 台</span>
              <Button
                v-if="!tenantPreview.active"
                severity="secondary"
                title="关联设备"
                label="+"
                class="!px-2 !py-0.5 !text-xs !min-w-0"
                @click="openLinkDevice(data.id)"
              />
            </div>
            <div v-if="data.devices?.length" class="flex flex-wrap gap-1">
              <Tag
                v-for="(serialNo, di) in data.devices"
                :key="`${data.id}-dev-${di}`"
                severity="info"
                :value="serialNo"
                :class="{ 'cursor-pointer': !tenantPreview.active }"
                :title="tenantPreview.active ? serialNo : `点击移除 ${serialNo}`"
                @click="!tenantPreview.active && confirmRemoveDevice(data.id, serialNo)"
              />
            </div>
          </div>
          <Skeleton v-else width="90%" height="1rem" />
        </template>
      </Column>

      <Column header="地址" :min-width="160">
        <template #body="{ data }: { data: Order }">
          <template v-if="data">
            <template v-if="!tenantPreview.active && editingCell?.rowId === data.id && editingCell?.field === 'address'">
              <input
                type="text"
                :value="data.address"
                class="input text-xs px-1 py-0 w-full min-w-[120px]"
                @blur="saveInlineEdit(data.id, 'address', ($event.target as HTMLInputElement).value)"
                @keydown.escape="cancelInlineEdit()"
                @keydown.enter="saveInlineEdit(data.id, 'address', ($event.target as HTMLInputElement).value)"
              />
            </template>
            <span
              v-else
              :class="!tenantPreview.active ? 'cursor-pointer hover:text-text-accent truncate block max-w-[14rem]' : 'truncate block max-w-[14rem]'"
              :title="data.address"
              @click="!tenantPreview.active && startInlineEdit(data.id, 'address')"
            >
              {{ data.address || '-' }}
            </span>
          </template>
          <Skeleton v-else width="80%" height="1rem" />
        </template>
      </Column>

      <Column header="备注" :min-width="160">
        <template #body="{ data }: { data: Order }">
          <template v-if="data">
            <template v-if="!tenantPreview.active && editingCell?.rowId === data.id && editingCell?.field === 'notes'">
              <input
                type="text"
                :value="data.notes"
                class="input text-xs px-1 py-0 w-full min-w-[120px]"
                @blur="saveInlineEdit(data.id, 'notes', ($event.target as HTMLInputElement).value)"
                @keydown.escape="cancelInlineEdit()"
                @keydown.enter="saveInlineEdit(data.id, 'notes', ($event.target as HTMLInputElement).value)"
              />
            </template>
            <div
              v-else
              :class="!tenantPreview.active ? 'cursor-pointer hover:text-text-accent min-w-0' : 'min-w-0'"
              @click="!tenantPreview.active && startInlineEdit(data.id, 'notes')"
            >
              <div class="flex flex-col gap-0.5 min-w-0">
                <span :title="getCleanNotes(data.notes)" class="truncate block max-w-[14rem]">{{ getCleanNotes(data.notes) || '-' }}</span>
                <span v-if="getAreaDisplay(data.notes)" class="text-xs text-text-accent font-mono">{{ getAreaDisplay(data.notes) }}</span>
              </div>
            </div>
          </template>
          <Skeleton v-else width="90%" height="1rem" />
        </template>
      </Column>

      <Column v-if="!tenantPreview.active" header="操作" :min-width="100">
        <template #body="{ data }: { data: Order }">
          <div v-if="data" class="flex gap-1">
            <button class="btn-secondary text-xs px-2 py-1" @click="openEdit(data)">编辑</button>
            <button class="btn-danger text-xs px-2 py-1" @click="confirmDelete(data.id)">删除</button>
          </div>
          <Skeleton v-else width="4rem" height="1.25rem" />
        </template>
      </Column>
    </DataTable>
    </div>

    <!-- Import Section -->
    <div v-if="!tenantPreview.active" class="pt-2 border-t border-border">
      <span class="mono-label block mb-2">数据导入</span>
      <div class="flex flex-wrap gap-2">
        <label class="btn-secondary text-xs px-3 py-1 cursor-pointer">
          导入订单 Excel
          <input
            ref="importOrdersRef"
            type="file"
            accept=".xlsx,.xls,.csv"
            class="hidden"
            @change="handleImport('orders', $event as Event)"
          />
        </label>
        <label class="btn-secondary text-xs px-3 py-1 cursor-pointer">
          导入设备关联 Excel
          <input
            ref="importDevicesRef"
            type="file"
            accept=".xlsx,.xls,.csv"
            class="hidden"
            @change="handleImport('devices', $event as Event)"
          />
        </label>
        <label class="btn-secondary text-xs px-3 py-1 cursor-pointer">
          导入备注 Excel
          <input
            ref="importNotesRef"
            type="file"
            accept=".xlsx,.xls,.csv"
            class="hidden"
            @change="handleImport('notes', $event as Event)"
          />
        </label>
      </div>
    </div>

    <!-- ── Edit Modal ── -->
    <Dialog
      v-model:visible="editVisible"
      modal
      header="编辑订单"
      :style="{ width: '36rem' }"
    >
      <div v-if="editOrder" class="space-y-3">
        <div>
          <label class="mono-label block mb-1">订单号</label>
          <input v-model="editOrder.orderNo" class="input w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">开始日期</label>
          <DatePicker v-model="editStartDate" show-icon date-format="yy-mm-dd" class="w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">结束日期</label>
          <DatePicker v-model="editEndDate" show-icon date-format="yy-mm-dd" class="w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">发货日期</label>
          <DatePicker v-model="editDeliveryDate" show-icon date-format="yy-mm-dd" class="w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">配送方式</label>
          <div class="flex gap-1">
            <button
              v-for="method in pickupFilterOptions"
              :key="method"
              :class="(editOrder?.pickupMethods || [])[0] === method ? 'btn-primary text-xs px-2 py-1 flex-1' : 'btn-secondary text-xs px-2 py-1 flex-1'"
              @click="() => {
                if (!editOrder) return
                const current = (editOrder.pickupMethods || [])[0]
                editOrder.pickupMethods = current === method ? [] : [method]
              }"
            >{{ method }}</button>
          </div>
        </div>
        <div>
          <label class="mono-label block mb-1">地址</label>
          <input v-model="editOrder.address" class="input w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">备注</label>
          <textarea v-model="editOrder.notes" class="textarea w-full" rows="3" />
        </div>
      </div>
      <template #footer>
        <button class="btn-secondary text-xs px-3 py-1.5" @click="closeEdit()">取消</button>
        <button class="btn-primary text-xs px-3 py-1.5" @click="saveEdit()">保存</button>
      </template>
    </Dialog>

    <!-- ── Link Device Modal ── -->
    <Dialog
      v-model:visible="linkVisible"
      modal
      header="关联设备"
      :style="{ width: '24rem' }"
    >
      <div class="space-y-3">
        <div>
          <label class="mono-label block mb-1">设备序列号</label>
          <input
            v-model="linkSerialNo"
            class="input w-full"
            placeholder="输入设备序列号..."
            @keydown.enter="confirmLinkDevice()"
          />
        </div>
      </div>
      <template #footer>
        <button class="btn-secondary text-xs px-3 py-1.5" @click="closeLinkDevice()">取消</button>
        <button
          class="btn-primary text-xs px-3 py-1.5"
          :disabled="!linkSerialNo.trim()"
          @click="confirmLinkDevice()"
        >确认关联</button>
      </template>
    </Dialog>

    <!-- Batch Search Dialog -->
    <Dialog
      v-model:visible="batchSearchDialog"
      modal
      header="批量搜索"
      :style="{ width: '32rem' }"
    >
      <div class="space-y-3">
        <textarea
          v-model="batchOrderInput"
          class="textarea"
          rows="4"
          placeholder="输入多个订单号，用换行、逗号或中文逗号分隔..."
        />
        <div class="flex gap-2">
          <button class="btn-primary text-xs px-3 py-1.5" @click="executeBatchSearch(); batchSearchDialog = false">批量查询</button>
          <button class="btn-secondary text-xs px-3 py-1.5" @click="batchOrderInput = ''">清空</button>
        </div>
      </div>
      <template #footer>
        <button class="btn-secondary text-xs px-3 py-1.5" @click="batchSearchDialog = false">关闭</button>
      </template>
    </Dialog>
  </div>
  </div>
</template>

<style scoped>
.skeleton-progress {
  height: 2px;
  background: var(--accent);
  animation: skeleton-slide 1200ms ease-in-out infinite;
  opacity: 0.7;
}
@keyframes skeleton-slide {
  0% { width: 0; margin-left: 0; }
  50% { width: 40%; margin-left: 30%; }
  100% { width: 0; margin-left: 100%; }
}
</style>
