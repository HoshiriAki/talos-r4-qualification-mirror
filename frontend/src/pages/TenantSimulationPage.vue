<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTenantSimulationStore } from '@/stores/tenantSimulation'

const route = useRoute()
const router = useRouter()
const simulation = useTenantSimulationStore()
const key = ref('')
const valueText = ref('{}')
const effectNote = ref('')
const editingKey = ref<string | null>(null)
const id = computed(() => String(route.params.simulationId || ''))
const createTenantId = ref(String(route.query.tenantId || ''))
const createScenarioName = ref('')
const createChangeIntent = ref('')
const createTtlMinutes = ref(30)
const createPlannedKeys = ref('')
// Retained across a failed submission so retries resolve to the same server outcome.
const createIdempotencyKey = ref(crypto.randomUUID())
const expiry = computed(() => simulation.session?.expiresAt ? new Date(simulation.session.expiresAt).toLocaleString('zh-CN', { hour12: false }) : '-')

function displayValue(value: unknown) { return JSON.stringify(value, null, 2) }
function parseValue() {
  try { return JSON.parse(valueText.value) }
  catch { throw new Error('配置值必须是有效 JSON') }
}
function resetEditor() { key.value = ''; valueText.value = '{}'; effectNote.value = ''; editingKey.value = null }
function beginEdit(item: { key: string; value: unknown }) { key.value = item.key; valueText.value = displayValue(item.value); editingKey.value = item.key }
async function save() {
  const trimmed = key.value.trim()
  if (!trimmed) { simulation.error = '请输入配置键'; return }
  await simulation.saveConfig(trimmed, parseValue(), !editingKey.value)
  resetEditor()
}
async function recordEffect() {
  const trimmed = key.value.trim()
  if (!trimmed) { simulation.error = '请输入配置键'; return }
  await simulation.recordEffect(trimmed, parseValue(), effectNote.value.trim())
  resetEditor()
}
async function discard() {
  if (!confirm('确认结束并丢弃此隔离模拟？此操作不会写入生产环境。')) return
  await simulation.discard()
  await router.replace('/control/governance')
}
async function createSimulation() {
  const tenantId = createTenantId.value.trim()
  const scenarioName = createScenarioName.value.trim()
  const changeIntent = createChangeIntent.value.trim()
  if (!tenantId || !scenarioName || !changeIntent) {
    simulation.error = '请填写租户、场景名称与变更意图'
    return
  }
  const plannedAbsentKeys = [...new Set(createPlannedKeys.value.split(/[\n,]/).map((value) => value.trim()).filter(Boolean))]
  try {
    const session = await simulation.create({
      tenantId,
      scenarioName,
      changeIntent,
      ttlMinutes: Number(createTtlMinutes.value),
      plannedAbsentKeys,
      idempotencyKey: createIdempotencyKey.value,
    })
    createIdempotencyKey.value = crypto.randomUUID()
    await router.replace(`/control/simulation/${encodeURIComponent(session.id)}`)
  } catch {
    // The store retains the sanitized server-safe error and the idempotency key.
  }
}
async function load() {
  if (!id.value) return
  await simulation.hydrate(id.value)
  if (simulation.session?.status === 'active') await simulation.loadWorkspace()
}
watch(id, () => { void load() })
onMounted(() => { void load() })
onBeforeUnmount(simulation.clear)
</script>

