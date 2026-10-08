<script setup lang="ts">
import type { DashboardStats } from '@/api/dashboard'
import StatusIndicator from '@/components/common/StatusIndicator.vue'
import { shanghaiBusinessDate, shanghaiDateRange } from '@/utils/businessDate'

defineProps<{ model: DashboardStats | null; loading: boolean }>()
</script>

<template>
  <div class="work-surface">
    <!-- Module bar -->
    <div class="module-bar">
      <span class="module-number-label">MODULE-01</span>
      <div class="structure-line" />
      <span class="module-page-label">{{ shanghaiBusinessDate() }}</span>
    </div>

    <!-- KPI row -->
    <div class="kpi-row" v-if="model">
      <div class="kpi-card">
        <span class="kpi-label">在租订单</span>
        <span class="kpi-value">{{ model.activeOrders }}</span>
      </div>
      <div class="kpi-card">
        <span class="kpi-label">在租设备</span>
        <span class="kpi-value">{{ model.devicesOut }}</span>
      </div>
      <div class="kpi-card">
        <span class="kpi-label">可用设备</span>
        <span class="kpi-value">{{ model.availableDevices }}</span>
      </div>
      <div class="kpi-card kpi-warning" v-if="model.overdueReturns > 0">
        <span class="kpi-label">逾期</span>
        <span class="kpi-value">{{ model.overdueReturns }}</span>
      </div>
    </div>

    <!-- Recent orders table -->
    <div class="card" v-if="model?.recentOrders?.length">
      <h3 class="panel-title">最近订单</h3>
      <table class="table">
        <thead>
          <tr><th class="table-th">订单号</th><th class="table-th">省</th><th class="table-th">金额</th><th class="table-th">日期</th></tr>
        </thead>
        <tbody>
          <tr v-for="o in model.recentOrders" :key="o.id" class="table-tr">
            <td class="table-td font-mono">{{ o.orderNo }}</td>
            <td class="table-td">{{ o.province || '-' }}</td>
            <td class="table-td">{{ o.totalPrice ? '¥' + o.totalPrice : '-' }}</td>
            <td class="table-td">{{ o.startDate }} → {{ o.endDate }}</td>
          </tr>
        </tbody>
      </table>
    </div>

    <slot />
  </div>
</template>

<style scoped>
.work-surface { display: flex; flex-direction: column; gap: 20px; }
.module-bar { display: flex; align-items: center; gap: 12px; margin-bottom: 8px; }
.kpi-row { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }
.kpi-card { background: var(--bg-surface); border: 1px solid var(--border-base); border-radius: 12px; padding: 20px; display: flex; flex-direction: column; gap: 6px; }
.kpi-label { font-family: 'Inter', sans-serif; font-size: 12px; color: var(--text-tertiary); }
.kpi-value { font-family: 'Space Mono', monospace; font-size: 36px; font-weight: 700; color: var(--text-primary); }
.kpi-warning .kpi-value { color: var(--warning); }
</style>
