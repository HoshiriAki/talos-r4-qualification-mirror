<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { listInvoices, issueInvoice, voidInvoice, type InvoiceInfo } from '@/api/finance-tax'
import { useToast } from 'primevue/usetoast'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Dialog from 'primevue/dialog'
import Button from 'primevue/button'
import InputText from 'primevue/inputtext'
import Select from 'primevue/select'
import { formatTime } from '@/composables/useFormatTime'

const toast = useToast()

// ── State ──────────────────────────────────────────────────────────
const rows = ref<InvoiceInfo[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(10)
const loading = ref(false)
const statusFilter = ref<string | null>(null)

// Issue dialog
const showIssueDialog = ref(false)
const issueForm = ref({ orderId: '', amount: 0, invoiceType: '普通发票' })

async function load() {
  loading.value = true
  try {
    const result = await listInvoices({
      status: statusFilter.value || undefined,
      page: page.value,
      pageSize: pageSize.value,
    })
    rows.value = result.invoices || []
    total.value = result.pagination?.total || 0
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
  } finally {
    loading.value = false
  }
}

onMounted(load)

function onPageChange(p: number) {
  page.value = p
  load()
}

function onStatusFilterChange() {
  page.value = 1
  load()
}

async function handleIssue() {
  if (!issueForm.value.orderId || issueForm.value.amount <= 0) {
    toast.add({ severity: 'warn', summary: '参数错误', detail: '请输入订单ID和金额', life: 3000 })
    return
  }
  try {
    await issueInvoice(issueForm.value.orderId, issueForm.value.amount, {
      invoiceType: issueForm.value.invoiceType,
    })
    toast.add({ severity: 'success', summary: '开票成功', detail: '发票已生成', life: 3000 })
    showIssueDialog.value = false
    issueForm.value = { orderId: '', amount: 0, invoiceType: '普通发票' }
    load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '开票失败', detail: e.message, life: 4000 })
  }
}

async function handleVoid(invoiceId: string) {
  try {
    await voidInvoice(invoiceId)
    toast.add({ severity: 'success', summary: '作废成功', detail: '发票已作废', life: 3000 })
    load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '作废失败', detail: e.message, life: 4000 })
  }
}

function statusVariant(status: string): string {
  switch (status) {
    case 'issued': return 'badge badge-success'
    case 'voided': return 'badge badge-error'
    case 'red_flushed': return 'badge badge-warning'
    default: return 'badge'
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case 'issued': return '已开具'
    case 'voided': return '已作废'
    case 'red_flushed': return '红字冲销'
    default: return status
  }
}
</script>

<template>
  <div class="invoice-list">
    <div class="list-header">
      <h3 class="section-title">发票列表</h3>
      <div class="header-actions">
        <Select
          v-model="statusFilter"
          :options="[
            { label: '全部状态', value: null },
            { label: '已开具', value: 'issued' },
            { label: '已作废', value: 'voided' },
            { label: '红字冲销', value: 'red_flushed' },
          ]"
          option-label="label"
          option-value="value"
          placeholder="筛选状态"
          class="filter-select"
          style="min-width: 120px"
          @update:model-value="onStatusFilterChange()"
        />
        <Button label="开具发票" icon="pi pi-plus" size="small" @click="showIssueDialog = true" />
      </div>
    </div>

    <DataTable
      :value="rows"
      :lazy="true"
      :loading="loading"
      :total-records="total"
      :rows="pageSize"
      :first="(page - 1) * pageSize"
      paginator
      :rows-per-page-options="[10, 20, 50]"
      @page="onPageChange($event.page + 1)"
      striped-rows
      size="small"
    >
      <Column field="invoiceNo" header="发票号码" :sortable="true" style="min-width: 180px" />
      <Column field="orderId" header="订单ID" style="min-width: 140px" />
      <Column field="type" header="票种" style="min-width: 80px" />
      <Column field="amount" header="金额">
        <template #body="{ data }">
          {{ '¥' + data.amount.toFixed(2) }}
        </template>
      </Column>
      <Column field="taxRate" header="税率">
        <template #body="{ data }">
          {{ (data.taxRate * 100).toFixed(0) + '%' }}
        </template>
      </Column>
      <Column field="taxAmount" header="税额">
        <template #body="{ data }">
          {{ '¥' + data.taxAmount.toFixed(2) }}
        </template>
      </Column>
      <Column field="status" header="状态">
        <template #body="{ data }">
          <span :class="statusVariant(data.status)">{{ statusLabel(data.status) }}</span>
        </template>
      </Column>
      <Column field="issuedAt" header="开票日期">
        <template #body="{ data }">
          {{ formatTime(data.issuedAt) }}
        </template>
      </Column>
      <Column header="操作" style="min-width: 100px">
        <template #body="{ data }">
          <Button
            v-if="data.status === 'issued'"
            label="作废"
            severity="danger"
            size="small"
            text
            @click="handleVoid(data.id)"
          />
        </template>
      </Column>
    </DataTable>

    <!-- Issue Dialog -->
    <Dialog
      v-model:visible="showIssueDialog"
      header="开具发票"
      :modal="true"
      :style="{ width: '400px' }"
    >
      <div class="dialog-form">
        <div class="form-group">
          <label>订单 ID</label>
          <InputText v-model="issueForm.orderId" placeholder="输入订单ID" class="w-full" />
        </div>
        <div class="form-group">
          <label>金额</label>
          <InputNumber v-model="issueForm.amount" :min="0" :min-fraction-digits="2" placeholder="0.00" class="w-full" />
        </div>
        <div class="form-group">
          <label>发票类型</label>
          <Select
            v-model="issueForm.invoiceType"
            :options="['普通发票', '专用发票']"
            class="w-full"
          />
        </div>
        <div class="dialog-actions">
          <Button label="取消" severity="secondary" @click="showIssueDialog = false" />
          <Button label="开具" @click="handleIssue" />
        </div>
      </div>
    </Dialog>
  </div>
</template>

<style scoped>
.invoice-list {
  margin-bottom: 2rem;
}

.list-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1rem;
}

.header-actions {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}

.section-title {
  font-family: 'Space Mono', monospace;
  font-size: 1.125rem;
  font-weight: 600;
  color: var(--text-primary);
  margin: 0;
}

.dialog-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.form-group label {
  font-family: 'Space Mono', monospace;
  font-size: 0.75rem;
  color: var(--text-secondary);
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  margin-top: 0.5rem;
}

.w-full {
  width: 100%;
}

:deep(.badge-success) {
  background: #22c55e20;
  color: #22c55e;
  border: 1px solid #22c55e40;
  padding: 2px 8px;
  border-radius: var(--radius-sm);
  font-size: 0.75rem;
}

:deep(.badge-error) {
  background: #ef444420;
  color: #ef4444;
  border: 1px solid #ef444440;
  padding: 2px 8px;
  border-radius: var(--radius-sm);
  font-size: 0.75rem;
}

:deep(.badge-warning) {
  background: #eab30820;
  color: #eab308;
  border: 1px solid #eab30840;
  padding: 2px 8px;
  border-radius: var(--radius-sm);
  font-size: 0.75rem;
}
</style>