<template>
  <main class="simulation-page">
    <section v-if="!id" class="create-card card">
      <header>
        <p class="eyebrow">TENANT SIMULATION · NEW ISOLATED OVERLAY</p>
        <h1>创建模拟会话</h1>
        <p class="banner-detail">创建后的所有写入仅存在于隔离 namespace；没有生产 apply。</p>
      </header>
      <p v-if="simulation.error" class="error" role="alert">{{ simulation.error }}</p>
      <form class="create-form" @submit.prevent="createSimulation">
        <label>租户 ID <input v-model="createTenantId" class="input-mono" required placeholder="tenant-acme" /></label>
        <label>场景名称 <input v-model="createScenarioName" class="input" required maxlength="80" placeholder="threshold experiment" /></label>
        <label>变更意图 <textarea v-model="createChangeIntent" class="textarea" required maxlength="200" placeholder="验证隔离 overlay 行为" /></label>
        <label>有效期（分钟）<input v-model.number="createTtlMinutes" class="input-mono" required type="number" min="1" max="1440" /></label>
        <label>计划新增键（逗号或换行分隔，可选）<textarea v-model="createPlannedKeys" class="textarea font-mono" placeholder="new-policy\nnew-threshold" /></label>
        <div class="actions"><button class="btn-primary" type="submit" :disabled="simulation.mutationLoading">{{ simulation.mutationLoading ? '正在创建…' : '创建隔离模拟' }}</button><button class="btn-ghost" type="button" @click="router.push('/control/governance')">取消</button></div>
      </form>
    </section>
    <template v-else>
    <section class="simulation-banner" role="status" aria-live="polite">
      <div>
        <p class="eyebrow">TENANT SIMULATION · ISOLATED OVERLAY</p>
        <h1>{{ simulation.session?.tenantName || '模拟会话' }}</h1>
        <p class="banner-detail">{{ simulation.session?.tenantSlug || id }} · BASE REV {{ simulation.session?.baseRevision ?? '-' }} · 到期 {{ expiry }}</p>
      </div>
      <div class="banner-status">
        <span class="badge-warning">{{ simulation.session?.status || 'loading' }}</span>
        <span class="counter">DIRTY {{ simulation.dirtyCount }} · CONFLICT {{ simulation.conflictCount }}</span>
        <button class="btn-danger" :disabled="simulation.mutationLoading || !simulation.executable" @click="discard">安全退出并丢弃</button>
      </div>
    </section>

    <p class="notice">此工作台所有写入仅进入隔离 overlay。没有生产回退、没有 apply-to-production 操作。</p>
    <div class="simulation-pricing-entry"><button class="btn-secondary" :disabled="!simulation.executable" @click="router.push(`/control/simulation/${encodeURIComponent(id)}/pricing`)">隔离定价实验</button></div>
    <p v-if="simulation.error" class="error" role="alert">{{ simulation.error }}</p>
    <div v-if="simulation.loading && !simulation.session" class="state">正在验证模拟会话…</div>
    <div v-else-if="simulation.session?.status !== 'active'" class="state">会话当前不可执行：{{ simulation.session?.status }} {{ simulation.session?.failureCode || '' }}</div>

    <template v-else>
      <section class="workspace-grid">
        <article class="card config-card">
          <header class="section-header"><div><p class="eyebrow">REFERENCE CONFIG</p><h2>隔离配置</h2></div><button class="btn-ghost" :disabled="simulation.loading" @click="simulation.loadWorkspace">刷新</button></header>
          <div class="config-list">
            <button v-for="item in simulation.configs" :key="item.key" class="config-row" type="button" @click="beginEdit(item)">
              <span>{{ item.key }}</span><code>{{ displayValue(item.value) }}</code>
            </button>
            <p v-if="!simulation.loading && !simulation.configs.length" class="empty">尚无 overlay 配置。</p>
          </div>
        </article>
        <article class="card editor-card">
          <header class="section-header"><div><p class="eyebrow">OVERLAY COMMAND</p><h2>{{ editingKey ? '编辑配置' : '新增配置' }}</h2></div><button class="btn-ghost" type="button" @click="resetEditor">清空</button></header>
          <label>配置键 <input v-model="key" class="input-mono" :disabled="!!editingKey" placeholder="threshold-policy" /></label>
          <label>JSON 值 <textarea v-model="valueText" class="textarea font-mono" rows="8" /></label>
          <div class="actions"><button class="btn-primary" :disabled="simulation.mutationLoading" @click="save">{{ editingKey ? '写入 overlay' : '创建 overlay' }}</button><button v-if="editingKey" class="btn-secondary" :disabled="simulation.mutationLoading" @click="simulation.deleteConfig(editingKey).then(resetEditor)">删除 overlay</button></div>
          <div class="effect-divider" />
          <label>受控效果备注 <input v-model="effectNote" class="input" maxlength="200" placeholder="仅记录固定 schema 效果，不执行外部 I/O" /></label>
          <button class="btn-secondary" :disabled="simulation.mutationLoading" @click="recordEffect">记录模拟效果</button>
        </article>
      </section>

      <section class="card diff-card">
        <header class="section-header"><div><p class="eyebrow">FROZEN EVIDENCE</p><h2>差异与冲突</h2></div><div class="actions"><button class="btn-secondary" :disabled="simulation.loading" @click="simulation.loadDiff()">重新评估</button><button class="btn-primary" :disabled="!simulation.diffEvaluationId" @click="simulation.exportDiff">导出证据</button></div></header>
        <div class="table-wrap"><table class="table"><thead><tr><th class="table-th">资源</th><th class="table-th">键</th><th class="table-th">状态</th><th class="table-th">模拟值</th></tr></thead><tbody><tr v-for="item in simulation.diffItems" :key="`${item.resourceKind}:${item.resourceKey}`" class="table-tr"><td class="table-td">{{ item.resourceKind }}</td><td class="table-td font-mono">{{ item.resourceKey }}</td><td class="table-td"><span :class="item.status === 'conflict' ? 'badge-error' : item.status === 'stale' ? 'badge-warning' : 'badge-success'">{{ item.status }}</span></td><td class="table-td"><code>{{ displayValue(item.simulation) }}</code></td></tr><tr v-if="!simulation.diffItems.length"><td class="table-td empty" colspan="4">暂无差异；读取仍来自冻结 base document。</td></tr></tbody></table></div>
        <button v-if="simulation.diffNextCursor" class="btn-ghost" @click="simulation.loadDiff(simulation.diffNextCursor || undefined)">加载更多差异</button>
      </section>

      <section class="card effects-card"><header class="section-header"><div><p class="eyebrow">DETERMINISTIC RECORDER</p><h2>模拟效果记录</h2></div></header><ul><li v-for="effect in simulation.effects" :key="effect.id"><span class="badge-info">{{ effect.kind }}</span><strong>{{ effect.command }}</strong><span>{{ effect.note || '—' }}</span><time>{{ effect.createdAt }}</time></li><li v-if="!simulation.effects.length" class="empty">尚无效果记录；此区域绝不代表真实外部副作用。</li></ul><button v-if="simulation.effectsNextCursor" class="btn-ghost" @click="simulation.loadEffects(simulation.effectsNextCursor || undefined)">加载更多记录</button></section>
    </template>
    </template>
  </main>
