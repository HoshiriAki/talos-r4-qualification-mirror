<script setup lang="ts">
import { ref, reactive, computed, onMounted, onUnmounted, nextTick, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { usePricingStore } from '@/stores/pricing'
import { useModelsStore } from '@/stores/models'
import { usePricingCalc, DAY_LABELS } from '@/composables/usePricingCalc'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import type { HolidayRule } from '@/api/pricing'
import type { DeviceModel } from '@/api/models'
import { fetchDailyOrderCounts, type DailyCount } from '@/api/dashboard'

const auth = useAuthStore()
const pricing = usePricingStore()
const models = useModelsStore()
const toast = useToast()
const confirm = useConfirm()
const { getRollingDateKeys, parseDateKey } = usePricingCalc()

// ==================== Selection ====================
const selectedModels = ref<DeviceModel[]>([])
const selectedHolidayRules = ref<HolidayRule[]>([])

// Per-model pricing editing state
const modelPrices = reactive<Record<string, { weekday: number; weekend: number; saving: boolean }>>({})
const modelPriceError = ref('')

const isAdmin = computed(() => auth.isTenantAdmin)

function generateId(): string {
  if (typeof crypto !== 'undefined' && crypto.randomUUID) {
    return crypto.randomUUID()
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, c => {
    const r = Math.random() * 16 | 0
    return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16)
  })
}

// ==================== Section 1: Base Prices
const weekdayPrice = ref(8.5)
const weekendPrice = ref(14)

const receiveShippingFees = reactive<Record<string, number>>({
  area1: 7,
  area2: 7,
  area3: 7,
  area4: 18,
})

// Section 2: Holiday Rules
const holidayRules = reactive<Array<HolidayRule & { _id?: string }>>([])

function localIsWeekend(dateKey: string): boolean {
  const d = parseDateKey(dateKey)
  if (!d) return false
  const day = d.getDay()
  return day === 0 || day === 5 || day === 6
}

function localHolidayPrice(dateKey: string): number | null {
  const d = parseDateKey(dateKey)
  if (!d) return null
  for (const rule of holidayRules) {
    const start = parseDateKey(rule.startDate)
    const end = parseDateKey(rule.endDate)
    if (!start || !end) continue
    if (rule.includePreviousDay) {
      const prev = new Date(start.getTime() - 24 * 60 * 60 * 1000)
      if (d.getTime() === prev.getTime()) return rule.price
    }
    if (d >= start && d <= end) return rule.price
  }
  return null
}

function localIsHoliday(dateKey: string): boolean {
  return localHolidayPrice(dateKey) !== null
}

function localDefaultPrice(dateKey: string): number {
  const hp = localHolidayPrice(dateKey)
  if (hp !== null) return hp
  return localIsWeekend(dateKey) ? weekendPrice.value : weekdayPrice.value
}

// Section 3: Dynamic Price Grid
const dynamicPrices = reactive<Record<string, number | null>>({})
const selectedDateKeys = ref<Set<string>>(new Set())
const batchMode = ref(false)
const batchOffset = ref(0)
const batchPercent = ref(0)

const AREA_LABELS: Record<string, string> = {
  area1: '区域1（本地/省内）',
  area2: '区域2（华东华南）',
  area3: '区域3（华北华中）',
  area4: '区域4（偏远地区）',
}

function localEffectivePrice(dateKey: string): number {
  const dp = dynamicPrices[dateKey]
  if (dp !== null && dp !== undefined) return dp
  return localDefaultPrice(dateKey)
}

const rollingEntries = computed(() => {
  return getRollingDateKeys(15).map(dateKey => {
    const d = parseDateKey(dateKey)
    return {
      dateKey,
      dayLabel: d ? DAY_LABELS[d.getDay()] : '',
    }
  })
})

