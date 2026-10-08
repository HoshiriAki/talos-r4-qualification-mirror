<script setup lang="ts">
import { computed } from 'vue'
import { Icon } from '@iconify/vue'

const props = withDefaults(defineProps<{
  tone?: 'success' | 'info'
  message: string
}>(), {
  tone: 'info',
})

const icon = computed(() => props.tone === 'success'
  ? 'material-symbols:check-circle'
  : 'material-symbols:info')
</script>

<template>
  <div class="auth-notice" :class="`auth-notice--${tone}`" role="status" aria-live="polite">
    <Icon class="auth-notice-icon" :icon="icon" aria-hidden="true" />
    <span>{{ message }}</span>
  </div>
</template>

<style scoped>
.auth-notice {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-s) var(--space-m);
  border: 1px solid currentColor;
  border-radius: var(--radius-md);
  font-family: var(--font-sans);
  font-size: var(--font-size-sm);
  line-height: var(--line-height-normal);
}
.auth-notice-icon {
  flex: 0 0 auto;
  width: var(--font-size-xl);
  height: var(--font-size-xl);
  margin-top: var(--space-1);
}
.auth-notice--success {
  color: var(--status-success);
  background: rgba(var(--status-success-rgb), 0.08);
}
.auth-notice--info {
  color: var(--status-info);
  background: rgba(var(--status-info-rgb), 0.08);
}
</style>
