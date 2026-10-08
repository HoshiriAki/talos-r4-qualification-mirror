<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuditStore } from '@/stores/audit'
import * as auditApi from '@/api/audit'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import { batchOrderNos } from '@/api/orders'
import { useToast } from 'primevue/usetoast'
import type { AuditLog } from '@/api/audit'
import { formatTime } from '@/composables/useFormatTime'
import DataTable, { type DataTableSortEvent } from 'primevue/datatable'

const audit = useAuditStore()
const toast = useToast()
const tenantPreview = useTenantPreviewStore()

function toDateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

function toDate(s: string | undefined): Date | null {
  return s ? new Date(s + 'T00:00:00') : null
}

// Date filter bridge: string <-> Date
const filterDate = computed<Date | null>({
  get: () => toDate(audit.filters.date),
  set: (val: Date | null) => {
    audit.filters.date = val ? toDateKey(val) : ''
  },
})
watch(filterDate, () => { if (filterDate.value) handleQuery() })

// ==================== Selection ====================
const selectedLogs = ref<AuditLog[]>([])
const selectAll = ref(false)
const uncheckedIds = ref<Set<string>>(new Set())
const selectedCount = computed(() => selectAll.value ? total.value - uncheckedIds.value.size : selectedLogs.value.length)

// ==================== Local pagination state ====================
const rows = ref<AuditLog[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(10)
const loading = ref(false)
const sortField = ref<string>()
const sortOrder = ref<1 | -1>()

async function loadPage(p?: number, ps?: number) {
  const pg = p ?? page.value
  const pz = ps ?? pageSize.value
  const offset = (pg - 1) * pz
  loading.value = true
  try {
    const result = await auditApi.fetchLogs({
      ...audit.filters,
      offset,
      limit: pz,
      sortBy: sortField.value,
      sortOrder: sortOrder.value != null ? (sortOrder.value === 1 ? 'asc' : 'desc') : undefined,
    })
    const newRows = result.logs || [] as AuditLog[]
    // Merge selection before setting rows so DataTable sees both together
    if (selectAll.value) {
      const existingIds = new Set(selectedLogs.value.map(l => l.id))
      const toAdd = newRows.filter(row => !existingIds.has(row.id) && !uncheckedIds.value.has(row.id))
      selectedLogs.value = [...selectedLogs.value, ...toAdd]
    }
    rows.value = newRows
    total.value = result.total || 0
    page.value = pg
    pageSize.value = pz
  } finally {
    loading.value = false
  }
}

const actionTypeLabels: Record<string, string> = {
  'auth_login': '登录',
  'auth_logout': '退出',
  'auth_change_password': '修改密码',
  'order_create': '创建订单',
  'order_update': '编辑订单',
  'order_delete': '删除订单',
  'order_attach_device': '关联设备',
  'order_detach_device': '移除设备关联',
  'device_create': '新增设备',
  'device_update_status': '更新设备状态',
  'device_update_notes': '更新设备备注',
  'device_delete': '删除设备',
  'device_bulk_update': '批量修改设备',
  'device_bulk_delete': '批量删除设备',
  'device_checkin_scan': '扫码入库',
  'device_import_excel': '导入设备',
  'order_import_devices': '导入设备关联',
  'order_import_orders': '导入订单',
  'order_import_notes': '导入备注',
  'pricing_config_update': '更新价格配置',
  'tenant_member_create': '创建员工',
  'tenant_member_reset_password': '重置密码',
  'tenant_member_toggle_enabled': '切换状态',
  'tenant_member_delete': '删除员工',
  'tenant_member_rename': '修改用户名',
  'order_batch_ship': '批量发货',
}

function formatActionType(type: string): string {
  return actionTypeLabels[type] || type
}

function formatSummary(actionType: string, detail: any): string {
  if (!detail) return '-'
  const d = typeof detail === 'object' ? detail : {}
  switch (actionType) {
    case 'auth_login':
      return `以 ${d.role === 'admin' ? '管理员' : '员工'} 身份登录`
    case 'auth_logout':
      return '退出登录'
    case 'auth_change_password':
      return '修改密码'
    case 'order_create':
      return d.orderNo ? `创建订单 #${d.orderNo}` : '创建订单'
    case 'order_update': {
      const ono = d.orderNo || d.before?.orderNo || d.after?.orderNo
      return ono ? `编辑订单 #${ono}` : '编辑订单'
    }
    case 'order_delete':
      return d.orderNo ? `删除订单 #${d.orderNo}` : '删除订单'
    case 'order_attach_device':
      return d.serialNo ? `关联设备 ${d.serialNo}` : '关联设备'
    case 'order_detach_device':
      return d.serialNo ? `移除设备 ${d.serialNo}` : '移除设备关联'
    case 'device_create':
      return d.serialNo ? `新增设备 ${d.serialNo}` : '新增设备'
    case 'device_update_status':
      return d.beforeStatus && d.afterStatus
        ? `状态 ${d.beforeStatus} → ${d.afterStatus}`
        : '更新设备状态'
    case 'device_update_notes':
      return '更新设备备注'
    case 'device_delete':
      return d.serialNo ? `删除设备 ${d.serialNo}` : '删除设备'
    case 'device_bulk_update': {
      const uc = d.updatedCount ?? 0
      const fc = d.failCount ?? 0
      const tc = d.totalCount ?? 0
      const act = d.updates?.rentalStatus ? `状态→${d.updates.rentalStatus}` : d.updates?.notes ? '备注' : '更新'
      return tc ? `批量${act} ${uc}/${tc} 成功` + (fc > 0 ? ` ${fc} 失败` : '') : '批量更新设备'
    }
    case 'device_bulk_delete': {
      const sc = d.successCount ?? 0
      const fc = d.failCount ?? 0
      const tc = d.totalCount ?? 0
      return tc ? `批量删除设备 ${sc}/${tc} 成功` + (fc > 0 ? ` ${fc} 失败` : '') : '批量删除设备'
    }
    case 'device_checkin_scan': {
      const act = d.action === 'not_found' ? '未找到' : d.action === 'already_in_stock' ? '已在库' : d.action === 'checked_in' ? '入库' : d.action || '扫描'
      return d.serialNo ? `扫码${act} ${d.serialNo}` : `扫码${act}`
    }
    case 'device_import_excel': {
      const s = d.successCount ?? 0
      const t = d.totalRows ?? 0
      return t ? `导入设备 ${s}/${t} 成功` : '导入设备'
    }
    case 'order_import_devices': {
      const s = d.successCount ?? 0
      const t = d.totalRows ?? 0
      return d.batchRejected ? `导入设备关联失败 (${d.failCount ?? 0})` : `导入设备关联 ${s}/${t} 成功`
    }
    case 'order_import_orders': {
      const s = d.successCount ?? 0
      const t = d.totalRows ?? 0
      return t ? `导入订单 ${s}/${t} 成功` : '导入订单'
    }
    case 'order_import_notes': {
      const s = d.successCount ?? 0
      const t = d.totalRows ?? 0
      return t ? `导入备注 ${s}/${t} 成功` : '导入备注'
    }
    case 'pricing_config_update':
      return '更新价格配置'
    case 'tenant_member_create':
      return d.targetUsername ? `创建员工 ${d.targetUsername}` : '创建员工'
    case 'tenant_member_reset_password':
      return d.targetUsername ? `重置 ${d.targetUsername} 密码` : '重置密码'
    case 'tenant_member_toggle_enabled':
      return d.targetUsername ? `${d.isEnabled ? '启用' : '禁用'}员工 ${d.targetUsername}` : '切换员工状态'
    case 'tenant_member_delete':
      return d.targetUsername ? `删除员工 ${d.targetUsername}` : '删除员工'
    case 'tenant_member_rename':
      return d.targetUsername && d.newUsername
        ? `员工改名 ${d.targetUsername} → ${d.newUsername}`
        : d.newUsername ? `员工改名 → ${d.newUsername}` : '修改用户名'
    case 'order_batch_ship':
      return `批量发货 ${d.successCount ?? 0}/${d.totalOrders ?? 0} 成功`
    default:
      return d.orderNo ? `#${d.orderNo}` : d.serialNo ? d.serialNo : d.targetUsername || actionType
  }
}


// Narrative detail sections — human-readable per actionType
interface DetailSection {
  type: 'header' | 'body' | 'highlight' | 'table'
  content?: string
  items?: { label: string; value: string; ok?: boolean }[]
}

const detailSections = computed((): DetailSection[] => {
  const log = detailTarget.value
  if (!log) return []
  const d = (log.detail || {}) as Record<string, unknown>
  const sections: DetailSection[] = []

  switch (log.actionType) {
    case 'auth_login':
      sections.push(
        { type: 'header', content: '用户登录' },
        { type: 'body', content: (d.username || '未知用户') + ' 以 ' + (d.role === 'admin' ? '管理员' : '员工') + ' 身份登录系统' }
      )
      break
    case 'auth_logout':
      sections.push(
        { type: 'header', content: '用户登出' },
        { type: 'body', content: (d.username || '未知用户') + ' 退出登录' }
      )
      break
    case 'auth_change_password':
      sections.push(
        { type: 'header', content: '修改密码' },
        { type: 'body', content: (d.username || '未知用户') + ' 修改了自己的登录密码' }
      )
      break
    case 'profile_update':
      sections.push(
        { type: 'header', content: '更新个人资料' },
        { type: 'body', content: formatFieldChanges(d.before as Record<string,unknown>|undefined, d.after as Record<string,unknown>|undefined, ['displayName','email','phone']) }
      )
      break
    case 'order_create':
      sections.push(
        { type: 'header', content: '创建订单 #' + (d.orderNo || '-') },
        { type: 'body', content: [
          '客户：' + (d.name || '-'),
          '省份：' + (d.province || '-'),
          '租期：' + (d.startDate || '-') + ' 至 ' + (d.endDate || '-'),
          '总价：' + Number(d.totalPrice || 0).toFixed(2),
          '设备数：' + (d.deviceCount || 0) + ' 台',
        ].join('  ·  ') }
      )
      break
    case 'order_update':
      sections.push(
        { type: 'header', content: '编辑订单 #' + (d.orderNo || (d.before as any)?.orderNo || '-') },
        { type: 'body', content: formatFieldChanges(d.before as Record<string,unknown>|undefined, d.after as Record<string,unknown>|undefined, ['startDate','endDate','deliveryDate','address','notes','pickupMethods']) }
      )
      break
    case 'order_delete':
      sections.push(
        { type: 'header', content: '删除订单 No.' + (d.orderNo || '-') },
        { type: 'highlight', content: '此操作不可撤销' },
        { type: 'body', content: '订单 #' + (d.orderNo || '-') + ' 已被永久删除' }
      )
      break
    case 'order_attach_device':
      sections.push(
        { type: 'header', content: '关联设备到订单' },
        { type: 'body', content: '设备 ' + (d.serialNo || '-') + ' 已关联到订单 No.' + (d.orderNo || '-') }
      )
      break
    case 'order_detach_device':
      sections.push(
        { type: 'header', content: '从订单移除设备' },
        { type: 'body', content: '设备 ' + (d.serialNo || '-') + ' 已从订单 No.' + (d.orderNo || '-') + ' 移除' }
      )
      break
    case 'device_create':
      sections.push(
        { type: 'header', content: '新增设备' },
        { type: 'body', content: '序列号：' + (d.serialNo || '-') + ' · 型号：' + (d.modelName || d.modelId || '-') + ' · 状态：' + (d.initialStatus || '已入库') }
      )
      break
    case 'device_update_status':
      sections.push(
        { type: 'header', content: '设备 ' + (d.serialNo || '-') + ' 状态变更' },
        { type: 'body', content: '从 「' + (d.beforeStatus || '?') + '」 变更到 「' + (d.afterStatus || '?') + '」' + (d.reason ? '。原因：' + d.reason : '') }
      )
      break
    case 'device_update_notes':
      sections.push(
        { type: 'header', content: '设备 ' + (d.serialNo || '-') + ' 备注更新' },
        { type: 'body', content: '「' + (d.oldNotes || '(空)') + '」 → 「' + (d.newNotes || '(空)') + '」' }
      )
      break
    case 'device_delete':
      sections.push(
        { type: 'header', content: '删除设备 ' + (d.serialNo || '-') },
        { type: 'highlight', content: '此操作不可撤销' }
      )
      break
    case 'device_bulk_update':
      sections.push(
        { type: 'header', content: '批量更新设备（' + (d.totalCount || 0) + ' 台）' },
        { type: 'table', items: [
          { label: '成功', value: (d.updatedCount || 0) + ' 台', ok: true },
          { label: '失败', value: (d.failCount || 0) + ' 台', ok: !(Number(d.failCount) > 0) },
        ]}
      )
      if (d.serialNos && Array.isArray(d.serialNos)) {
        sections.push({ type: 'body', content: '序列号：' + (d.serialNos as string[]).slice(0, 20).join('、') + ((d.serialNos as string[]).length > 20 ? ' ...等' + (d.serialNos as string[]).length + '台' : '') })
      }
      break
    case 'device_bulk_delete':
      sections.push(
        { type: 'header', content: '批量删除设备（' + (d.totalCount || 0) + ' 台）' },
        { type: 'highlight', content: '此操作不可撤销' },
        { type: 'table', items: [
          { label: '成功删除', value: (d.successCount || 0) + ' 台', ok: true },
          { label: '删除失败', value: (d.failCount || 0) + ' 台', ok: !(Number(d.failCount) > 0) },
        ]}
      )
      break
    case 'device_checkin_scan':
      sections.push(
        { type: 'header', content: '扫码入库：' + (d.serialNo || '-') },
        { type: 'body', content: d.action === 'checked_in' ? '设备已成功入库。先前状态：「' + (d.previousStatus || '?') + '」' : d.action === 'already_in_stock' ? '设备已在库中，无需重复入库' : d.action === 'not_found' ? '设备未在系统中找到' : (d.serialNo || '-') + '：' + (d.action || '扫描') }
      )
      break
    case 'device_import_excel':
      sections.push(
        { type: 'header', content: 'Excel 导入设备' },
        { type: 'body', content: '共 ' + (d.totalRows || 0) + ' 行，成功 ' + (d.successCount || 0) + '，失败 ' + (d.failCount || 0) }
      )
      break
    case 'order_import_orders':
      sections.push(
        { type: 'header', content: 'Excel 导入订单' },
        { type: 'body', content: '共 ' + (d.totalRows || 0) + ' 行，成功 ' + (d.successCount || 0) + '，失败 ' + (d.failCount || 0) }
      )
      break
    case 'order_import_devices':
      sections.push(
        { type: 'header', content: 'Excel 导入设备关联' },
        { type: 'body', content: '共 ' + (d.totalRows || 0) + ' 行，成功 ' + (d.successCount || 0) + '，失败 ' + (d.failCount || 0) }
      )
      break
    case 'order_import_notes':
      sections.push(
        { type: 'header', content: 'Excel 导入备注' },
        { type: 'body', content: '共 ' + (d.totalRows || 0) + ' 行，成功 ' + (d.successCount || 0) + '，失败 ' + (d.failCount || 0) }
      )
      break
    case 'order_batch_ship':
      sections.push(
        { type: 'header', content: '批量发货（' + (d.totalOrders || 0) + ' 单）' },
        { type: 'table', items: [
          { label: '成功', value: (d.successCount || 0) + ' 单', ok: true },
          { label: '失败', value: (d.failCount || 0) + ' 单', ok: !(Number(d.failCount) > 0) },
        ]}
      )
      break
    case 'pricing_config_update':
      sections.push(
        { type: 'header', content: '更新价格配置' }
      )
      break
    case 'tenant_member_create':
      sections.push(
        { type: 'header', content: '创建员工账号' },
        { type: 'body', content: '员工 「' + (d.targetUsername || '-') + '」 已被创建，角色为 ' + (d.role === 'admin' ? '管理员' : '员工') }
      )
      break
    case 'tenant_member_reset_password':
      sections.push(
        { type: 'header', content: '重置密码' },
        { type: 'body', content: '重置了员工 「' + (d.targetUsername || '-') + '」 的登录密码' }
      )
      break
    case 'tenant_member_toggle_enabled':
      sections.push(
        { type: 'header', content: (d.isEnabled ? '启用' : '禁用') + '员工账号' },
        { type: 'body', content: '员工 「' + (d.targetUsername || '-') + '」 已被' + (d.isEnabled ? '启用' : '禁用') }
      )
      break
    case 'tenant_member_delete':
      sections.push(
        { type: 'header', content: '删除员工账号' },
        { type: 'highlight', content: '此操作不可撤销' }
      )
      break
    case 'tenant_member_rename':
      sections.push(
        { type: 'header', content: '修改员工用户名' },
        { type: 'body', content: '「' + (d.targetUsername || '-') + '」 → 「' + (d.newUsername || '-') + '」' }
      )
      break
    default:
      sections.push(
        { type: 'header', content: log.actionType || '未知操作' },
        { type: 'body', content: '（无额外信息）' }
      )
  }
  return sections
})

function formatFieldChanges(
  before: Record<string, unknown> | undefined,
  after: Record<string, unknown> | undefined,
  fields: string[]
): string {
  const labels: Record<string, string> = {
    displayName: '显示名称', email: '邮箱', phone: '电话',
    startDate: '开始日期', endDate: '结束日期', deliveryDate: '发货日期',
    address: '地址', notes: '备注', pickupMethods: '取货方式', province: '省份',
  }
  const changes: string[] = []
  for (const f of fields) {
    const ov = before?.[f]; const nv = after?.[f]
    if (ov !== nv) changes.push((labels[f]||f) + '：「' + (ov || '(空)') + '」→「' + (nv || '(空)') + '」')
  }
  return changes.length > 0 ? changes.join('\n') : '未检测到字段变更'
}

// Detail modal
const detailModalOpen = ref(false)
const detailTarget = ref<AuditLog | null>(null)
const orderNoCache = ref<Record<string, string>>({})

async function openDetail(log: AuditLog) {
  detailTarget.value = log
  detailModalOpen.value = true

  // 批量发货：补齐缺失的 orderNo
  if (!tenantPreview.active && log.actionType === 'order_batch_ship' && log.detail?.results) {
    const missing = (log.detail.results as Array<{ orderId: string; orderNo?: string }>)
      .filter(r => !r.orderNo && r.orderId)
      .map(r => r.orderId)
    const unique = [...new Set(missing)]
    if (unique.length > 0) {
      const fetched = await batchOrderNos(unique)
      orderNoCache.value = { ...orderNoCache.value, ...fetched }
    }
  }
}

function closeDetail() {
  detailModalOpen.value = false
  detailTarget.value = null
  orderNoCache.value = {}
}

const detailJson = computed(() => {
  if (!detailTarget.value) return ''
  const d = detailTarget.value.detail
  if (!d) return '{}'
  return typeof d === 'object' ? JSON.stringify(d, null, 2) : String(d)
})

const batchSerialNos = computed(() => {
  const d = detailTarget.value?.detail
  if (!d || typeof d !== 'object') return null
  const sns = (d as any).serialNos
  return Array.isArray(sns) && sns.length > 0 ? (sns as string[]) : null
})

const batchActionType = computed(() => {
  return detailTarget.value?.actionType === 'device_bulk_update' || detailTarget.value?.actionType === 'device_bulk_delete'
})

const batchShipResults = computed(() => {
  const d = detailTarget.value?.detail
  if (!d || typeof d !== 'object' || detailTarget.value?.actionType !== 'order_batch_ship') return null
  const results = (d as any).results
  return Array.isArray(results) && results.length > 0 ? (results as Array<{ orderId: string; orderNo: string; ok: boolean; error: string; linkedDevices: string[] }>) : null
})

const collapsibleOpen = ref(false)   // serial numbers collapsible
const shipCollapsibleOpen = ref(false)   // batch ship results collapsible

const detailEntries = computed(() => {
  if (!detailTarget.value?.detail) return []
  const d = detailTarget.value.detail
  if (typeof d === 'object' && d !== null && !Array.isArray(d)) {
    return Object.entries(d as Record<string, unknown>)
      .filter(([k]) => k !== 'serialNos' && k !== 'results')
      .map(([k, v]) => {
        let display = ''
        if (k === 'updates' && v && typeof v === 'object') {
          const parts = Object.entries(v as Record<string, unknown>)
            .filter(([, val]) => val !== '' && val !== undefined)
            .map(([kk, vv]) => `${kk}: ${vv}`)
          display = parts.length > 0 ? parts.join('，') : String(v)
        } else if (k === 'failedItems' && Array.isArray(v)) {
          display = (v as any[]).length === 0 ? '无' : (v as any[]).map((it: any) => `${it.serialNo || ''}(${it.reason || ''})`).join('，')
        } else {
          display = typeof v === 'object' ? JSON.stringify(v) : String(v)
        }
        return { key: k, value: display }
      })
  }
  return []
})

const activeFilterCount = computed(() => {
  let count = 0
  if (audit.filters.actionType) count++
  if (audit.filters.entityType) count++
  if (audit.filters.actorUsername) count++
  if (audit.filters.date) count++
  if (audit.filters.keyword) count++
  return count
})

function clearAllFilters() {
  audit.resetFilters()
  filterDate.value = null
}

// Initial load
onMounted(async () => {
  try {
    await loadPage()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
  }
})

async function handleQuery() {
  await loadPage(1)
}

function onPage(event: { page: number; rows: number }) {
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
    uncheckedIds.value = new Set()
    selectedLogs.value = [...rows.value]
  } else {
    selectedLogs.value = []
    uncheckedIds.value = new Set()
  }
}

