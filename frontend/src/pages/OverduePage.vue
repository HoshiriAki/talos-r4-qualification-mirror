<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useOverdueStore } from '@/stores/overdue'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { formatTime } from '@/composables/useFormatTime'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import TabBar from '@/components/common/TabBar.vue'

// ── State ────────────────────────────────────────────────────────────────

const auth = useAuthStore()
const overdue = useOverdueStore()
const toast = useToast()
const confirm = useConfirm()

const activeTab = ref<'stats' | 'list' | 'config' | 'actions'>('stats')

// ── Tab 1: Stats ─────────────────────────────────────────────────────────

const statsCards = computed(() => {
  if (!overdue.stats) return []
  const s = overdue.stats
  return [
    { label: '活跃逾期数', value: s.active_count ?? s.activeCount ?? 0, color: 'var(--status-warning)' },
    { label: '总逾期金额', value: s.total_fee ? `¥${s.total_fee}` : (s.totalFee ? `¥${s.totalFee}` : '¥0'), color: 'var(--status-error)' },
    { label: 'D+1 通知', value: s.d1_count ?? s.d1Count ?? 0, color: 'var(--status-info)' },
    { label: 'D+3 通知', value: s.d3_count ?? s.d3Count ?? 0, color: 'var(--status-warning)' },
    { label: 'D+7 通知', value: s.d7_count ?? s.d7Count ?? 0, color: 'var(--status-error)' },
    { label: '已豁免', value: s.waived_count ?? s.waivedCount ?? 0, color: 'var(--status-success)' },
  ]
})

// ── Tab 2: List ──────────────────────────────────────────────────────────

const listPage = ref(0)
const listRows = ref(10)
const listStatusFilter = ref('')
const listPhoneFilter = ref('')

const listTotalRecords = computed(() => overdue.recordsTotal)

