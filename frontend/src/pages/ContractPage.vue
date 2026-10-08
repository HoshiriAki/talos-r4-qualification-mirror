<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { useContractStore } from '@/stores/contract'
import { useToast } from 'primevue/usetoast'
import { useConfirm } from 'primevue/useconfirm'
import { formatTime } from '@/composables/useFormatTime'
import DataTable from 'primevue/datatable'
import Column from 'primevue/column'
import TabBar from '@/components/common/TabBar.vue'

// ── State ────────────────────────────────────────────────────────────────

const auth = useAuthStore()
const contract = useContractStore()
const toast = useToast()
const confirm = useConfirm()

const activeTab = ref<'templates' | 'contracts' | 'signing'>('templates')

// ── Tab 1: Templates ─────────────────────────────────────────────────────

const tmplPage = ref(0)
const tmplRows = ref(10)
const tmplFilter = ref('')

const tmplTotalRecords = computed(() => contract.templatesTotal)

const tmplOpen = ref(false)
const tmplEditing = ref(false)
const tmplForm = ref({
  id: '',
  name: '',
  type: 'rental' as string,
  description: '',
  content: '',
  enabled: true,
})
const tmplLoading = ref(false)
const tmplError = ref('')

const previewOpen = ref(false)
const previewHtml = ref('')
const previewLoading = ref(false)

async function loadTemplates() {
  try {
    const params: Record<string, any> = {
      page: String(tmplPage.value + 1),
      page_size: String(tmplRows.value),
    }
    if (tmplFilter.value.trim()) params.search = tmplFilter.value.trim()
    await contract.fetchTemplates(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载模板列表失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onTmplPageChange(e: { first: number; rows: number; page: number }) {
  tmplPage.value = e.page
  tmplRows.value = e.rows
  loadTemplates()
}

function openCreateTemplate() {
  tmplForm.value = { id: '', name: '', type: 'rental', description: '', content: '', enabled: true }
  tmplEditing.value = false
  tmplError.value = ''
  tmplOpen.value = true
}

function openEditTemplate(record: any) {
  tmplForm.value = {
    id: record.id,
    name: record.name || '',
    type: record.type || 'rental',
    description: record.description || '',
    content: record.content || record.body || '',
    enabled: record.enabled ?? true,
  }
  tmplEditing.value = true
  tmplError.value = ''
  tmplOpen.value = true
}

function closeTemplateForm() {
  tmplOpen.value = false
}

async function handleTemplateSave() {
  const f = tmplForm.value
  if (!f.name.trim() || !f.content.trim()) {
    tmplError.value = '模板名称和内容不能为空'
    return
  }
  tmplLoading.value = true
  tmplError.value = ''
  try {
    if (tmplEditing.value) {
      await contract.updateTemplate({
        id: f.id,
        name: f.name.trim(),
        type: f.type,
        description: f.description.trim(),
        content: f.content.trim(),
        enabled: f.enabled,
      })
      toast.add({ severity: 'success', summary: '模板已更新', life: 2000 })
    } else {
      await contract.createTemplate({
        name: f.name.trim(),
        type: f.type,
        description: f.description.trim(),
        content: f.content.trim(),
        enabled: f.enabled,
      })
      toast.add({ severity: 'success', summary: '模板已创建', life: 2000 })
    }
    closeTemplateForm()
    await loadTemplates()
  } catch (e: any) {
    tmplError.value = e.message || '保存失败'
  } finally {
    tmplLoading.value = false
  }
}

function confirmDeleteTemplate(record: any) {
  confirm.require({
    message: `确定要删除模板 "${record.name}" 吗？此操作不可撤销。`,
    header: '删除模板',
    accept: async () => {
      try {
        await contract.deleteTemplate({ id: record.id })
        toast.add({ severity: 'success', summary: '模板已删除', life: 2000 })
        await loadTemplates()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '删除失败', detail: e.message || '请重试', life: 4000 })
      }
    },
  })
}

function sanitizeHtml(raw: string): string {
  // Strip script/style/event-handler tags and attributes to prevent stored XSS via v-html
  return raw
    .replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '')
    .replace(/<style\b[^<]*(?:(?!<\/style>)<[^<]*)*<\/style>/gi, '')
    .replace(/\son\w+\s*=\s*"[^"]*"/gi, '')
    .replace(/\son\w+\s*=\s*'[^']*'/gi, '')
    .replace(/javascript\s*:/gi, 'blocked:')
    .replace(/<iframe\b[^<]*(?:(?!<\/iframe>)<[^<]*)*<\/iframe>/gi, '')
}

