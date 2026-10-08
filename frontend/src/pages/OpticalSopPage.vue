<script setup lang="ts">
import { ref, computed, onMounted, watch } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useOpticalSopStore } from '@/stores/optical-sop'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { formatTime } from '@/composables/useFormatTime'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import TabBar from '@/components/common/TabBar.vue'

// ── State ────────────────────────────────────────────────────────────

const auth = useAuthStore()
const sop = useOpticalSopStore()
const toast = useToast()
const confirm = useConfirm()

const activeTab = ref<'list' | 'wizard'>('list')

// ── Inspection list ──────────────────────────────────────────────────

const listPage = ref(0)
const listRows = ref(10)
const listGradeFilter = ref('')
const listSerialFilter = ref('')

const listTotalRecords = computed(() => sop.totalRecords)

async function loadList() {
  try {
    const params: Record<string, any> = {
      page: String(listPage.value + 1),
      page_size: String(listRows.value),
    }
    if (listGradeFilter.value) params.overall_grade = listGradeFilter.value
    if (listSerialFilter.value.trim()) params.device_serial_no = listSerialFilter.value.trim()
    await sop.fetchList(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载检查记录失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onListPageChange(e: { first: number; rows: number; page: number }) {
  listPage.value = e.page
  listRows.value = e.rows
  loadList()
}

function selectInspection(item: any) {
  sop.currentInspection = item
  activeTab.value = 'wizard'
}

// ── Stats ────────────────────────────────────────────────────────────

const statsCards = computed(() => {
  if (!sop.stats) return []
  const s = sop.stats
  return [
    { label: '检查总数', value: s.total ?? 0, color: 'var(--text-primary)' },
    { label: '通过', value: s.passCount ?? 0, color: 'var(--status-success)' },
    { label: '损伤', value: s.damageCount ?? 0, color: 'var(--status-warning)' },
  ]
})

// ── Inspection Wizard ────────────────────────────────────────────────

const wizardLoading = ref(false)
const stepNotes = ref<Record<string, string>>({})
const stepPassed = ref<Record<string, boolean | null>>({})

const steps = [
  { key: 'body', field: 'bodyOk', noteField: 'bodyNote', label: '外观检查', description: '检查机身是否有划痕、凹陷、变形、漆面损伤' },
  { key: 'lens', field: 'lensOk', noteField: 'lensNote', label: '镜头检查', description: '检查镜头是否有划痕、灰尘、霉斑、对焦异常' },
  { key: 'screen', field: 'screenOk', noteField: 'screenNote', label: '屏幕检查', description: '检查屏幕是否有裂纹、坏点、触控失灵、显示异常' },
  { key: 'accessory', field: 'accessoryOk', noteField: 'accessoryNote', label: '配件检查', description: '检查电池、充电器、数据线、保护壳等配件是否齐全完好' },
  { key: 'function', field: 'functionOk', noteField: 'functionNote', label: '功能检查', description: '检查拍摄、录音、防抖、WiFi、蓝牙等功能是否正常' },
]

function initWizardFromInspection() {
  const insp = sop.currentInspection
  stepNotes.value = {}
  stepPassed.value = {}

  if (insp) {
    for (const step of steps) {
      const val = insp[step.field] ?? insp[`${step.key}_ok`]
      stepPassed.value[step.key] = val !== undefined ? val : null
      stepNotes.value[step.key] = insp[step.noteField] ?? insp[`${step.key}_note`] ?? ''
    }
  }
}

watch(() => sop.currentInspection, () => {
  initWizardFromInspection()
})

function currentStepPassed(stepKey: string): boolean | null {
  return stepPassed.value[stepKey] ?? null
}

function toggleStep(stepKey: string) {
  const current = currentStepPassed(stepKey)
  const next = current === null ? true : current === true ? false : null
  stepPassed.value[stepKey] = next
}

function stepPassedLabel(stepKey: string): { text: string; class: string } {
  const v = currentStepPassed(stepKey)
  if (v === true) return { text: '通过', class: 'badge-success' }
  if (v === false) return { text: '未通过', class: 'badge-error' }
  return { text: '待检查', class: 'badge-warning' }
}

function allStepsCompleted(): boolean {
  return steps.every(s => currentStepPassed(s.key) !== null)
}

async function handleSaveStep(stepKey: string) {
  if (!sop.currentInspection) return
  wizardLoading.value = true
  try {
    await sop.updateStep({
      inspection_id: sop.currentInspection.id,
      step: stepKey,
      passed: currentStepPassed(stepKey) ?? false,
      note: stepNotes.value[stepKey],
    })
    toast.add({ severity: 'success', summary: '步骤已保存', life: 1500 })
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '保存失败', detail: e.message || '请重试', life: 4000 })
  } finally {
    wizardLoading.value = false
  }
}

async function handleComplete() {
  if (!sop.currentInspection) return
  confirm.require({
    message: `确定要完成此项检查吗？系统将计算整体评级并生成损伤报告。`,
    header: '完成检查',
    accept: async () => {
      wizardLoading.value = true
      try {
        const result = await sop.completeInspection(sop.currentInspection.id)
        toast.add({
          severity: 'success',
          summary: '检查已完成',
          detail: `评级：${gradeLabel(result.overallGrade ?? result.overall_grade ?? sop.currentInspection?.overallGrade ?? sop.currentInspection?.overall_grade ?? '-')}`,
          life: 3000,
        })
        await loadList()
        await sop.fetchStats()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '完成检查失败', detail: e.message || '请重试', life: 4000 })
      } finally {
        wizardLoading.value = false
      }
    },
  })
}

