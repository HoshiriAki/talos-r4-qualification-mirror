<script setup lang="ts">
import type { Order } from '@/api/orders'
import { getOrderSerialNos } from '@/composables/useOrdersWorkbench'
import { STATUS_LABELS } from '@/api/orders'

defineProps<{ order: Order | null }>()
const emit = defineEmits<{ 'close': [] }>()
</script>

<template>
  <Teleport to="body">
    <div v-if="order" class="context-overlay" @click.self="emit('close')">
      <div class="context-panel">
        <div class="context-header">
          <h3 class="context-title">{{ order.orderNo }}</h3>
          <button class="context-close" @click="emit('close')">×</button>
        </div>
        <dl class="context-body">
          <dt>状态</dt><dd>{{ STATUS_LABELS[order.status || ''] || order.status }}</dd>
          <dt>设备</dt><dd>{{ getOrderSerialNos(order).join(', ') || '-' }}</dd>
          <dt>日期</dt><dd>{{ order.startDate }} → {{ order.endDate }}</dd>
          <dt>金额</dt><dd>{{ order.totalPrice ? '¥' + order.totalPrice : '-' }}</dd>
          <dt>省</dt><dd>{{ order.province || '-' }}</dd>
        </dl>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.context-overlay { position: fixed; inset: 0; z-index: 90; display: flex; justify-content: flex-end; }
.context-panel { width: 340px; max-width: 90vw; background: var(--bg-surface); border-left: 1px solid var(--border-base); height: 100%; display: flex; flex-direction: column; }
.context-header { display: flex; justify-content: space-between; align-items: center; padding: 16px; border-bottom: 1px solid var(--border-base); }
.context-title { font-family: 'Space Mono', monospace; font-size: 14px; font-weight: 700; color: var(--text-primary); margin: 0; }
.context-close { background: none; border: none; font-size: 20px; color: var(--text-tertiary); cursor: pointer; }
.context-body { padding: 16px; display: grid; grid-template-columns: auto 1fr; gap: 8px 16px; }
.context-body dt { font-family: 'Space Mono', monospace; font-size: 11px; color: var(--text-tertiary); }
.context-body dd { font-family: 'Inter', sans-serif; font-size: 13px; color: var(--text-primary); margin: 0; }
</style>