const windowRange = computed(() => {
  const entries = rollingEntries.value
  if (entries.length === 0) return ''
  return `${entries[0].dateKey} ~ ${entries[entries.length - 1].dateKey} (${entries.length} 天)`
})

function toggleDateSelection(dateKey: string) {
  const s = new Set(selectedDateKeys.value)
  if (s.has(dateKey)) {
    s.delete(dateKey)
  } else {
    s.add(dateKey)
  }
  selectedDateKeys.value = s
}

function clearSelection() {
  selectedDateKeys.value = new Set()
  batchMode.value = false
}

function applyBatchOffset() {
  const n = Number(batchOffset.value)
  if (!n) return
  const s = new Set(selectedDateKeys.value)
  s.forEach(dateKey => {
    const current = dynamicPrices[dateKey] ?? localDefaultPrice(dateKey)
    dynamicPrices[dateKey] = Math.max(0, current + n)
  })
  toast.add({ severity: 'success', summary: `已对 ${s.size} 个日期应用偏移 ${n > 0 ? '+' : ''}${n}`, life: 2000 })
  clearSelection()
}

function applyBatchPercent() {
  const pct = Number(batchPercent.value)
  if (!pct) return
  const s = new Set(selectedDateKeys.value)
  const mult = 1 + pct / 100
  s.forEach(dateKey => {
    const current = dynamicPrices[dateKey] ?? localDefaultPrice(dateKey)
    dynamicPrices[dateKey] = Math.max(0, Math.round(current * mult * 100) / 100)
  })
  toast.add({ severity: 'success', summary: `已对 ${s.size} 个日期应用 ${pct > 0 ? '+' : ''}${pct}%`, life: 2000 })
  clearSelection()
}

function addHolidayRule() {
  holidayRules.push({
    name: '',
    startDate: '',
    endDate: '',
    price: 0,
    includePreviousDay: false,
    _id: generateId(),
  })
}

function removeHolidayRule(index: number) {
  holidayRules.splice(index, 1)
}

function resetDynamicPrices() {
  confirm.require({
    message: '确定要重置所有动态价格吗？这将丢弃所有手动修改，恢复为默认价格。',
    header: '重置动态价格',
    accept: () => {
      Object.keys(dynamicPrices).forEach(key => {
        delete dynamicPrices[key]
      })
      Object.entries(pricing.config.dynamicPriceMap).forEach(([key, val]) => {
        dynamicPrices[key] = val
      })
      clearSelection()
      toast.add({ severity: 'info', summary: '已重置动态价格', life: 2000 })
    },
  })
}

function setDynamicPrice(dateKey: string, e: Event) {
  const val = (e.target as HTMLInputElement).value
  dynamicPrices[dateKey] = val === '' ? null : Number(val)
}

// Load from store
onMounted(async () => {
  await Promise.all([pricing.fetchConfig(), models.fetchAll()])
  const cfg = pricing.config
  weekdayPrice.value = cfg.baseWeekdayPrice
  weekendPrice.value = cfg.baseWeekendPrice

  holidayRules.splice(0, holidayRules.length)
  cfg.holidayRules.forEach(r => {
    holidayRules.push({ ...r, _id: generateId() })
  })

  if (cfg.receiveShippingFees) {
    Object.assign(receiveShippingFees, cfg.receiveShippingFees)
  }

  Object.entries(cfg.dynamicPriceMap).forEach(([key, val]) => {
    dynamicPrices[key] = val
  })

  for (const m of models.models) {
    modelPrices[m.id] = {
      weekday: m.weekdayPrice ?? cfg.baseWeekdayPrice,
      weekend: m.weekendPrice ?? cfg.baseWeekendPrice,
      saving: false,
    }
  }
})

watch(() => models.models, (newModels) => {
  for (const m of newModels) {
    if (!modelPrices[m.id]) {
      modelPrices[m.id] = {
        weekday: m.weekdayPrice ?? pricing.config.baseWeekdayPrice,
        weekend: m.weekendPrice ?? pricing.config.baseWeekendPrice,
        saving: false,
      }
    }
  }
}, { deep: true })

