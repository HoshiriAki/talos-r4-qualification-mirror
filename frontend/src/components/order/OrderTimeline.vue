<script setup lang="ts">
import { computed } from 'vue'
import { STATUS_LABELS } from '@/api/orders'

const props = defineProps<{
  currentStatus?: string           // current DB status value
  history?: { from: string; to: string; at: string; by: string }[]
}>()

// 9 态顺序 (不含 cancelled — 它由终止态单独处理)
const ORDER = ['draft', 'confirmed', 'paid', 'shipped', 'in_use', 'returned', 'inspected', 'completed', 'closed']

const steps = computed(() => {
  const currentIdx = ORDER.indexOf(props.currentStatus || '')
  return ORDER.map((key, i) => ({
    key,
    label: STATUS_LABELS[key] || key,
    active: i <= currentIdx,
    isCurrent: i === currentIdx,
  }))
})
</script>

<template>
  <div class="order-timeline">
    <div
      v-for="(step, i) in steps"
      :key="step.key"
      class="timeline-step"
      :class="{
        'timeline-step--active': step.active,
        'timeline-step--current': step.isCurrent,
      }"
    >
      <div class="timeline-dot" />
      <div v-if="i < steps.length - 1" class="timeline-line" :class="{ 'timeline-line--active': step.active }" />
      <span class="timeline-label font-mono text-xs">{{ step.label }}</span>
    </div>
  </div>
</template>

<style scoped>
.order-timeline {
  display: flex;
  align-items: flex-start;
  gap: 0;
  padding: 12px 0;
  overflow-x: auto;
}

.timeline-step {
  display: flex;
  flex-direction: column;
  align-items: center;
  position: relative;
  min-width: 52px;
  flex-shrink: 0;
}

.timeline-dot {
  width: 10px;
  height: 10px;
  border-radius: var(--radius-sm);
  background: var(--border);
  border: 2px solid var(--border);
  z-index: 1;
  transition: background 0.2s, border-color 0.2s;
}

.timeline-line {
  position: absolute;
  top: 4px;
  left: calc(50% + 7px);
  width: calc(100% - 14px);
  height: 2px;
  background: var(--border);
  transition: background 0.2s;
}

.timeline-label {
  margin-top: 6px;
  color: var(--text-muted);
  text-align: center;
  white-space: nowrap;
  transition: color 0.2s;
}

.timeline-step--active .timeline-dot {
  background: var(--primary);
  border-color: var(--primary);
}

.timeline-step--active .timeline-line--active {
  background: var(--primary);
}

.timeline-step--active .timeline-label {
  color: var(--text-primary);
}

.timeline-step--current .timeline-dot {
  width: 14px;
  height: 14px;
  border: 2px solid var(--accent);
}
</style>