// ── Create inspection dialog ─────────────────────────────────────────

const createOpen = ref(false)
const createForm = ref({
  order_id: '',
  device_serial_no: '',
})
const createLoading = ref(false)
const createError = ref('')

function openCreate() {
  createForm.value = { order_id: '', device_serial_no: '' }
  createError.value = ''
  createOpen.value = true
}

function closeCreate() {
  createOpen.value = false
}

async function handleCreate() {
  const f = createForm.value
  if (!f.order_id.trim() || !f.device_serial_no.trim()) {
    createError.value = '请填写订单ID和设备序列号'
    return
  }
  createLoading.value = true
  createError.value = ''
  try {
    await sop.createInspection({
      order_id: f.order_id.trim(),
      device_serial_no: f.device_serial_no.trim(),
    })
    closeCreate()
    toast.add({ severity: 'success', summary: '检查记录已创建', life: 2000 })
    activeTab.value = 'wizard'
    await loadList()
  } catch (e: any) {
    createError.value = e.message || '创建失败'
  } finally {
    createLoading.value = false
  }
}

// ── Helpers ──────────────────────────────────────────────────────────

function gradeBadge(grade: string): string {
  switch (grade) {
    case 'pass': return 'badge-success'
    case 'minor':
    case 'minor_damage': return 'badge-warning'
    case 'major':
    case 'major_damage': return 'badge-error'
    case 'total_loss': return 'badge-error'
    default: return 'badge'
  }
}

function gradeLabel(grade: string): string {
  switch (grade) {
    case 'pass': return '通过'
    case 'minor':
    case 'minor_damage': return '轻微损伤'
    case 'major':
    case 'major_damage': return '严重损伤'
    case 'total_loss': return '全损'
    default: return grade || '-'
  }
}

function stepName(key: string): string {
  return steps.find(s => s.key === key)?.label ?? key
}

// ── Tab definitions ──────────────────────────────────────────────────

const tabs = [
  { key: 'list' as const, label: '检查记录' },
  { key: 'wizard' as const, label: '检查向导' },
]

// ── Init ─────────────────────────────────────────────────────────────

