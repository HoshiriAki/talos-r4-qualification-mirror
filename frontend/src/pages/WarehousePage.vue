<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import type { Warehouse, WarehouseStats, RegionRule, WarehouseDevice } from '@/api/warehouses'
import * as warehousesApi from '@/api/warehouses'

const toast = useToast()
const confirm = useConfirm()

// ── State ──
const stats = ref<WarehouseStats[]>([])
const loading = ref(false)

// Regions expanded per warehouse
const expandedRegions = ref<Record<string, boolean>>({})
const regionsData = ref<Record<string, RegionRule[]>>({})
const regionsLoading = ref<Record<string, boolean>>({})

// Device list dialog
const deviceDialogVisible = ref(false)
const deviceDialogWhId = ref('')
const deviceDialogName = ref('')
const devices = ref<WarehouseDevice[]>([])
const deviceTotal = ref(0)
const devicePage = ref(1)
const devicePageSize = ref(20)
const deviceKeyword = ref('')
const devicesLoading = ref(false)

// Edit dialog
const editDialogVisible = ref(false)
const editId = ref<string | null>(null)
const editForm = ref({
  name: '', type: 'owned' as 'owned' | 'partner', enabled: true,
  address: '', contactName: '', contactPhone: '', notes: '', capacity: 0,
})
const editLoading = ref(false)
const editError = ref('')

// Region rule dialog
const ruleDialogVisible = ref(false)
const ruleWhId = ref('')
const ruleForm = ref({ province: '', shippingDays: 1, returnDays: 1, isPrimary: false })
const ruleLoading = ref(false)
const ruleError = ref('')

// All provinces for dropdown
const allProvinces = ref<string[]>([])

// ── Computed ──
const totalDevices = computed(() => stats.value.reduce((s, w) => s + w.totalDevices, 0))
const totalAvailable = computed(() => stats.value.reduce((s, w) => s + w.availableDevices, 0))

// ── Data loading ──
async function loadData() {
  loading.value = true
  try {
    const r = await warehousesApi.fetchWarehouseStats()
    if (r.ok) stats.value = r.warehouses
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载失败', detail: e.message, life: 4000 })
  } finally {
    loading.value = false
  }
}

async function loadProvinces() {
  try {
    const r = await fetch('/api/pricing/provinces').then(r => r.json())
    if (r.provinces) allProvinces.value = r.provinces
  } catch { /* fallback: empty */ }
}

// ── Region rule expand ──
async function toggleRegions(whId: string) {
  if (expandedRegions.value[whId]) {
    expandedRegions.value[whId] = false
    return
  }
  expandedRegions.value[whId] = true
  if (!regionsData.value[whId]) {
    regionsLoading.value[whId] = true
    try {
      const r = await warehousesApi.fetchWarehouse(whId)
      if (r.ok) regionsData.value[whId] = r.regions || []
    } catch { /* ignore */ }
    finally { regionsLoading.value[whId] = false }
  }
}

// ── Device list ──
async function openDeviceList(whId: string, whName: string) {
  deviceDialogWhId.value = whId
  deviceDialogName.value = whName
  devicePage.value = 1
  deviceKeyword.value = ''
  deviceDialogVisible.value = true
  await loadDevices()
}

async function loadDevices() {
  devicesLoading.value = true
  try {
    const query: Record<string, string> = { page: String(devicePage.value), pageSize: String(devicePageSize.value) }
    if (deviceKeyword.value) query.keyword = deviceKeyword.value
    const r = await warehousesApi.fetchWarehouseDevices(deviceDialogWhId.value, query)
    if (r.ok) {
      devices.value = r.data
      deviceTotal.value = r.total
    }
  } catch { /* ignore */ }
  finally { devicesLoading.value = false }
}

function onDevicePageChange(e: { first: number; rows: number }) {
  devicePage.value = Math.floor(e.first / e.rows) + 1
  devicePageSize.value = e.rows
  loadDevices()
}

// ── Edit dialog ──
function openCreate() {
  editId.value = null
  editForm.value = { name: '', type: 'owned', enabled: true, address: '', contactName: '', contactPhone: '', notes: '', capacity: 0 }
  editError.value = ''
  editDialogVisible.value = true
}