async function loadList() {
  try {
    const params: Record<string, any> = {
      page: String(listPage.value + 1),
      page_size: String(listRows.value),
    }
    if (listStatusFilter.value) params.status = listStatusFilter.value
    if (listPhoneFilter.value.trim()) params.phone = listPhoneFilter.value.trim()
    await overdue.fetchList(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载逾期列表失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onListPageChange(e: { first: number; rows: number; page: number }) {
  listPage.value = e.page
  listRows.value = e.rows
  loadList()
}

// ── Calc dialog ──────────────────────────────────────────────────────────

const calcOpen = ref(false)
const calcRecord = ref<any>(null)
const calcResult = ref<any>(null)
const calcLoading = ref(false)
const calcError = ref('')

function openCalc(record: any) {
  calcRecord.value = record
  calcResult.value = null
  calcError.value = ''
  calcOpen.value = true
}

function closeCalc() {
  calcOpen.value = false
  calcRecord.value = null
  calcResult.value = null
}

async function handleCalc() {
  if (!calcRecord.value) return
  calcLoading.value = true
  calcError.value = ''
  try {
    const data = await overdue.calcFee({
      order_id: calcRecord.value.order_id || calcRecord.value.orderId,
      phone: calcRecord.value.phone,
    })
    calcResult.value = data
  } catch (e: any) {
    calcError.value = e.message || '计算失败'
  } finally {
    calcLoading.value = false
  }
}

async function handleApplyFee() {
  if (!calcRecord.value || !calcResult.value) return
  confirm.require({
    message: `确定要为订单 ${calcRecord.value.order_no || calcRecord.value.orderNo || calcRecord.value.order_id} 应用费用 ¥${calcResult.value.total_fee ?? calcResult.value.totalFee ?? 0} 吗？`,
    header: '确认应用费用',
    accept: async () => {
      try {
        await overdue.applyFee({
          order_id: calcRecord.value.order_id || calcRecord.value.orderId,
          total_fee: calcResult.value.total_fee ?? calcResult.value.totalFee,
        })
        toast.add({ severity: 'success', summary: '逾期费用已应用', life: 2000 })
        closeCalc()
        await loadList()
        await overdue.fetchStats()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '应用失败', detail: e.message || '请重试', life: 4000 })
      }
    },
  })
}

// ── Waive ────────────────────────────────────────────────────────────────

const waiveOpen = ref(false)
const waiveRecord = ref<any>(null)
const waiveReason = ref('')
const waiveLoading = ref(false)
const waiveError = ref('')

function openWaive(record: any) {
  waiveRecord.value = record
  waiveReason.value = ''
  waiveError.value = ''
  waiveOpen.value = true
}

function closeWaive() {
  waiveOpen.value = false
  waiveRecord.value = null
}

async function handleWaive() {
  if (!waiveReason.value.trim()) {
    waiveError.value = '请填写豁免原因'
    return
  }
  waiveLoading.value = true
  waiveError.value = ''
  try {
    await overdue.waiveFee({
      order_id: waiveRecord.value.order_id || waiveRecord.value.orderId,
      reason: waiveReason.value.trim(),
    })
    closeWaive()
    toast.add({ severity: 'success', summary: '费用已豁免', life: 2000 })
    await loadList()
    await overdue.fetchStats()
  } catch (e: any) {
    waiveError.value = e.message || '豁免失败'
  } finally {
    waiveLoading.value = false
  }
}

// ── Escalation history ───────────────────────────────────────────────────

const escOpen = ref(false)
const escRecord = ref<any>(null)
const escHistory = ref<any[]>([])
const escLoading = ref(false)

async function openEscalationHistory(record: any) {
  escRecord.value = record
  escHistory.value = []
  escOpen.value = true
  escLoading.value = true
  try {
    const data = await overdue.fetchEscalationHistory({
      order_id: record.order_id || record.orderId,
    })
    escHistory.value = data.history || data.records || data || []
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '获取升级历史失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    escLoading.value = false
  }
}

function closeEscalationHistory() {
  escOpen.value = false
  escRecord.value = null
}

// ── Tab 3: Config ────────────────────────────────────────────────────────

const cfgOpen = ref(false)
const cfgForm = ref({
  daily_fee_rate: 0,
  max_days: 0,
  max_multiplier: 0,
  grace_period_days: 0,
})
const cfgLoading = ref(false)
const cfgError = ref('')

function openConfigEdit() {
  const c = overdue.config || {}
  cfgForm.value = {
    daily_fee_rate: c.daily_fee_rate ?? c.dailyFeeRate ?? 0,
    max_days: c.max_days ?? c.maxDays ?? 0,
    max_multiplier: c.max_multiplier ?? c.maxMultiplier ?? 0,
    grace_period_days: c.grace_period_days ?? c.gracePeriodDays ?? 0,
  }
  cfgError.value = ''
  cfgOpen.value = true
}

function closeConfigEdit() {
  cfgOpen.value = false
}

async function handleConfigSave() {
  cfgLoading.value = true
  cfgError.value = ''
  try {
    await overdue.upsertConfig(cfgForm.value)
    closeConfigEdit()
    toast.add({ severity: 'success', summary: '配置已保存', life: 2000 })
    await overdue.fetchConfig()
  } catch (e: any) {
    cfgError.value = e.message || '保存失败'
  } finally {
    cfgLoading.value = false
  }
}

// ── Tab 4: Actions ───────────────────────────────────────────────────────

const actionLoading = ref(false)
const actionResult = ref<any>(null)

async function handleDetect() {
  actionLoading.value = true
  actionResult.value = null
  try {
    const data = await overdue.detect()
    actionResult.value = data
    toast.add({
      severity: 'success',
      summary: '逾期检测完成',
      detail: `发现 ${data.detected ?? data.count ?? 0} 条逾期记录`,
      life: 3000,
    })
    await overdue.fetchStats()
    await loadList()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '检测失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    actionLoading.value = false
  }
}

async function handleEscalate() {
  confirm.require({
    message: '确定要执行逾期升级通知吗？这将向逾期的客户发送通知。',
    header: '执行升级通知',
    accept: async () => {
      actionLoading.value = true
      actionResult.value = null
      try {
        const data = await overdue.escalate()
        actionResult.value = data
        toast.add({
          severity: 'success',
          summary: '升级通知已发送',
          detail: `已发送 ${data.notified ?? data.count ?? 0} 条通知`,
          life: 3000,
        })
        await overdue.fetchStats()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '升级通知失败', detail: e.message || '请重试', life: 4000 })
      } finally {
        actionLoading.value = false
      }
    },
  })
}

// ── Helpers ──────────────────────────────────────────────────────────────