async function saveModelPricing(modelId: string) {
  const mp = modelPrices[modelId]
  if (!mp) return
  mp.saving = true
  modelPriceError.value = ''
  try {
    await models.updatePricing(modelId, { weekdayPrice: mp.weekday, weekendPrice: mp.weekend })
    toast.add({ severity: 'success', summary: '型号价格已更新', life: 2000 })
    await models.fetchAll()
    for (const m of models.models) {
      if (!modelPrices[m.id]) {
        modelPrices[m.id] = {
          weekday: m.weekdayPrice ?? pricing.config.baseWeekdayPrice,
          weekend: m.weekendPrice ?? pricing.config.baseWeekendPrice,
          saving: false,
        }
      }
    }
  } catch (e: any) {
    modelPriceError.value = e?.message || '保存失败'
  } finally {
    mp.saving = false
  }
}

const saving = ref(false)

async function handleSave() {
  saving.value = true

  const cleanHolidayRules: HolidayRule[] = holidayRules.map(r => ({
    name: r.name,
    startDate: r.startDate,
    endDate: r.endDate,
    price: Number(r.price) || 0,
    includePreviousDay: r.includePreviousDay || false,
  }))

  const cleanDynamicPriceMap: Record<string, number> = {}
  Object.entries(dynamicPrices).forEach(([key, val]) => {
    if (val !== null && val !== undefined && !Number.isNaN(Number(val))) {
      cleanDynamicPriceMap[key] = Number(val)
    }
  })

  try {
    await pricing.saveConfig({
      baseWeekdayPrice: Number(weekdayPrice.value) || 0,
      baseWeekendPrice: Number(weekendPrice.value) || 0,
      holidayRules: cleanHolidayRules,
      receiveShippingFees: { ...receiveShippingFees },
      dynamicPriceMap: cleanDynamicPriceMap,
    })
    toast.add({ severity: 'success', summary: '保存成功', life: 2000 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '保存失败', detail: e?.message, life: 4000 })
  } finally {
    saving.value = false
  }
}

function heatmapColor(dateKey: string): string {
  const price = localEffectivePrice(dateKey)
  const base = localDefaultPrice(dateKey)
  if (price > base) return 'border-status-error bg-accent-muted'
  if (price < base) return 'border-status-success bg-accent-muted'
  return 'border-border'
}

function formatPrice(val: number | null | undefined): string {
  if (val == null) return '-'
  return `¥${Number(val).toFixed(2)}`
}

// ═══ Order Occupancy Chart ═══
const chartData = ref<DailyCount[]>([])
const chartLoading = ref(false)
const chartContainer = ref<HTMLElement | null>(null)
const chartWidth = ref(760)
const chartHeight = ref(200)

let chartResizeObs: ResizeObserver | null = null

const CHART_PAD = { top: 24, right: 16, bottom: 38, left: 42 }

const chartMax = computed(() => Math.max(1, ...chartData.value.map(d => d.count), 1))
const chartYMax = computed(() => Math.max(1, Math.ceil(chartMax.value * 1.25)))
const plotW = computed(() => chartWidth.value - CHART_PAD.left - CHART_PAD.right)
const plotH = computed(() => chartHeight.value - CHART_PAD.top - CHART_PAD.bottom)

function chartX(i: number): number {
  if (chartData.value.length <= 1) return CHART_PAD.left + plotW.value / 2
  return CHART_PAD.left + (plotW.value * i) / (chartData.value.length - 1)
}

function chartY(count: number): number {
  return CHART_PAD.top + plotH.value - (plotH.value * count) / chartYMax.value
}

function dayLabel(dateKey: string): string {
  const m = /^\d{4}-\d{2}-(\d{2})$/.exec(dateKey)
  return m ? String(Number(m[1])) : dateKey
}

