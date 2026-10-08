<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useModelsStore } from '@/stores/models'
import { usePricingStore } from '@/stores/pricing'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import type { DeviceModel } from '@/api/models'

const store = useModelsStore()
const pricing = usePricingStore()
const toast = useToast()
const confirm = useConfirm()

// ==================== Selection ====================
const selectedModels = ref<DeviceModel[]>([])
const bulkDeleting = ref(false)

async function bulkDelete() {
  if (selectedModels.value.length === 0) return
  confirm.require({
    message: `确认删除选中的 ${selectedModels.value.length} 个型号？此操作不可撤销。`,
    header: '批量删除确认',
    accept: async () => {
      bulkDeleting.value = true
      let deleted = 0
      let failed = 0
      for (const model of selectedModels.value) {
        try {
          await store.remove(model.id)
          deleted++
        } catch {
          failed++
        }
      }
      selectedModels.value = []
      bulkDeleting.value = false
      if (failed > 0) {
        toast.add({ severity: 'warn', summary: `已删除 ${deleted} 个，${failed} 个失败`, life: 3000 })
      } else {
        toast.add({ severity: 'success', summary: `已删除 ${deleted} 个型号`, life: 2000 })
      }
    },
  })
}

// Edit modal
const modalOpen = ref(false)
const editingId = ref<string | null>(null)
const form = ref({ name: '', category: '', prefix: '', weekdayPrice: 0, weekendPrice: 0 })
const useCustomPrice = ref(false)
const formLoading = ref(false)
const formError = ref('')

const globalWeekday = computed(() => pricing.config.baseWeekdayPrice)
const globalWeekend = computed(() => pricing.config.baseWeekendPrice)

function openCreate() {
  editingId.value = null
  form.value = { name: '', category: '', prefix: '', weekdayPrice: 0, weekendPrice: 0 }
  useCustomPrice.value = false
  formError.value = ''
  modalOpen.value = true
}

function openEdit(model: DeviceModel) {
  editingId.value = model.id
  const hasCustom = model.weekdayPrice !== null && model.weekendPrice !== null
  useCustomPrice.value = hasCustom
  form.value = {
    name: model.name,
    category: model.category,
    prefix: model.prefix,
    weekdayPrice: model.weekdayPrice ?? pricing.config.baseWeekdayPrice,
    weekendPrice: model.weekendPrice ?? pricing.config.baseWeekendPrice,
  }
  formError.value = ''
  modalOpen.value = true
}

function closeModal() {
  modalOpen.value = false
}

async function handleSave() {
  if (!form.value.name.trim() || !form.value.prefix.trim()) {
    formError.value = '名称和前缀不能为空'
    return
  }
  formLoading.value = true
  formError.value = ''
  try {
    const payload: any = {
      name: form.value.name.trim(),
      category: form.value.category.trim(),
      prefix: form.value.prefix.trim(),
    }
    if (useCustomPrice.value) {
      payload.weekdayPrice = form.value.weekdayPrice
      payload.weekendPrice = form.value.weekendPrice
    } else {
      payload.useGlobalPrice = true
    }
    if (editingId.value) {
      await store.updateFull(editingId.value, payload)
      toast.add({ severity: 'success', summary: '更新成功', detail: `型号 "${form.value.name}" 已更新`, life: 2000 })
    } else {
      await store.create(payload)
      toast.add({ severity: 'success', summary: '创建成功', detail: `型号 "${form.value.name}" 已创建`, life: 2000 })
    }
    closeModal()
  } catch (e: any) {
    formError.value = e?.message || '操作失败'
  } finally {
    formLoading.value = false
  }
}

