<script setup lang="ts">
import { ref, onMounted, computed } from 'vue'
import { useDevicesStore } from '@/stores/devices'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import * as devicesApi from '@/api/devices'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { useRouteQuery } from '@/composables/useRouteQuery'
import { shanghaiBusinessDate } from '@/utils/businessDate'
import type { Device } from '@/api/devices'
import DataTable, { type DataTableSortEvent } from 'primevue/datatable'
import Column from 'primevue/column'

const store = useDevicesStore()
const tenantPreview = useTenantPreviewStore()
const toast = useToast()
const confirm = useConfirm()

// ==================== Local pagination state ====================
const rows = ref<Device[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(10)
const loading = ref(false)
const sortField = ref<string>()
const sortOrder = ref<1 | -1>()

// URL-synced filter state
const filters = useRouteQuery({
  keyword: '',
  rentalStatus: '',
})

// Build API-level filter object from URL-synced filters + store-bound filters
function buildApiFilters(): any {
  const f: any = {}
  if (filters.keyword) f.keyword = filters.keyword
  if (filters.rentalStatus) f.rentalStatus = filters.rentalStatus
  if (store.filters.warningStatus) f.warningStatus = store.filters.warningStatus
  if (store.filters.notes) f.notes = store.filters.notes
  return f
}

async function loadPage(p?: number, ps?: number) {
  const pg = p ?? page.value
  const pz = ps ?? pageSize.value
  const f = buildApiFilters()
  loading.value = true
  try {
    const result = await devicesApi.fetchPage(f, pg, pz, sortField.value, sortOrder.value)
    const newRows = result.devices || [] as Device[]
    // Merge selection before setting rows so DataTable sees both together
    if (selectAll.value) {
      const existing = new Set(selectedDevices.value.map(d => d.serialNo))
      const toAdd = newRows.filter(row => !existing.has(row.serialNo) && !uncheckedSerialNos.value.has(row.serialNo))
      selectedDevices.value = [...selectedDevices.value, ...toAdd]
    }
    rows.value = newRows
    total.value = result.pagination?.total || 0
    page.value = pg
    pageSize.value = pz
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  try {
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载设备列表失败', life: 4000 })
  }
})

// ==================== Add device form ====================
const newSerialNo = ref('')
const newStatus = ref('已入库')
const newNotes = ref('')
const adding = ref(false)

// Device rental status transition rules:
// - '确认丢失' and '已报废' are terminal states — cannot go back to '已入库'
// - Only admins can set these statuses (backend-enforced via audit)
const statusOptions = ['已入库', '租赁中', '返厂维修', '确认丢失', '已报废']
const wsFilterOptions = [
  { label: '全部预警状态', value: '' },
  { label: '正常', value: '正常' },
  { label: '疑似丢失', value: '疑似丢失' },
]
const statusFilterOptions = [
  { label: '全部状态', value: '' },
  { label: '已入库', value: '已入库' },
  { label: '租赁中', value: '租赁中' },
  { label: '返厂维修', value: '返厂维修' },
  { label: '确认丢失', value: '确认丢失' },
  { label: '已报废', value: '已报废' },
]

// ==================== Filters ====================
const warningFilter = ref('')

// ==================== Selection ====================
const selectedDevices = ref<Device[]>([])
const selectAll = ref(false)
const uncheckedSerialNos = ref<Set<string>>(new Set())
const selectedSerialNos = computed(() => selectedDevices.value.map(d => d.serialNo))
const selectedCount = computed(() => selectAll.value ? total.value - uncheckedSerialNos.value.size : selectedSerialNos.value.length)
const selectAllLoading = ref(false)
const selectAllAbort = ref<AbortController | null>(null)

// ==================== Edit tracking ====================
const editNotes = ref<Record<string, string>>({})

function initEditNotes(device: Device) {
  if (!(device.serialNo in editNotes.value)) {
    editNotes.value[device.serialNo] = device.notes ?? ''
  }
}

function getEditNotes(device: Device): string {
  return editNotes.value[device.serialNo] ?? device.notes ?? ''
}

// ==================== Add device ====================
async function addDevice() {
  const sn = newSerialNo.value.trim()
  if (!sn) {
    toast.add({ severity: 'error', summary: '错误', detail: '请输入设备序列号', life: 4000 })
    return
  }
  adding.value = true
  try {
    await store.create({ serialNo: sn, rentalStatus: newStatus.value, notes: newNotes.value })
    newSerialNo.value = ''
    newNotes.value = ''
    await loadPage()
    toast.add({ severity: 'success', summary: '成功', detail: '设备添加成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '添加失败', life: 4000 })
  } finally {
    adding.value = false
  }
}

// ==================== Inline edits ====================
async function saveStatus(device: Device, nextStatus: string) {
  if (device.rentalStatus === nextStatus) return
  try {
    await store.update(device.serialNo, { rentalStatus: nextStatus })
    toast.add({ severity: 'success', summary: '成功', detail: '状态更新成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '更新失败', life: 4000 })
  }
}

async function saveNotes(device: Device) {
  const notes = editNotes.value[device.serialNo]
  if (notes === (device.notes ?? '')) return
  try {
    await store.update(device.serialNo, { notes })
    toast.add({ severity: 'success', summary: '成功', detail: '备注更新成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '更新失败', life: 4000 })
  }
}

// ==================== Delete ====================
async function deleteDevice(serialNo: string) {
  confirm.require({
    message: '确认删除该设备？',
    header: '删除确认',
    icon: 'pi pi-exclamation-triangle',
    accept: async () => {
      try {
        await store.remove(serialNo)
        selectedDevices.value = selectedDevices.value.filter(d => d.serialNo !== serialNo)
        toast.add({ severity: 'success', summary: '成功', detail: '删除成功', life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '错误', detail: e.message || '删除失败', life: 4000 })
      }
    }
  })
}

const bulkDeleting = ref(false)

async function bulkDelete() {
  const ids = await getEffectiveSerialNos()
  if (ids.length === 0) {
    toast.add({ severity: 'error', summary: '错误', detail: '请先选择要删除的设备', life: 4000 })
    return
  }
  confirm.require({
    message: `确认删除选中的 ${ids.length} 个设备？此操作不可恢复。`,
    header: '批量删除确认',
    icon: 'pi pi-exclamation-triangle',
    accept: async () => {
      bulkDeleting.value = true
      try {
        const result = await store.bulkDelete(ids)
        selectedDevices.value = []
        selectAll.value = false
        uncheckedSerialNos.value = new Set()
        await loadPage()
        toast.add({ severity: result.failCount > 0 ? 'error' : 'success', summary: result.failCount > 0 ? '错误' : '成功', detail: `批量删除完成：成功 ${result.successCount}，失败 ${result.failCount}`, life: result.failCount > 0 ? 4000 : 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '错误', detail: e.message || '批量删除失败', life: 4000 })
      } finally {
        bulkDeleting.value = false
      }
    }
  })
}

// ==================== Batch action card ====================
const batchStatus = ref('')
const batchNotes = ref('')
const batchUpdating = ref(false)
const batchExporting = ref(false)

async function applyBatchStatus() {
  if (batchUpdating.value) return
  if (!batchStatus.value) {
    toast.add({ severity: 'error', summary: '错误', detail: '请选择租赁状态', life: 4000 })
    return
  }
  batchUpdating.value = true
  try {
    const ids = await getEffectiveSerialNos()
    if (ids.length === 0) { batchUpdating.value = false; return }
    const result = await store.bulkUpdate(ids, { rentalStatus: batchStatus.value })
    selectedDevices.value = []
    selectAll.value = false
    uncheckedSerialNos.value = new Set()
    await loadPage()
    toast.add({ severity: result.failCount > 0 ? 'error' : 'success', summary: result.failCount > 0 ? '部分失败' : '成功', detail: `批量更新状态完成：成功 ${result.updatedCount}，失败 ${result.failCount}`, life: result.failCount > 0 ? 4000 : 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '批量更新失败', life: 4000 })
  } finally {
    batchUpdating.value = false
  }
}

async function applyBatchNotes() {
  if (batchUpdating.value) return
  const notes = batchNotes.value.trim()
  if (!notes) {
    toast.add({ severity: 'error', summary: '错误', detail: '请输入备注内容', life: 4000 })
    return
  }
  batchUpdating.value = true
  try {
    const ids = await getEffectiveSerialNos()
    if (ids.length === 0) { batchUpdating.value = false; return }
    const result = await store.bulkUpdate(ids, { notes })
    selectedDevices.value = []
    selectAll.value = false
    uncheckedSerialNos.value = new Set()
    batchNotes.value = ''
    await loadPage()
    toast.add({ severity: result.failCount > 0 ? 'error' : 'success', summary: result.failCount > 0 ? '部分失败' : '成功', detail: `批量更新备注完成：成功 ${result.updatedCount}，失败 ${result.failCount}`, life: result.failCount > 0 ? 4000 : 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '批量更新失败', life: 4000 })
  } finally {
    batchUpdating.value = false
  }
}

async function exportSelected() {
  const ids = await getEffectiveSerialNos()
  if (ids.length === 0) {
    toast.add({ severity: 'error', summary: '错误', detail: '请先选择要导出的设备', life: 4000 })
    return
  }
  batchExporting.value = true
  try {
    const blob = await devicesApi.exportDevices(undefined, ids)
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `devices-${shanghaiBusinessDate()}.xlsx`
    a.click()
    URL.revokeObjectURL(url)
    toast.add({ severity: 'success', summary: '成功', detail: `已导出 ${ids.length} 个设备`, life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '导出失败', life: 4000 })
  } finally {
    batchExporting.value = false
  }
}

// ==================== Search / Reset ====================
async function doSearch() {
  store.filters.warningStatus = warningFilter.value || undefined
  try {
    await loadPage(1)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '搜索失败', life: 4000 })
  }
}

async function resetFilters() {
  filters.keyword = ''
  filters.rentalStatus = ''
  warningFilter.value = ''
  store.resetFilters()
  try {
    await loadPage(1)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '加载失败', life: 4000 })
  }
}

// ==================== Pagination ====================
function hasDirtyEdits(): boolean {
  for (const d of rows.value) {
    const edited = editNotes.value[d.serialNo]
    if (edited !== undefined && edited !== (d.notes ?? '')) return true
  }
  return false
}

function onPageChange(event: { page: number; rows: number }) {
  if (hasDirtyEdits()) {
    confirm.require({
      message: '当前页面有未保存的备注修改，切换页面将丢失修改。',
      header: '未保存的修改',
      accept: () => {
        editNotes.value = {}
        loadPage(event.page + 1, event.rows).catch(e =>
          toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
        )
      },
    })
    return
  }
  loadPage(event.page + 1, event.rows).catch(e =>
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
  )
}

function onSort(event: DataTableSortEvent) {
  sortField.value = typeof event.sortField === 'string' ? event.sortField : undefined
  sortOrder.value = event.sortOrder === 1 || event.sortOrder === -1 ? event.sortOrder : undefined
  loadPage(1).catch(e =>
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
  )
}

// ── Select All ──
function onSelectAllChange(event: { checked: boolean }) {
  selectAll.value = event.checked
  if (event.checked) {
    uncheckedSerialNos.value = new Set()
    selectedDevices.value = [...rows.value]
  } else {
    selectedDevices.value = []
    uncheckedSerialNos.value = new Set()
  }
}

function onRowSelect(event: { data: Device }) {
  if (selectAll.value && event.data?.serialNo) {
    uncheckedSerialNos.value.delete(event.data.serialNo)
  }
}

function onRowUnselect(event: { data: Device }) {
  if (selectAll.value && event.data?.serialNo) {
    uncheckedSerialNos.value.add(event.data.serialNo)
  }
}

const MAX_SELECT_ALL = 5000
const SELECT_ALL_PAGE_SIZE = 200 // query-builder caps pageSize at 200

async function getAllMatchingDeviceSerialNos(): Promise<string[]> {
  const f = buildApiFilters()

  selectAllLoading.value = true
  const ctrl = new AbortController()
  selectAllAbort.value = ctrl
  try {
    const serials: string[] = []
    let pg = 1
    while (serials.length < MAX_SELECT_ALL) {
      const result = await devicesApi.fetchPage(f, pg, SELECT_ALL_PAGE_SIZE, null, null, ctrl.signal)
      const batch = (result.devices || []).map((d: Device) => d.serialNo)
      serials.push(...batch)
      const total = result.pagination?.total ?? 0
      if (total > MAX_SELECT_ALL) {
        toast.add({ severity: 'warn', summary: '已达上限', detail: `仅选中前 ${MAX_SELECT_ALL} 条，请缩小筛选范围`, life: 3000 })
        break
      }
      if (batch.length < SELECT_ALL_PAGE_SIZE || serials.length >= total) break
      pg++
    }
    return serials.slice(0, MAX_SELECT_ALL)
  } catch (e: any) {
    if (e?.name === 'AbortError') return selectedSerialNos.value
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

async function getEffectiveSerialNos(): Promise<string[]> {
  if (!selectAll.value) return selectedSerialNos.value
  const allIds = await getAllMatchingDeviceSerialNos()
  return allIds.filter(sn => !uncheckedSerialNos.value.has(sn))
}

// ==================== Import / Export ====================
const fileInput = ref<HTMLInputElement | null>(null)
const exporting = ref(false)

async function handleExport() {
  exporting.value = true
  try {
    await store.exportDevices()
    toast.add({ severity: 'success', summary: '成功', detail: '导出成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '错误', detail: e.message || '导出失败', life: 4000 })
  } finally {
    exporting.value = false
  }
}

async function handleImport(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    const result = await store.importExcel(file)
    await loadPage()
    toast.add({ severity: result.failCount > 0 ? 'error' : 'success', summary: result.failCount > 0 ? '错误' : '成功', detail: `导入完成：成功 ${result.successCount}，失败 ${result.failCount}`, life: result.failCount > 0 ? 4000 : 2000 })
  } catch (err: any) {
    toast.add({ severity: 'error', summary: '错误', detail: err.message || '导入失败', life: 4000 })
  }
  input.value = ''
}

function triggerImport() {
  fileInput.value?.click()
}

// ==================== Edit dialog ====================
const editDialogVisible = ref(false)
const editDevice = ref<Device | null>(null)
const editForm = ref({ serialNo: '', rentalStatus: '', notes: '' })
const editSaving = ref(false)

function openEditDialog(device: Device) {
  editDevice.value = device
  editForm.value = {
    serialNo: device.serialNo,
    rentalStatus: device.rentalStatus,
    notes: device.notes ?? '',
  }
  editDialogVisible.value = true
}

async function saveEdit() {
  if (editSaving.value || !editDevice.value) return
  editSaving.value = true
  try {
    const payload: Partial<Device> = {
      rentalStatus: editForm.value.rentalStatus,
      notes: editForm.value.notes,
    }
    // If serialNo changed, include it in the body so backend can rename
    if (editForm.value.serialNo !== editDevice.value.serialNo) {
      payload.serialNo = editForm.value.serialNo
    }
    await store.update(editDevice.value.serialNo, payload)
    editDialogVisible.value = false
    await loadPage()
    toast.add({ severity: 'success', summary: '保存成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '保存失败', detail: e.message || '未知错误', life: 4000 })
  } finally {
    editSaving.value = false
  }
}

// ==================== Helpers ====================
function warningClass(ws: string | undefined): string {
  return ws === '疑似丢失' ? 'badge-warning' : 'badge-success'
}
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-NN bar -->
    <div class="module-bar mb-2">
      <span class="module-number-label">MODULE-04</span>
      <div class="structure-line" />
      <span class="module-page-label">设备管理</span>
    </div>
    <!-- Add device form -->
    <div v-if="!tenantPreview.active" class="panel">
      <h2 class="panel-title">新增设备</h2>
      <div class="flex flex-wrap items-end gap-3">
        <div class="flex-1 min-w-[180px]">
          <label class="mono-label block mb-1">设备序列号</label>
          <InputText v-model="newSerialNo" class="w-full" placeholder="输入序列号" @keydown.enter="addDevice" />
        </div>
        <div class="w-[140px]">
          <label class="mono-label block mb-1">租赁状态</label>
          <Select v-model="newStatus" :options="statusOptions.map(s => ({ label: s, value: s }))" option-label="label" option-value="value" class="w-full" />
        </div>
        <div class="flex-1 min-w-[180px]">
          <label class="mono-label block mb-1">备注</label>
          <InputText v-model="newNotes" class="w-full" placeholder="可选备注" @keydown.enter="addDevice" />
        </div>
        <div>
          <Button severity="primary" :disabled="adding" :label="adding ? '添加中...' : '添加设备'" @click="addDevice" />
        </div>
      </div>
    </div>

    <!-- Filter toolbar -->
    <div class="panel">
      <div class="flex flex-wrap items-end gap-3">
        <div class="flex-1 min-w-[160px]">
          <label class="mono-label block mb-1">关键词搜索</label>
          <InputText v-model="filters.keyword" class="w-full" placeholder="序列号 / 备注" @keydown.enter="doSearch" />
        </div>
        <div class="w-[140px]">
          <label class="mono-label block mb-1">租赁状态</label>
          <Select v-model="filters.rentalStatus" :options="statusFilterOptions" option-label="label" option-value="value" class="w-full" />
        </div>
        <div class="w-[140px]">
          <label class="mono-label block mb-1">预警状态</label>
          <Select v-model="warningFilter" :options="wsFilterOptions" option-label="label" option-value="value" class="w-full" />
        </div>
        <div class="flex gap-2">
          <Button severity="primary" label="搜索" @click="doSearch" />
          <Button severity="secondary" label="重置" @click="resetFilters" />
        </div>
      </div>
    </div>

    <!-- Batch action card -->
    <Transition name="batch-panel">
    <div v-if="selectedCount > 0 && !tenantPreview.active" class="panel batch-card overflow-hidden">
      <div class="flex items-center justify-between mb-3">
        <h3 class="panel-title mb-0">批量操作</h3>
        <span class="mono-label">已选 {{ selectedCount }} 个设备</span>
      </div>
      <div class="flex flex-wrap items-end gap-3">
        <!-- 修改租赁状态 -->
        <div class="w-[150px]">
          <label class="mono-label block mb-1">修改租赁状态</label>
          <Select v-model="batchStatus" :options="statusOptions.map(s => ({ label: s, value: s }))" option-label="label" option-value="value" placeholder="选择状态" class="w-full" />
        </div>
        <Button severity="primary" :disabled="batchUpdating || !batchStatus" :label="batchUpdating ? '应用...' : '应用'" @click="applyBatchStatus" />

        <!-- 修改备注 -->
        <div class="flex-1 min-w-[200px]">
          <label class="mono-label block mb-1">修改备注</label>
          <InputText v-model="batchNotes" class="w-full" placeholder="输入新备注（覆盖现有）" @keydown.enter="applyBatchNotes" />
        </div>
        <Button severity="primary" :disabled="batchUpdating || !batchNotes.trim()" :label="batchUpdating ? '应用...' : '应用'" @click="applyBatchNotes" />

        <!-- 分隔 -->
        <div class="w-px h-8 bg-border self-center" />

        <!-- 删除 -->
        <Button severity="danger" :disabled="batchUpdating || bulkDeleting || selectAllLoading" :label="bulkDeleting ? '删除中...' : `删除 (${selectedCount})`" @click="bulkDelete" />

        <!-- 导出 -->
        <Button severity="secondary" :disabled="batchExporting || selectAllLoading" :label="batchExporting ? '导出中...' : `导出信息 (${selectedCount})`" @click="exportSelected" />
      </div>
    </div>
    </Transition>

    <!-- Device table -->
    <div class="panel table-panel">
      <div v-if="loading &amp;&amp; rows.length" class="skeleton-progress" />
      <DataTable
        v-model:selection="selectedDevices"
        :value="rows"
        dataKey="serialNo"
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
        :sortField="sortField"
        :sortOrder="sortOrder"
        removableSort
        :selectAll="selectAll"
        @page="onPageChange"
        @sort="onSort"
        @select-all-change="onSelectAllChange"
        @row-select="onRowSelect"
        @row-unselect="onRowUnselect"
      >
        <template #empty>暂无设备</template>
        <Column v-if="!tenantPreview.active" selectionMode="multiple" headerStyle="width: 3rem" />
        <Column field="serialNo" header="序列号" :min-width="140" sortable>
          <template #body="{ data }: { data: Device }">
            <div>
              <span class="font-mono">{{ data.serialNo }}</span>
              <div v-if="data.returnNode" class="text-xs text-text-muted">归还节点: {{ data.returnNode }}</div>
            </div>
          </template>
        </Column>
        <Column field="rentalStatus" header="租赁状态" :min-width="130" sortable>
          <template #body="{ data }: { data: Device }">
            <select v-if="!tenantPreview.active" :value="data.rentalStatus" class="input text-xs py-1" @change="saveStatus(data, ($event.target as HTMLSelectElement).value)">
              <option v-for="s in statusOptions" :key="s" :value="s">{{ s }}</option>
            </select>
            <span v-else class="badge-info">{{ data.rentalStatus }}</span>
          </template>
        </Column>
        <Column field="warningStatus" header="预警状态" :min-width="120">
          <template #body="{ data }: { data: Device }">
            <Tag :severity="data.warningStatus === '疑似丢失' ? 'warn' : 'success'" :value="data.warningStatus || '正常'" />
            <div v-if="data.warningReason" class="text-xs text-text-muted mt-1">{{ data.warningReason }}</div>
          </template>
        </Column>
        <Column header="备注" :min-width="150">
          <template #body="{ data }: { data: Device }">
            <input v-if="!tenantPreview.active" v-model="editNotes[data.serialNo]" type="text" class="input text-xs py-1 px-2 w-full" placeholder="备注" @focus="initEditNotes(data)" @blur="saveNotes(data)" @keydown.enter="($event.target as HTMLInputElement).blur()" />
            <span v-else>{{ data.notes || '-' }}</span>
          </template>
        </Column>
        <Column v-if="!tenantPreview.active" header="操作" :min-width="140">
          <template #body="{ data }: { data: Device }">
            <div class="flex gap-2">
              <Button severity="secondary" label="编辑" @click="openEditDialog(data)" />
              <Button severity="danger" label="删除" @click="deleteDevice(data.serialNo)" />
            </div>
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Bottom action bar -->
    <div v-if="!tenantPreview.active" class="panel">
      <div class="flex flex-wrap items-center gap-3">
        <input ref="fileInput" type="file" accept=".xlsx,.xls,.csv" class="hidden" @change="handleImport" />
        <Button severity="secondary" label="导入Excel" @click="triggerImport" />
        <Button severity="secondary" :disabled="exporting" :label="exporting ? '导出中...' : '导出Excel'" @click="handleExport" />
        <Button v-if="selectAllLoading" severity="secondary" label="取消加载" @click="cancelSelectAll" />
        <span v-if="selectAllLoading" class="font-mono text-xs text-text-muted">正在获取所有匹配设备...</span>
        <Button severity="danger" :disabled="(!selectAll && selectedSerialNos.length === 0) || bulkDeleting || selectAllLoading" :label="bulkDeleting ? '删除中...' : `批量删除 (${selectedCount})`" @click="bulkDelete" />
      </div>
    </div>

    <!-- Edit dialog -->
    <Dialog
      v-if="!tenantPreview.active"
      v-model:visible="editDialogVisible"
      modal
      header="编辑设备"
      :style="{ width: '28rem' }"
    >
      <div v-if="editDevice" class="space-y-3">
        <div>
          <label class="mono-label block mb-1">序列号</label>
          <InputText v-model="editForm.serialNo" class="w-full" />
        </div>
        <div>
          <label class="mono-label block mb-1">租赁状态</label>
          <Select
            v-model="editForm.rentalStatus"
            :options="statusOptions.map(s => ({ label: s, value: s }))"
            option-label="label"
            option-value="value"
            class="w-full"
          />
        </div>
        <div>
          <label class="mono-label block mb-1">备注</label>
          <Textarea v-model="editForm.notes" :rows="3" class="w-full" placeholder="备注" />
        </div>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="editSaving" @click="editDialogVisible = false" />
        <Button severity="primary" :label="editSaving ? '保存中...' : '保存'" :disabled="editSaving" @click="saveEdit" />
      </template>
    </Dialog>
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
