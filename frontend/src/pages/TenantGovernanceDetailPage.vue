<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTenantGovernanceStore } from '@/stores/tenantGovernance'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import { shanghaiTimestampLabel } from '@/utils/businessDate'
import TenantHealthSummary from '@/components/governance/TenantHealthSummary.vue'
import GovernanceAuditTable from '@/components/governance/GovernanceAuditTable.vue'
import ChangeIntentDialog from '@/components/governance/ChangeIntentDialog.vue'
import type { GovernanceAuditEvent } from '@/api/tenant-governance'

const route = useRoute()
const router = useRouter()
const store = useTenantGovernanceStore()
const preview = useTenantPreviewStore()
const intentVisible = ref(false)
const selectedAudit = ref<GovernanceAuditEvent | null>(null)
const successMessage = ref('')
const tenantId = computed(() => String(route.params.id || ''))

async function load() {
  if (!tenantId.value) return
  await Promise.allSettled([
    store.fetchTenant(tenantId.value),
    store.fetchHealth(tenantId.value),
    store.fetchAudit({ tenantId: tenantId.value, cursor: undefined, limit: 10 }),
  ])
}
function recorded(intentId: string) {
  intentVisible.value = false
  successMessage.value = `变更意图 ${intentId} 已记录；生产数据未发生变化。`
  store.fetchAudit({ tenantId: tenantId.value, cursor: undefined, limit: 10 }).catch(() => undefined)
}
function nextAuditPage() {
  if (!store.auditNextCursor) return
  store.fetchAudit({ tenantId: tenantId.value, cursor: store.auditNextCursor, limit: 10 }).catch(() => undefined)
}
async function startPreview() {
  if (!store.selectedTenant || preview.loading) return
  try {
    const session = await preview.start(store.selectedTenant.id)
    await router.push(`/control/workspace/${encodeURIComponent(session.id)}`)
  } catch {
    // The store exposes the server-safe error next to the action.
  }
}
onMounted(load)
onBeforeUnmount(store.resetSelection)
</script>

<template>
  <main class="detail-page">
    <button class="btn-ghost back" type="button" @click="router.push('/control/governance')">← 返回租户目录</button>
    <p v-if="store.loading.detail" class="state" role="status">正在读取租户详情…</p>
    <div v-else-if="store.errors.detail" class="state error" role="alert"><p>{{ store.errors.detail }}</p><button class="btn-secondary" @click="load">重试</button></div>
    <template v-else-if="store.selectedTenant">
      <header class="tenant-hero card-emphasis">
        <div><p class="eyebrow">TENANT / {{ store.selectedTenant.slug }}</p><h1>{{ store.selectedTenant.name }}</h1><p class="tenant-id">{{ store.selectedTenant.id }}</p></div>
        <div class="hero-actions"><span class="status" :data-status="store.selectedTenant.status">{{ store.selectedTenant.status }}</span><button class="btn-secondary" type="button" :disabled="preview.loading || store.selectedTenant.status !== 'active'" @click="startPreview">{{ preview.loading ? '正在创建…' : '进入只读预览' }}</button><button class="btn-primary" type="button" @click="intentVisible = true">记录变更意图</button></div>
      </header>
      <p v-if="preview.error" class="state error" role="alert">{{ preview.error }}</p>
      <p v-if="successMessage" class="success" role="status">{{ successMessage }}</p>
      <section class="identity-grid card" aria-label="租户身份摘要">
        <div><span>订阅计划</span><strong>{{ store.selectedTenant.plan }}</strong></div><div><span>创建时间（上海）</span><strong>{{ shanghaiTimestampLabel(store.selectedTenant.createdAt) }}</strong></div><div><span>最后更新（上海）</span><strong>{{ shanghaiTimestampLabel(store.selectedTenant.updatedAt) }}</strong></div>
      </section>
      <TenantHealthSummary :health="store.health" :loading="store.loading.health" :error="store.errors.health" />
      <section class="audit-section">
        <header><div><p class="eyebrow">IMMUTABLE AUDIT</p><h2>最近治理事件</h2></div><button class="btn-secondary" type="button" @click="store.fetchAudit({ tenantId, cursor: undefined, limit: 10 })">刷新审计</button></header>
        <GovernanceAuditTable :events="store.auditEvents" :loading="store.loading.audit" :error="store.errors.audit" @select="selectedAudit = $event" />
        <footer class="audit-pagination">
          <span>本页 {{ store.auditEvents.length }} 条事件</span>
          <button class="btn-ghost" type="button" :disabled="!store.auditNextCursor || store.loading.audit" @click="nextAuditPage">下一页</button>
        </footer>
      </section>
    </template>

    <ChangeIntentDialog v-if="store.selectedTenant" :visible="intentVisible" :tenant-id="store.selectedTenant.id" :tenant-name="store.selectedTenant.name" @close="intentVisible = false" @recorded="recorded" />
    <div v-if="selectedAudit" class="audit-overlay" @click.self="selectedAudit = null"><aside class="audit-detail glass" role="dialog" aria-modal="true" aria-labelledby="audit-event-title"><header><h2 id="audit-event-title">审计事件</h2><button class="btn-ghost" @click="selectedAudit = null">×</button></header><dl><dt>命令</dt><dd>{{ selectedAudit.command }}</dd><dt>结果</dt><dd>{{ selectedAudit.result }}</dd><dt>关联 ID</dt><dd>{{ selectedAudit.correlationId }}</dd><dt>来源</dt><dd>{{ selectedAudit.source || '—' }}</dd><dt>变更意图</dt><dd>{{ selectedAudit.changeIntentId || '—' }}</dd><dt>详情</dt><dd><pre>{{ JSON.stringify(selectedAudit.detail || {}, null, 2) }}</pre></dd></dl></aside></div>
  </main>
