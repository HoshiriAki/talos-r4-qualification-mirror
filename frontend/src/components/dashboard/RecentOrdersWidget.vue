<script setup lang="ts">
import type { RecentOrder } from '@/api/dashboard'

const props = withDefaults(defineProps<{
  orders?: RecentOrder[]
}>(), {
  orders: () => [],
})
</script>

<template>
  <div class="recent-orders-widget">
    <div v-if="orders.length === 0" class="text-xs text-text-muted p-4 font-mono text-center">
      暂无订单
    </div>
    <div
      v-for="order in orders"
      :key="order.id"
      class="order-row"
    >
      <div class="flex items-center gap-3 min-w-0">
        <span class="badge shrink-0">{{ order.orderNo }}</span>
        <span class="text-text-muted shrink-0 font-mono text-xs">{{ order.province || '-' }}</span>
        <span class="text-text-muted text-xs font-mono truncate hidden sm:inline">{{ order.startDate }} — {{ order.endDate }}</span>
      </div>
      <span class="font-bold text-sm font-mono text-text-primary shrink-0 ml-2">¥{{ order.totalPrice.toFixed(0) }}</span>
    </div>
  </div>
</template>

<style scoped>
.recent-orders-widget {
  max-height: 300px;
  overflow-y: auto;
}

.order-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 14px;
  border-bottom: 1px solid var(--border-default);
  transition: background var(--transition-fast);
}

.order-row:hover {
  background: var(--bg-surface-hover);
}

.order-row:last-child {
  border-bottom: none;
}

.badge {
  font-family: var(--font-mono);
  font-size: 10px;
  padding: 2px 8px;
  border: 1px solid var(--border-default);
  background: var(--bg-surface);
  color: var(--text-secondary);
  letter-spacing: 0.3px;
}
</style>
