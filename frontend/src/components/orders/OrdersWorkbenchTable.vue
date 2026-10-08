<script setup lang="ts">
import type { Order } from '@/api/orders'
import { getOrderSerialNos } from '@/composables/useOrdersWorkbench'
import { STATUS_LABELS } from '@/api/orders'

defineProps<{
  rows: Order[]
  selectedIds: Set<string>
  loading: boolean
}>()
const emit = defineEmits<{ 'toggle': [id: string]; 'selectAll': [ids: string[]]; 'openContext': [order: Order] }>()
</script>

<template>
  <table class="table workbench-table">
    <thead>
      <tr>
        <th class="table-th" style="width:40px">
          <input type="checkbox" @change="(e: Event) => emit('selectAll', (e.target as HTMLInputElement).checked ? rows.map(r => r.id) : [])" />
        </th>
        <th class="table-th">订单号</th>
        <th class="table-th">设备</th>
        <th class="table-th">状态</th>
        <th class="table-th">日期</th>
        <th class="table-th">金额</th>
      </tr>
    </thead>
    <tbody>
      <tr v-if="loading && rows.length === 0">
        <td colspan="6" class="table-td text-center">加载中...</td>
      </tr>
      <tr v-for="o in rows" :key="o.id" class="table-tr" @click="emit('openContext', o)" style="cursor:pointer">
        <td class="table-td">
          <input type="checkbox" :checked="selectedIds.has(o.id)" @click.stop @change="emit('toggle', o.id)" />
        </td>
        <td class="table-td font-mono text-sm">{{ o.orderNo }}</td>
        <td class="table-td text-xs">{{ getOrderSerialNos(o).join(', ') || '-' }}</td>
        <td class="table-td">
          <span class="status-badge" :class="o.status">{{ STATUS_LABELS[o.status || ''] || o.status }}</span>
        </td>
        <td class="table-td text-xs">{{ o.startDate }} → {{ o.endDate }}</td>
        <td class="table-td font-mono text-sm">{{ o.totalPrice ? '¥' + o.totalPrice : '-' }}</td>
      </tr>
    </tbody>
  </table>
</template>

<style scoped>
.workbench-table { width: 100%; }
.status-badge { font-family: 'Inter', sans-serif; font-size: 11px; padding: 2px 8px; border-radius: 6px; border: 1px solid var(--border-base); }
</style>