async function handlePreviewTemplate(record: any) {
  previewLoading.value = true
  previewHtml.value = ''
  previewOpen.value = true
  try {
    const data = await contract.renderTemplate({ id: record.id })
    const raw = data.html || data.rendered || data.content || ''
    previewHtml.value = sanitizeHtml(raw) || '<p class="font-mono text-text-muted">无法渲染预览</p>'
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '渲染失败', detail: e.message || '请重试', life: 4000 })
    previewHtml.value = '<p class="font-mono text-text-muted">渲染失败</p>'
  } finally {
    previewLoading.value = false
  }
}

function closePreview() {
  previewOpen.value = false
  previewHtml.value = ''
}

// ── Tab 2: Contracts ─────────────────────────────────────────────────────

const ctPage = ref(0)
const ctRows = ref(10)
const ctFilter = ref('')
const ctStatusFilter = ref('')

const ctTotalRecords = computed(() => contract.contractsTotal)

const ctDetailOpen = ref(false)

const ctGenOpen = ref(false)
const ctGenForm = ref({
  template_id: '',
  order_id: '',
  customer_name: '',
  customer_phone: '',
  amount: 0,
  device_info: '',
  start_date: '',
  end_date: '',
})
const ctGenLoading = ref(false)
const ctGenError = ref('')

