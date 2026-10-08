<script setup lang="ts">
/**
 * StatusIndicator — 8×8px square with 4px micro-radius
 * §6.3 of Industrial Avant UI v1.1
 *
 * Variants:
 *   - success:  solid fill = var(--status-success)  → completed/正常
 *   - warning:  half-fill = var(--status-warning)   → in-progress/待处理
 *   - pending:  hollow border = var(--text-tertiary)    → pending/空闲
 *   - error:    solid + stripes = var(--status-error) → overdue/错误
 *   - cancelled: hollow + X = var(--text-tertiary)      → cancelled/已取消
 *
 * ALWAYS pair with text label. NEVER rely on color alone.
 */
defineProps<{
  variant: 'success' | 'warning' | 'pending' | 'error' | 'cancelled'
  label?: string
}>()
</script>

<template>
  <span class="status-indicator-wrapper">
    <span
      class="status-indicator"
      :class="`status-indicator--${variant}`"
      role="img"
      :aria-label="label || variant"
    />
    <span v-if="label" class="status-indicator-label">{{ label }}</span>
  </span>
</template>

<style scoped>
.status-indicator-wrapper {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.status-indicator {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}

.status-indicator--success {
  background: var(--status-success);
}

.status-indicator--warning {
  background: linear-gradient(
    to right,
    var(--status-warning) 50%,
    transparent 50%
  );
  border: 1px solid var(--status-warning);
}

.status-indicator--pending {
  background: transparent;
  border: 1px solid var(--text-tertiary);
}

.status-indicator--error {
  background: var(--status-error);
  background-image: repeating-linear-gradient(
    45deg,
    transparent,
    transparent 2px,
    rgba(0, 0, 0, 0.3) 2px,
    rgba(0, 0, 0, 0.3) 4px
  );
}

.status-indicator--cancelled {
  background: transparent;
  border: 1px solid var(--text-tertiary);
  position: relative;
}

.status-indicator--cancelled::before,
.status-indicator--cancelled::after {
  content: '';
  position: absolute;
  top: 50%;
  left: 50%;
  width: 8px;
  height: 1px;
  background: var(--text-tertiary);
}

.status-indicator--cancelled::before {
  transform: translate(-50%, -50%) rotate(45deg);
}

.status-indicator--cancelled::after {
  transform: translate(-50%, -50%) rotate(-45deg);
}

.status-indicator-label {
  font-family: var(--font-sans);
  font-size: var(--font-size-sm);
  color: var(--text-secondary);
  line-height: 1;
}
</style>
