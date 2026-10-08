<script setup lang="ts">
import { computed } from 'vue'
import type { DashboardStats } from '@/api/dashboard'

const props = defineProps<{ model: DashboardStats | null }>()

const alerts = computed(() => {
  if (!props.model) return []
  return [
    { code: 'RISK', label: '逾期归还', count: props.model.overdueReturns, tone: 'critical' },
    { code: 'DUE', label: '今日待还', count: props.model.returnsDueToday, tone: 'warning' },
    { code: 'NEW', label: '今日新单', count: props.model.todayNewOrders, tone: 'normal' },
  ].filter(item => item.count > 0)
})
</script>

<template>
  <div class="alerts-stack" aria-label="实时状态摘要">
    <article v-for="alert in alerts" :key="alert.code" class="alert-item" :class="`is-${alert.tone}`">
      <div class="alert-meta"><span>{{ alert.code }}</span><i></i><b>{{ String(alert.count).padStart(2, '0') }}</b></div>
      <span class="alert-label">{{ alert.label }}</span>
    </article>
    <div v-if="alerts.length === 0" class="alert-empty">等待实时状态</div>
  </div>
</template>

<style scoped>
.alerts-stack { display: grid; gap: 5px; }
.alert-item { position: relative; display: grid; align-items: end; min-height: 46px; padding: 8px 10px; overflow: hidden; border: 2px solid var(--text-primary); border-radius: 0 10px 0 10px; background: color-mix(in srgb, var(--text-primary) 94%, transparent); color: var(--bg-base); box-shadow: 3px 3px 0 var(--bg-base); }
.alert-meta { grid-column: 1 / -1; display: flex; align-items: center; gap: 7px; font: 700 11px/1 var(--font-mono); letter-spacing: .1em; }
.alert-meta i { flex: 1; height: 2px; background: currentColor; opacity: .3; }
.alert-meta b { font: 700 20px/1 var(--font-mono); }
.alert-label { font: 700 11px/1 var(--font-sans); }
.is-critical { background: var(--bg-base); color: var(--accent); }
.is-warning { background: var(--text-primary); color: var(--bg-base); }
.is-normal { background: color-mix(in srgb, var(--text-primary) 82%, var(--accent)); color: var(--bg-base); }
.alert-empty { padding: 10px; border: 2px dashed var(--text-primary); color: var(--text-primary); font: 700 11px/1.4 var(--font-mono); }
</style>