onMounted(async () => {
  await Promise.all([
    loadList(),
    sop.fetchStats(),
  ])
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-19 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-19</span>
      <div class="structure-line" />
      <span class="module-page-label">光学检测</span>
    </div>

    <!-- Tab bar -->
    <TabBar :tabs="tabs" v-model="activeTab" />

    <!-- ═══ Tab: Inspection List ═══════════════════════════════════════ -->
    <div v-if="activeTab === 'list'" class="space-y-4">
      <!-- Stats cards -->
      <div class="grid grid-cols-2 md:grid-cols-3 gap-3">
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

      <!-- Table -->
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">检查记录</h2>
          <Button
            severity="primary"
            label="新建检查"
            @click="openCreate"
          />
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="listSerialFilter"
            class="w-40"
            placeholder="设备序列号"
            @keydown.enter="listPage = 0; loadList()"
          />
          <Select
            v-model="listGradeFilter"
            :options="[
              { label: '全部评级', value: '' },
              { label: '通过', value: 'pass' },
              { label: '轻微损伤', value: 'minor' },
              { label: '严重损伤', value: 'major' },
              { label: '全损', value: 'total_loss' },
            ]"
            option-label="label"
            option-value="value"
            class="w-36"
            @change="listPage = 0; loadList()"
          />
          <Button severity="secondary" label="查询" @click="listPage = 0; loadList()" />
        </div>

        <!-- DataTable -->
        <DataTable
          :value="sop.inspections"
          :loading="sop.loading"
          :paginator="true"
          :rows="listRows"
          :first="listPage * listRows"
          :totalRecords="listTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          selectionMode="single"
          @page="onListPageChange"
          @row-click="({ data }: { data: any }) => selectInspection(data)"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无检查记录</span>
            </div>
          </template>
          <Column field="id" header="ID">
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ data.id?.slice(0, 8) ?? '-' }}</span>
            </template>
          </Column>
          <Column field="order_id" header="订单号">
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm">{{ data.order_id ?? data.order_no ?? data.orderId ?? '-' }}</span>
            </template>
          </Column>
          <Column field="device_serial_no" header="设备序列号">
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm">{{ data.device_serial_no ?? data.deviceSerialNo ?? '-' }}</span>
            </template>
          </Column>
          <Column field="overall_grade" header="评级" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="gradeBadge(data.overall_grade ?? data.overallGrade)">
                {{ gradeLabel(data.overall_grade ?? data.overallGrade) }}
              </span>
            </template>
          </Column>
          <Column field="created_at" header="时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.created_at ?? data.createdAt) }}</span>
            </template>
          </Column>
        </DataTable>
      </div>
    </div>

    <!-- ═══ Tab: Inspection Wizard ═════════════════════════════════════ -->
    <div v-if="activeTab === 'wizard'" class="space-y-4">
      <!-- No inspection selected -->
      <div v-if="!sop.currentInspection" class="panel">
        <div class="py-8 text-center">
          <p class="font-mono text-sm text-text-muted mb-3">请从左侧检查列表中选择一条记录，或新建检查。</p>
          <Button severity="primary" label="新建检查" @click="openCreate" />
        </div>
      </div>

      <!-- Wizard: 5-step checklist -->
      <div v-else>
        <!-- Inspection header -->
        <div class="panel flex items-center justify-between mb-0">
          <div>
            <h2 class="panel-title mb-1">光学检测向导</h2>
            <div class="flex items-center gap-4 font-mono text-sm">
              <span class="text-text-muted">订单：<span class="text-text-primary">{{ sop.currentInspection.order_id ?? sop.currentInspection.orderId ?? '-' }}</span></span>
              <span class="text-text-muted">设备：<span class="text-text-primary">{{ sop.currentInspection.device_serial_no ?? sop.currentInspection.deviceSerialNo ?? '-' }}</span></span>
              <span class="text-text-muted">检查ID：<span class="text-text-primary font-mono">{{ sop.currentInspection.id?.slice(0, 8) ?? '-' }}</span></span>
            </div>
          </div>
          <!-- Overall grade badge (if completed) -->
          <div v-if="sop.currentInspection.overall_grade || (sop.currentInspection.overallGrade)">
            <span :class="['badge text-sm', gradeBadge(sop.currentInspection.overall_grade ?? sop.currentInspection.overallGrade)]">
              {{ gradeLabel(sop.currentInspection.overall_grade ?? sop.currentInspection.overallGrade) }}
            </span>
          </div>
        </div>

        <!-- Step cards -->
        <div class="space-y-3 mt-4">
          <div
            v-for="step in steps"
            :key="step.key"
            class="panel"
            style="padding: 1rem;"
          >
            <div class="flex items-start justify-between gap-4">
              <!-- Step info -->
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-3 mb-2">
                  <h3 class="font-mono font-bold text-md text-text-primary">
                    {{ steps.indexOf(step) + 1 }}. {{ step.label }}
                  </h3>
                  <span :class="['badge text-xs', stepPassedLabel(step.key).class]">
                    {{ stepPassedLabel(step.key).text }}
                  </span>
                </div>
                <p class="font-mono text-xs text-text-muted mb-2">{{ step.description }}</p>

                <!-- Remark input -->
                <div>
                  <label class="mono-label block mb-1">备注</label>
                  <Textarea
                    v-model="stepNotes[step.key]"
                    class="w-full"
                    placeholder="填写检查备注..."
                    rows="2"
                  />
                </div>
              </div>

              <!-- Toggle + Save buttons -->
              <div class="flex flex-col items-end gap-2 shrink-0">
                <div class="flex items-center gap-2">
                  <Button
                    :severity="currentStepPassed(step.key) === true ? 'primary' : 'secondary'"
                    :label="currentStepPassed(step.key) === true ? '通过' : 'Pass'"
                    size="small"
                    @click="toggleStep(step.key)"
                  />
                  <Button
                    :severity="currentStepPassed(step.key) === false ? 'danger' : 'secondary'"
                    :label="currentStepPassed(step.key) === false ? '未通过' : 'Fail'"
                    size="small"
                    @click="toggleStep(step.key)"
                  />
                </div>
                <Button
                  severity="secondary"
                  label="保存此步"
                  size="small"
                  :disabled="wizardLoading || currentStepPassed(step.key) === null"
                  @click="handleSaveStep(step.key)"
                />
              </div>
            </div>
          </div>
        </div>

        <!-- Completion section -->
        <div class="panel mt-4 flex items-center justify-between">
          <div>
            <span class="font-mono text-sm text-text-secondary">
              {{ steps.filter(s => currentStepPassed(s.key) !== null).length }} / {{ steps.length }} 步已完成
            </span>
            <span v-if="!allStepsCompleted()" class="font-mono text-xs text-text-muted ml-3">
              请完成所有步骤后再提交
            </span>
          </div>
          <Button
            severity="primary"
            label="完成检查"
            :disabled="wizardLoading || !allStepsCompleted()"
            @click="handleComplete"
          />
        </div>
      </div>
    </div>

    <!-- ═══ Create Inspection Dialog ═══════════════════════════════════ -->
    <Dialog
      v-model:visible="createOpen"
      modal
      header="新建光学检测"
      :style="{ width: '28rem' }"
    >
      <div class="space-y-3">
        <div>
          <label class="mono-label block mb-1">订单ID</label>
          <InputText v-model="createForm.order_id" class="w-full" placeholder="请输入订单ID" />
        </div>
        <div>
          <label class="mono-label block mb-1">设备序列号</label>
          <InputText v-model="createForm.device_serial_no" class="w-full" placeholder="请输入设备序列号" />
        </div>
        <p v-if="createError" class="text-xs font-mono text-status-error">{{ createError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="createLoading" @click="closeCreate" />
        <Button severity="primary" :label="createLoading ? '创建中...' : '确认创建'" :disabled="createLoading" @click="handleCreate" />
      </template>
    </Dialog>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>

<style scoped>
.panel {
  padding: 1rem;
}
</style>
