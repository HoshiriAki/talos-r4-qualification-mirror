<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { useToast } from 'primevue/usetoast'
import { useBarcodeStore } from '@/stores/barcode'
import type { BarcodeLabel, ScanEvent } from '@/api/barcode'

const toast = useToast()
const store = useBarcodeStore()

// ── Tab state ──
const activeTab = ref(0)

// ── Tab 1: Generate ──
const singleSerialNo = ref('')
const generatingSingle = ref(false)

async function handleBatchGenerate() {
  try {
    const result = await store.batchGenerate()
    toast.add({ severity: 'success', summary: '批量生成完成', detail: `已为 ${result.generated} 台设备生成条码`, life: 3000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '批量生成失败', detail: e.message, life: 4000 })
  }
}

async function handleSingleGenerate() {
  const sn = singleSerialNo.value.trim()
  if (!sn) return
  generatingSingle.value = true
  try {
    await store.generate(sn)
    toast.add({ severity: 'success', summary: '条码已生成', detail: `设备 ${sn} 的条码已生成`, life: 2000 })
    singleSerialNo.value = ''
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '生成失败', detail: e.message, life: 4000 })
  } finally {
    generatingSingle.value = false
  }
}

async function copyBarcode(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    toast.add({ severity: 'info', summary: '已复制', detail: text, life: 1500 })
  } catch {
    toast.add({ severity: 'error', summary: '复制失败', detail: '请手动复制', life: 2000 })
  }
}

// ── Tab 2: Scan History ──
const scanHistoryPage = ref(1)
const scanHistoryPageSize = ref(20)
const scanTypeFilter = ref('')
const scanDeviceFilter = ref('')
const scanStartDate = ref('')
const scanEndDate = ref('')

async function loadScanHistory() {
  await store.fetchScanHistory({
    deviceSerialNo: scanDeviceFilter.value || undefined,
    scanType: scanTypeFilter.value || undefined,
    startDate: scanStartDate.value || undefined,
    endDate: scanEndDate.value || undefined,
    page: scanHistoryPage.value,
    pageSize: scanHistoryPageSize.value,
  })
}

async function handleScanPageChange(event: { page: number; rows: number }) {
  scanHistoryPage.value = event.page + 1
  scanHistoryPageSize.value = event.rows
  await loadScanHistory()
}

const scanTypeOptions = [
  { label: '全部', value: '' },
  { label: '出库', value: 'checkout' },
  { label: '入库', value: 'checkin' },
  { label: '盘点', value: 'inventory' },
  { label: '调拨', value: 'transfer' },
]

const scanTypeLabels: Record<string, string> = {
  checkout: '出库',
  checkin: '入库',
  inventory: '盘点',
  transfer: '调拨',
}