</template>

<style scoped>
.detail-page{max-width:1280px;margin:0 auto;padding:28px;display:grid;gap:20px}.back{justify-self:start}.tenant-hero,.tenant-hero,.hero-actions,.audit-section>header,.audit-detail header{display:flex;align-items:center;justify-content:space-between;gap:18px}.eyebrow{margin:0 0 7px;font:12px var(--font-mono);letter-spacing:.12em;color:var(--accent)}h1{margin:0;font:700 28px var(--font-mono);color:var(--text-primary)}h2{margin:0;font:600 18px var(--font-mono);color:var(--text-primary)}.tenant-id{margin:8px 0 0;font:12px var(--font-mono);color:var(--text-tertiary)}.hero-actions{align-items:flex-end;flex-direction:column}.status{padding:5px 10px;border:1px solid currentColor;border-radius:var(--radius-lg);font:600 12px var(--font-mono)}[data-status=active]{color:var(--status-success)}[data-status=suspended]{color:var(--status-warning)}[data-status=inactive]{color:var(--status-error)}.identity-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:1px;padding:0;overflow:hidden;background:var(--border-base)}.identity-grid div{display:grid;gap:7px;padding:17px;background:var(--bg-surface)}.identity-grid span{font-size:12px;color:var(--text-tertiary)}.identity-grid strong{font:600 14px var(--font-mono);color:var(--text-primary)}.audit-section{display:grid;gap:12px}.success{margin:0;padding:12px 14px;border:1px solid var(--status-success);border-radius:var(--radius-md);color:var(--status-success);background:var(--bg-elevated)}.state{padding:42px;text-align:center;color:var(--text-tertiary)}.error{color:var(--status-error)}.audit-overlay{position:fixed;inset:0;z-index:1200;background:var(--overlay-bg);display:flex;justify-content:flex-end}.audit-detail{width:min(520px,100%);height:100%;padding:22px;overflow:auto}.audit-detail dl{display:grid;grid-template-columns:110px 1fr;gap:14px;margin-top:24px}.audit-detail dt{color:var(--text-tertiary);font-size:12px}.audit-detail dd{margin:0;color:var(--text-primary);font-family:var(--font-mono);overflow-wrap:anywhere}.audit-detail pre{white-space:pre-wrap;margin:0;padding:12px;background:var(--bg-elevated);border:1px solid var(--border-base);border-radius:var(--radius-md);font-size:12px}@media(max-width:720px){.detail-page{padding:18px}.tenant-hero,.audit-section>header{align-items:stretch;flex-direction:column}.hero-actions{align-items:stretch}.identity-grid{grid-template-columns:1fr}}
.audit-pagination{display:flex;align-items:center;justify-content:space-between;gap:18px;color:var(--text-tertiary)}
</style>