function handleDelete(model: DeviceModel) {
  confirm.require({
    message: `确认删除型号 "${model.name}"？此操作不可撤销。`,
    header: '删除确认',
    accept: async () => {
      try {
        await store.remove(model.id)
        toast.add({ severity: 'success', summary: '已删除', life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

function formatTime(iso: string): string {
  if (!iso) return '-'
  const d = new Date(iso)
  const y = d.getFullYear()
  const m = String(d.getMonth() + 1).padStart(2, '0')
  const day = String(d.getDate()).padStart(2, '0')
  const h = String(d.getHours()).padStart(2, '0')
  const mi = String(d.getMinutes()).padStart(2, '0')
  return `${y}-${m}-${day} ${h}:${mi}`
}

onMounted(() => {
  store.fetchAll().catch(e => toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 }))
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-10 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-10</span>
      <div class="structure-line" />
      <span class="module-page-label">型号管理</span>
    </div>

    <div class="panel">
      <div class="flex items-center justify-between mb-3">
        <h2 class="panel-title mb-0">型号管理</h2>
        <Button severity="primary" label="添加型号" @click="openCreate" />
      </div>

      <div v-if="selectedModels.length > 0" class="flex items-center gap-2 mb-3 p-2 border border-accent bg-accent-muted">
        <span class="font-mono text-xs">已选 {{ selectedModels.length }} 个型号</span>
        <Button severity="danger" :disabled="bulkDeleting" :label="bulkDeleting ? '删除中...' : `批量删除 (${selectedModels.length})`" @click="bulkDelete" />
      </div>

      <DataTable
        v-model:selection="selectedModels"
        :value="store.models"
        :loading="store.loading"
        :paginator="true"
        :rows="10"
        :rowsPerPageOptions="[10, 20, 30, 50]"
        paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
        currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
        :resizableColumns="true"
        columnResizeMode="expand"
        removableSort
      >
        <template #empty>
          <div class="py-6 text-center">
            <span class="font-mono text-xs text-text-muted">暂无型号，点击"添加型号"创建</span>
          </div>
        </template>
        <Column selectionMode="multiple" headerStyle="width: 3rem" />
        <Column field="name" header="名称" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <span class="font-mono">{{ data.name }}</span>
          </template>
        </Column>
        <Column field="category" header="类别" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            {{ data.category || '-' }}
          </template>
        </Column>
        <Column field="prefix" header="前缀" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <span class="font-mono text-xs">{{ data.prefix }}</span>
          </template>
        </Column>
        <Column field="weekdayPrice" header="平日价" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <span class="font-mono">&yen;{{ data.weekdayPrice?.toFixed(2) ?? '-' }}</span>
          </template>
        </Column>
        <Column field="weekendPrice" header="周末价" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <span class="font-mono">&yen;{{ data.weekendPrice?.toFixed(2) ?? '-' }}</span>
          </template>
        </Column>
        <Column field="enabled" header="状态" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <Tag :severity="data.enabled ? 'success' : 'danger'" :value="data.enabled ? '启用' : '禁用'" />
          </template>
        </Column>
        <Column field="createdAt" header="创建时间" sortable>
          <template #body="{ data }: { data: DeviceModel }">
            <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.createdAt) }}</span>
          </template>
        </Column>
        <Column header="操作">
          <template #body="{ data }: { data: DeviceModel }">
            <div class="flex items-center gap-2">
              <Button severity="secondary" label="编辑" @click="openEdit(data)" />
              <Button severity="danger" label="删除" @click="handleDelete(data)" />
            </div>
          </template>
        </Column>
      </DataTable>
    </div>

    <!-- Edit/Create Modal -->
    <Dialog
      v-model:visible="modalOpen"
      modal
      :header="editingId ? '编辑型号' : '添加型号'"
      :style="{ width: '28rem' }"
    >
      <div class="space-y-3">
        <div>
          <label class="mono-label block mb-1">名称</label>
          <InputText v-model="form.name" class="w-full" placeholder="例如：Pocket 3" @keydown.enter="handleSave" />
        </div>
        <div>
          <label class="mono-label block mb-1">类别</label>
          <InputText v-model="form.category" class="w-full" placeholder="例如：camera" />
        </div>
        <div>
          <label class="mono-label block mb-1">序列号前缀</label>
          <InputText v-model="form.prefix" class="w-full" placeholder="例如：5W" />
        </div>
        <div class="flex items-center justify-between py-1">
          <span class="mono-label">自定义独立价格</span>
          <InputSwitch v-model="useCustomPrice" />
        </div>
        <p class="text-xs text-text-muted font-mono">
          {{ useCustomPrice ? '为该型号设置独立的基础价格' : `使用全局基础价格 (平日 ¥${globalWeekday} / 周末 ¥${globalWeekend})` }}
        </p>

        <div v-if="useCustomPrice" class="grid grid-cols-2 gap-2">
          <div style="min-width:0">
            <label class="mono-label block mb-1">平日价格</label>
            <InputNumber v-model="form.weekdayPrice" :min-fraction-digits="2" :min="0" class="w-full" style="min-width:0" />
          </div>
          <div style="min-width:0">
            <label class="mono-label block mb-1">周末价格</label>
            <InputNumber v-model="form.weekendPrice" :min-fraction-digits="2" :min="0" class="w-full" style="min-width:0" />
          </div>
        </div>

        <p v-if="formError" class="text-xs font-mono text-btn-danger-text">{{ formError }}</p>
      </div>

      <template #footer>
        <Button severity="secondary" label="取消" :disabled="formLoading" @click="closeModal" />
        <Button severity="primary" :label="formLoading ? '保存中...' : (editingId ? '保存' : '创建')" :disabled="formLoading" @click="handleSave" />
      </template>
    </Dialog>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
