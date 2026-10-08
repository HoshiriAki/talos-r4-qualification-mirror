<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { useToast } from 'primevue/usetoast'
import type {
  DepositInfo,
  LedgerEntry,
  RefundInfo,
} from '@/api/finance'
import {
  getDeposit,
  collectDeposit,
  releaseDeposit,
  forfeitDeposit,
  requestRefund,
  approveRefund,
  rejectRefund,
  executeRefund,
  listRefunds,
} from '@/api/finance'

const props = defineProps<{ orderId: string; orderNo: string }>()
const toast = useToast()

const visible = defineModel<boolean>('visible', { default: false })

// ── State ───────────────────────────────────────────────────────

const loading = ref(false)
const deposit = ref<DepositInfo | null>(null)
const ledger = ref<LedgerEntry[]>([])
const refunds = ref<RefundInfo[]>([])

const collectVisible = ref(false)
const collectAmount = ref(2000)
const releaseVisible = ref(false)
const releaseReason = ref('')
const forfeitVisible = ref(false)
const forfeitAmount = ref(0)
const forfeitReason = ref('')
const refundVisible = ref(false)
const refundAmount = ref(0)
const refundReason = ref('')
const actionLoading = ref(false)

// ── Status helpers ──────────────────────────────────────────────

const STATUS_LABELS: Record<string, string> = {
  pending: '待缴纳',
  paid: '已缴纳',
  released: '已释放',
  forfeited: '已罚没',
  partially_forfeited: '部分罚没',
}

const STATUS_SEVERITY: Record<string, string> = {
  pending: 'warn',
  paid: 'success',
  released: 'info',
  forfeited: 'error',
  partially_forfeited: 'warn',
}

function statusLabel(s: string): string { return STATUS_LABELS[s] || s }
function statusSeverity(s: string): string { return STATUS_SEVERITY[s] || 'secondary' }

const canCollect = computed(() => deposit.value?.status === 'pending')
const canRelease = computed(() => deposit.value?.status === 'paid' || deposit.value?.status === 'partially_forfeited')
const canForfeit = computed(() => deposit.value?.status === 'paid' || deposit.value?.status === 'partially_forfeited')

const totalForfeited = computed(() => {
  return ledger.value
    .filter(e => e.entryType === 'forfeit')
    .reduce((sum, e) => sum + e.amount, 0)
})

const remainingBalance = computed(() => {
  if (!deposit.value) return 0
  return deposit.value.amount - totalForfeited.value
})

// ── Load ────────────────────────────────────────────────────────

async function load() {
  if (!props.orderId) return
  loading.value = true
  try {
    const [depositRes, refundRes] = await Promise.all([
      getDeposit(props.orderId),
      listRefunds({ orderId: props.orderId }),
    ])
    deposit.value = depositRes.deposit
    ledger.value = depositRes.ledger || []
    refunds.value = refundRes.refunds || []
  } catch (e: any) {
    // 404 on deposit — that's fine, no deposit yet
    deposit.value = null
    ledger.value = []
    refunds.value = []
  } finally {
    loading.value = false
  }
}

watch(visible, (v) => { if (v) load() })

// ── Actions ─────────────────────────────────────────────────────

async function doCollect() {
  actionLoading.value = true
  try {
    await collectDeposit(props.orderId, collectAmount.value)
    toast.add({ severity: 'success', summary: '押金收取成功', life: 3000 })
    collectVisible.value = false
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '收取失败', detail: e.message, life: 5000 })
  } finally { actionLoading.value = false }
}

async function doRelease() {
  actionLoading.value = true
  try {
    await releaseDeposit(props.orderId, releaseReason.value)
    toast.add({ severity: 'success', summary: '押金释放成功', life: 3000 })
    releaseVisible.value = false
    releaseReason.value = ''
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '释放失败', detail: e.message, life: 5000 })
  } finally { actionLoading.value = false }
}

async function doForfeit() {
  actionLoading.value = true
  try {
    await forfeitDeposit(props.orderId, forfeitAmount.value, forfeitReason.value)
    toast.add({ severity: 'success', summary: '罚没完成', life: 3000 })
    forfeitVisible.value = false
    forfeitAmount.value = 0
    forfeitReason.value = ''
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '罚没失败', detail: e.message, life: 5000 })
  } finally { actionLoading.value = false }
}

async function doRequestRefund() {
  actionLoading.value = true
  try {
    await requestRefund(props.orderId, refundAmount.value, refundReason.value)
    toast.add({ severity: 'success', summary: '退款申请已提交', life: 3000 })
    refundVisible.value = false
    refundAmount.value = 0
    refundReason.value = ''
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '申请失败', detail: e.message, life: 5000 })
  } finally { actionLoading.value = false }
}