// ── Init: load stats + scan history ──
onMounted(async () => {
  store.fetchStats().catch(() => {})
  loadScanHistory()
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-15 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-15</span>
      <div class="structure-line" />
      <span class="module-page-label">条码管理</span>
    </div>
    <div class="panel">
      <h2 class="panel-title">条码管理</h2>

      <TabView v-model:active-index="activeTab">
        <!-- ══════════ Tab 1: 生成标签 ══════════ -->
        <TabPanel value="generate" header="生成标签">
          <div class="space-y-4 pt-3">
            <!-- Batch + Single generate -->
            <div class="flex flex-wrap items-end gap-3">
              <Button
                severity="primary"
                :disabled="store.loading"
                :label="store.loading ? '生成中...' : '批量生成条码'"
                @click="handleBatchGenerate"
              />
              <div class="border-l-2 border-border h-8 mx-1 hidden sm:block" />
              <div class="flex-1 min-w-240px">
                <label class="mono-label block mb-1">设备序列号</label>
                <InputText
                  v-model="singleSerialNo"
                  class="w-full"
                  placeholder="输入序列号生成单个条码"
                  :disabled="generatingSingle"
                  @keydown.enter="handleSingleGenerate"
                />
              </div>
              <Button
                severity="secondary"
                :disabled="generatingSingle || !singleSerialNo.trim()"
                :label="generatingSingle ? '生成中...' : '单个生成'"
                @click="handleSingleGenerate"
              />
            </div>

            <!-- Error display -->
            <div v-if="store.error" class="font-mono text-xs text-status-error py-2">
              {{ store.error }}
            </div>

            <!-- Labels table -->
            <div v-if="store.labels.length > 0">
              <DataTable :value="store.labels" class="font-mono text-xs" striped-rows size="small">
                <Column field="id" header="ID" style="width: 60px" />
                <Column field="deviceSerialNo" header="设备序列号" style="width: 160px" />
                <Column field="barcodeText" header="条码内容" style="width: 200px">
                  <template #body="{ data }">
                    <span class="tracking-wider">{{ data.barcodeText }}</span>
                  </template>
                </Column>
                <Column field="barcodeType" header="类型" style="width: 80px" />
                <Column field="labelFormat" header="格式" style="width: 80px" />
                <Column field="generatedAt" header="生成时间" style="width: 160px" />
                <Column header="操作" style="width: 80px">
                  <template #body="{ data }">
                    <Button
                      severity="secondary"
                      size="small"
                      label="复制"
                      @click="copyBarcode(data.barcodeText)"
                    />
                  </template>
                </Column>
              </DataTable>
            </div>
            <div v-else class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无条码标签，点击"批量生成"或输入序列号生成</span>
            </div>
          </div>
        </TabPanel>

        <!-- ══════════ Tab 2: 扫码记录 ══════════ -->
        <TabPanel value="scan" header="扫码记录">
          <div class="space-y-4 pt-3">
            <!-- Filter bar -->
            <div class="flex flex-wrap items-end gap-3">
              <div class="w-140px">
                <label class="mono-label block mb-1">扫码类型</label>
                <Select
                  v-model="scanTypeFilter"
                  :options="scanTypeOptions"
                  option-label="label"
                  option-value="value"
                  class="w-full"
                />
              </div>
              <div class="flex-1 min-w-160px">
                <label class="mono-label block mb-1">设备序列号</label>
                <InputText
                  v-model="scanDeviceFilter"
                  class="w-full"
                  placeholder="输入序列号筛选"
                  @keydown.enter="loadScanHistory"
                />
              </div>
              <div class="w-160px">
                <label class="mono-label block mb-1">开始日期</label>
                <InputText
                  v-model="scanStartDate"
                  type="date"
                  class="w-full"
                />
              </div>
              <div class="w-160px">
                <label class="mono-label block mb-1">结束日期</label>
                <InputText
                  v-model="scanEndDate"
                  type="date"
                  class="w-full"
                />
              </div>
              <Button
                severity="primary"
                size="small"
                label="查询"
                @click="loadScanHistory"
              />
            </div>

            <!-- Scan events table -->
            <DataTable
              :value="store.scanEvents"
              :total-records="store.scanTotal"
              :rows="scanHistoryPageSize"
              :first="(scanHistoryPage - 1) * scanHistoryPageSize"
              lazy
              paginator
              :paginator-template="'FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown'"
              :rows-per-page-options="[10, 20, 50]"
              class="font-mono text-xs"
              striped-rows
              size="small"
              @page="handleScanPageChange"
            >
              <Column field="id" header="ID" style="width: 60px" />
              <Column field="deviceSerialNo" header="序列号" style="width: 140px" />
              <Column field="barcodeText" header="条码内容" style="width: 180px">
                <template #body="{ data }">
                  <span v-if="data.barcodeText" class="tracking-wider">{{ data.barcodeText }}</span>
                  <span v-else class="text-text-muted">-</span>
                </template>
              </Column>
              <Column field="scanType" header="类型" style="width: 80px">
                <template #body="{ data }">
                  <Tag :value="scanTypeLabels[data.scanType] || data.scanType" />
                </template>
              </Column>
              <Column field="scannedBy" header="操作人ID" style="width: 90px" />
              <Column field="createdAt" header="时间" style="width: 160px" />
              <Column field="notes" header="备注" style="width: 160px">
                <template #body="{ data }">
                  <span v-if="data.notes" class="text-text-muted">{{ data.notes }}</span>
                  <span v-else class="text-text-muted">-</span>
                </template>
              </Column>
            </DataTable>
          </div>
        </TabPanel>

        <!-- ══════════ Tab 3: 统计 ══════════ -->
        <TabPanel value="stats" header="统计">
          <div class="space-y-4 pt-3">
            <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
              <!-- Today scans -->
              <div class="panel">
                <div class="font-mono text-xs text-text-muted mb-1">今日扫码</div>
                <div class="font-mono text-2xl font-bold text-text-primary">
                  {{ store.stats?.todayScans ?? 0 }}
                </div>
              </div>

              <!-- This week scans -->
              <div class="panel">
                <div class="font-mono text-xs text-text-muted mb-1">本周扫码</div>
                <div class="font-mono text-2xl font-bold text-text-primary">
                  {{ store.stats?.thisWeekScans ?? 0 }}
                </div>
              </div>

              <!-- By type cards -->
              <div class="panel">
                <div class="font-mono text-xs text-text-muted mb-2">按类型分布</div>
                <div class="space-y-1">
                  <div class="flex justify-between font-mono text-xs">
                    <span class="text-text-muted">出库</span>
                    <span class="font-bold">{{ store.stats?.byType?.checkout ?? 0 }}</span>
                  </div>
                  <div class="flex justify-between font-mono text-xs">
                    <span class="text-text-muted">入库</span>
                    <span class="font-bold">{{ store.stats?.byType?.checkin ?? 0 }}</span>
                  </div>
                  <div class="flex justify-between font-mono text-xs">
                    <span class="text-text-muted">盘点</span>
                    <span class="font-bold">{{ store.stats?.byType?.inventory ?? 0 }}</span>
                  </div>
                  <div class="flex justify-between font-mono text-xs">
                    <span class="text-text-muted">调拨</span>
                    <span class="font-bold">{{ store.stats?.byType?.transfer ?? 0 }}</span>
                  </div>
                </div>
              </div>

              <!-- Total scans -->
              <div class="panel">
                <div class="font-mono text-xs text-text-muted mb-1">总计</div>
                <div class="font-mono text-2xl font-bold text-text-primary">
                  {{ Object.values(store.stats?.byType ?? {}).reduce((a, b) => a + b, 0) }}
                </div>
              </div>
            </div>
          </div>
        </TabPanel>
      </TabView>
    </div>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