function onRowSelect(event: { data: AuditLog }) {
  if (selectAll.value && event.data?.id) {
    uncheckedIds.value.delete(event.data.id)
  }
}

function onRowUnselect(event: { data: AuditLog }) {
  if (selectAll.value && event.data?.id) {
    uncheckedIds.value.add(event.data.id)
  }
}

function handleReset() {
  audit.resetFilters()
  loadPage(1).catch(e => toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 }))
}
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-NN bar -->
    <div class="module-bar mb-2">
      <span class="module-number-label">MODULE-07</span>
      <div class="structure-line" />
      <span class="module-page-label">操作日志</span>
    </div>
    <div class="panel">
      <div class="flex items-center justify-between">
        <h2 class="panel-title mb-0">操作日志</h2>
        <span class="mono-label">
          共 {{ total }} 条
          <template v-if="selectedCount > 0"> / 已选 {{ selectedCount }} 条</template>
        </span>
      </div>

      <!-- Filter Bar -->
      <div class="mb-4 space-y-3">
        <div class="flex items-center gap-2">
          <span class="mono-label">筛选条件</span>
          <span v-if="activeFilterCount > 0" class="badge inline-flex items-center justify-center font-mono text-2xs px-1.5 h-4 min-w-4 bg-accent text-base">{{ activeFilterCount }}</span>
        </div>
        <div class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3">
          <div>
            <label class="mono-label block mb-1">操作类型</label>
            <Select
              v-model="audit.filters.actionType"
              :options="Object.entries(actionTypeLabels).map(([k, v]) => ({ label: v, value: k }))"
              option-label="label"
              option-value="value"
              placeholder="全部操作"
              class="w-full"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">实体类型</label>
            <InputText
              v-model="audit.filters.entityType"
              class="w-full"
              placeholder="例如：order"
              @keydown.enter="handleQuery"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">操作人</label>
            <InputText
              v-model="audit.filters.actorUsername"
              class="w-full"
              placeholder="用户名"
              @keydown.enter="handleQuery"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">日期</label>
            <DatePicker
              v-model="filterDate"
              show-icon
              class="w-full"
              date-format="yy-mm-dd"
              @date-select="handleQuery"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">关键词</label>
            <InputText
              v-model="audit.filters.keyword"
              class="w-full"
              placeholder="搜索详情"
              @keydown.enter="handleQuery"
            />
          </div>
        </div>
        <div class="flex items-center gap-2">
          <Button severity="primary" :disabled="loading" :label="loading ? '查询中...' : '查询'" @click="handleQuery" />
          <Button v-if="activeFilterCount > 0" severity="secondary" label="清除筛选" @click="clearAllFilters" />
          <Button severity="secondary" label="重置" @click="handleReset" />
        </div>
      </div>

      <!-- Paginated DataTable -->
      <div v-if="loading &amp;&amp; rows.length" class="skeleton-progress" />
      <DataTable
        v-model:selection="selectedLogs"
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
        :sortField="sortField"
        :sortOrder="sortOrder"
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
            <span class="font-mono text-xs text-text-muted">暂无数据</span>
          </div>
        </template>
        <Column selectionMode="multiple" headerStyle="width: 3rem" />
        <Column field="createdAt" header="时间" sortable>
          <template #body="{ data }: { data: AuditLog }">
            <span v-if="data" class="font-mono text-xs">{{ formatTime(data.createdAt, true) }}</span>
            <Skeleton v-else width="100%" height="1rem" />
          </template>
        </Column>
        <Column field="actorUsername" header="操作人" sortable>
          <template #body="{ data }: { data: AuditLog }">
            <template v-if="data">{{ data.actorUsername || '-' }}</template>
            <Skeleton v-else width="60%" height="1rem" />
          </template>
        </Column>
        <Column field="actionType" header="操作类型" sortable>
          <template #body="{ data }: { data: AuditLog }">
            <span v-if="data" class="badge-info">{{ formatActionType(data.actionType) }}</span>
            <Skeleton v-else width="70%" height="1rem" />
          </template>
        </Column>
        <Column field="entityType" header="实体类型" sortable>
          <template #body="{ data }: { data: AuditLog }">
            <span v-if="data" class="font-mono text-xs">{{ data.entityType || '-' }}</span>
            <Skeleton v-else width="50%" height="1rem" />
          </template>
        </Column>
        <Column header="实体名称">
          <template #body="{ data }: { data: AuditLog }">
            <template v-if="data">{{ data.entityLabel || '-' }}</template>
            <Skeleton v-else width="80%" height="1rem" />
          </template>
        </Column>
        <Column header="操作简介">
          <template #body="{ data }: { data: AuditLog }">
            <template v-if="data">
              <span class="font-mono text-xs text-text-secondary truncate block" style="max-width: 18rem">
                {{ formatSummary(data.actionType, data.detail) }}
              </span>
            </template>
            <Skeleton v-else width="90%" height="1rem" />
          </template>
        </Column>
        <Column header="操作">
          <template #body="{ data }: { data: AuditLog }">
            <button v-if="data" class="btn-secondary text-xs px-2 py-1" @click="openDetail(data)">
              详情
            </button>
            <Skeleton v-else width="3rem" height="1.25rem" />
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Detail Modal -->
    <Dialog
      v-model:visible="detailModalOpen"
      modal
      header="操作详情"
      :style="{ width: '520px' }"
    >
      <div v-if="detailTarget" class="space-y-4">

        <template v-for="(section, si) in detailSections" :key="si">
          <!-- Header -->
          <p v-if="section.type === 'header'" class="text-sm font-bold text-text-primary border-b border-border pb-1">
            {{ section.content }}
          </p>

          <!-- Body paragraph -->
          <p v-else-if="section.type === 'body'" class="text-xs text-text-secondary whitespace-pre-line leading-relaxed">
            {{ section.content }}
          </p>

          <!-- Highlight warning -->
          <div v-else-if="section.type === 'highlight'" class="text-xs font-bold text-status-error bg-status-error/10 px-3 py-1.5">
            {{ section.content }}
          </div>

          <!-- Table -->
          <table v-else-if="section.type === 'table' && section.items" class="w-full text-xs border-collapse">
            <tr v-for="(item, ti) in section.items" :key="ti"
              :class="item.ok === true ? 'text-status-success' : item.ok === false ? 'text-status-error' : 'text-text-secondary'">
              <td class="py-1 pr-3 text-text-muted">{{ item.label }}</td>
              <td class="py-1 font-bold">{{ item.value }}</td>
            </tr>
          </table>
        </template>

        <!-- Batch: affected serial numbers -->
        <div v-if="batchSerialNos" class="border border-border">
          <button
            class="w-full text-left px-3 py-1.5 font-mono text-xs text-text-secondary hover:text-text-primary select-none flex items-center gap-1 cursor-pointer"
            @click="collapsibleOpen = !collapsibleOpen"
          >
            <span class="inline-block transition-transform duration-150" :class="{ 'rotate-90': collapsibleOpen }">▸</span>
            作用于 {{ batchSerialNos.length }} 个设备
          </button>
          <Transition name="collapse">
            <div v-if="collapsibleOpen" class="border-t border-border max-h-48 overflow-y-auto">
              <div v-for="(sn, i) in batchSerialNos" :key="i" class="px-3 py-0.5 font-mono text-xs text-text-primary border-t border-border first:border-t-0">
                {{ sn }}
              </div>
            </div>
          </Transition>
        </div>

        <!-- Batch ship: per-order result rows -->
        <div v-if="batchShipResults">
          <button
            class="mono-label w-full text-left px-3 py-1.5 border border-border cursor-pointer hover:text-text-primary select-none flex items-center gap-1"
            @click="shipCollapsibleOpen = !shipCollapsibleOpen"
          >
            <span class="text-xs" :class="{ 'rotate-90': shipCollapsibleOpen }">▸</span>
            各订单详情（{{ batchShipResults.length }} 笔）
          </button>
          <Transition name="collapse">
            <div v-if="shipCollapsibleOpen" class="border border-border border-t-0 max-h-64 overflow-y-auto">
              <table class="w-full font-mono text-xs">
                <thead>
                  <tr class="bg-base">
                    <th class="px-3 py-1 text-left text-text-muted font-normal border-b border-border">订单号</th>
                    <th class="px-3 py-1 text-left text-text-muted font-normal border-b border-border">结果</th>
                    <th class="px-3 py-1 text-left text-text-muted font-normal border-b border-border">关联设备</th>
                    <th class="px-3 py-1 text-left text-text-muted font-normal border-b border-border">错误</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="(r, i) in batchShipResults" :key="i" class="border-t border-border">
                    <td class="px-3 py-1">
                      <span class="text-text-primary">{{ r.orderNo || orderNoCache[r.orderId] || '-' }}</span>
                      <span class="block font-mono text-2xs text-text-muted mt-0.5">{{ r.orderId || '-' }}</span>
                    </td>
                    <td class="px-3 py-1">
                      <span :class="r.ok ? 'text-status-success' : 'text-status-error'">
                        {{ r.ok ? '✓ 成功' : '✗ 失败' }}
                      </span>
                    </td>
                    <td class="px-3 py-1 text-text-primary">
                      <template v-if="r.linkedDevices.length > 0">
                        <span v-for="(sn, j) in r.linkedDevices" :key="j" class="font-mono">{{ sn }}<template v-if="j < r.linkedDevices.length - 1">, </template></span>
                      </template>
                      <span v-else class="text-text-muted">-</span>
                    </td>
                    <td class="px-3 py-1 text-status-error">{{ r.error || '-' }}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </Transition>
        </div>

        <!-- Fallback: key-value table for unrecognized action types -->
        <div v-if="detailSections.length === 0 && detailEntries.length > 0" class="border border-border overflow-hidden">
          <table class="w-full font-mono text-xs">
            <tbody>
              <tr v-for="(entry, i) in detailEntries" :key="i" class="border-t border-border" :class="{ 'border-t-0': i === 0 }">
                <td class="px-3 py-1.5 text-text-muted w-1/3 bg-base align-top whitespace-nowrap">{{ entry.key }}</td>
                <td class="px-3 py-1.5 text-text-primary align-top break-all">{{ entry.value }}</td>
              </tr>
            </tbody>
          </table>
        </div>
        <pre v-else-if="detailSections.length === 0" class="bg-input border border-border p-3 font-mono text-xs text-text-primary overflow-auto max-h-96 whitespace-pre-wrap">{{ detailJson }}</pre>

        <!-- Meta footer -->
        <div class="border-t border-border pt-3 space-y-1 text-2xs text-text-muted">
          <div class="flex gap-4">
            <span>操作人：{{ detailTarget.actorUsername || '系统' }}</span>
            <span>IP：{{ detailTarget.ip || '-' }}</span>
          </div>
          <div>时间：{{ detailTarget.createdAt }}</div>
          <div v-if="detailTarget.userAgent" class="truncate opacity-50">{{ detailTarget.userAgent }}</div>
        </div>

      </div>
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
