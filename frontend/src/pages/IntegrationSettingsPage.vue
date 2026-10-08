<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useToast } from 'primevue/usetoast'
import {
  integrationApi,
  type BindingInspection,
  type ProviderBindingSummary,
  type ProviderHealth,
  type ProviderInstanceInspection,
  type ProviderLifecycle,
  type ProviderReadiness,
  type UpsertProviderBindingInput,
  type UpsertProviderInstanceInput,
  type WebhookDeadLetterInspection,
  type WebhookEndpointInspection,
} from '@/api/integrations'

const toast = useToast()
const instances = ref<ProviderInstanceInspection[]>([])
const bindings = ref<ProviderBindingSummary[]>([])
const webhookEndpoints = ref<WebhookEndpointInspection[]>([])
const webhookDeadLetters = ref<WebhookDeadLetterInspection[]>([])
const loading = ref(false)
const instanceDialogVisible = ref(false)
const bindingDialogVisible = ref(false)
const webhookEndpointDialogVisible = ref(false)
const replayDialogVisible = ref(false)
const savingInstance = ref(false)
const savingBinding = ref(false)
const savingWebhookEndpoint = ref(false)
const replayingWebhook = ref(false)
const formError = ref('')
const inspection = ref<BindingInspection | null>(null)
const inspectingCapability = ref('')
const webhookEndpointBindingId = ref('')
const issuedWebhookToken = ref('')
const replayCandidate = ref<WebhookDeadLetterInspection | null>(null)
const replayReason = ref('')

const instanceForm = ref({
  id: '',
  providerId: '',
  manifestVersion: '',
  configRevision: '',
  configJson: '{}',
  secretRefsJson: '{}',
  lifecycle: 'draft' as ProviderLifecycle,
  health: 'unknown' as ProviderHealth,
  readiness: 'stub' as Extract<ProviderReadiness, 'stub' | 'fixture'>,
})

const bindingForm = ref<UpsertProviderBindingInput>({
  id: '',
  providerInstanceId: '',
  capability: '',
  configRevision: '',
  enabled: true,
})

const configuredBindingCount = computed(() => bindings.value.filter(binding => binding.enabled).length)
const fixtureInstanceCount = computed(() => instances.value.filter(instance => instance.readiness === 'fixture').length)

function readinessSeverity(readiness: ProviderReadiness): 'success' | 'warn' | 'secondary' | 'danger' {
  if (readiness === 'fixture') return 'success'
  if (readiness === 'stub') return 'secondary'
  return readiness === 'sandbox' ? 'warn' : 'danger'
}

function healthSeverity(health: ProviderHealth): 'success' | 'warn' | 'secondary' | 'danger' {
  if (health === 'ready') return 'success'
  if (health === 'degraded') return 'warn'
  if (health === 'unhealthy') return 'danger'
  return 'secondary'
}

function configurationStatus(binding: ProviderBindingSummary): string {
  const instance = instances.value.find(item => item.providerInstanceId === binding.providerInstanceId)
  if (!binding.enabled) return '已禁用'
  if (!instance) return '实例不可见'
  if (instance.lifecycle !== 'active') return '实例未激活'
  if (instance.health !== 'ready') return '健康状态未就绪'
  if (instance.readiness !== 'fixture') return '非 fixture，禁止调度'
  if (instance.configRevision !== binding.bindingRevision) return '修订不一致'
  return '已配置（仍需解析验证）'
}

async function load() {
  loading.value = true
  try {
    const [instanceResult, bindingResult, endpointResult, deadLetterResult] = await Promise.all([
      integrationApi.listInstances(),
      integrationApi.listBindings(),
      integrationApi.listWebhookEndpoints(),
      integrationApi.listWebhookDeadLetters(),
    ])
    instances.value = instanceResult.instances
    bindings.value = bindingResult.bindings
    webhookEndpoints.value = endpointResult.webhookEndpoints
    webhookDeadLetters.value = deadLetterResult.webhookDeadLetters
  } catch (error: any) {
    toast.add({ severity: 'error', summary: '加载失败', detail: error?.message || '无法读取 Integration 状态', life: 5000 })
  } finally {
    loading.value = false
  }
}

