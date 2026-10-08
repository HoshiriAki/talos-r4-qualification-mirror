<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useTenantPreviewStore, type PreviewSurface } from '@/stores/tenantPreview'
import { useAuthStore } from '@/stores/auth'

const route = useRoute()
const router = useRouter()
const preview = useTenantPreviewStore()
const auth = useAuthStore()
const surface = ref<PreviewSurface>('dashboard')
const sessionId = computed(() => String(route.params.workspaceSessionId || ''))
const iframeUrl = computed(() => `/embedded/preview/${encodeURIComponent(sessionId.value)}/${surface.value}`)
const simulationUrl = computed(() => preview.session?.simulationId
  ? `/control/simulation/${encodeURIComponent(preview.session.simulationId)}`
  : '/control/simulation')

onMounted(async () => {
  try {
    await preview.hydrate(sessionId.value)
  } catch {
    await router.replace('/control/governance')
  }
})

async function closeWorkspace() {
  await preview.exit()
  await router.replace('/control/governance')
}
</script>

<template>
  <section class="workspace">
    <header class="workspace__status">
      <div>
        <span class="module-code">TENANT WORKSPACE / {{ preview.session?.mode?.toUpperCase() || 'PREVIEW' }}</span>
        <h1>{{ preview.session?.tenantName || preview.session?.tenantId || '正在解析租户…' }}</h1>
      </div>
      <dl>
        <div><dt>MODE</dt><dd>{{ preview.session?.mode === 'diagnostics' ? 'DIAGNOSTICS' : preview.session?.mode === 'simulation' ? 'ISOLATED SIMULATION' : 'READ-ONLY PREVIEW' }}</dd></div>
        <div><dt>ACTOR</dt><dd>{{ auth.user?.username }}</dd></div>
        <div><dt>SESSION</dt><dd>{{ sessionId.slice(0, 12) }}</dd></div>
      </dl>
      <button class="btn btn-secondary" type="button" @click="closeWorkspace">结束工作区</button>
    </header>
    <nav v-if="preview.session?.mode !== 'simulation'" class="workspace__nav" aria-label="工作区只读页面">
      <button v-for="item in (['dashboard', 'orders', 'devices', 'audit'] as PreviewSurface[])" :key="item" type="button" :class="{ active: surface === item }" @click="surface = item">
        {{ item.toUpperCase() }}
      </button>
    </nav>
    <div v-if="preview.session?.mode === 'simulation'" class="workspace__simulation">
      <span class="module-code">SIMULATION PLANE</span>
      <h2>该工作区绑定隔离模拟命名空间</h2>
      <p>生产租户 iframe 在模拟模式下保持关闭。所有读取、变更与差异检查由 Simulation Plane 执行。</p>
      <RouterLink class="btn btn-primary" :to="simulationUrl">进入隔离模拟控制台</RouterLink>
    </div>
    <div v-else class="workspace__frame">
      <iframe :src="iframeUrl" title="租户只读预览" sandbox="allow-scripts allow-same-origin allow-forms" />
    </div>
  </section>
</template>

<style scoped>
.workspace { display: grid; grid-template-rows: auto auto minmax(560px, 1fr); gap: 12px; min-height: calc(100vh - 132px); }
.workspace__status { display: grid; grid-template-columns: minmax(220px, 1fr) auto auto; align-items: center; gap: 24px; padding: 18px 20px; border: 1px solid var(--border-default); border-radius: var(--radius-lg); background: var(--bg-elevated); }
.module-code, dt, dd { font-family: var(--font-mono); }
.module-code { color: var(--accent); font-size: var(--font-size-xs); letter-spacing: .1em; }
h1 { margin: 6px 0 0; font-family: var(--font-mono); font-size: var(--font-size-xl); }
dl { display: flex; gap: 22px; margin: 0; }
dl div { display: grid; gap: 4px; }
dt { color: var(--text-tertiary); font-size: var(--font-size-xs); }
dd { margin: 0; color: var(--text-primary); font-size: var(--font-size-sm); }
.workspace__nav { display: flex; gap: 8px; }
.workspace__nav button { min-height: 38px; padding: 0 14px; border: 1px solid var(--border-default); border-radius: var(--radius-sm); background: var(--bg-surface); color: var(--text-secondary); font-family: var(--font-mono); cursor: pointer; }
.workspace__nav button.active { border-color: var(--accent); color: var(--accent); }
.workspace__frame { overflow: hidden; border: 1px solid var(--border-strong); border-radius: var(--radius-lg); background: var(--bg-base); box-shadow: var(--shadow-hard-sm); }
.workspace__simulation { display: grid; align-content: center; justify-items: start; gap: 12px; min-height: 560px; padding: 48px; border: 1px solid var(--border-strong); border-radius: var(--radius-lg); background: var(--bg-base); box-shadow: var(--shadow-offset-accent); }
.workspace__simulation h2 { margin: 0; font-family: var(--font-mono); font-size: var(--font-size-2xl); }
.workspace__simulation p { max-width: 720px; margin: 0; color: var(--text-secondary); }
iframe { display: block; width: 100%; height: 100%; min-height: 560px; border: 0; background: var(--bg-base); }
@media (max-width: 900px) { .workspace__status { grid-template-columns: 1fr; } dl { flex-wrap: wrap; } }
</style>