function statusBadge(status: string): string {
  switch (status) {
    case 'active': return 'badge-warning'
    case 'escalated_d1': return 'badge-info'
    case 'escalated_d3': return 'badge-warning'
    case 'escalated_d7': return 'badge-error'
    case 'waived': return 'badge-success'
    case 'paid': return 'badge-info'
    case 'applied': return 'badge-warning'
    default: return 'badge'
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case 'active': return '活跃'
    case 'escalated_d1': return 'D+1 通知'
    case 'escalated_d3': return 'D+3 通知'
    case 'escalated_d7': return 'D+7 通知'
    case 'waived': return '已豁免'
    case 'paid': return '已支付'
    case 'applied': return '已计费'
    default: return status
  }
}

function escalationLevelBadge(level: string): string {
  switch (level) {
    case 'd1': return 'badge-info'
    case 'd3': return 'badge-warning'
    case 'd7': return 'badge-error'
    default: return 'badge'
  }
}

function escalationLevelLabel(level: string): string {
  switch (level) {
    case 'd1': return 'D+1'
    case 'd3': return 'D+3'
    case 'd7': return 'D+7'
    default: return level
  }
}

// ── Tab definitions ──────────────────────────────────────────────────────

const tabs = [
  { key: 'stats' as const, label: '逾期统计' },
  { key: 'list' as const, label: '逾期列表' },
  { key: 'config' as const, label: '费用配置' },
  { key: 'actions' as const, label: '操作面板' },
]

// ── Init ─────────────────────────────────────────────────────────────────