function attachChartObserver() {
  if (!chartContainer.value) return
  chartResizeObs = new ResizeObserver(entries => {
    const rect = entries[0]?.contentRect
    if (!rect || rect.width <= 0) return
    // Width fills container, height keeps 16:5-ish ratio, capped
    chartWidth.value = Math.max(480, rect.width)
    chartHeight.value = Math.max(120, Math.min(300, Math.round(rect.width * 0.26)))
  })
  chartResizeObs.observe(chartContainer.value)
}

function detachChartObserver() {
  if (chartResizeObs) { chartResizeObs.disconnect(); chartResizeObs = null }
}

async function loadChartData() {
  chartLoading.value = true
  try {
    const result = await fetchDailyOrderCounts(15)
    chartData.value = result.counts
    await nextTick()
    attachChartObserver()
  } catch {
    chartData.value = []
  } finally {
    chartLoading.value = false
  }
}

onMounted(() => { loadChartData() })
onUnmounted(() => { detachChartObserver() })
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-11 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-11</span>
      <div class="structure-line" />
      <span class="module-page-label">动态调价</span>
    </div>
    <div class="panel">
      <h2 class="panel-title">动态调价</h2>
      <div v-if="pricing.loading" class="py-6 text-center">
        <span class="font-mono text-xs text-text-muted">加载中...</span>
      </div>

      <template v-else>
      <p v-if="!isAdmin" class="font-mono text-sm text-text-muted mb-4">
        当前账号仅可查看价格配置，只有 admin 可修改
      </p>

      <!-- Section 1: Base Prices -->
      <div class="mb-6">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 pb-2 border-b border-border">基础价格</h3>
        <div class="space-y-4">
          <div>
            <h4 class="mono-label mb-3">全局基础价格</h4>
            <div class="grid grid-cols-2 gap-3">
              <div>
                <label class="mono-label block mb-1">周一至周四价格</label>
                <InputNumber
                  v-model="weekdayPrice"
                  :min-fraction-digits="2"
                  :min="0"
                  class="w-full"
                  :disabled="!isAdmin"
                />
              </div>
              <div>
                <label class="mono-label block mb-1">周五至周日价格</label>
                <InputNumber
                  v-model="weekendPrice"
                  :min-fraction-digits="2"
                  :min="0"
                  class="w-full"
                  :disabled="!isAdmin"
                />
              </div>
            </div>
          </div>

          <div class="divider" />

          <div>
            <h4 class="mono-label mb-3">收货区域运费</h4>
            <p class="text-xs text-text-muted mb-2">收货运费按区域配置</p>
            <div class="grid grid-cols-4 gap-3">
              <div v-for="area in ['area1', 'area2', 'area3', 'area4']" :key="area">
                <label class="mono-label block mb-1 text-xs">{{ AREA_LABELS[area] || area }}</label>
                <InputNumber
                  v-model="receiveShippingFees[area]"
                  :min-fraction-digits="2"
                  :min="0"
                  class="w-full"
                  :disabled="!isAdmin"
                />
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Section 2: Model Pricing -->
      <div class="mb-6">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 pb-2 border-b border-border">型号定价</h3>
        <div class="space-y-4">
          <p class="text-xs text-text-muted">每个型号可设置独立的平日/周末基础价格，未设置的型号将使用全局基础价格。</p>
          <DataTable
            v-if="models.models.length > 0"
            v-model:selection="selectedModels"
            :value="models.models"
            dataKey="id"
            :paginator="true"
            :rows="10"
            :rowsPerPageOptions="[10, 20, 30, 50]"
            paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
            currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
            removableSort
          >
            <template #empty>
              <div class="py-2 text-center">
                <span class="font-mono text-xs text-text-muted">暂无型号</span>
              </div>
            </template>
            <Column selectionMode="multiple" headerStyle="width: 3rem" />
            <Column field="name" header="型号" sortable>
              <template #body="{ data }: { data: DeviceModel }">
                <span class="font-mono">{{ data.name }}</span>
              </template>
            </Column>
            <Column field="prefix" header="前缀" sortable>
              <template #body="{ data }: { data: DeviceModel }">
                <span class="font-mono text-xs">{{ data.prefix }}</span>
              </template>
            </Column>
            <Column header="平日价格">
              <template #body="{ data }: { data: DeviceModel }">
                <InputNumber
                  v-if="modelPrices[data.id]"
                  v-model="modelPrices[data.id].weekday"
                  :min-fraction-digits="2"
                  :min="0"
                  class="w-24"
                  :disabled="!isAdmin"
                />
              </template>
            </Column>
            <Column header="周末价格">
              <template #body="{ data }: { data: DeviceModel }">
                <InputNumber
                  v-if="modelPrices[data.id]"
                  v-model="modelPrices[data.id].weekend"
                  :min-fraction-digits="2"
                  :min="0"
                  class="w-24"
                  :disabled="!isAdmin"
                />
              </template>
            </Column>
            <Column header="操作">
              <template #body="{ data }: { data: DeviceModel }">
                <Button
                  v-if="modelPrices[data.id]"
                  severity="primary"
                  :disabled="!isAdmin || modelPrices[data.id].saving"
                  :label="modelPrices[data.id].saving ? '保存中...' : '保存'"
                  @click="saveModelPricing(data.id)"
                />
              </template>
            </Column>
          </DataTable>
          <div v-else class="py-2 text-center">
            <span class="font-mono text-xs text-text-muted">暂无型号</span>
          </div>
          <p v-if="modelPriceError" class="text-xs font-mono text-btn-danger-text">{{ modelPriceError }}</p>
        </div>
      </div>

      <!-- Section 3: Holiday Rules -->
      <div class="mb-6">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 pb-2 border-b border-border">节假日规则</h3>
        <div class="space-y-4">
          <DataTable
            :value="holidayRules"
            dataKey="_id"
            removableSort
            columnResizeMode="expand"
            responsiveLayout="scroll"
          >
            <template #empty>
              <div class="py-4 text-center">
                <span class="font-mono text-xs text-text-muted">暂未设置节假日规则</span>
              </div>
            </template>
            <Column field="name" header="名称" sortable headerStyle="width: 12rem">
              <template #body="{ data }: { data: any }">
                <InputText v-model="data.name" class="w-full" :disabled="!isAdmin" placeholder="春节" />
              </template>
            </Column>
            <Column field="startDate" header="开始日期" sortable headerStyle="width: 8.5rem">
              <template #body="{ data }: { data: any }">
                <input v-model="data.startDate" type="date" class="input" :disabled="!isAdmin" />
              </template>
            </Column>
            <Column field="endDate" header="结束日期" sortable headerStyle="width: 8.5rem">
              <template #body="{ data }: { data: any }">
                <input v-model="data.endDate" type="date" class="input" :disabled="!isAdmin" />
              </template>
            </Column>
            <Column field="price" header="价格" sortable headerStyle="width: 5rem">
              <template #body="{ data }: { data: any }">
                <InputNumber v-model="data.price" :min-fraction-digits="2" :min="0" class="w-full" :disabled="!isAdmin" />
              </template>
            </Column>
            <Column field="includePreviousDay" header="含前一日" sortable headerStyle="width: 5rem">
              <template #body="{ data }: { data: any }">
                <div class="text-center">
                  <Checkbox v-model="data.includePreviousDay" binary :disabled="!isAdmin" />
                </div>
              </template>
            </Column>
            <Column header="操作" headerStyle="width: 3.5rem">
              <template #body="{ data, index }: { data: any; index: number }">
                <Button severity="danger" label="删除" :disabled="!isAdmin" @click="removeHolidayRule(index)" />
              </template>
            </Column>
          </DataTable>
          <Button
            v-if="isAdmin"
            severity="secondary"
            label="添加节假日规则"
            @click="addHolidayRule"
          />
        </div>
      </div>

      <!-- Section 4: Dynamic Price Map — Grid Cards -->
      <div class="mb-2">
        <h3 class="font-mono text-sm font-bold text-text-primary mb-4 pb-2 border-b border-border">动态价格映射</h3>

        <!-- Order Occupancy Chart -->
        <div ref="chartContainer" class="mb-4">
          <div class="flex items-center justify-between mb-2">
            <h4 class="mono-label">未来 15 天订单占用</h4>
            <span v-if="chartLoading" class="font-mono text-xs text-text-muted">加载中...</span>
          </div>
          <div v-if="chartData.length > 0">
            <svg
              :viewBox="`0 0 ${chartWidth} ${chartHeight}`"
              class="w-full block"
              preserveAspectRatio="xMidYMid meet"
            >
              <!-- Y-axis -->
              <line
                :x1="CHART_PAD.left" :y1="CHART_PAD.top"
                :x2="CHART_PAD.left" :y2="chartHeight - CHART_PAD.bottom"
                stroke="var(--border-default)" stroke-width="1"
              />
              <!-- X-axis -->
              <line
                :x1="CHART_PAD.left" :y1="chartHeight - CHART_PAD.bottom"
                :x2="chartWidth - CHART_PAD.right" :y2="chartHeight - CHART_PAD.bottom"
                stroke="var(--border-default)" stroke-width="1"
              />
              <!-- Grid lines + Y labels -->
              <template v-for="tick in [0, Math.ceil(chartYMax / 2), chartYMax]" :key="'y'+tick">
                <line
                  :x1="CHART_PAD.left" :y1="chartY(tick)"
                  :x2="chartWidth - CHART_PAD.right" :y2="chartY(tick)"
                  stroke="var(--border-default)" stroke-width="1" stroke-dasharray="3 3"
                />
                <text
                  :x="CHART_PAD.left - 10" :y="chartY(tick) + 4"
                  text-anchor="end" fill="var(--text-muted)" font-size="10"
                  font-family="var(--font-mono)"
                >{{ tick }}</text>
              </template>
              <!-- Data line -->
              <polyline
                v-if="chartData.length > 0"
                :points="chartData.map((d, i) => `${chartX(i).toFixed(1)},${chartY(d.count).toFixed(1)}`).join(' ')"
                fill="none" stroke="var(--status-info)" stroke-width="2"
              />
              <!-- Data points + labels -->
              <template v-for="(d, i) in chartData" :key="'p'+i">
                <circle
                  :cx="chartX(i)" :cy="chartY(d.count)" r="3.5"
                  fill="var(--status-info)" stroke="var(--bg-surface)" stroke-width="1.5"
                />
                <text
                  v-if="d.count > 0"
                  :x="chartX(i)" :y="chartY(d.count) - 8"
                  text-anchor="middle" fill="var(--text-primary)" font-size="10"
                  font-family="var(--font-mono)" font-weight="bold"
                >{{ d.count }}</text>
              </template>
              <!-- X-axis date labels -->
              <template v-for="(d, i) in chartData" :key="'x'+i">
                <text
                  :x="chartX(i)" :y="chartHeight - 10"
                  text-anchor="middle" fill="var(--text-muted)" font-size="9"
                  font-family="var(--font-mono)"
                >{{ dayLabel(d.dateKey) }}</text>
              </template>
            </svg>
          </div>
          <div v-else-if="!chartLoading" class="text-xs text-text-muted font-mono py-2">
            暂无订单占用数据
          </div>
        </div>

        <div class="space-y-3">
          <div class="flex items-center justify-between">
            <p class="font-mono text-xs text-text-muted">滚动窗口：{{ windowRange }}</p>
            <div class="flex items-center gap-2">
              <span class="font-mono text-xs text-text-muted">
                已选 {{ selectedDateKeys.size }} 个
              </span>
              <Button
                v-if="selectedDateKeys.size > 0"
                severity="secondary"
                label="取消选择"
                @click="clearSelection"
              />
              <Button
                v-if="selectedDateKeys.size > 0"
                severity="secondary"
                label="批量编辑"
                @click="batchMode = !batchMode"
              />
            </div>
          </div>

          <!-- Batch edit panel -->
          <div
            v-if="batchMode && selectedDateKeys.size > 0"
            class="flex items-center gap-3 border border-border bg-surface-raised p-3"
          >
            <div class="flex items-center gap-2">
              <label class="mono-label text-xs">偏移 ±N</label>
              <InputNumber
                v-model="batchOffset"
                class="w-24"
                :min="-999"
                :max="999"
                placeholder="±N"
              />
              <Button severity="primary" label="应用" @click="applyBatchOffset()" />
            </div>
            <div class="flex items-center gap-2">
              <label class="mono-label text-xs">缩放 %</label>
              <InputNumber
                v-model="batchPercent"
                class="w-24"
                :min="-99"
                :max="999"
                placeholder="±%"
              />
              <Button severity="primary" label="应用" @click="applyBatchPercent()" />
            </div>
          </div>

          <!-- Grid Cards -->
          <div class="grid grid-cols-3 md:grid-cols-5 gap-2">
            <div
              v-for="entry in rollingEntries"
              :key="entry.dateKey"
              class="border cursor-pointer transition-colors p-2"
              :class="[
                heatmapColor(entry.dateKey),
                selectedDateKeys.has(entry.dateKey) ? '!border-accent ring-1 ring-accent' : '',
              ]"
              @click="toggleDateSelection(entry.dateKey)"
            >
              <div class="flex justify-between items-start mb-1">
                <span class="font-mono text-xs text-text-muted">{{ entry.dateKey }}</span>
                <span class="font-mono text-xs text-text-secondary">{{ entry.dayLabel }}</span>
              </div>
              <div class="flex items-center gap-1 mb-1">
                <Tag v-if="localIsHoliday(entry.dateKey)" severity="warn" value="节假日" class="!text-xs !py-0" />
              </div>
              <div class="flex justify-between items-baseline">
                <span class="font-mono text-xs text-text-muted">
                  {{ formatPrice(localDefaultPrice(entry.dateKey)) }}
                </span>
                <span class="font-mono text-sm font-bold" :class="{
                  'text-status-success': localEffectivePrice(entry.dateKey) < localDefaultPrice(entry.dateKey),
                  'text-status-error': localEffectivePrice(entry.dateKey) > localDefaultPrice(entry.dateKey),
                  'text-text-primary': localEffectivePrice(entry.dateKey) === localDefaultPrice(entry.dateKey),
                }">
                  {{ formatPrice(localEffectivePrice(entry.dateKey)) }}
                </span>
              </div>
              <input
                v-if="isAdmin"
                type="number"
                step="0.5"
                min="0"
                class="input w-full mt-1 text-xs"
                :value="dynamicPrices[entry.dateKey] ?? ''"
                :placeholder="String(localDefaultPrice(entry.dateKey))"
                @input="setDynamicPrice(entry.dateKey, $event)"
                @click.stop
              />
            </div>
          </div>

          <Button
            v-if="isAdmin"
            severity="secondary"
            label="重置动态价格"
            @click="resetDynamicPrices"
          />
        </div>
      </div>

      <!-- Save -->
      <div class="flex items-center gap-3 mt-4 pt-3 border-t border-border">
        <Button
          v-if="isAdmin"
          severity="primary"
          :disabled="saving"
          :label="saving ? '保存中...' : '保存所有配置'"
          @click="handleSave"
        />
      </div>
      </template>
    </div>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
