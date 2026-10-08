<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useCreditStore } from '@/stores/credit'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { formatTime } from '@/composables/useFormatTime'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import TabBar from '@/components/common/TabBar.vue'

// ── State ────────────────────────────────────────────────────────────

const auth = useAuthStore()
const credit = useCreditStore()
const toast = useToast()
const confirm = useConfirm()

// Active tab
const activeTab = ref<'query' | 'blacklist' | 'violations'>('query')

// ── Tab 1: Credit Query ──────────────────────────────────────────────

const searchPhone = ref('')
const searchedPhone = ref('')

// Score level helpers
const scoreLevel = computed(() => {
  const s = credit.currentScore?.score ?? null
  if (s === null) return null
  if (s >= 150) return { label: '优秀', class: 'badge-success', color: 'var(--status-success)' }
  if (s >= 120) return { label: '良好', class: 'badge-info', color: 'var(--status-info)' }
  if (s >= 90) return { label: '一般', class: 'badge-warning', color: 'var(--status-warning)' }
  if (s >= 60) return { label: '较差', class: 'badge-error', color: 'var(--status-error)' }
  return { label: '差', class: 'badge-error', color: 'var(--status-error)' }
})

const isWarning = computed(() => {
  const s = credit.currentScore?.score ?? 100
  return s < 80
})

async function handleSearch() {
  const phone = searchPhone.value.trim()
  if (!phone) {
    toast.add({ severity: 'warn', summary: '请输入手机号', life: 2000 })
    return
  }
  searchedPhone.value = phone
  try {
    await credit.fetchCreditScore(phone)
    await credit.fetchCreditHistory(phone)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '查询失败', detail: e.message || '请重试', life: 4000 })
  }
}

async function handleRecalculate() {
  if (!searchedPhone.value) return
  confirm.require({
    message: `确定要重新计算 ${searchedPhone.value} 的信用评分吗？`,
    header: '重新计算',
    accept: async () => {
      try {
        await credit.recalculateCredit(searchedPhone.value)
        toast.add({ severity: 'success', summary: '信用评分已重新计算', life: 2000 })
        await credit.fetchCreditScore(searchedPhone.value)
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '重新计算失败', detail: e.message || '请重试', life: 4000 })
      }
    },
  })
}

// ── Tab 2: Blacklist ─────────────────────────────────────────────────

const blPage = ref(0)
const blRows = ref(10)
const blFilter = ref('')
const blStatusFilter = ref<'all' | 'active' | 'removed'>('all')

const blAddOpen = ref(false)
const blAddForm = ref({
  name: '',
  phone: '',
  reason: '',
  severity: 'medium' as string,
})
const blAddLoading = ref(false)
const blAddError = ref('')

const blRemoveId = ref('')
const blRemoving = ref(false)

const blTotalRecords = computed(() => credit.blacklistTotal)