async function loadContracts() {
  try {
    const params: Record<string, any> = {
      page: String(ctPage.value + 1),
      page_size: String(ctRows.value),
    }
    if (ctFilter.value.trim()) params.search = ctFilter.value.trim()
    if (ctStatusFilter.value) params.status = ctStatusFilter.value
    await contract.fetchContracts(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载合同列表失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onCtPageChange(e: { first: number; rows: number; page: number }) {
  ctPage.value = e.page
  ctRows.value = e.rows
  loadContracts()
}

async function viewContractDetail(record: any) {
  try {
    await contract.fetchContract({ id: record.id })
    ctDetailOpen.value = true
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '获取合同详情失败', detail: e.message || '请重试', life: 4000 })
  }
}

function closeContractDetail() {
  ctDetailOpen.value = false
  contract.currentContract = null
}

function openGenerateContract() {
  ctGenForm.value = {
    template_id: '',
    order_id: '',
    customer_name: '',
    customer_phone: '',
    amount: 0,
    device_info: '',
    start_date: '',
    end_date: '',
  }
  ctGenError.value = ''
  ctGenOpen.value = true
}

function closeGenerateContract() {
  ctGenOpen.value = false
}

async function handleGenerateContract() {
  const f = ctGenForm.value
  if (!f.template_id || !f.order_id || !f.customer_name || !f.customer_phone) {
    ctGenError.value = '模板、订单号、客户姓名和手机号不能为空'
    return
  }
  ctGenLoading.value = true
  ctGenError.value = ''
  try {
    await contract.generateContract({
      template_id: f.template_id,
      order_id: f.order_id,
      customer_name: f.customer_name.trim(),
      customer_phone: f.customer_phone.trim(),
      amount: f.amount,
      device_info: f.device_info.trim(),
      start_date: f.start_date,
      end_date: f.end_date,
    })
    closeGenerateContract()
    toast.add({ severity: 'success', summary: '合同已生成', life: 2000 })
    await loadContracts()
  } catch (e: any) {
    ctGenError.value = e.message || '生成失败'
  } finally {
    ctGenLoading.value = false
  }
}

function confirmVoidContract(record: any) {
  confirm.require({
    message: `确定要作废合同 #${record.contract_no || record.contractNo || record.id} 吗？此操作不可撤销。`,
    header: '作废合同',
    accept: async () => {
      try {
        await contract.voidContract({ id: record.id })
        toast.add({ severity: 'success', summary: '合同已作废', life: 2000 })
        await loadContracts()
      } catch (e: any) {
        toast.add({ severity: 'error', summary: '作废失败', detail: e.message || '请重试', life: 4000 })
      }
    },
  })
}

// ── Tab 3: Signing ───────────────────────────────────────────────────────

const signPage = ref(0)
const signRows = ref(10)
const signFilter = ref('')
const signStatusFilter = ref('')

const signTotalRecords = computed(() => contract.signRecordsTotal)

const signReqOpen = ref(false)
const signReqForm = ref({
  contract_id: '',
  signer_name: '',
  signer_phone: '',
  sign_type: 'digital' as string,
})
const signReqLoading = ref(false)
const signReqError = ref('')

const signVerifyOpen = ref(false)
const signVerifyForm = ref({
  contract_id: '',
  code: '',
})
const signVerifyLoading = ref(false)
const signVerifyError = ref('')
const signVerifyResult = ref<any>(null)

async function loadSignRecords() {
  try {
    const params: Record<string, any> = {
      page: String(signPage.value + 1),
      page_size: String(signRows.value),
    }
    if (signFilter.value.trim()) params.search = signFilter.value.trim()
    if (signStatusFilter.value) params.status = signStatusFilter.value
    await contract.fetchSignRecords(params)
  } catch (e: any) {
    toast.add({ severity: 'error', summary: '加载签署记录失败', detail: e.message || '请重试', life: 4000 })
  }
}

function onSignPageChange(e: { first: number; rows: number; page: number }) {
  signPage.value = e.page
  signRows.value = e.rows
  loadSignRecords()
}

function openSignRequest() {
  signReqForm.value = { contract_id: '', signer_name: '', signer_phone: '', sign_type: 'digital' }
  signReqError.value = ''
  signReqOpen.value = true
}

function closeSignRequest() {
  signReqOpen.value = false
}

async function handleSignRequest() {
  const f = signReqForm.value
  if (!f.contract_id || !f.signer_name || !f.signer_phone) {
    signReqError.value = '合同ID、签署人姓名和手机号不能为空'
    return
  }
  signReqLoading.value = true
  signReqError.value = ''
  try {
    await contract.requestSign({
      contract_id: f.contract_id,
      signer_name: f.signer_name.trim(),
      signer_phone: f.signer_phone.trim(),
      sign_type: f.sign_type,
    })
    closeSignRequest()
    toast.add({ severity: 'success', summary: '签署请求已发送', life: 2000 })
    await loadSignRecords()
  } catch (e: any) {
    signReqError.value = e.message || '发起失败'
  } finally {
    signReqLoading.value = false
  }
}

function openSignVerify(record?: any) {
  signVerifyForm.value = {
    contract_id: record?.contract_id || record?.contractId || '',
    code: '',
  }
  signVerifyError.value = ''
  signVerifyResult.value = null
  signVerifyOpen.value = true
}

function closeSignVerify() {
  signVerifyOpen.value = false
  signVerifyResult.value = null
}

async function handleSignVerify() {
  const f = signVerifyForm.value
  if (!f.contract_id || !f.code.trim()) {
    signVerifyError.value = '合同ID和验证码不能为空'
    return
  }
  signVerifyLoading.value = true
  signVerifyError.value = ''
  signVerifyResult.value = null
  try {
    const data = await contract.verifySign({
      contract_id: f.contract_id,
      code: f.code.trim(),
    })
    signVerifyResult.value = data
    toast.add({ severity: 'success', summary: '签名验证通过', life: 2000 })
  } catch (e: any) {
    signVerifyError.value = e.message || '验证失败'
  } finally {
    signVerifyLoading.value = false
  }
}

// ── Helpers ──────────────────────────────────────────────────────────────

function templateTypeLabel(type: string): string {
  switch (type) {
    case 'rental': return '租赁合同'
    case 'waiver': return '免责声明'
    case 'deposit': return '押金协议'
    case 'custom': return '自定义'
    default: return type
  }
}

function templateTypeBadge(type: string): string {
  switch (type) {
    case 'rental': return 'badge-info'
    case 'waiver': return 'badge-warning'
    case 'deposit': return 'badge-success'
    case 'custom': return 'badge'
    default: return 'badge'
  }
}

function contractStatusBadge(status: string): string {
  switch (status) {
    case 'draft': return 'badge'
    case 'pending_sign': return 'badge-warning'
    case 'signed': return 'badge-success'
    case 'voided': return 'badge-error'
    case 'expired': return 'badge-error'
    case 'active': return 'badge-info'
    default: return 'badge'
  }
}

function contractStatusLabel(status: string): string {
  switch (status) {
    case 'draft': return '草稿'
    case 'pending_sign': return '待签署'
    case 'signed': return '已签署'
    case 'voided': return '已作废'
    case 'expired': return '已过期'
    case 'active': return '生效中'
    default: return status
  }
}

function signStatusBadge(status: string): string {
  switch (status) {
    case 'pending': return 'badge-warning'
    case 'sent': return 'badge-info'
    case 'signed': return 'badge-success'
    case 'verified': return 'badge-success'
    case 'rejected': return 'badge-error'
    case 'expired': return 'badge-error'
    default: return 'badge'
  }
}

function signStatusLabel(status: string): string {
  switch (status) {
    case 'pending': return '待发送'
    case 'sent': return '已发送'
    case 'signed': return '已签署'
    case 'verified': return '已验证'
    case 'rejected': return '已拒绝'
    case 'expired': return '已过期'
    default: return status
  }
}

function signTypeLabel(type: string): string {
  switch (type) {
    case 'digital': return '电子签名'
    case 'sms': return '短信验证'
    case 'face': return '人脸识别'
    default: return type
  }
}

// ── Tab definitions ──────────────────────────────────────────────────────

const tabs = [
  { key: 'templates' as const, label: '合同模板' },
  { key: 'contracts' as const, label: '已生成合同' },
  { key: 'signing' as const, label: '在线签署' },
]

// ── Init ─────────────────────────────────────────────────────────────────

onMounted(() => {
  loadTemplates()
  loadContracts()
  loadSignRecords()
})
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-18 bar -->
    <div class="module-bar mb-4">
      <span class="module-number-label">MODULE-18</span>
      <div class="structure-line" />
      <span class="module-page-label">合同管理</span>
    </div>

    <!-- Tab bar -->
    <TabBar :tabs="tabs" v-model="activeTab" />

    <!-- ═══ Tab 1: Templates ═══════════════════════════════════════════════ -->
    <div v-if="activeTab === 'templates'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">合同模板管理</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            label="创建模板"
            @click="openCreateTemplate"
          />
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="tmplFilter"
            class="w-48"
            placeholder="搜索模板名称"
            @keydown.enter="tmplPage = 0; loadTemplates()"
          />
          <Button severity="secondary" label="查询" @click="tmplPage = 0; loadTemplates()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="contract.templates"
          :loading="contract.loading"
          :paginator="true"
          :rows="tmplRows"
          :first="tmplPage * tmplRows"
          :totalRecords="tmplTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onTmplPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无合同模板</span>
            </div>
          </template>
          <Column field="name" header="模板名称" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm font-bold">{{ data.name }}</span>
            </template>
          </Column>
          <Column field="type" header="类型" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="templateTypeBadge(data.type)">{{ templateTypeLabel(data.type) }}</span>
            </template>
          </Column>
          <Column field="description" header="描述">
            <template #body="{ data }: { data: any }">
              <span class="text-sm text-text-secondary">{{ data.description || '-' }}</span>
            </template>
          </Column>
          <Column field="enabled" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="data.enabled !== false ? 'badge-success' : 'badge-error'">
                {{ data.enabled !== false ? '启用' : '停用' }}
              </span>
            </template>
          </Column>
          <Column field="updated_at" header="更新时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.updated_at || data.updatedAt || data.created_at || data.createdAt) }}</span>
            </template>
          </Column>
          <Column header="操作" headerStyle="width: 14rem">
            <template #body="{ data }: { data: any }">
              <div class="flex items-center gap-2">
                <Button
                  severity="secondary"
                  label="预览"
                  size="small"
                  @click="handlePreviewTemplate(data)"
                />
                <Button
                  v-if="auth.isTenantAdmin"
                  severity="secondary"
                  label="编辑"
                  size="small"
                  @click="openEditTemplate(data)"
                />
                <Button
                  v-if="auth.isTenantAdmin"
                  severity="danger"
                  label="删除"
                  size="small"
                  @click="confirmDeleteTemplate(data)"
                />
              </div>
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Template Form Dialog -->
      <Dialog
        v-model:visible="tmplOpen"
        modal
        :header="tmplEditing ? '编辑合同模板' : '创建合同模板'"
        :style="{ width: '34rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">模板名称</label>
            <InputText v-model="tmplForm.name" class="w-full" placeholder="请输入模板名称" />
          </div>
          <div>
            <label class="mono-label block mb-1">模板类型</label>
            <Select
              v-model="tmplForm.type"
              :options="[
                { label: '租赁合同', value: 'rental' },
                { label: '免责声明', value: 'waiver' },
                { label: '押金协议', value: 'deposit' },
                { label: '自定义', value: 'custom' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">描述</label>
            <InputText v-model="tmplForm.description" class="w-full" placeholder="模板描述" />
          </div>
          <div>
            <label class="mono-label block mb-1" v-text="'模板内容 (支持变量: {{name}}, {{orderNo}}, {{deviceInfo}}, {{amount}})'"></label>
            <Textarea v-model="tmplForm.content" class="w-full" placeholder="请输入合同模板内容..." rows="8" style="font-family: var(--font-mono); font-size: 0.75rem;" />
          </div>
          <div class="flex items-center gap-2">
            <Checkbox v-model="tmplForm.enabled" :binary="true" input-id="tmpl-enabled" />
            <label for="tmpl-enabled" class="mono-label">启用</label>
          </div>
          <p v-if="tmplError" class="text-xs font-mono text-status-error">{{ tmplError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="tmplLoading" @click="closeTemplateForm" />
          <Button
            severity="primary"
            :label="tmplLoading ? '保存中...' : (tmplEditing ? '更新模板' : '创建模板')"
            :disabled="tmplLoading"
            @click="handleTemplateSave"
          />
        </template>
      </Dialog>

      <!-- Preview Dialog -->
      <Dialog
        v-model:visible="previewOpen"
        modal
        header="模板预览"
        :style="{ width: '42rem', maxHeight: '80vh' }"
      >
        <div class="max-h-96 overflow-y-auto">
          <div v-if="previewLoading" class="py-6 text-center">
            <span class="font-mono text-sm text-text-muted">渲染中...</span>
          </div>
          <div
            v-else
            class="border border-border p-4 font-sans text-sm leading-relaxed"
            v-html="previewHtml"
          />
        </div>
        <template #footer>
          <Button severity="secondary" label="关闭" @click="closePreview" />
        </template>
      </Dialog>
    </div>

    <!-- ═══ Tab 2: Contracts ════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'contracts'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">已生成合同</h2>
          <Button
            v-if="auth.isTenantAdmin"
            severity="primary"
            label="生成合同"
            @click="openGenerateContract"
          />
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="ctFilter"
            class="w-48"
            placeholder="搜索合同号/客户"
            @keydown.enter="ctPage = 0; loadContracts()"
          />
          <Select
            v-model="ctStatusFilter"
            :options="[
              { label: '全部状态', value: '' },
              { label: '草稿', value: 'draft' },
              { label: '待签署', value: 'pending_sign' },
              { label: '已签署', value: 'signed' },
              { label: '生效中', value: 'active' },
              { label: '已作废', value: 'voided' },
              { label: '已过期', value: 'expired' },
            ]"
            option-label="label"
            option-value="value"
            class="w-32"
            @change="ctPage = 0; loadContracts()"
          />
          <Button severity="secondary" label="查询" @click="ctPage = 0; loadContracts()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="contract.contracts"
          :loading="contract.loading"
          :paginator="true"
          :rows="ctRows"
          :first="ctPage * ctRows"
          :totalRecords="ctTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onCtPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无已生成合同</span>
            </div>
          </template>
          <Column field="contract_no" header="合同号" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm font-bold">{{ data.contract_no ?? data.contractNo ?? data.id ?? '-' }}</span>
            </template>
          </Column>
          <Column field="template_name" header="模板">
            <template #body="{ data }: { data: any }">
              <span :class="templateTypeBadge(data.template_type ?? data.templateType)">
                {{ data.template_name ?? data.templateName ?? '-' }}
              </span>
            </template>
          </Column>
          <Column field="customer_name" header="客户">
            <template #body="{ data }: { data: any }">
              <div class="font-mono text-sm">
                <div>{{ data.customer_name ?? data.customerName ?? '-' }}</div>
                <div class="text-xs text-text-muted">{{ data.customer_phone ?? data.customerPhone ?? '-' }}</div>
              </div>
            </template>
          </Column>
          <Column field="amount" header="金额" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm">¥{{ data.amount ?? 0 }}</span>
            </template>
          </Column>
          <Column field="status" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="contractStatusBadge(data.status)">{{ contractStatusLabel(data.status) }}</span>
            </template>
          </Column>
          <Column field="signed_at" header="签署时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">
                {{ data.signed_at || data.signedAt ? formatTime(data.signed_at ?? data.signedAt) : '-' }}
              </span>
            </template>
          </Column>
          <Column field="created_at" header="创建时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.created_at || data.createdAt) }}</span>
            </template>
          </Column>
          <Column header="操作" headerStyle="width: 12rem">
            <template #body="{ data }: { data: any }">
              <div class="flex items-center gap-2">
                <Button
                  severity="secondary"
                  label="详情"
                  size="small"
                  @click="viewContractDetail(data)"
                />
                <Button
                  v-if="auth.isTenantAdmin && data.status !== 'voided'"
                  severity="danger"
                  label="作废"
                  size="small"
                  @click="confirmVoidContract(data)"
                />
              </div>
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Generate Contract Dialog -->
      <Dialog
        v-model:visible="ctGenOpen"
        modal
        header="生成合同"
        :style="{ width: '30rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">模板</label>
            <Select
              v-model="ctGenForm.template_id"
              :options="contract.templates.filter((t: any) => t.enabled !== false).map((t: any) => ({ label: t.name, value: t.id }))"
              option-label="label"
              option-value="value"
              class="w-full"
              placeholder="选择模板"
            />
          </div>
          <div>
            <label class="mono-label block mb-1">订单号</label>
            <InputText v-model="ctGenForm.order_id" class="w-full" placeholder="关联订单号" />
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="mono-label block mb-1">客户姓名</label>
              <InputText v-model="ctGenForm.customer_name" class="w-full" placeholder="客户姓名" />
            </div>
            <div>
              <label class="mono-label block mb-1">客户手机号</label>
              <InputText v-model="ctGenForm.customer_phone" class="w-full" placeholder="手机号" />
            </div>
          </div>
          <div>
            <label class="mono-label block mb-1">设备信息</label>
            <InputText v-model="ctGenForm.device_info" class="w-full" placeholder="设备型号/序列号" />
          </div>
          <div>
            <label class="mono-label block mb-1">合同金额 (元)</label>
            <InputNumber v-model="ctGenForm.amount" class="w-full" :min="0" placeholder="0" />
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="mono-label block mb-1">租赁开始日期</label>
              <InputText v-model="ctGenForm.start_date" class="w-full" type="date" />
            </div>
            <div>
              <label class="mono-label block mb-1">租赁结束日期</label>
              <InputText v-model="ctGenForm.end_date" class="w-full" type="date" />
            </div>
          </div>
          <p v-if="ctGenError" class="text-xs font-mono text-status-error">{{ ctGenError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="ctGenLoading" @click="closeGenerateContract" />
          <Button severity="primary" :label="ctGenLoading ? '生成中...' : '确认生成'" :disabled="ctGenLoading" @click="handleGenerateContract" />
        </template>
      </Dialog>

      <!-- Contract Detail Dialog -->
      <Dialog
        v-model:visible="ctDetailOpen"
        modal
        header="合同详情"
        :style="{ width: '34rem' }"
      >
        <div v-if="contract.currentContract" class="space-y-3">
          <div class="grid grid-cols-2 gap-3 font-mono text-sm">
            <div class="flex justify-between">
              <span class="text-text-muted">合同号</span>
              <span class="text-text-primary font-bold">{{ contract.currentContract.contract_no ?? contract.currentContract.contractNo ?? contract.currentContract.id ?? '-' }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">状态</span>
              <span :class="contractStatusBadge(contract.currentContract.status)">
                {{ contractStatusLabel(contract.currentContract.status) }}
              </span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">模板</span>
              <span class="text-text-primary">{{ contract.currentContract.template_name ?? contract.currentContract.templateName ?? '-' }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">金额</span>
              <span class="text-text-primary" style="color: var(--status-warning)">¥{{ contract.currentContract.amount ?? 0 }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">客户</span>
              <span class="text-text-primary">{{ contract.currentContract.customer_name ?? contract.currentContract.customerName ?? '-' }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">手机号</span>
              <span class="text-text-primary">{{ contract.currentContract.customer_phone ?? contract.currentContract.customerPhone ?? '-' }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">订单号</span>
              <span class="text-text-primary">{{ contract.currentContract.order_id ?? contract.currentContract.orderId ?? '-' }}</span>
            </div>
            <div class="flex justify-between">
              <span class="text-text-muted">签署时间</span>
              <span class="text-text-primary">{{ contract.currentContract.signed_at || contract.currentContract.signedAt ? formatTime(contract.currentContract.signed_at ?? contract.currentContract.signedAt) : '-' }}</span>
            </div>
            <div class="flex justify-between col-span-2">
              <span class="text-text-muted">设备信息</span>
              <span class="text-text-primary">{{ contract.currentContract.device_info ?? contract.currentContract.deviceInfo ?? '-' }}</span>
            </div>
            <div class="flex justify-between col-span-2">
              <span class="text-text-muted">创建时间</span>
              <span class="text-text-primary">{{ formatTime(contract.currentContract.created_at || contract.currentContract.createdAt) }}</span>
            </div>
          </div>
        </div>
        <div v-else class="py-6 text-center">
          <span class="font-mono text-sm text-text-muted">加载中...</span>
        </div>
        <template #footer>
          <Button severity="secondary" label="关闭" @click="closeContractDetail" />
        </template>
      </Dialog>
    </div>

    <!-- ═══ Tab 3: Signing ═══════════════════════════════════════════════════ -->
    <div v-if="activeTab === 'signing'" class="space-y-4">
      <div class="panel">
        <div class="flex items-center justify-between mb-3">
          <h2 class="panel-title mb-0">在线签署管理</h2>
          <div class="flex items-center gap-2">
            <Button
              v-if="auth.isTenantAdmin"
              severity="primary"
              label="发起签署"
              @click="openSignRequest"
            />
            <Button
              severity="secondary"
              label="验证签名"
              @click="openSignVerify()"
            />
          </div>
        </div>

        <!-- Filters -->
        <div class="flex items-center gap-3 mb-3">
          <InputText
            v-model="signFilter"
            class="w-48"
            placeholder="搜索合同号"
            @keydown.enter="signPage = 0; loadSignRecords()"
          />
          <Select
            v-model="signStatusFilter"
            :options="[
              { label: '全部状态', value: '' },
              { label: '待发送', value: 'pending' },
              { label: '已发送', value: 'sent' },
              { label: '已签署', value: 'signed' },
              { label: '已验证', value: 'verified' },
              { label: '已拒绝', value: 'rejected' },
              { label: '已过期', value: 'expired' },
            ]"
            option-label="label"
            option-value="value"
            class="w-28"
            @change="signPage = 0; loadSignRecords()"
          />
          <Button severity="secondary" label="查询" @click="signPage = 0; loadSignRecords()" />
        </div>

        <!-- Table -->
        <DataTable
          :value="contract.signRecords"
          :loading="contract.loading"
          :paginator="true"
          :rows="signRows"
          :first="signPage * signRows"
          :totalRecords="signTotalRecords"
          :rowsPerPageOptions="[10, 20, 30, 50]"
          paginatorTemplate="FirstPageLink PrevPageLink PageLinks NextPageLink LastPageLink RowsPerPageDropdown CurrentPageReport"
          currentPageReportTemplate="第 {first}-{last} 条，共 {totalRecords} 条"
          @page="onSignPageChange"
        >
          <template #empty>
            <div class="py-6 text-center">
              <span class="font-mono text-xs text-text-muted">暂无签署记录</span>
            </div>
          </template>
          <Column field="contract_no" header="合同号">
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-sm font-bold">{{ data.contract_no ?? data.contractNo ?? data.contract_id ?? '-' }}</span>
            </template>
          </Column>
          <Column field="signer_name" header="签署人">
            <template #body="{ data }: { data: any }">
              <div class="font-mono text-sm">
                <div>{{ data.signer_name ?? data.signerName ?? '-' }}</div>
                <div class="text-xs text-text-muted">{{ data.signer_phone ?? data.signerPhone ?? '-' }}</div>
              </div>
            </template>
          </Column>
          <Column field="sign_type" header="签署方式">
            <template #body="{ data }: { data: any }">
              <span class="text-sm">{{ signTypeLabel(data.sign_type ?? data.signType) }}</span>
            </template>
          </Column>
          <Column field="status" header="状态" sortable>
            <template #body="{ data }: { data: any }">
              <span :class="signStatusBadge(data.status)">{{ signStatusLabel(data.status) }}</span>
            </template>
          </Column>
          <Column field="signed_at" header="签署时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">
                {{ data.signed_at || data.signedAt ? formatTime(data.signed_at ?? data.signedAt) : '-' }}
              </span>
            </template>
          </Column>
          <Column field="verified_at" header="验证时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">
                {{ data.verified_at || data.verifiedAt ? formatTime(data.verified_at ?? data.verifiedAt) : '-' }}
              </span>
            </template>
          </Column>
          <Column field="created_at" header="发起时间" sortable>
            <template #body="{ data }: { data: any }">
              <span class="font-mono text-xs text-text-secondary">{{ formatTime(data.created_at || data.createdAt) }}</span>
            </template>
          </Column>
        </DataTable>
      </div>

      <!-- Sign Request Dialog -->
      <Dialog
        v-model:visible="signReqOpen"
        modal
        header="发起在线签署"
        :style="{ width: '28rem' }"
      >
        <div class="space-y-3">
          <div>
            <label class="mono-label block mb-1">合同ID</label>
            <InputText v-model="signReqForm.contract_id" class="w-full" placeholder="请输入合同ID" />
          </div>
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label class="mono-label block mb-1">签署人姓名</label>
              <InputText v-model="signReqForm.signer_name" class="w-full" placeholder="签署人姓名" />
            </div>
            <div>
              <label class="mono-label block mb-1">签署人手机号</label>
              <InputText v-model="signReqForm.signer_phone" class="w-full" placeholder="手机号" />
            </div>
          </div>
          <div>
            <label class="mono-label block mb-1">签署方式</label>
            <Select
              v-model="signReqForm.sign_type"
              :options="[
                { label: '电子签名', value: 'digital' },
                { label: '短信验证', value: 'sms' },
                { label: '人脸识别', value: 'face' },
              ]"
              option-label="label"
              option-value="value"
              class="w-full"
            />
          </div>
          <p v-if="signReqError" class="text-xs font-mono text-status-error">{{ signReqError }}</p>
        </div>
        <template #footer>
          <Button severity="secondary" label="取消" :disabled="signReqLoading" @click="closeSignRequest" />
          <Button severity="primary" :label="signReqLoading ? '发送中...' : '发起签署'" :disabled="signReqLoading" @click="handleSignRequest" />
        </template>
      </Dialog>

      <!-- Sign Verify Dialog -->
      <Dialog
        v-model:visible="signVerifyOpen"
        modal
        header="验证电子签名"
        :style="{ width: '28rem' }"
      >
        <div class="space-y-4">
          <!-- Verify form -->
          <div class="space-y-3">
            <div>
              <label class="mono-label block mb-1">合同ID</label>
              <InputText v-model="signVerifyForm.contract_id" class="w-full" placeholder="请输入合同ID" />
            </div>
            <div>
              <label class="mono-label block mb-1">验证码</label>
              <InputText v-model="signVerifyForm.code" class="w-full" placeholder="请输入验证码" />
            </div>
            <Button
              severity="primary"
              label="验证"
              :loading="signVerifyLoading"
              :disabled="signVerifyLoading"
              class="w-full"
              @click="handleSignVerify"
            />
            <p v-if="signVerifyError" class="text-xs font-mono text-status-error">{{ signVerifyError }}</p>
          </div>

          <!-- Verify result -->
          <div v-if="signVerifyResult" class="border border-border p-3 space-y-2">
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">验证结果</span>
              <span :class="signVerifyResult.valid !== false ? 'badge-success' : 'badge-error'">
                {{ signVerifyResult.valid !== false ? '验证通过' : '验证失败' }}
              </span>
            </div>
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">合同号</span>
              <span class="text-text-primary">{{ signVerifyResult.contract_no ?? signVerifyResult.contractNo ?? signVerifyForm.contract_id }}</span>
            </div>
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">签署人</span>
              <span class="text-text-primary">{{ signVerifyResult.signer_name ?? signVerifyResult.signerName ?? '-' }}</span>
            </div>
            <div class="flex justify-between font-mono text-sm">
              <span class="text-text-muted">签署时间</span>
              <span class="text-text-primary">{{ signVerifyResult.signed_at || signVerifyResult.signedAt ? formatTime(signVerifyResult.signed_at ?? signVerifyResult.signedAt) : '-' }}</span>
            </div>
          </div>
        </div>
        <template #footer>
          <Button severity="secondary" label="关闭" @click="closeSignVerify" />
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
