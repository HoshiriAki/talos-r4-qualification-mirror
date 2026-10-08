<script setup lang="ts">
defineProps<{
  label: string
  value: string | number
  accent?: string
}>()
</script>

<template>
  <div class="stat-body">
    <div class="stat-value" :style="accent ? { color: accent } : {}">{{ value }}</div>
    <div class="stat-label">{{ label }}</div>
  </div>
</template>

<style scoped>
.stat-body {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.stat-value {
  font-family: var(--font-mono);
  font-size: 28px;
  font-weight: 700;
  line-height: 1.1;
  color: var(--text-primary);
}

.stat-label {
  font-family: var(--font-mono);
  font-size: 10px;
  letter-spacing: 0.06em;
  color: var(--text-muted);
}
</style>