onMounted(async () => {
  await Promise.all([
    overdue.fetchStats(),
    overdue.fetchConfig(),
    loadList(),
  ])
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-17 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-17</span>
      <div class="structure-line" />
      <span class="module-page-label">逾期管理</span>
    </div>

    <!-- Tab bar -->
    <TabBar :tabs="tabs" v-model="activeTab" />

    <!-- ═══ Tab 1: Stats ═══════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'stats'" class="space-y-4">
      <!-- Stat cards -->
      <div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3">
        <div
          v-for="card in statsCards"
          :key="card.label"
          class="panel flex flex-col items-center justify-center text-center"
          style="padding: 1.25rem 1rem;"
        >
          <span class="font-mono text-2xs tracking-wider uppercase text-text-muted mb-1">{{ card.label }}</span>
          <span class="font-mono font-bold text-lg" :style="{ color: card.color }">{{ card.value }}</span>
        </div>
      </div>

      <!-- Empty state -->
      <div v-if="!overdue.stats" class="panel">
        <div class="py-6 text-center">
          <span class="font-mono text-sm text-text-muted">加载统计信息中...</span>
        </div>
      </div>

      <!-- Detail summary -->
      <div v-if="overdue.stats" class="panel">
        <h3 class="panel-title">统计详情</h3>
        <div class="grid grid-cols-2 gap-4 font-mono text-sm">
          <div class="flex justify-between">
            <span class="text-text-muted">活跃逾期</span>
            <span class="text-text-primary">{{ overdue.stats.active_count ?? overdue.stats.activeCount ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">总金额</span>
            <span class="text-text-primary">¥{{ overdue.stats.total_fee ?? overdue.stats.totalFee ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">D+1 待通知</span>
            <span class="text-text-primary">{{ overdue.stats.d1_count ?? overdue.stats.d1Count ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">D+3 待通知</span>
            <span class="text-text-primary">{{ overdue.stats.d3_count ?? overdue.stats.d3Count ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">D+7 待通知</span>
            <span class="text-text-primary">{{ overdue.stats.d7_count ?? overdue.stats.d7Count ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">已豁免</span>
            <span class="text-text-primary" style="color: var(--status-success)">{{ overdue.stats.waived_count ?? overdue.stats.waivedCount ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">已支付</span>
            <span class="text-text-primary" style="color: var(--status-info)">{{ overdue.stats.paid_count ?? overdue.stats.paidCount ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">宽限期内</span>
            <span class="text-text-primary">{{ overdue.stats.grace_count ?? overdue.stats.graceCount ?? 0 }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- ═══ Tab 2: List ════════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'list'" class="space-y-4">
      <div class="panel">
        <h2 class="panel-title">逾期记录</h2>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="listPhoneFilter"
            class="w-40"
            placeholder="搜索手机号"
            @keydown.enter="listPage = 0; loadList()"
          />
          <Select
            v-model="listStatusFilter"
            :options="[
              { label: '全部状态', value: '' },
              { label: '活跃', value: 'active' },
              { label: 'D+1', value: 'escalated_d1' },
              { label: 'D+3', value: 'escalated_d3' },
              { label: 'D+7', value: 'escalated_d7' },
              { label: '已计费', value: 'applied' },
              { label: '已豁免', value: 'waived' },
              { label: '已支付', value: 'paid' },
            ]"
            option-label="label"
            option-value="value"
            class="w-32"
            @change="listPage = 0; loadList()"
          />
          <Button severity="secondary" label="查询" @click="listPage = 0; loadList()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="overdue.records"
          :loading="overdue.loading"
          :paginator="true"
          :rows="listRows"
          :first="listPage * listRows"
          :totalRecords="listTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onListPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无逾期记录</span>
            </div>
          </template>
          <Column field="order_no" header="订单号" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm">{{ data.order_no ?? data.orderNo ?? data.order_id ?? '-' }}</span>
            </template>
          </Column>
          <Column field="customer_name" header="客户">
            <template #body="{ data }: { data: any }">
              <div class="font-mono text-sm">
                <div>{{ data.customer_name ?? data.customerName ?? data.name ?? '-' }}</div>
                <div class="text-xs text-text-muted">{{ data.phone || '-' }}</div>
              </div>
            </template>
          </Column>
          <Column field="due_date" header="应还日期" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.due_date ?? data.dueDate) }}</span>
            </template>
          </Column>
          <Column field="overdue_days" header="逾期天数" sortable>
            <template #body="{ data }: { data: any }">
              <span
                class="font-mono font-bold"
                :style="{ color: (data.overdue_days ?? data.overdueDays ?? 0) >= 7 ? 'var(--status-error)' : 'var(--status-warning)' }"
              >
                {{ data.overdue_days ?? data.overdueDays ?? 0 }}
              </span>
            </template>
          </Column>
          <Column field="daily_fee" header="日费率">
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm">¥{{ data.daily_fee ?? data.dailyFee ?? 0 }}</span>
            </template>
          </Column>
          <Column field="total_fee" header="总费用" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono font-bold text-sm" :style="{ color: 'var(--status-error)' }">
                ¥{{ data.total_fee ?? data.totalFee ?? 0 }}
              </span>
            </template>
          </Column>
          <Column field="status" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="statusBadge(data.status)">{{ statusLabel(data.status) }}</span>
            </template>
          </Column>
          <Column header="操作" headerStyle="width: 14rem">
            <template #body="{ data }: { data: any }">
              <div class="flex items-center gap-2">
                <Button
                  v-if="data.status === 'active'"
                  severity="secondary"
                  label="计费"
                  size="small"
                  @click="openCalc(data)"
                />
                <Button
                  v-if="auth.isTenantAdmin && (data.status === 'active' || data.status === 'applied' || data.status === 'escalated_d1' || data.status === 'escalated_d3' || data.status === 'escalated_d7')"
                  severity="danger"
                  label="豁免"
                  size="small"
                  @click="openWaive(data)"
                />
                <Button
                  severity="secondary"
                  label="历史"
                  size="small"
                  @click="openEscalationHistory(data)"
                />
              </div>
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Calc Fee Dialog -->
      <Dialog
        v-model:visible="calcOpen"
        modal
        header="逾期费用计算"
        :style="{ width: '26rem' }"
      >
        <div class="space-y-3" v-if="calcRecord">
          <div class="text-sm font-mono">
            <span class="text-text-muted">订单号：</span>
            <span>{{ calcRecord.order_no ?? calcRecord.orderNo ?? calcRecord.order_id ?? '-' }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">客户：</span>
            <span>{{ calcRecord.customer_name ?? calcRecord.customerName ?? calcRecord.name ?? '-' }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">逾期天数：</span>
            <span :style="{ color: 'var(--status-warning)' }">{{ calcRecord.overdue_days ?? calcRecord.overdueDays ?? 0 }} 天</span>
          </div>

          <!-- Calc button -->
          <div v-if="!calcResult">
            <Button
              severity="primary"
              label="计算费用"
              :loading="calcLoading"
              @click="handleCalc"
            />
            <p v-if="calcError" class="text-xs font-mono text-status-error mt-2">{{ calcError }}</p>
          </div>

          <!-- Calc result -->
          <div v-if="calcResult" class="border border-border p-3 space-y-2">
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">日费率</span>
              <span class="text-text-primary">¥{{ calcResult.daily_fee ?? calcResult.dailyFee ?? 0 }}/天</span>
            </div>
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">计算天数</span>
              <span class="text-text-primary">{{ calcResult.days ?? calcResult.overdueDays ?? 0 }} 天</span>
            </div>
            <div class="flex justify-between font-mono text-sm font-bold">
              <span class="text-text-muted">总费用</span>
              <span style="color: var(--status-error)">¥{{ calcResult.total_fee ?? calcResult.totalFee ?? 0 }}</span>
            </div>
            <div v-if="calcResult.max_reached ?? calcResult.maxReached" class="font-mono text-xs text-text-muted">
              已达到费用上限
            </div>
          </div>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="calcLoading" @click="closeCalc" />
          <Button
            v-if="calcResult"
            severity="primary"
            label="应用费用"
            @click="handleApplyFee"
          />
        </template>
      </Dialog>

      <!-- Waive Dialog -->
      <Dialog
        v-model:visible="waiveOpen"
        modal
        header="豁免逾期费用"
        :style="{ width: '26rem' }"
      >
        <div class="space-y-3" v-if="waiveRecord">
          <div class="text-sm font-mono">
            <span class="text-text-muted">订单号：</span>
            <span>{{ waiveRecord.order_no ?? waiveRecord.orderNo ?? waiveRecord.order_id ?? '-' }}</span>
          </div>
          <div class="text-sm font-mono">
            <span class="text-text-muted">当前费用：</span>
            <span style="color: var(--status-error)">¥{{ waiveRecord.total_fee ?? waiveRecord.totalFee ?? 0 }}</span>
          </div>
          <div>
            <label class="mono-label block mb-1">豁免原因</label>
            <Textarea v-model="waiveReason" class="w-full" placeholder="请填写豁免原因" rows="3" />
          </div>
          <p v-if="waiveError" class="text-xs font-mono text-status-error">{{ waiveError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="waiveLoading" @click="closeWaive" />
          <Button severity="danger" :label="waiveLoading ? '处理中...' : '确认豁免'" :disabled="waiveLoading" @click="handleWaive" />
        </template>
      </Dialog>

      <!-- Escalation History Dialog -->
      <Dialog
        v-model:visible="escOpen"
        modal
        header="通知升级历史"
        :style="{ width: '34rem' }"
      >
        <div v-if="escLoading" class="py-4 text-center">
          <span class="font-mono text-xs text-text-muted">加载中...</span>
        </div>
        <div v-else-if="escHistory.length === 0" class="py-4 text-center">
          <span class="font-mono text-xs text-text-muted">暂无升级历史</span>
        </div>
        <div v-else class="space-y-2">
          <div
            v-for="(h, i) in escHistory"
            :key="i"
            class="border border-border p-3 font-mono text-sm"
          >
            <div class="flex justify-between mb-1">
              <span :class="escalationLevelBadge(h.level ?? h.escalation_level)">{{ escalationLevelLabel(h.level ?? h.escalation_level) }}</span>
              <span class="text-xs text-text-muted">{{ formatTime(h.created_at ?? h.createdAt) }}</span>
            </div>
            <div class="text-text-muted text-xs">
              {{ h.message ?? h.content ?? h.detail ?? '-' }}
            </div>
          </div>
        </div>
        <template #footer>
          <Button severity="secondary" label="关闭" @click="closeEscalationHistory" />
        </template>
      </Dialog>
    </div>

    <!-- ═══ Tab 3: Config ═══════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'config'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">费用配置</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            label="编辑配置"
            @click="openConfigEdit"
          />
        </div>

        <!-- Config display -->
        <div v-if="overdue.config" class="grid grid-cols-2 gap-4 font-mono text-sm">
          <div class="flex justify-between">
            <span class="text-text-muted">日费率 (元)</span>
            <span class="text-text-primary">¥{{ overdue.config.daily_fee_rate ?? overdue.config.dailyFeeRate ?? 0 }}</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">最大计费天数</span>
            <span class="text-text-primary">{{ overdue.config.max_days ?? overdue.config.maxDays ?? 0 }} 天</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">费用上限倍数</span>
            <span class="text-text-primary">{{ overdue.config.max_multiplier ?? overdue.config.maxMultiplier ?? 0 }}x</span>
          </div>
          <div class="flex justify-between">
            <span class="text-text-muted">宽限期</span>
            <span class="text-text-primary">{{ overdue.config.grace_period_days ?? overdue.config.gracePeriodDays ?? 0 }} 天</span>
          </div>
        </div>

        <div v-else class="py-6 text-center">
          <span class="font-mono text-sm text-text-muted">加载配置中...</span>
        </div>
      </div>

      <!-- Config Edit Dialog -->
      <Dialog
        v-model:visible="cfgOpen"
        modal
        header="编辑费用配置"
        :style="{ width: '28rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">日费率 (元/天)</label>
            <InputNumber v-model="cfgForm.daily_fee_rate" class="w-full" :min="0" :step="0.01" placeholder="0.00" />
          </div>
          <div>
            <label class="mono-label block mb-1">最大计费天数</label>
            <InputNumber v-model="cfgForm.max_days" class="w-full" :min="0" placeholder="0" />
          </div>
          <div>
            <label class="mono-label block mb-1">费用上限倍数 (相对原租金)</label>
            <InputNumber v-model="cfgForm.max_multiplier" class="w-full" :min="0" :step="0.1" placeholder="0" />
          </div>
          <div>
            <label class="mono-label block mb-1">宽限期 (天)</label>
            <InputNumber v-model="cfgForm.grace_period_days" class="w-full" :min="0" placeholder="0" />
          </div>
          <p v-if="cfgError" class="text-xs font-mono text-status-error">{{ cfgError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="cfgLoading" @click="closeConfigEdit" />
          <Button severity="primary" :label="cfgLoading ? '保存中...' : '保存配置'" :disabled="cfgLoading" @click="handleConfigSave" />
        </template>
      </Dialog>
    </div>

    <!-- ═══ Tab 4: Actions ═════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'actions'" class="space-y-4">
      <!-- Detect -->
      <div class="panel">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="font-mono font-bold text-text-primary mb-1">执行逾期检测</h3>
            <p class="font-mono text-xs text-text-muted">扫描所有到期订单，自动标记逾期状态。</p>
          </div>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            :label="actionLoading ? '检测中...' : '立即检测'"
            :disabled="actionLoading"
            @click="handleDetect"
          />
        </div>

        <!-- Detection result -->
        <div v-if="actionResult && actionResult.detected !== undefined" class="mt-3 border border-border p-3">
          <div class="flex items-center gap-4 font-mono text-sm">
            <div>
              <span class="text-text-muted">检测到：</span>
              <span :style="{ color: 'var(--status-warning)' }">{{ actionResult.detected ?? actionResult.count ?? 0 }}</span>
              <span class="text-text-muted"> 条逾期</span>
            </div>
            <div v-if="actionResult.new_overdue !== undefined">
              <span class="text-text-muted">新增：</span>
              <span :style="{ color: 'var(--status-error)' }">{{ actionResult.new_overdue ?? actionResult.newOverdue ?? 0 }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Escalate -->
      <div class="panel">
        <div class="flex items-center justify-between">
          <div>
            <h3 class="font-mono font-bold text-text-primary mb-1">执行升级通知</h3>
            <p class="font-mono text-xs text-text-muted">D+1 SMS提醒 / D+3 邮件通知 / D+7 法律告知 + 触发信用扣分。</p>
          </div>
          <Button
            v-if="auth.isTenantAdmin"
            severity="danger"
            :label="actionLoading ? '发送中...' : '执行升级'"
            :disabled="actionLoading"
            @click="handleEscalate"
          />
        </div>

        <!-- Escalation result -->
        <div v-if="actionResult && actionResult.notified !== undefined" class="mt-3 border border-border p-3">
          <div class="flex items-center gap-4 font-mono text-sm">
            <div>
              <span class="text-text-muted">已通知：</span>
              <span style="color: var(--status-info)">{{ actionResult.notified ?? actionResult.count ?? 0 }}</span>
            </div>
            <div v-if="actionResult.d1 !== undefined">
              <span class="text-text-muted">D+1：</span>
              <span>{{ actionResult.d1 ?? 0 }}</span>
            </div>
            <div v-if="actionResult.d3 !== undefined">
              <span class="text-text-muted">D+3：</span>
              <span :style="{ color: 'var(--status-warning)' }">{{ actionResult.d3 ?? 0 }}</span>
            </div>
            <div v-if="actionResult.d7 !== undefined">
              <span class="text-text-muted">D+7：</span>
              <span :style="{ color: 'var(--status-error)' }">{{ actionResult.d7 ?? 0 }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Admin-only notice -->
      <div v-if="!auth.isTenantAdmin" class="panel">
        <div class="py-4 text-center">
          <span class="font-mono text-sm text-text-muted">操作面板仅限管理员访问。请联系管理员执行逾期检测和升级通知。</span>
        </div>
      </div>
    </div>

    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>

<style scoped>
.panel {
  padding: 1rem;
}
</style>
