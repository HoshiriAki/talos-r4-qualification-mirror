<script setup lang="ts">
import type { WidgetConfig } from '@/stores/dashboard'

defineProps<{
  config: WidgetConfig
  title: string
}>()
</script>

<template>
  <div class="widget-panel">
    <div class="widget-panel-head">
      <span class="widget-panel-label">{{ title }}</span>
      <span class="status-indicator status-indicator--success" />
    </div>
    <slot />
  </div>
</template>

<style scoped>
/* ── Match sys-telemetry exactly ──────────────────────────────────── */
.widget-panel {
  background: var(--bg-surface);
  border: 1px solid var(--border-default);
  border-radius: var(--radius-xl);
  padding: 16px;
  box-shadow: var(--shadow-offset-default);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.widget-panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.widget-panel-label {
  font-family: var(--font-mono);
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.1em;
  color: var(--text-muted);
  text-transform: uppercase;
}
</style>
