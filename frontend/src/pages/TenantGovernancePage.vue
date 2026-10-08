<script setup lang="ts">
import { onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { useTenantGovernanceStore } from '@/stores/tenantGovernance'
import { shanghaiTimestampLabel } from '@/utils/businessDate'

const router = useRouter()
const store = useTenantGovernanceStore()
let searchTimer: ReturnType<typeof setTimeout> | undefined

function refresh(reset = false) {
  if (reset) store.directoryQuery.cursor = undefined
  return store.fetchDirectory().catch(() => undefined)
}
function search() {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(() => refresh(true), 250)
}
function nextPage() {
  if (!store.directoryNextCursor) return
  store.directoryQuery.cursor = store.directoryNextCursor
  refresh()
}
onMounted(() => refresh())
</script>

<template>
  <main class="governance-page">
    <header class="hero">
      <div><p class="eyebrow">CONTROL PLANE / MVP 1</p><h1>租户治理</h1><p class="subtitle">观察租户目录与健康状态。所有生产配置变更必须先记录变更意图。</p></div>
      <button class="btn-secondary" type="button" :disabled="store.loading.directory" @click="refresh()">刷新目录</button>
    </header>

    <section class="summary-grid" aria-label="租户目录摘要">
      <div class="card"><span>当前页租户</span><strong>{{ store.tenants.length }}</strong></div>
      <div class="card"><span>当前页活跃</span><strong>{{ store.activeCount }}</strong></div>
      <div class="card"><span>健康检查入口</span><strong>详情</strong></div>
    </section>

    <section class="directory-section">
      <div class="toolbar">
        <label><span>搜索租户</span><input v-model="store.directoryQuery.search" class="input" placeholder="名称、slug 或 ID" @input="search" /></label>
        <label><span>生命周期</span><select v-model="store.directoryQuery.status" class="input" @change="refresh(true)"><option value="">全部状态</option><option value="active">活跃</option><option value="suspended">已暂停</option><option value="inactive">未启用</option></select></label>
      </div>
      <p v-if="store.loading.directory" class="state" role="status">正在读取租户目录…</p>
      <div v-else-if="store.errors.directory" class="state error" role="alert"><p>{{ store.errors.directory }}</p><button class="btn-secondary" @click="refresh()">重试</button></div>
      <div v-else-if="store.tenants.length" class="directory-grid">
        <button v-for="tenant in store.tenants" :key="tenant.id" class="tenant-card card-hover" type="button" @click="router.push(`/control/governance/${tenant.id}`)">
          <span class="tenant-top"><span class="tenant-code">{{ tenant.slug }}</span><span class="status" :data-status="tenant.status">{{ tenant.status }}</span></span>
          <strong>{{ tenant.name }}</strong><small>{{ tenant.id }}</small>
          <span class="tenant-meta"><span>计划 {{ tenant.plan }}</span><span>更新 {{ shanghaiTimestampLabel(tenant.updatedAt) }}</span></span>
        </button>
      </div>
      <p v-else class="state">没有符合条件的租户</p>
      <footer class="pagination"><span>本页 {{ store.tenants.length }} 个租户</span><button class="btn-ghost" :disabled="!store.directoryNextCursor" @click="nextPage">下一页</button></footer>
    </section>
  </main>
</template>

<style scoped>
.governance-page{max-width:1440px;margin:0 auto;padding:28px;display:grid;gap:22px}.hero,.toolbar,.pagination,.tenant-top,.tenant-meta{display:flex;align-items:center;justify-content:space-between;gap:16px}.eyebrow{margin:0 0 7px;font:12px var(--font-mono);letter-spacing:.12em;color:var(--accent)}h1{margin:0;font:700 28px var(--font-mono);letter-spacing:-.04em;color:var(--text-primary)}.subtitle{margin:9px 0 0;color:var(--text-secondary)}.summary-grid{display:grid;grid-template-columns:repeat(3,1fr);gap:14px}.summary-grid .card{display:grid;gap:8px}.summary-grid span{color:var(--text-tertiary);font-size:12px}.summary-grid strong{font:700 36px var(--font-mono);color:var(--text-primary)}.directory-section{display:grid;gap:14px}.toolbar{padding:14px;border:1px solid var(--border-base);border-radius:var(--radius-xl);background:var(--bg-surface)}.toolbar label{display:grid;gap:6px;flex:1;color:var(--text-tertiary);font-size:12px}.toolbar label:last-child{max-width:220px}.directory-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(280px,1fr));gap:14px}.tenant-card{text-align:left;display:grid;gap:13px;color:var(--text-secondary)}.tenant-card>strong{font:600 18px var(--font-mono);color:var(--text-primary)}.tenant-card small,.tenant-code{font:12px var(--font-mono);color:var(--text-tertiary)}.status{font:600 12px var(--font-mono);padding:3px 7px;border:1px solid var(--border-base);border-radius:var(--radius-lg)}[data-status=active]{color:var(--status-success)}[data-status=suspended]{color:var(--status-warning)}[data-status=inactive]{color:var(--status-error)}.tenant-meta{padding-top:12px;border-top:1px solid var(--border-base);font-size:12px}.state{padding:42px;text-align:center;border:1px dashed var(--border-base);border-radius:var(--radius-xl);color:var(--text-tertiary)}.error{color:var(--status-error)}.pagination{color:var(--text-tertiary);font-size:12px}@media(max-width:720px){.governance-page{padding:18px}.hero,.toolbar{align-items:stretch;flex-direction:column}.summary-grid{grid-template-columns:1fr}.toolbar label:last-child{max-width:none}}
</style>