function openInstanceDialog() {
  formError.value = ''
  instanceForm.value = {
    id: '', providerId: '', manifestVersion: '', configRevision: '', configJson: '{}', secretRefsJson: '{}',
    lifecycle: 'draft', health: 'unknown', readiness: 'stub',
  }
  instanceDialogVisible.value = true
}

function openBindingDialog() {
  formError.value = ''
  bindingForm.value = { id: '', providerInstanceId: '', capability: '', configRevision: '', enabled: true }
  bindingDialogVisible.value = true
}

function openWebhookEndpointDialog() {
  formError.value = ''
  issuedWebhookToken.value = ''
  webhookEndpointBindingId.value = bindings.value.find(binding => binding.enabled)?.providerBindingId || ''
  webhookEndpointDialogVisible.value = true
}

async function createWebhookEndpoint() {
  formError.value = ''
  if (!webhookEndpointBindingId.value.trim()) {
    formError.value = '必须选择一个已启用的 Provider Binding'
    return
  }
  savingWebhookEndpoint.value = true
  try {
    const created = await integrationApi.createWebhookEndpoint(webhookEndpointBindingId.value.trim())
    issuedWebhookToken.value = created.endpointToken
    await load()
    toast.add({ severity: 'success', summary: 'Webhook endpoint 已创建', detail: 'Token 只显示这一次，请在 fixture 配置中安全保存。', life: 5000 })
  } catch (error: any) {
    formError.value = error?.message || '创建 Webhook endpoint 失败'
  } finally {
    savingWebhookEndpoint.value = false
  }
}

async function setWebhookEndpointEnabled(endpoint: WebhookEndpointInspection, enabled: boolean) {
  try {
    await integrationApi.setWebhookEndpointEnabled(endpoint.webhookEndpointId, enabled)
    toast.add({ severity: 'success', summary: enabled ? 'Webhook endpoint 已启用' : 'Webhook endpoint 已停用', life: 2500 })
    await load()
  } catch (error: any) {
    toast.add({ severity: 'error', summary: '更新失败', detail: error?.message || '无法更新 endpoint 状态', life: 5000 })
  }
}

function openReplayDialog(letter: WebhookDeadLetterInspection) {
  formError.value = ''
  replayCandidate.value = letter
  replayReason.value = ''
  replayDialogVisible.value = true
}

async function replayWebhook() {
  if (!replayCandidate.value || !replayReason.value.trim()) {
    formError.value = '回放原因是必填项'
    return
  }
  replayingWebhook.value = true
  try {
    await integrationApi.replayWebhook(replayCandidate.value.inboxId, replayReason.value.trim())
    replayDialogVisible.value = false
    toast.add({ severity: 'success', summary: 'Webhook 已回放到正常队列', life: 2500 })
    await load()
  } catch (error: any) {
    formError.value = error?.message || '回放 Webhook 失败'
  } finally {
    replayingWebhook.value = false
  }
}

function parseStringMap(raw: string, label: string): Record<string, string> {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw || '{}')
  } catch {
    throw new Error(`${label}必须是 JSON 对象`)
  }
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new Error(`${label}必须是 JSON 对象`)
  }
  const result: Record<string, string> = {}
  for (const [key, value] of Object.entries(parsed as Record<string, unknown>)) {
    if (!key.trim() || typeof value !== 'string' || !value.trim()) {
      throw new Error(`${label}中的键和值都必须是非空字符串`)
    }
    result[key] = value
  }
  return result
}

function parseSecretReferenceMap(raw: string): Record<string, string> {
  const references = parseStringMap(raw, 'Secret reference')
  for (const reference of Object.values(references)) {
    if (!/^keystore:\/\/fixture\/[A-Za-z0-9._/-]+$/.test(reference)) {
      throw new Error('Secret reference 必须是已登记的 keystore://fixture/<opaque-id>；不能输入 secret 值')
    }
  }
  return references
}

