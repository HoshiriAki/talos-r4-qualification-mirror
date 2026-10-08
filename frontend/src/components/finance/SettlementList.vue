<script setup lang="ts">
import { ref, onMounted } from 'vue'
import {
  listSettlements,
  generateSettlement,
  confirmSettlement,
  exportSettlementCsv,
  type SettlementInfo,
} from '@/api/finance-tax'
import { useToast } from 'primevue/usetoast'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import Dialog from 'primevue/dialog'
import Button from 'primevue/button'
import Select from 'primevue/select'
import { formatTime } from '@/composables/useFormatTime'

const toast = useToast()

// ── State ──────────────────────────────────────────────────────────
const rows = ref<SettlementInfo[]>([])
const total = ref(0)
const page = ref(1)
const pageSize = ref(10)
const loading = ref(false)
const periodTypeFilter = ref<string | null>(null)

// Generate dialog
const showGenerateDialog = ref(false)
const generateForm = ref({ periodType: 'daily', periodKey: '' })

async function load() {
  loading.value = true
  try {
    const result = await listSettlements({
      periodType: periodTypeFilter.value || undefined,
      page: page.value,
      pageSize: pageSize.value,
    })
    rows.value = result.settlements || []
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

function onPeriodTypeChange() {
  page.value = 1
  load()
}

async function handleGenerate() {
  try {
    await generateSettlement(generateForm.value.periodType, {
      periodKey: generateForm.value.periodKey || undefined,
    })
    toast.add({ severity: 'success', summary: '生成成功', detail: '结算单已生成', life: 3000 })
    showGenerateDialog.value = false
    load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '生成失败', detail: e.message, life: 4000 })
  }
}

async function handleConfirm(settlementId: string) {
  try {
    await confirmSettlement(settlementId)
    toast.add({ severity: 'success', summary: '确认成功', detail: '结算单已确认', life: 3000 })
    load()
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '确认失败', detail: e.message, life: 4000 })
  }
}

async function handleExport(settlementId: string) {
  try {
    const result = await exportSettlementCsv(settlementId)
    if (result.ok && result.csv) {
      const blob = new Blob(['﻿' + result.csv], { type: 'text/csv;charset=utf-8' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `settlement-${settlementId}.csv`
      a.click()
      URL.revokeObjectURL(url)
      toast.add({ severity: 'success', summary: '导出成功', detail: 'CSV 已下载', life: 3000 })
    }
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '导出失败', detail: e.message, life: 4000 })
  }
}

function periodTypeLabel(pt: string): string {
  switch (pt) {
    case 'daily': return '日结'
    case 'weekly': return '周结'
    case 'monthly': return '月结'
    default: return pt
  }
}

function confirmedLabel(c: boolean): string {
  return c ? '已确认' : '待确认'
}

function confirmedVariant(c: boolean): string {
  return c ? 'badge badge-success' : 'badge badge-warning'
}
</script>

<template>
  <div class="settlement-list">
    <div class="list-header">
      <h3 class="section-title">结算单</h3>
      <div class="header-actions">
        <Select
          v-model="periodTypeFilter"
          :options="[
            { label: '全部类型', value: null },
            { label: '日结', value: 'daily' },
            { label: '周结', value: 'weekly' },
            { label: '月结', value: 'monthly' },
          ]"
          option-label="label"
          option-value="value"
          placeholder="筛选类型"
          class="filter-select"
          style="min-width: 120px"
          @update:model-value="onPeriodTypeChange()"
        />
        <Button label="生成结算单" icon="pi pi-cog" size="small" @click="showGenerateDialog = true" />
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
      <Column field="periodType" header="类型">
        <template #body="{ data }">
          {{ periodTypeLabel(data.periodType) }}
        </template>
      </Column>
      <Column field="periodKey" header="期间" style="min-width: 120px" />
      <Column field="totalRevenue" header="收入">
        <template #body="{ data }">
          {{ '¥' + data.totalRevenue.toFixed(2) }}
        </template>
      </Column>
      <Column field="totalDeposits" header="押金">
        <template #body="{ data }">
          {{ '¥' + data.totalDeposits.toFixed(2) }}
        </template>
      </Column>
      <Column field="totalRefunds" header="退款">
        <template #body="{ data }">
          {{ '¥' + data.totalRefunds.toFixed(2) }}
        </template>
      </Column>
      <Column field="confirmed" header="状态">
        <template #body="{ data }">
          <span :class="confirmedVariant(data.confirmed)">{{ confirmedLabel(data.confirmed) }}</span>
        </template>
      </Column>
      <Column field="createdAt" header="创建时间">
        <template #body="{ data }">
          {{ formatTime(data.createdAt) }}
        </template>
      </Column>
      <Column header="操作" style="min-width: 150px">
        <template #body="{ data }">
          <div class="action-buttons">
            <Button
              v-if="!data.confirmed"
              label="确认"
              severity="success"
              size="small"
              text
              @click="handleConfirm(data.id)"
            />
            <Button
              label="导出"
              size="small"
              text
              @click="handleExport(data.id)"
            />
          </div>
        </template>
      </Column>
    </DataTable>

    <!-- Generate Dialog -->
    <Dialog
      v-model:visible="showGenerateDialog"
      header="生成结算单"
      :modal="true"
      :style="{ width: '400px' }"
    >
      <div class="dialog-form">
        <div class="form-group">
          <label>结算类型</label>
          <Select
            v-model="generateForm.periodType"
            :options="[
              { label: '日结', value: 'daily' },
              { label: '周结', value: 'weekly' },
              { label: '月结', value: 'monthly' },
            ]"
            option-label="label"
            option-value="value"
            class="w-full"
          />
        </div>
        <div class="form-group">
          <label>期间键 (可选，留空用今天)</label>
          <div class="help-text">
            <template v-if="generateForm.periodType === 'daily'">格式: YYYY-MM-DD</template>
            <template v-else-if="generateForm.periodType === 'weekly'">格式: YYYY-Www</template>
            <template v-else>格式: YYYY-MM</template>
          </div>
          <input v-model="generateForm.periodKey" placeholder="留空自动" class="form-input w-full" />
        </div>
        <div class="dialog-actions">
          <Button label="取消" severity="secondary" @click="showGenerateDialog = false" />
          <Button label="生成" @click="handleGenerate" />
        </div>
      </div>
    </Dialog>
  </div>
</template>

<style scoped>
.settlement-list {
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

.help-text {
  font-size: 0.7rem;
  color: var(--text-secondary);
  opacity: 0.7;
}

.form-input {
  padding: 0.5rem;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-base);
  color: var(--text-primary);
  font-family: 'Space Mono', monospace;
  font-size: 0.875rem;
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

.action-buttons {
  display: flex;
  gap: 0.25rem;
}

:deep(.badge-success) {
  background: #22c55e20;
  color: #22c55e;
  border: 1px solid #22c55e40;
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