</template>

<style scoped>
.simulation-page{max-width:1280px;margin:0 auto;padding:28px;display:grid;gap:18px}.simulation-banner{position:sticky;top:0;z-index:20;display:flex;justify-content:space-between;gap:20px;padding:18px 20px;border:2px solid var(--status-warning);background:var(--bg-elevated);box-shadow:4px 4px 0 var(--status-warning)}.eyebrow{margin:0 0 6px;font:var(--font-size-xs) var(--font-mono);letter-spacing:.12em;color:var(--status-warning)}h1,h2{margin:0;font-family:var(--font-mono);color:var(--text-primary)}h1{font-size:var(--font-size-3xl)}.banner-detail,.notice,.counter{font:var(--font-size-xs) var(--font-mono);color:var(--text-secondary)}.banner-status{display:flex;align-items:flex-end;flex-direction:column;gap:8px}.notice{margin:0;padding:12px 14px;background:var(--bg-surface);outline:1px solid var(--status-warning)}.simulation-pricing-entry{display:flex;justify-content:flex-end}.error{margin:0;color:var(--status-error);font-family:var(--font-mono)}.state,.empty{padding:24px;color:var(--text-tertiary);font-family:var(--font-mono)}.workspace-grid{display:grid;grid-template-columns:1.1fr .9fr;gap:18px}.card{display:grid;gap:14px}.create-card{max-width:720px;margin:48px auto;width:100%}.create-form{display:grid;gap:14px}.section-header,.actions{display:flex;align-items:center;justify-content:space-between;gap:10px}.section-header .eyebrow{font-size:var(--font-size-caption)}.config-list{border:1px solid var(--border-base);max-height:480px;overflow:auto}.config-row{display:grid;grid-template-columns:180px 1fr;gap:12px;width:100%;padding:12px;text-align:left;border:0;border-bottom:1px solid var(--border-base);background:transparent;color:var(--text-primary);cursor:pointer}.config-row:hover{background:var(--bg-elevated)}code{font:var(--font-size-xs) var(--font-mono);white-space:pre-wrap;overflow-wrap:anywhere;color:var(--text-secondary)}label{display:grid;gap:6px;font:var(--font-size-xs) var(--font-mono);color:var(--text-secondary)}.effect-divider{height:1px;background:var(--border-base);margin:4px 0}.table-wrap{overflow:auto}.table-td{vertical-align:top}.effects-card ul{display:grid;gap:0;margin:0;padding:0;list-style:none;border:1px solid var(--border-base)}.effects-card li{display:grid;grid-template-columns:auto 180px 1fr auto;align-items:center;gap:10px;padding:11px;border-bottom:1px solid var(--border-base);font-size:var(--font-size-xs);color:var(--text-secondary)}.effects-card time{font-family:var(--font-mono);color:var(--text-tertiary)}@media(max-width:760px){.simulation-page{padding:18px}.simulation-banner,.workspace-grid{grid-template-columns:1fr;display:grid}.banner-status{align-items:stretch}.config-row,.effects-card li{grid-template-columns:1fr}.section-header,.actions{align-items:stretch;flex-direction:column}}
</style>