async function doApproveRefund(refundId: string) {
  try {
    await approveRefund(refundId)
    toast.add({ severity: 'success', summary: '已审批', life: 2000 })
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '审批失败', detail: e.message, life: 5000 })
  }
}

async function doRejectRefund(refundId: string) {
  try {
    await rejectRefund(refundId, '驳回')
    toast.add({ severity: 'warn', summary: '已驳回', life: 2000 })
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '驳回失败', detail: e.message, life: 5000 })
  }
}

async function doExecuteRefund(refundId: string) {
  try {
    await executeRefund(refundId)
    toast.add({ severity: 'success', summary: '退款已执行', life: 3000 })
    await load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '执行失败', detail: e.message, life: 5000 })
  }
}

const REFUND_STATUS: Record<string, string> = {
  pending: '待审批',
  approved: '已审批',
  rejected: '已驳回',
  executed: '已执行',
}
</script>

<template>
  <Dialog
    v-model:visible="visible"
    modal
    header="押金管理"
    class="w-full max-w-2xl"
    :pt="{ content: { class: '!p-0' }, root: { class: '!bg-surface !border-border' } }"
  >
    <div class="p-5 space-y-5 font-mono text-sm">
      <!-- Loading -->
      <div v-if="loading" class="text-text-muted text-center py-8">加载中...</div>

      <!-- Deposit Card -->
      <template v-else-if="deposit">
        <div class="panel space-y-3">
          <div class="flex items-center justify-between">
            <span class="heading text-lg">押金 #{{ props.orderNo }}</span>
            <span class="badge" :class="`badge-${statusSeverity(deposit.status)}`">
              {{ statusLabel(deposit.status) }}
            </span>
          </div>

          <div class="grid grid-cols-2 gap-3 text-xs">
            <div>
              <span class="text-text-muted">总额</span>
              <p class="text-text-primary font-bold text-lg">¥{{ deposit.amount.toFixed(2) }}</p>
            </div>
            <div>
              <span class="text-text-muted">可用余额</span>
              <p class="text-text-primary font-bold text-lg">¥{{ remainingBalance.toFixed(2) }}</p>
            </div>
            <div v-if="deposit.paidAt">
              <span class="text-text-muted">缴纳时间</span>
              <p class="text-text-primary">{{ deposit.paidAt }}</p>
            </div>
            <div v-if="deposit.releasedAt">
              <span class="text-text-muted">释放时间</span>
              <p class="text-text-primary">{{ deposit.releasedAt }}</p>
            </div>
            <div v-if="deposit.forfeitedAt">
              <span class="text-text-muted">罚没时间</span>
              <p class="text-text-primary">{{ deposit.forfeitedAt }}</p>
            </div>
            <div v-if="totalForfeited > 0">
              <span class="text-text-muted">累计罚没</span>
              <p class="text-status-error font-bold">¥{{ totalForfeited.toFixed(2) }}</p>
            </div>
          </div>

          <!-- Action buttons -->
          <div class="flex gap-2 pt-2">
            <Button v-if="canCollect" label="收取押金" severity="primary" size="small" @click="collectVisible = true" :disabled="actionLoading" />
            <Button v-if="canRelease" label="释放押金" severity="secondary" size="small" @click="releaseVisible = true" :disabled="actionLoading" />
            <Button v-if="canForfeit" label="罚没" severity="danger" size="small" @click="forfeitVisible = true; forfeitAmount = remainingBalance" :disabled="actionLoading" />
          </div>
        </div>

        <!-- Ledger -->
        <div v-if="ledger.length > 0" class="panel">
          <h3 class="heading text-md mb-3">流水记录</h3>
          <table class="table w-full">
            <thead>
              <tr>
                <th class="table-th">时间</th>
                <th class="table-th">类型</th>
                <th class="table-th text-right">金额</th>
                <th class="table-th">说明</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="(entry, i) in ledger" :key="i" class="table-tr">
                <td class="table-td text-xs">{{ entry.createdAt }}</td>
                <td class="table-td">
                  <span class="badge" :class="entry.entryType === 'forfeit' ? 'badge-error' : entry.entryType === 'refund' ? 'badge-warning' : 'badge-info'">
                    {{ entry.entryType === 'collect' ? '收' : entry.entryType === 'release' ? '释' : entry.entryType === 'forfeit' ? '罚' : '退' }}
                  </span>
                </td>
                <td class="table-td text-right" :class="entry.entryType === 'forfeit' || entry.entryType === 'refund' ? 'text-status-error' : 'text-status-success'">
                  {{ entry.entryType === 'forfeit' || entry.entryType === 'refund' ? '-' : '+' }}¥{{ entry.amount.toFixed(2) }}
                </td>
                <td class="table-td text-xs text-text-secondary">{{ entry.description }}</td>
              </tr>
            </tbody>
          </table>
        </div>

        <!-- Refunds -->
        <div class="panel">
          <div class="flex items-center justify-between mb-3">
            <h3 class="heading text-md">退款审批</h3>
            <Button label="申请退款" severity="secondary" size="small" @click="refundVisible = true" :disabled="!canRelease" />
          </div>

          <div v-if="refunds.length === 0" class="text-text-muted text-xs">暂无退款记录</div>

          <table v-else class="table w-full">
            <thead>
              <tr>
                <th class="table-th">金额</th>
                <th class="table-th">状态</th>
                <th class="table-th">原因</th>
                <th class="table-th">操作</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="r in refunds" :key="r.id" class="table-tr">
                <td class="table-td font-bold">¥{{ r.amount.toFixed(2) }}</td>
                <td class="table-td">
                  <span class="badge" :class="r.status === 'executed' ? 'badge-success' : r.status === 'rejected' ? 'badge-error' : r.status === 'approved' ? 'badge-info' : 'badge-warning'">
                    {{ REFUND_STATUS[r.status] || r.status }}
                  </span>
                </td>
                <td class="table-td text-xs text-text-secondary max-w-[120px] truncate">{{ r.reason }}</td>
                <td class="table-td">
                  <div class="flex gap-1">
                    <Button v-if="r.status === 'pending'" label="批" severity="primary" size="small" @click="doApproveRefund(r.id)" />
                    <Button v-if="r.status === 'pending'" label="驳" severity="danger" size="small" @click="doRejectRefund(r.id)" />
                    <Button v-if="r.status === 'approved'" label="执行" severity="success" size="small" @click="doExecuteRefund(r.id)" />
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </template>

      <!-- No deposit yet -->
      <div v-else class="text-text-muted text-center py-8">
        暂无押金记录
        <br />
        <Button label="收取押金" severity="primary" class="mt-3" @click="collectVisible = true" />
      </div>
    </div>
  </Dialog>

  <!-- Collect Dialog -->
  <Dialog v-model:visible="collectVisible" modal header="收取押金" class="w-full max-w-sm">
    <div class="space-y-3 font-mono text-sm">
      <label class="mono-label">金额 (元)</label>
      <InputNumber v-model="collectAmount" :min="0" :step="100" class="w-full" inputClass="input w-full" />
    </div>
    <template #footer>
      <Button label="取消" severity="secondary" @click="collectVisible = false" />
      <Button label="确认收款" severity="primary" @click="doCollect" :disabled="actionLoading || collectAmount <= 0" />
    </template>
  </Dialog>

  <!-- Release Dialog -->
  <Dialog v-model:visible="releaseVisible" modal header="释放押金" class="w-full max-w-sm">
    <div class="space-y-3 font-mono text-sm">
      <p class="text-text-secondary">释放金额：¥{{ remainingBalance.toFixed(2) }}</p>
      <label class="mono-label">释放原因 (可选)</label>
      <InputText v-model="releaseReason" class="w-full" autocomplete="off" />
    </div>
    <template #footer>
      <Button label="取消" severity="secondary" @click="releaseVisible = false" />
      <Button label="确认释放" severity="primary" @click="doRelease" :disabled="actionLoading" />
    </template>
  </Dialog>

  <!-- Forfeit Dialog -->
  <Dialog v-model:visible="forfeitVisible" modal header="罚没押金" class="w-full max-w-sm">
    <div class="space-y-3 font-mono text-sm">
      <label class="mono-label">罚没金额 (元)</label>
      <InputNumber v-model="forfeitAmount" :min="0" :max="remainingBalance" :step="100" class="w-full" inputClass="input w-full" />
      <label class="mono-label">罚没原因</label>
      <Textarea v-model="forfeitReason" class="w-full" rows="2" placeholder="设备损坏、配件丢失等" />
    </div>
    <template #footer>
      <Button label="取消" severity="secondary" @click="forfeitVisible = false" />
      <Button label="确认罚没" severity="danger" @click="doForfeit" :disabled="actionLoading || forfeitAmount <= 0" />
    </template>
  </Dialog>

  <!-- Refund Request Dialog -->
  <Dialog v-model:visible="refundVisible" modal header="申请退款" class="w-full max-w-sm">
    <div class="space-y-3 font-mono text-sm">
      <label class="mono-label">退款金额 (元)</label>
      <InputNumber v-model="refundAmount" :min="0" :max="remainingBalance" :step="100" class="w-full" inputClass="input w-full" />
      <label class="mono-label">退款原因</label>
      <Textarea v-model="refundReason" class="w-full" rows="2" placeholder="退款原因" />
    </div>
    <template #footer>
      <Button label="取消" severity="secondary" @click="refundVisible = false" />
      <Button label="提交申请" severity="primary" @click="doRequestRefund" :disabled="actionLoading || refundAmount <= 0" />
    </template>
  </Dialog>
</template>