async function saveInstance() {
  formError.value = ''
  const form = instanceForm.value
  if (![form.id, form.providerId, form.manifestVersion, form.configRevision].every(value => value.trim())) {
    formError.value = '实例 ID、Provider ID、Manifest 版本和配置修订均为必填项'
    return
  }
  savingInstance.value = true
  try {
    const input: UpsertProviderInstanceInput = {
      id: form.id.trim(),
      providerId: form.providerId.trim(),
      manifestVersion: form.manifestVersion.trim(),
      configRevision: form.configRevision.trim(),
      config: parseStringMap(form.configJson, '非敏感配置'),
      secretRefs: parseSecretReferenceMap(form.secretRefsJson),
      lifecycle: form.lifecycle,
      health: form.health,
      readiness: form.readiness,
    }
    await integrationApi.upsertInstance(input)
    instanceDialogVisible.value = false
    toast.add({ severity: 'success', summary: 'Provider 实例已保存', life: 2500 })
    await load()
  } catch (error: any) {
    formError.value = error?.message || '保存 Provider 实例失败'
  } finally {
    savingInstance.value = false
  }
}

async function saveBinding() {
  formError.value = ''
  const form = bindingForm.value
  if (![form.id, form.providerInstanceId, form.capability, form.configRevision].every(value => value.trim())) {
    formError.value = 'Binding ID、实例、能力与配置修订均为必填项'
    return
  }
  savingBinding.value = true
  try {
    await integrationApi.upsertBinding({
      ...form,
      id: form.id.trim(),
      providerInstanceId: form.providerInstanceId.trim(),
      capability: form.capability.trim(),
      configRevision: form.configRevision.trim(),
    })
    bindingDialogVisible.value = false
    toast.add({ severity: 'success', summary: 'Provider Binding 已保存', life: 2500 })
    await load()
  } catch (error: any) {
    formError.value = error?.message || '保存 Provider Binding 失败'
  } finally {
    savingBinding.value = false
  }
}

async function inspectBinding(capability: string) {
  inspectingCapability.value = capability
  inspection.value = null
  try {
    inspection.value = await integrationApi.inspectBinding(capability)
    const detail = inspection.value.readiness === 'fixture'
      ? `${capability} 的 fixture 配置解析通过`
      : `${capability} 的配置解析通过；非 fixture 调度仍被阻止`
    toast.add({ severity: 'success', summary: 'Binding 解析通过', detail, life: 3000 })
  } catch (error: any) {
    toast.add({ severity: 'warn', summary: 'Binding 未就绪', detail: error?.message || '解析被拒绝', life: 5000 })
  } finally {
    inspectingCapability.value = ''
  }
}

onMounted(load)
</script>