async function loadBlacklist() {
  try {
    const params: Record<string, any> = {
      page: String(blPage.value + 1),
      page_size: String(blRows.value),
    }
    if (blFilter.value.trim()) params.phone = blFilter.value.trim()
    if (blStatusFilter.value !== 'all') params.status = blStatusFilter.value
    await credit.fetchBlacklist(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载黑名单失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onBlPageChange(e: { first: number; rows: number; page: number }) {
  blPage.value = e.page
  blRows.value = e.rows
  loadBlacklist()
}

function openAddBlacklist() {
  blAddForm.value = { name: '', phone: '', reason: '', severity: 'medium' }
  blAddError.value = ''
  blAddOpen.value = true
}

function closeAddBlacklist() {
  blAddOpen.value = false
}

async function handleAddBlacklist() {
  const f = blAddForm.value
  if (!f.name.trim() || !f.phone.trim()) {
    blAddError.value = '姓名和手机号不能为空'
    return
  }
  blAddLoading.value = true
  blAddError.value = ''
  try {
    await credit.addToBlacklist({
      name: f.name.trim(),
      phone: f.phone.trim(),
      reason: f.reason.trim(),
      severity: f.severity,
    })
    closeAddBlacklist()
    toast.add({ severity: 'success', summary: '已添加黑名单', life: 2000 })
    await loadBlacklist()
  } catch (e: any) {
    blAddError.value = e.message || '添加失败'
  } finally {
    blAddLoading.value = false
  }
}

function confirmRemove(record: any) {
  blRemoveId.value = record.id
  confirm.require({
    message: `确定要将 "${record.name}" 从黑名单中移除吗？`,
    header: '移除黑名单',
    accept: async () => {
      blRemoving.value = true
      try {
        await credit.removeFromBlacklist({ id: record.id })
        toast.add({ severity: 'success', summary: '已从黑名单中移除', life: 2000 })
        await loadBlacklist()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '移除失败', detail: e.message || '请重试', life: 4000 })
      } finally {
        blRemoving.value = false
        blRemoveId.value = ''
      }
    },
  })
}

// ── Tab 3: Violations ────────────────────────────────────────────────

const vlPage = ref(0)
const vlRows = ref(10)
const vlPhoneFilter = ref('')
const vlStatusFilter = ref('')
const vlTypeFilter = ref('')

const vlTotalRecords = computed(() => credit.violationsTotal)

const vlAddOpen = ref(false)
const vlAddForm = ref({
  phone: '',
  type: 'late_return' as string,
  severity: 'medium' as string,
  description: '',
  fine_amount: 0,
})
const vlAddLoading = ref(false)
const vlAddError = ref('')

const vlAppealOpen = ref(false)
const vlAppealRecord = ref<any>(null)
const vlAppealReason = ref('')
const vlAppealLoading = ref(false)
const vlAppealError = ref('')

const vlReviewOpen = ref(false)
const vlReviewRecord = ref<any>(null)
const vlReviewVerdict = ref<'upheld' | 'overturned'>('upheld')
const vlReviewNote = ref('')
const vlReviewLoading = ref(false)
const vlReviewError = ref('')

async function loadViolations() {
  try {
    const params: Record<string, any> = {
      page: String(vlPage.value + 1),
      page_size: String(vlRows.value),
    }
    if (vlPhoneFilter.value.trim()) params.phone = vlPhoneFilter.value.trim()
    if (vlStatusFilter.value) params.status = vlStatusFilter.value
    if (vlTypeFilter.value) params.type = vlTypeFilter.value
    await credit.fetchViolations(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载违规记录失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onVlPageChange(e: { first: number; rows: number; page: number }) {
  vlPage.value = e.page
  vlRows.value = e.rows
  loadViolations()
}

function openAddViolation() {
  vlAddForm.value = { phone: '', type: 'late_return', severity: 'medium', description: '', fine_amount: 0 }
  vlAddError.value = ''
  vlAddOpen.value = true
}

function closeAddViolation() {
  vlAddOpen.value = false
}

async function handleAddViolation() {
  const f = vlAddForm.value
  if (!f.phone.trim()) {
    vlAddError.value = '手机号不能为空'
    return
  }
  vlAddLoading.value = true
  vlAddError.value = ''
  try {
    await credit.recordViolation({
      phone: f.phone.trim(),
      type: f.type,
      severity: f.severity,
      description: f.description.trim(),
      fine_amount: f.fine_amount,
    })
    closeAddViolation()
    toast.add({ severity: 'success', summary: '已记录违规', life: 2000 })
    await loadViolations()
  } catch (e: any) {
    vlAddError.value = e.message || '记录失败'
  } finally {
    vlAddLoading.value = false
  }
}

function openAppeal(record: any) {
  vlAppealRecord.value = record
  vlAppealReason.value = ''
  vlAppealError.value = ''
  vlAppealOpen.value = true
}

function closeAppeal() {
  vlAppealOpen.value = false
  vlAppealRecord.value = null
}

async function handleAppeal() {
  if (!vlAppealReason.value.trim()) {
    vlAppealError.value = '请填写申诉原因'
    return
  }
  vlAppealLoading.value = true
  vlAppealError.value = ''
  try {
    await credit.appealViolation({
      id: vlAppealRecord.value.id,
      reason: vlAppealReason.value.trim(),
    })
    closeAppeal()
    toast.add({ severity: 'success', summary: '申诉已提交', life: 2000 })
    await loadViolations()
  } catch (e: any) {
    vlAppealError.value = e.message || '申诉失败'
  } finally {
    vlAppealLoading.value = false
  }
}

function openReview(record: any) {
  vlReviewRecord.value = record
  vlReviewVerdict.value = 'upheld'
  vlReviewNote.value = ''
  vlReviewError.value = ''
  vlReviewOpen.value = true
}

function closeReview() {
  vlReviewOpen.value = false
  vlReviewRecord.value = null
}

async function handleReview() {
  vlReviewLoading.value = true
  vlReviewError.value = ''
  try {
    await credit.reviewViolation({
      id: vlReviewRecord.value.id,
      verdict: vlReviewVerdict.value,
      note: vlReviewNote.value.trim(),
    })
    closeReview()
    toast.add({ severity: 'success', summary: vlReviewVerdict.value === 'upheld' ? '违规维持' : '违规已撤销', life: 2000 })
    await loadViolations()
  } catch (e: any) {
    vlReviewError.value = e.message || '审核失败'
  } finally {
    vlReviewLoading.value = false
  }
}

// ── Helpers ──────────────────────────────────────────────────────────

function severityBadge(severity: string): string {
  switch (severity) {
    case 'high': return 'badge-error'
    case 'medium': return 'badge-warning'
    case 'low': return 'badge-info'
    default: return 'badge'
  }
}

function severityLabel(severity: string): string {
  switch (severity) {
    case 'high': return '高'
    case 'medium': return '中'
    case 'low': return '低'
    default: return severity
  }
}

function statusBadge(status: string): string {
  switch (status) {
    case 'active': return 'badge-error'
    case 'removed': return 'badge-success'
    case 'pending': return 'badge-warning'
    case 'appealed': return 'badge-info'
    case 'upheld': return 'badge-error'
    case 'overturned': return 'badge-success'
    default: return 'badge'
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case 'active': return '活跃'
    case 'removed': return '已移除'
    case 'pending': return '待处理'
    case 'appealed': return '已申诉'
    case 'upheld': return '维持'
    case 'overturned': return '撤销'
    default: return status
  }
}

function violationTypeLabel(type: string): string {
  switch (type) {
    case 'late_return': return '逾期归还'
    case 'damage': return '设备损坏'
    case 'lost': return '设备丢失'
    case 'unauthorized_use': return '违规使用'
    case 'payment_default': return '付款违约'
    default: return type
  }
}

function violationTypeBadge(type: string): string {
  switch (type) {
    case 'late_return': return 'badge-warning'
    case 'damage': return 'badge-error'
    case 'lost': return 'badge-error'
    case 'unauthorized_use': return 'badge-info'
    case 'payment_default': return 'badge-error'
    default: return 'badge'
  }
}

// ── Tab definitions ──────────────────────────────────────────────────

const tabs = [
  { key: 'query' as const, label: '信用查询' },
  { key: 'blacklist' as const, label: '黑名单' },
  { key: 'violations' as const, label: '违规记录' },
]

// ── Init ─────────────────────────────────────────────────────────────

onMounted(() => {
  loadBlacklist()
  loadViolations()
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-16 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-16</span>
      <div class="structure-line" />
      <span class="module-page-label">信用管理</span>
    </div>
    <!-- Tab bar -->
    <TabBar :tabs="tabs" v-model="activeTab" />

    <!-- ── Tab 1: Credit Query ──────────────────────────────────────── -->
    <div v-if="activeTab === 'query'" class="space-y-4">
      <!-- Search -->
      <div class="panel">
        <div class="flex items-end gap-3">
          <div class="flex-1">
            <label class="mono-label block mb-1">客户手机号</label>
            <InputText
              v-model="searchPhone"
              class="w-full"
              placeholder="请输入客户手机号"
              @keydown.enter="handleSearch"
            />
          </div>
          <Button severity="primary" label="查询" :disabled="credit.loading" @click="handleSearch" />
        </div>
      </div>

      <!-- Score card -->
      <div v-if="credit.currentScore" class="panel">
        <div class="flex items-center justify-between mb-4">
          <h2 class="panel-title mb-0">信用评分</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="secondary"
            label="重新计算"
            :disabled="credit.loading"
            @click="handleRecalculate"
          />
        </div>

        <!-- Score circle -->
        <div class="flex items-center gap-6 mb-4">
          <div
            class="flex flex-col items-center justify-center border-2 p-6"
            :style="{
              borderColor: scoreLevel?.color ?? 'var(--border-active)',
              minWidth: '140px',
              minHeight: '140px',
              background: isWarning ? 'rgba(var(--status-error-rgb), 0.05)' : 'transparent',
            }"
          >
            <span class="font-mono font-bold text-3xl leading-none" :style="{ color: scoreLevel?.color ?? 'var(--text-primary)' }">
              {{ credit.currentScore.score }}
            </span>
            <span :class="['badge mt-2', scoreLevel?.class ?? 'badge']">{{ scoreLevel?.label ?? '-' }}</span>
          </div>

          <div class="grid grid-cols-2 gap-x-6 gap-y-2 text-sm font-mono">
            <span class="text-text-muted">客户姓名</span>
            <span class="text-text-primary">{{ credit.currentScore.name || '-' }}</span>
            <span class="text-text-muted">手机号</span>
            <span class="text-text-primary">{{ credit.currentScore.phone || searchedPhone }}</span>
            <span class="text-text-muted">总订单</span>
            <span class="text-text-primary">{{ credit.currentScore.total_orders ?? 0 }}</span>
            <span class="text-text-muted">按时归还</span>
            <span class="text-text-primary" style="color: var(--status-success)">{{ credit.currentScore.on_time_returns ?? 0 }}</span>
            <span class="text-text-muted">逾期次数</span>
            <span class="text-text-primary" style="color: var(--status-warning)">{{ credit.currentScore.late_returns ?? 0 }}</span>
            <span class="text-text-muted">损坏次数</span>
            <span class="text-text-primary" style="color: var(--status-error)">{{ credit.currentScore.damage_count ?? 0 }}</span>
          </div>
        </div>

        <!-- Warning banner -->
        <div v-if="isWarning" class="panel border-status-error p-3">
          <span class="font-mono text-sm text-status-error">警告：该客户信用评分较低（< 80 分），建议谨慎处理订单。</span>
        </div>
      </div>

      <!-- Empty state -->
      <div v-else-if="searchedPhone && !credit.loading" class="panel">
        <div class="py-6 text-center">
          <span class="font-mono text-sm text-text-muted">未找到该客户的信用记录</span>
        </div>
      </div>

      <!-- History -->
      <div v-if="credit.scoreHistory.length > 0" class="panel">
        <h3 class="panel-title">信用变更历史</h3>
        <table class="table">
          <thead>
            <tr>
              <th class="table-th">时间</th>
              <th class="table-th">变更</th>
              <th class="table-th">原因</th>
              <th class="table-th">变更后</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(h, i) in credit.scoreHistory" :key="i" class="table-tr">
              <td class="table-td">{{ formatTime(h.created_at || h.createdAt) }}</td>
              <td class="table-td">
                <span :style="{ color: (h.change ?? h.delta ?? 0) >= 0 ? 'var(--status-success)' : 'var(--status-error)' }">
                  {{ (h.change ?? h.delta ?? 0) >= 0 ? '+' : '' }}{{ h.change ?? h.delta ?? 0 }}
                </span>
              </td>
              <td class="table-td text-text-secondary">{{ h.reason || '-' }}</td>
              <td class="table-td">{{ h.score_after ?? h.scoreAfter ?? '-' }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <!-- ── Tab 2: Blacklist ──────────────────────────────────────── -->
    <div v-if="activeTab === 'blacklist'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">黑名单管理</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            label="添加黑名单"
            @click="openAddBlacklist"
          />
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="blFilter"
            class="w-48"
            placeholder="搜索手机号"
            @keydown.enter="blPage = 0; loadBlacklist()"
          />
          <Select
            v-model="blStatusFilter"
            :options="[
              { label: '全部状态', value: 'all' },
              { label: '活跃', value: 'active' },
              { label: '已移除', value: 'removed' },
            ]"
            option-label="label"
            option-value="value"
            class="w-32"
            @change="blPage = 0; loadBlacklist()"
          />
          <Button severity="secondary" label="查询" @click="blPage = 0; loadBlacklist()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="credit.blacklist"
          :loading="credit.loading"
          :paginator="true"
          :rows="blRows"
          :first="blPage * blRows"
          :totalRecords="blTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onBlPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无黑名单记录</span>
            </div>
          </template>
          <Column field="name" header="客户姓名" sortable />
          <Column field="phone" header="手机号" sortable />
          <Column field="reason" header="原因" />
          <Column field="severity" header="严重程度" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="severityBadge(data.severity)">{{ severityLabel(data.severity) }}</span>
            </template>
          </Column>
          <Column field="status" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="statusBadge(data.status)">{{ statusLabel(data.status) }}</span>
            </template>
          </Column>
          <Column field="created_at" header="加入时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.created_at || data.createdAt) }}</span>
            </template>
          </Column>
          <Column v-if="auth.isTenantAdmin" header="操作" headerStyle="width: 8rem">
            <template #body="{ data }: { data: any }">
              <Button
                v-if="data.status === 'active'"
                severity="danger"
                label="移除"
                :disabled="blRemoving && blRemoveId === data.id"
                @click="confirmRemove(data)"
              />
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Add Blacklist Dialog -->
      <Dialog
        v-model:visible="blAddOpen"
        modal
        header="添加黑名单"
        :style="{ width: '26rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">客户姓名</label>
            <InputText v-model="blAddForm.name" class="w-full" placeholder="请输入姓名" />
          </div>
          <div>
            <label class="mono-label block mb-1">手机号</label>
            <InputText v-model="blAddForm.phone" class="w-full" placeholder="请输入手机号" />
          </div>
          <div>
            <label class="mono-label block mb-1">原因</label>
            <Textarea v-model="blAddForm.reason" class="w-full" placeholder="添加原因" rows="2" />
          </div>
          <div>
            <label class="mono-label block mb-1">严重程度</label>
            <Select
              v-model="blAddForm.severity"
              :options="[
                { label: '低', value: 'low' },
                { label: '中', value: 'medium' },
                { label: '高', value: 'high' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <p v-if="blAddError" class="text-xs font-mono text-status-error">{{ blAddError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="blAddLoading" @click="closeAddBlacklist" />
          <Button severity="primary" :label="blAddLoading ? '添加中...' : '确认添加'" :disabled="blAddLoading" @click="handleAddBlacklist" />
        </template>
      </Dialog>
    </div>

    <!-- ── Tab 3: Violations ─────────────────────────────────────── -->
    <div v-if="activeTab === 'violations'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">违规记录</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            label="记录违规"
            @click="openAddViolation"
          />
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3 flex-wrap">
          <InputText
            v-model="vlPhoneFilter"
            class="w-36"
            placeholder="手机号"
            @keydown.enter="vlPage = 0; loadViolations()"
          />
          <Select
            v-model="vlStatusFilter"
            :options="[
              { label: '全部状态', value: '' },
              { label: '待处理', value: 'pending' },
              { label: '已申诉', value: 'appealed' },
              { label: '维持', value: 'upheld' },
              { label: '撤销', value: 'overturned' },
            ]"
            option-label="label"
            option-value="value"
            class="w-28"
            @change="vlPage = 0; loadViolations()"
          />
          <Select
            v-model="vlTypeFilter"
            :options="[
              { label: '全部类型', value: '' },
              { label: '逾期归还', value: 'late_return' },
              { label: '设备损坏', value: 'damage' },
              { label: '设备丢失', value: 'lost' },
              { label: '违规使用', value: 'unauthorized_use' },
              { label: '付款违约', value: 'payment_default' },
            ]"
            option-label="label"
            option-value="value"
            class="w-32"
            @change="vlPage = 0; loadViolations()"
          />
          <Button severity="secondary" label="查询" @click="vlPage = 0; loadViolations()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="credit.violations"
          :loading="credit.loading"
          :paginator="true"
          :rows="vlRows"
          :first="vlPage * vlRows"
          :totalRecords="vlTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onVlPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无违规记录</span>
            </div>
          </template>
          <Column field="phone" header="客户">
            <template #body="{ data }: { data: any }">
              <div class="font-mono text-sm">
                <div>{{ data.name || '-' }}</div>
                <div class="text-xs text-text-muted">{{ data.phone }}</div>
              </div>
            </template>
          </Column>
          <Column field="type" header="类型" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="violationTypeBadge(data.type)">{{ violationTypeLabel(data.type) }}</span>
            </template>
          </Column>
          <Column field="severity" header="严重程度" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="severityBadge(data.severity)">{{ severityLabel(data.severity) }}</span>
            </template>
          </Column>
          <Column field="description" header="描述">
            <template #body="{ data }: { data: any }">
              <span class="text-sm text-text-secondary">{{ data.description || '-' }}</span>
            </template>
          </Column>
          <Column field="status" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="statusBadge(data.status)">{{ statusLabel(data.status) }}</span>
            </template>
          </Column>
          <Column field="fine_amount" header="罚款金额" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono">{{ data.fine_amount ? `¥${data.fine_amount}` : '-' }}</span>
            </template>
          </Column>
          <Column field="created_at" header="时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.created_at || data.createdAt) }}</span>
            </template>
          </Column>
          <Column header="操作" headerStyle="width: 12rem">
            <template #body="{ data }: { data: any }">
              <div class="flex items-center gap-2">
                <Button
                  v-if="data.status === 'pending' || data.status === 'upheld'"
                  severity="secondary"
                  label="申诉"
                  size="small"
                  @click="openAppeal(data)"
                />
                <Button
                  v-if="auth.isTenantAdmin && data.status === 'appealed'"
                  severity="primary"
                  label="审核"
                  size="small"
                  @click="openReview(data)"
                />
              </div>
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Record Violation Dialog -->
      <Dialog
        v-model:visible="vlAddOpen"
        modal
        header="记录违规"
        :style="{ width: '28rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">客户手机号</label>
            <InputText v-model="vlAddForm.phone" class="w-full" placeholder="请输入手机号" />
          </div>
          <div>
            <label class="mono-label block mb-1">违规类型</label>
            <Select
              v-model="vlAddForm.type"
              :options="[
                { label: '逾期归还', value: 'late_return' },
                { label: '设备损坏', value: 'damage' },
                { label: '设备丢失', value: 'lost' },
                { label: '违规使用', value: 'unauthorized_use' },
                { label: '付款违约', value: 'payment_default' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">严重程度</label>
            <Select
              v-model="vlAddForm.severity"
              :options="[
                { label: '低', value: 'low' },
                { label: '中', value: 'medium' },
                { label: '高', value: 'high' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">描述</label>
            <Textarea v-model="vlAddForm.description" class="w-full" placeholder="违规描述" rows="3" />
          </div>
          <div>
            <label class="mono-label block mb-1">罚款金额 (元)</label>
            <InputNumber v-model="vlAddForm.fine_amount" class="w-full" :min="0" placeholder="0" />
          </div>
          <p v-if="vlAddError" class="text-xs font-mono text-status-error">{{ vlAddError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="vlAddLoading" @click="closeAddViolation" />
          <Button severity="primary" :label="vlAddLoading ? '记录中...' : '确认记录'" :disabled="vlAddLoading" @click="handleAddViolation" />
        </template>
      </Dialog>

      <!-- Appeal Dialog -->
      <Dialog
        v-model:visible="vlAppealOpen"
        modal
        header="提交申诉"
        :style="{ width: '26rem' }"
      >
        <div class="space-y-3" v-if="vlAppealRecord">
          <div class="text-sm font-mono">
            <span class="text-text-muted">类型：</span>
            <span>{{ violationTypeLabel(vlAppealRecord.type) }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">描述：</span>
            <span>{{ vlAppealRecord.description || '-' }}</span>
          </div>
          <div>
            <label class="mono-label block mb-1">申诉原因</label>
            <Textarea v-model="vlAppealReason" class="w-full" placeholder="请描述申诉原因" rows="3" />
          </div>
          <p v-if="vlAppealError" class="text-xs font-mono text-status-error">{{ vlAppealError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="vlAppealLoading" @click="closeAppeal" />
          <Button severity="primary" :label="vlAppealLoading ? '提交中...' : '提交申诉'" :disabled="vlAppealLoading" @click="handleAppeal" />
        </template>
      </Dialog>

      <!-- Review Dialog -->
      <Dialog
        v-model:visible="vlReviewOpen"
        modal
        header="审核违规"
        :style="{ width: '26rem' }"
      >
        <div class="space-y-3" v-if="vlReviewRecord">
          <div class="text-sm font-mono">
            <span class="text-text-muted">类型：</span>
            <span>{{ violationTypeLabel(vlReviewRecord.type) }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">描述：</span>
            <span>{{ vlReviewRecord.description || '-' }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">申诉原因：</span>
            <span>{{ vlReviewRecord.appeal_reason || '-' }}</span>
          </div>
          <div>
            <label class="mono-label block mb-1">审核结果</label>
            <Select
              v-model="vlReviewVerdict"
              :options="[
                { label: '维持违规', value: 'upheld' },
                { label: '撤销违规', value: 'overturned' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">审核备注</label>
            <Textarea v-model="vlReviewNote" class="w-full" placeholder="审核备注" rows="2" />
          </div>
          <p v-if="vlReviewError" class="text-xs font-mono text-status-error">{{ vlReviewError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="vlReviewLoading" @click="closeReview" />
          <Button severity="primary" :label="vlReviewLoading ? '审核中...' : '确认审核'" :disabled="vlReviewLoading" @click="handleReview" />
        </template>
      </Dialog>
    </div>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>

<style scoped>
.panel {
  padding: 1rem;
}
</style>
