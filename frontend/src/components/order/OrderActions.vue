<script setup lang="ts">
import { computed } from 'vue'
import type { OrderAllowedAction } from '@/api/orders'

const props = defineProps<{
  orderId: string
  currentStatus?: string
  allowedActions?: OrderAllowedAction[]
}>()

const emit = defineEmits<{
  action: [action: string, expectedVersion: number, reason: string]
}>()

const availableActions = computed(() => props.allowedActions || [])

function handleClick(action: OrderAllowedAction) {
  emit('action', action.action, action.expectedVersion, '')
}
</script>

<template>
  <div v-if="availableActions.length > 0" class="order-actions">
    <span class="text-xs text-text-muted mr-2 font-mono">操作:</span>
    <Button
      v-for="action in availableActions"
      :key="`${action.action}:${action.targetStatus}`"
      :label="action.label"
      size="small"
      variant="outlined"
      severity="secondary"
      class="order-action-btn"
      @click="handleClick(action)"
    />
  </div>
</template>

<style scoped>
.order-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 0;
  flex-wrap: wrap;
}

.order-action-btn {
  font-family: var(--font-mono);
  font-size: 12px;
}
</style>