<template>
  <main class="flex flex-col gap-5" style="min-height: calc(100vh - 88px)">
    <div class="module-bar">
      <span class="module-number-label">MODULE-21</span>
      <div class="structure-line" />
      <span class="module-page-label">INTEGRATION FABRIC</span>
    </div>

    <section class="hero-anchor">
      <p class="mono-label">Provider control surface</p>
      <div class="flex flex-wrap items-end justify-between gap-5">
        <div>
          <h1 class="font-mono text-2xl font-bold tracking-wider text-text-primary">Provider 集成配置</h1>
          <p class="mt-2 max-w-3xl text-sm text-text-secondary">
            管理租户内的声明式实例与能力 Binding。该页面只处理 stub / fixture 配置；不会显示 secret 值、不会调用真实 Provider，也不是支付或退款生产就绪证明。
          </p>
        </div>
        <div class="flex gap-2">
          <Button severity="secondary" label="刷新状态" :loading="loading" @click="load" />
          <Button severity="secondary" label="添加实例" @click="openInstanceDialog" />
          <Button severity="primary" label="添加 Binding" @click="openBindingDialog" />
        </div>
      </div>
    </section>

    <section class="grid gap-3 lg:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)]" aria-label="Integration summary">
      <div class="card">
        <p class="mono-label">Provider instances</p>
        <p class="hero-kpi mt-2">{{ instances.length }}</p>
        <p class="mt-1 text-xs text-text-tertiary">仅显示当前租户；secret reference 不会返回。</p>
      </div>
      <div class="card">
        <p class="mono-label">Enabled bindings</p>
        <p class="hero-kpi mt-2">{{ configuredBindingCount }}</p>
        <p class="mt-1 text-xs text-text-tertiary">Binding 必须与实例的配置修订一致。</p>
      </div>
      <div class="card-alert">
        <p class="mono-label">Fixture runtime</p>
        <p class="hero-kpi mt-2">{{ fixtureInstanceCount }}</p>
        <p class="mt-1 text-xs text-text-tertiary">只有 fixture runtime 被实现为可调度边界。</p>
      </div>
    </section>

    <section class="panel">
      <div class="mb-3 flex items-center justify-between gap-3">
        <div>
          <h2 class="panel-title mb-1">Provider Instances</h2>
          <p class="text-xs text-text-tertiary">配置键与 secret requirement 名称可见；配置值和 secret reference 始终隐藏。</p>
        </div>
      </div>
      <DataTable :value="instances" :loading="loading" size="small">
        <template #empty><p class="py-5 text-center font-mono text-xs text-text-tertiary">暂无实例。请先由平台注册 Manifest，再创建租户实例。</p></template>
        <Column field="providerInstanceId" header="实例" />
        <Column field="providerId" header="Provider" />
        <Column field="manifestVersion" header="Manifest" />
        <Column field="configRevision" header="配置修订" />
        <Column header="就绪度">
          <template #body="{ data }"><Tag :severity="readinessSeverity(data.readiness)" :value="data.readiness" /></template>
        </Column>
        <Column header="健康">
          <template #body="{ data }"><Tag :severity="healthSeverity(data.health)" :value="data.health" /></template>
        </Column>
        <Column header="配置 / Secret requirement">
          <template #body="{ data }"><span class="font-mono text-xs text-text-secondary">{{ data.configKeys.join(', ') || '—' }} / {{ data.configuredSecretNames.join(', ') || '—' }}</span></template>
        </Column>
      </DataTable>
    </section>

    <section class="panel">
      <div class="mb-3">
        <h2 class="panel-title mb-1">Capability Bindings</h2>
        <p class="text-xs text-text-tertiary">“已配置”不等于已经可运行。点击解析会使用当前 Registry、租户、版本和 readiness 约束重新验证。</p>
      </div>
      <DataTable :value="bindings" :loading="loading" size="small">
        <template #empty><p class="py-5 text-center font-mono text-xs text-text-tertiary">暂无 Binding。Binding 只能指向当前租户的实例。</p></template>
        <Column field="capability" header="Capability" />
        <Column field="providerInstanceId" header="实例" />
        <Column field="bindingRevision" header="Binding 修订" />
        <Column header="状态">
          <template #body="{ data }"><span class="font-mono text-xs text-text-secondary">{{ configurationStatus(data) }}</span></template>
        </Column>
        <Column header="操作" header-style="width: 9rem">
          <template #body="{ data }">
            <Button size="small" severity="secondary" :loading="inspectingCapability === data.capability" label="解析验证" @click="inspectBinding(data.capability)" />
          </template>
        </Column>
      </DataTable>
    </section>

    <section class="panel">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 class="panel-title mb-1">Fixture Webhook Endpoints</h2>
          <p class="text-xs text-text-tertiary">Token 只在创建时显示一次；系统只持久化 token hash。仅 active / ready / fixture Binding 可以创建端点。</p>
        </div>
        <Button severity="primary" label="创建 Endpoint" @click="openWebhookEndpointDialog" />
      </div>
      <DataTable :value="webhookEndpoints" :loading="loading" size="small">
        <template #empty><p class="py-5 text-center font-mono text-xs text-text-tertiary">暂无 fixture Webhook endpoint。</p></template>
        <Column field="webhookEndpointId" header="Endpoint" />
        <Column field="providerBindingId" header="Binding" />
        <Column field="providerId" header="Provider" />
        <Column field="createdAt" header="创建时间" />
        <Column header="状态">
          <template #body="{ data }"><Tag :severity="data.enabled ? 'success' : 'secondary'" :value="data.enabled ? 'enabled' : 'disabled'" /></template>
        </Column>
        <Column header="操作" header-style="width: 10rem">
          <template #body="{ data }">
            <Button size="small" severity="secondary" :label="data.enabled ? '停用' : '启用'" @click="setWebhookEndpointEnabled(data, !data.enabled)" />
          </template>
        </Column>
      </DataTable>
    </section>

    <section class="panel">
      <div class="mb-3">
        <h2 class="panel-title mb-1">Webhook Dead Letters</h2>
        <p class="text-xs text-text-tertiary">回放必须给出操作原因，并重新进入正常异步队列；不会直接写业务表。</p>
      </div>
      <DataTable :value="webhookDeadLetters" :loading="loading" size="small">
        <template #empty><p class="py-5 text-center font-mono text-xs text-text-tertiary">暂无死信。</p></template>
        <Column field="inboxId" header="Inbox" />
        <Column field="providerEventId" header="Provider event" />
        <Column field="reason" header="原因" />
        <Column field="replayCount" header="回放次数" />
        <Column header="操作" header-style="width: 8rem">
          <template #body="{ data }"><Button size="small" severity="secondary" label="回放" @click="openReplayDialog(data)" /></template>
        </Column>
      </DataTable>
    </section>

    <section v-if="inspection" class="card-focus" aria-live="polite">
      <p class="mono-label">Resolved binding</p>
      <div class="mt-3 grid gap-3 text-sm md:grid-cols-3">
        <p><span class="text-text-tertiary">能力</span><br><code>{{ inspection.capability }}</code></p>
        <p><span class="text-text-tertiary">实例 / 修订</span><br><code>{{ inspection.providerInstanceId }} / {{ inspection.bindingRevision }}</code></p>
        <p><span class="text-text-tertiary">运行状态</span><br><code>{{ inspection.lifecycle }} · {{ inspection.health }} · {{ inspection.readiness }}</code></p>
      </div>
    </section>

    <Dialog v-model:visible="instanceDialogVisible" modal header="添加 Provider 实例" :style="{ width: '42rem' }">
      <div class="grid gap-3">
        <p class="text-sm text-text-secondary">此控制面仅允许选择 stub 或 fixture。先在 KeyStore 登记 fixture secret，再填写其 opaque reference；绝不能输入 secret 值。</p>
        <div class="grid gap-3 md:grid-cols-2">
          <label><span class="mono-label">实例 ID</span><InputText v-model="instanceForm.id" class="mt-1 w-full" /></label>
          <label><span class="mono-label">Provider ID</span><InputText v-model="instanceForm.providerId" class="mt-1 w-full" /></label>
          <label><span class="mono-label">Manifest 版本</span><InputText v-model="instanceForm.manifestVersion" class="mt-1 w-full" /></label>
          <label><span class="mono-label">配置修订</span><InputText v-model="instanceForm.configRevision" class="mt-1 w-full" /></label>
          <label><span class="mono-label">Lifecycle</span><Select v-model="instanceForm.lifecycle" class="mt-1 w-full" :options="['draft', 'active', 'suspended']" /></label>
          <label><span class="mono-label">Health</span><Select v-model="instanceForm.health" class="mt-1 w-full" :options="['unknown', 'ready', 'degraded', 'unhealthy']" /></label>
          <label><span class="mono-label">Readiness</span><Select v-model="instanceForm.readiness" class="mt-1 w-full" :options="['stub', 'fixture']" /></label>
        </div>
        <label><span class="mono-label">非敏感配置 JSON</span><Textarea v-model="instanceForm.configJson" class="mt-1 w-full font-mono" rows="3" /></label>
        <label><span class="mono-label">Secret reference JSON</span><Textarea v-model="instanceForm.secretRefsJson" class="mt-1 w-full font-mono" rows="3" placeholder='{"api_key":"keystore://fixture/payment-key-a"}' /></label>
        <p v-if="formError" class="text-sm text-status-error" role="alert">{{ formError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="savingInstance" @click="instanceDialogVisible = false" />
        <Button severity="primary" :label="savingInstance ? '保存中…' : '保存实例'" :disabled="savingInstance" @click="saveInstance" />
      </template>
    </Dialog>

    <Dialog v-model:visible="bindingDialogVisible" modal header="添加 Provider Binding" :style="{ width: '34rem' }">
      <div class="grid gap-3">
        <p class="text-sm text-text-secondary">Binding 显式把一个租户实例连接到一个已声明能力。配置修订必须与实例一致。</p>
        <label><span class="mono-label">Binding ID</span><InputText v-model="bindingForm.id" class="mt-1 w-full" /></label>
        <label><span class="mono-label">Provider 实例 ID</span><InputText v-model="bindingForm.providerInstanceId" class="mt-1 w-full" /></label>
        <label><span class="mono-label">Capability</span><InputText v-model="bindingForm.capability" class="mt-1 w-full" /></label>
        <label><span class="mono-label">配置修订</span><InputText v-model="bindingForm.configRevision" class="mt-1 w-full" /></label>
        <label class="flex items-center gap-2 text-sm text-text-primary"><Checkbox v-model="bindingForm.enabled" binary /> 启用此 Binding</label>
        <p v-if="formError" class="text-sm text-status-error" role="alert">{{ formError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="savingBinding" @click="bindingDialogVisible = false" />
        <Button severity="primary" :label="savingBinding ? '保存中…' : '保存 Binding'" :disabled="savingBinding" @click="saveBinding" />
      </template>
    </Dialog>

    <Dialog v-model:visible="webhookEndpointDialogVisible" modal header="创建 Fixture Webhook Endpoint" :style="{ width: '38rem' }">
      <div class="grid gap-3">
        <p class="text-sm text-text-secondary">Endpoint 的 fixture token 仅在创建成功后显示一次。它不是 secret reference，也不会返回到端点列表。</p>
        <label><span class="mono-label">Provider Binding</span><Select v-model="webhookEndpointBindingId" class="mt-1 w-full" :options="bindings.filter(binding => binding.enabled)" option-label="providerBindingId" option-value="providerBindingId" placeholder="选择 enabled Binding" /></label>
        <label v-if="issuedWebhookToken"><span class="mono-label">一次性 Endpoint token</span><Textarea :model-value="issuedWebhookToken" readonly class="mt-1 w-full font-mono" rows="2" /></label>
        <p v-if="formError" class="text-sm text-status-error" role="alert">{{ formError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="关闭" :disabled="savingWebhookEndpoint" @click="webhookEndpointDialogVisible = false" />
        <Button v-if="!issuedWebhookToken" severity="primary" :label="savingWebhookEndpoint ? '创建中…' : '创建 Endpoint'" :disabled="savingWebhookEndpoint" @click="createWebhookEndpoint" />
      </template>
    </Dialog>

    <Dialog v-model:visible="replayDialogVisible" modal header="回放 Webhook 死信" :style="{ width: '34rem' }">
      <div class="grid gap-3">
        <p class="text-sm text-text-secondary">将 <code>{{ replayCandidate?.inboxId }}</code> 返回 verified 队列。回放不会绕过验证、异步 worker 或正常治理。</p>
        <label><span class="mono-label">回放原因</span><Textarea v-model="replayReason" class="mt-1 w-full" rows="3" /></label>
        <p v-if="formError" class="text-sm text-status-error" role="alert">{{ formError }}</p>
      </div>
      <template #footer>
        <Button severity="secondary" label="取消" :disabled="replayingWebhook" @click="replayDialogVisible = false" />
        <Button severity="primary" :label="replayingWebhook ? '回放中…' : '确认回放'" :disabled="replayingWebhook" @click="replayWebhook" />
      </template>
    </Dialog>

    <div class="version-footer">2026 — V2.00 — INTEGRATION FABRIC · fixture-only control surface</div>
  </main>
</template>