function openEdit(wh: WarehouseStats) {
  editId.value = wh.id
  editForm.value = {
    name: wh.name, type: wh.type, enabled: wh.enabled,
    address: wh.address || '', contactName: wh.contactName || '', contactPhone: wh.contactPhone || '',
    notes: wh.notes || '', capacity: wh.capacity || 0,
  }
  editError.value = ''
  editDialogVisible.value = true
}

async function saveEdit() {
  if (!editForm.value.name.trim()) {
    editError.value = '仓库名称不能为空'
    return
  }
  editLoading.value = true
  editError.value = ''
  try {
    if (editId.value) {
      await warehousesApi.updateWarehouse(editId.value, editForm.value)
    } else {
      await warehousesApi.createWarehouse(editForm.value)
    }
    editDialogVisible.value = false
    toast.add({ severity: 'success', summary: '保存成功', life: 2000 })
    await loadData()
  } catch (e: any) {
    editError.value = e?.message || '操作失败'
  } finally {
    editLoading.value = false
  }
}

// ── Delete ──
function handleDelete(wh: WarehouseStats) {
  confirm.require({
    message: `确认删除仓库 "${wh.name}"？${wh.totalDevices > 0 ? `该仓下有 ${wh.totalDevices} 台设备，无法删除。` : '关联区域规则将一并删除。'}`,
    header: '删除确认',
    accept: async () => {
      try {
        await warehousesApi.deleteWarehouse(wh.id)
        toast.add({ severity: 'success', summary: '已删除', life: 2000 })
        await loadData()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

// ── Region rule ──
function openAddRule(whId: string) {
  ruleWhId.value = whId
  ruleForm.value = { province: '', shippingDays: 1, returnDays: 1, isPrimary: false }
  ruleError.value = ''
  ruleDialogVisible.value = true
}

function openEditRule(whId: string, rule: RegionRule) {
  ruleWhId.value = whId
  ruleForm.value = { province: rule.province, shippingDays: rule.shippingDays, returnDays: rule.returnDays, isPrimary: rule.isPrimary }
  ruleError.value = ''
  ruleDialogVisible.value = true
}

async function saveRule() {
  if (!ruleForm.value.province.trim()) {
    ruleError.value = '省份不能为空'
    return
  }
  ruleLoading.value = true
  ruleError.value = ''
  try {
    const r = await warehousesApi.upsertRegionRule(ruleWhId.value, ruleForm.value)
    if (r.ok) {
      regionsData.value[ruleWhId.value] = r.regions
      ruleDialogVisible.value = false
      toast.add({ severity: 'success', summary: '区域规则已更新', life: 2000 })
      await loadData() // refresh stats in case primary changed
    }
  } catch (e: any) {
    ruleError.value = e?.message || '操作失败'
  } finally {
    ruleLoading.value = false
  }
}

async function deleteRule(whId: string, province: string) {
  confirm.require({
    message: `确认删除 "${province}" 的区域规则？`,
    header: '删除确认',
    accept: async () => {
      try {
        await warehousesApi.deleteRegionRule(whId, province)
        // Refresh
        const r = await warehousesApi.fetchWarehouse(whId)
        if (r.ok) regionsData.value[whId] = r.regions
        toast.add({ severity: 'success', summary: '已删除', life: 2000 })
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message, life: 4000 })
      }
    },
  })
}

// ── Helpers ──
const typeLabel: Record<string, string> = { owned: '自有', partner: '合作' }
const typeSeverity: Record<string, string> = { owned: 'success', partner: 'info' }

onMounted(() => {
  loadData()
  loadProvinces()
})
</script>

<template>
  <div class="flex flex-col gap-4" style="min-height: calc(100vh - 88px)">
    <!-- MODULE-09 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-09</span>
      <div class="structure-line" />
      <span class="module-page-label">仓库管理</span>
    </div>

    <div class="flex items-center justify-between flex-shrink-0">
      <div>
        <h1 class="font-mono font-bold text-2xl text-text-primary tracking-wider">仓库管理</h1>
        <p class="text-xs text-text-muted mt-0.5">
          共 {{ stats.length }} 个仓库 · {{ totalAvailable }} 台在库 / {{ totalDevices }} 台总计
        </p>
      </div>
      <Button severity="primary" label="添加仓库" @click="openCreate" />
    </div>

    <!-- Loading -->
    <div v-if="loading" class="flex-1 flex items-center justify-center">
      <span class="font-mono text-xs text-text-muted">加载中...</span>
    </div>

    <!-- Warehouse Cards -->
    <div v-else class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      <Card v-for="wh in stats" :key="wh.id" class="relative" :class="wh.enabled ? '' : 'opacity-50'">
        <template #content>
          <!-- Header -->
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-2.5">
              <span class="font-mono font-bold text-lg text-text-primary">{{ wh.name }}</span>
              <Tag :severity="(typeSeverity[wh.type] || 'info') as any" :value="typeLabel[wh.type] || wh.type" />
              <Tag :severity="wh.enabled ? 'success' : 'danger'" :value="wh.enabled ? '启用' : '禁用'" />
            </div>
          </div>

          <!-- Meta info -->
          <div class="space-y-1.5 mb-3" v-if="wh.address || wh.contactName || wh.contactPhone">
            <div v-if="wh.address" class="flex items-start gap-2 text-xs">
              <span class="text-text-muted whitespace-nowrap">地址:</span>
              <span class="text-text-primary">{{ wh.address }}</span>
            </div>
            <div v-if="wh.contactName" class="flex items-center gap-2 text-xs">
              <span class="text-text-muted whitespace-nowrap">联系人:</span>
              <span class="text-text-primary">{{ wh.contactName }}</span>
            </div>
            <div v-if="wh.contactPhone" class="flex items-center gap-2 text-xs">
              <span class="text-text-muted whitespace-nowrap">电话:</span>
              <span class="text-text-primary font-mono">{{ wh.contactPhone }}</span>
            </div>
          </div>

          <div v-if="wh.notes" class="bg-surface-raised border-l-2 border-border-active px-3 py-2 mb-3 text-xs text-text-primary break-all">
            {{ wh.notes }}
          </div>

          <!-- Capacity bar -->
          <div v-if="wh.capacity > 0" class="mb-3">
            <div class="flex justify-between text-2xs text-text-muted mb-1">
              <span>在库 {{ wh.availableDevices }} / 容量 {{ wh.capacity }}</span>
              <span>{{ wh.utilizationPercent }}%</span>
            </div>
            <div class="w-full bg-border" style="height: 4px;">
              <div
                class="h-full transition-all duration-300"
                :class="{ 'bg-status-success': wh.utilizationPercent < 60, 'bg-status-warning': wh.utilizationPercent >= 60 && wh.utilizationPercent < 85, 'bg-status-error': wh.utilizationPercent >= 85 }"
                :style="{ width: Math.min(wh.utilizationPercent, 100) + '%' }"
              />
            </div>
          </div>

          <!-- Device counts -->
          <div class="flex gap-4 text-xs border-b border-border pb-3 mb-3">
            <div class="text-center flex-1">
              <div class="text-text-primary font-bold font-mono">{{ wh.availableDevices }}</div>
              <div class="text-text-muted text-2xs">在库</div>
            </div>
            <div class="text-center flex-1">
              <div class="text-text-primary font-bold font-mono">{{ wh.rentedDevices }}</div>
              <div class="text-text-muted text-2xs">租赁中</div>
            </div>
            <div class="text-center flex-1">
              <div class="text-text-primary font-bold font-mono">{{ wh.repairingDevices }}</div>
              <div class="text-text-muted text-2xs">维修</div>
            </div>
          </div>

          <!-- Actions -->
          <div class="flex flex-wrap gap-2">
            <Button severity="secondary" size="small" label="查看设备" @click="openDeviceList(wh.id, wh.name)" />
            <Button
              severity="secondary"
              size="small"
              :label="expandedRegions[wh.id] ? '收起规则' : '区域规则'"
              @click="toggleRegions(wh.id)"
            />
            <span class="flex-1" />
            <Button severity="secondary" size="small" label="编辑" variant="outlined" @click="openEdit(wh)" />
            <Button severity="danger" size="small" label="删除" variant="outlined" @click="handleDelete(wh)" />
          </div>

          <!-- Region Rules (expandable) -->
          <div v-if="expandedRegions[wh.id]" class="mt-3 pt-3 border-t border-border">
            <div class="flex items-center justify-between mb-2">
              <span class="text-xs font-bold text-text-secondary uppercase">区域规则</span>
              <Button severity="secondary" size="small" label="添加规则" @click="openAddRule(wh.id)" />
            </div>

            <div v-if="regionsLoading[wh.id]" class="py-4 text-center text-xs text-text-muted">加载中...</div>

            <DataTable
              v-else-if="regionsData[wh.id] && regionsData[wh.id].length > 0"
              :value="regionsData[wh.id]"
              size="small"
              :paginator="true"
              :rows="5"
              :rowsPerPageOptions="[5, 10, 20]"
              paginatorTemplate="PrevPageLink PageLinks NextPageLink CurrentPageReport"
              currentPageReportTemplate="{first}-{last} / {totalRecords}"
            >
              <Column field="province" header="省份" class="font-mono text-xs" />
              <Column field="shippingDays" header="发货">
                <template #body="{ data }: { data: RegionRule }">{{ data.shippingDays }}天</template>
              </Column>
              <Column field="returnDays" header="退货">
                <template #body="{ data }: { data: RegionRule }">{{ data.returnDays }}天</template>
              </Column>
              <Column field="isPrimary" header="主仓">
                <template #body="{ data }: { data: RegionRule }">
                  <Tag :severity="data.isPrimary ? 'success' : 'info'" :value="data.isPrimary ? '是' : '否'" />
                </template>
              </Column>
              <Column header="操作" style="width:120px">
                <template #body="{ data }: { data: RegionRule }">
                  <div class="flex gap-1">
                    <Button severity="secondary" size="small" label="编辑" @click="openEditRule(wh.id, data)" />
                    <Button severity="danger" size="small" label="删除" @click="deleteRule(wh.id, data.province)" />
                  </div>
                </template>
              </Column>
            </DataTable>

            <div v-else class="py-2 text-center text-xs text-text-muted">暂无区域规则</div>
          </div>
        </template>
      </Card>

      <!-- Empty state -->
      <div v-if="stats.length === 0" class="col-span-full panel text-center py-12">
        <p class="text-text-muted text-sm">暂无仓库</p>
        <p class="text-text-muted text-xs mt-1">点击 "添加仓库" 开始设置</p>
      </div>
    </div>

    <!-- ═══ Edit Dialog ═══ -->
    <Dialog v-model:visible="editDialogVisible" modal :header="editId ? '编辑仓库' : '添加仓库'" :style="{ width: '32rem' }">
      <div class="space-y-3">
        <div class="flex gap-3">
          <div class="flex-1">
            <label class="text-xs font-bold text-text-secondary block mb-1">名称</label>
            <InputText v-model="editForm.name" class="w-full" placeholder="仓库名称" @keydown.enter="saveEdit" />
          </div>
          <div>
            <label class="text-xs font-bold text-text-secondary block mb-1">类型</label>
            <SelectButton
              v-model="editForm.type"
              :options="[{ label: '自有', value: 'owned' }, { label: '合作', value: 'partner' }]"
              option-label="label"
              option-value="value"
              size="small"
            />
          </div>
        </div>

        <div class="flex items-center gap-2">
          <label class="text-xs font-bold text-text-secondary">启用</label>
          <InputSwitch v-model="editForm.enabled" />
        </div>

        <div>
          <label class="text-xs font-bold text-text-secondary block mb-1">地址</label>
          <InputText v-model="editForm.address" class="w-full" placeholder="仓库物理地址" />
        </div>

        <div class="flex gap-3">
          <div class="flex-1">
            <label class="text-xs font-bold text-text-secondary block mb-1">联系人</label>
            <InputText v-model="editForm.contactName" class="w-full" placeholder="联系人姓名" />
          </div>
          <div class="flex-1">
            <label class="text-xs font-bold text-text-secondary block mb-1">联系电话</label>
            <InputText v-model="editForm.contactPhone" class="w-full" placeholder="电话号码" />
          </div>
        </div>

        <div>
          <label class="text-xs font-bold text-text-secondary block mb-1">容量上限（0 = 不限制）</label>
          <InputNumber v-model="editForm.capacity" :min="0" class="w-full" />
        </div>

        <div>
          <label class="text-xs font-bold text-text-secondary block mb-1">备注</label>
          <Textarea v-model="editForm.notes" :rows="2" class="w-full" placeholder="内部备注" />
        </div>

        <p v-if="editError" class="text-xs text-status-error">{{ editError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="editLoading" @click="editDialogVisible = false" />
        <Button severity="primary" :label="editLoading ? '保存中...' : (editId ? '保存' : '创建')" :disabled="editLoading" @click="saveEdit" />
      </template>
    </Dialog>

    <!-- ═══ Device List Dialog ═══ -->
    <Dialog v-model:visible="deviceDialogVisible" modal :header="`${deviceDialogName} — 设备清单`" :style="{ width: '48rem' }" :maximizable="true">
      <div class="flex gap-2 mb-3">
        <InputText v-model="deviceKeyword" class="flex-1" placeholder="搜索序列号或备注..." @keydown.enter="devicePage = 1; loadDevices()" />
        <Button severity="secondary" label="搜索" @click="devicePage = 1; loadDevices()" />
      </div>

      <DataTable
        :value="devices"
        :loading="devicesLoading"
        size="small"
        :paginator="true"
        :rows="devicePageSize"
        :rowsPerPageOptions="[10, 20, 50]"
        :totalRecords="deviceTotal"
        :lazy="true"
        :first="(devicePage - 1) * devicePageSize"
        paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
        currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
        @page="onDevicePageChange"
      >
        <template #empty>
          <div class="py-4 text-center text-xs text-text-muted">暂无设备</div>
        </template>
        <Column field="serialNo" header="序列号" class="font-mono text-xs" />
        <Column field="modelId" header="型号" class="text-xs" />
        <Column field="rentalStatus" header="状态">
          <template #body="{ data }: { data: WarehouseDevice }">
            <Tag
              :severity="data.rentalStatus === '已入库' ? 'success' : data.rentalStatus === '租赁中' ? 'warn' : 'danger'"
              :value="data.rentalStatus"
            />
          </template>
        </Column>
        <Column field="expectedAvailableDate" header="预计归还日" class="text-xs" />
        <Column field="notes" header="备注" class="text-xs" />
      </DataTable>
    </Dialog>

    <!-- ═══ Region Rule Dialog ═══ -->
    <Dialog v-model:visible="ruleDialogVisible" modal header="区域规则" :style="{ width: '26rem' }">
      <div class="space-y-3">
        <div>
          <label class="text-xs font-bold text-text-secondary block mb-1">省份</label>
          <Select
            v-model="ruleForm.province"
            :options="allProvinces.map((p: string) => ({ label: p, value: p }))"
            option-label="label"
            option-value="value"
            placeholder="请选择省份"
            class="w-full"
          />
        </div>
        <div class="flex gap-3">
          <div class="flex-1">
            <label class="text-xs font-bold text-text-secondary block mb-1">发货天数</label>
            <InputNumber v-model="ruleForm.shippingDays" :min="0" class="w-full" />
          </div>
          <div class="flex-1">
            <label class="text-xs font-bold text-text-secondary block mb-1">归还天数</label>
            <InputNumber v-model="ruleForm.returnDays" :min="0" class="w-full" />
          </div>
        </div>
        <div class="flex items-center gap-2">
          <Checkbox v-model="ruleForm.isPrimary" input-id="rule-primary" binary />
          <label for="rule-primary" class="text-xs font-bold text-text-secondary">设为主要仓</label>
        </div>
        <p v-if="ruleError" class="text-xs text-status-error">{{ ruleError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="ruleLoading" @click="ruleDialogVisible = false" />
        <Button severity="primary" :label="ruleLoading ? '保存中...' : '保存'" :disabled="ruleLoading" @click="saveRule" />
      </template>
    </Dialog>
    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
