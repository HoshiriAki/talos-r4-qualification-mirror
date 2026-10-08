<script setup lang="ts">
import { computed } from 'vue'
import type { TalosStatus } from '../component-types'

const props = withDefaults(defineProps<{
  status?: TalosStatus
  label?: string
  stacked?: boolean
}>(), {
  status: 'pending',
  label: '',
  stacked: false,
})

const defaults: Record<TalosStatus, string> = {
  completed: '已完成',
  active: '进行中',
  pending: '待处理',
  overdue: '已逾期',
  cancelled: '已取消',
}

const displayLabel = computed(() => props.label || defaults[props.status])
</script>

<template>
  <span
    class="talos-status"
    :class="[
      `talos-status--${status}`,
      { 'talos-status--stacked': stacked },
    ]"
    role="status"
    :aria-label="displayLabel"
  >
    <span class="talos-status__mark" aria-hidden="true" data-lab-slot="status-mark" />
    <span class="talos-status__label" data-lab-slot="status-label">{{ displayLabel }}</span>
  </span>
</template>

<style scoped>
.talos-status {
  --talos-status-color: var(--text-tertiary);
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--text-secondary);
  font-family: var(--font-sans);
  font-size: var(--font-size-xs);
  line-height: 1.3;
}

.talos-status--stacked {
  flex-direction: column;
  align-items: flex-start;
}

.talos-status__mark {
  position: relative;
  display: inline-block;
  width: 8px;
  height: 8px;
  flex: 0 0 8px;
  border: 1px solid var(--talos-status-color);
  border-radius: var(--radius-sm);
  background: transparent;
}

.talos-status--completed {
  --talos-status-color: var(--status-success);
}

.talos-status--completed .talos-status__mark {
  background: var(--talos-status-color);
}

.talos-status--active {
  --talos-status-color: var(--status-info);
}

.talos-status--active .talos-status__mark {
  background: linear-gradient(90deg, var(--talos-status-color) 50%, transparent 50%);
}

.talos-status--pending {
  --talos-status-color: var(--text-tertiary);
}

.talos-status--overdue {
  --talos-status-color: var(--status-error);
}

.talos-status--overdue .talos-status__mark {
  background: repeating-linear-gradient(
    135deg,
    var(--talos-status-color) 0 2px,
    transparent 2px 4px
  );
}

.talos-status--cancelled {
  --talos-status-color: var(--text-tertiary);
}

.talos-status--cancelled .talos-status__mark::before,
.talos-status--cancelled .talos-status__mark::after {
  content: '';
  position: absolute;
  left: 1px;
  top: 3px;
  width: 4px;
  height: 1px;
  background: var(--talos-status-color);
}

.talos-status--cancelled .talos-status__mark::before { transform: rotate(45deg); }
.talos-status--cancelled .talos-status__mark::after { transform: rotate(-45deg); }
</style>
