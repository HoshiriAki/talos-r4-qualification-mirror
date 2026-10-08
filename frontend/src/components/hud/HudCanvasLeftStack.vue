<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import type { DashboardStats } from '@/api/dashboard'

const props = defineProps<{ model: DashboardStats | null }>()
const router = useRouter()

interface StackItem {
  key: string
  glyph: string
  label: string
  tag?: string
  tagVariant?: 'accent' | 'warning'
  route: string
  count?: number
}

const items = computed<StackItem[]>(() => [
  {
    key: 'new-order',
    glyph: '+',
    label: '新订单',
    tag: props.model && props.model.todayNewOrders > 0 ? 'NEW' : undefined,
    tagVariant: 'accent',
    route: '/app/pricing',
    count: props.model?.todayNewOrders ?? 0,
  },
  {
    key: 'devices',
    glyph: '▦',
    label: '设备台账',
    route: '/app/devices',
    count: props.model?.totalDevices ?? 0,
  },
  {
    key: 'exceptions',
    glyph: '!',
    label: '异常处理',
    tag: props.model && props.model.overdueReturns > 0 ? '!!' : undefined,
    tagVariant: 'warning',
    route: '/app/orders',
    count: props.model?.overdueReturns ?? 0,
  },
])

function navigate(route: string) {
  router.push(route)
}
</script>

<template>
  <div class="hud-left-stack" v-if="items.length">
    <button
      v-for="item in items"
      :key="item.key"
      class="stack-card"
      type="button"
      :aria-label="`${item.label}${item.count != null ? `，${item.count}` : ''}`"
      @click="navigate(item.route)"
    >
      <span
        v-if="item.tag"
        class="stack-tag"
        :class="{ 'stack-tag--warning': item.tagVariant === 'warning' }"
      >{{ item.tag }}</span>
      <div class="stack-icon">
        <span aria-hidden="true">{{ item.glyph }}</span>
      </div>
      <div class="stack-label mono">{{ item.label }}</div>
      <div class="stack-count mono">{{ item.count }}</div>
      <div class="dot-trail" aria-hidden="true">
        <i v-for="n in 5" :key="n"></i>
      </div>
    </button>
  </div>
</template>

<style scoped>
.hud-left-stack {
  display: flex;
  flex-direction: column;
  gap: 14px;
  width: 140px;
}

.stack-card {
  position: relative;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  border-radius: 10px;
  padding: 14px;
  cursor: pointer;
  text-align: left;
  color: var(--text-primary);
  transition: border-color var(--duration-micro), background var(--duration-micro), transform var(--duration-micro);
  overflow: hidden;
}
.stack-card:hover, .stack-card:focus-visible {
  border-color: var(--border-strong);
  background: var(--bg-field);
  transform: translateX(4px);
}

.stack-icon {
  font-size: 20px;
  margin-bottom: 26px;
  color: var(--text-primary);
}

.stack-label {
  font-weight: 700;
  font-size: 14px;
}

.stack-count {
  position: absolute;
  right: 12px;
  bottom: 12px;
  color: var(--text-secondary);
  font-size: 12px;
  font-weight: 700;
}

.stack-tag {
  position: absolute;
  top: 10px;
  right: 10px;
  font-size: 9px;
  padding: 2px 6px;
  border-radius: 5px;
  background: var(--accent);
  color: #fff;
  font-family: var(--font-mono);
  letter-spacing: 0.03em;
}
.stack-tag--warning {
  background: var(--status-warning);
  color: var(--bg-base);
}

.dot-trail {
  position: absolute;
  bottom: 10px;
  left: 14px;
  display: flex;
  gap: 3px;
}
.dot-trail i {
  width: 3px;
  height: 3px;
  border-radius: var(--radius-sm);
  background: var(--border-strong);
}

@media (max-width: 1100px) {
  .hud-left-stack { display: none; }
}
@media (prefers-reduced-motion: reduce) {
  .stack-card { transition: none; }
}
</style>
