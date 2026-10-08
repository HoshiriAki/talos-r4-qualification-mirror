<script setup lang="ts">
import { computed } from 'vue'
import { useUiModeStore } from '@/stores/uiMode'
import { useAuthStore } from '@/stores/auth'
import { useRouter } from 'vue-router'

const props = defineProps<{
  utilizationPercent?: number
  overdueCount?: number
  orderCount?: number
  notificationCount?: number
}>()

const emit = defineEmits<{ exit: [] }>()

const uiMode = useUiModeStore()
const auth = useAuthStore()
const router = useRouter()

const topIcons = computed(() => [
  { key: 'notifications', glyph: '↩', label: '待还', route: '/app/orders', badge: props.notificationCount ?? 0 },
  { key: 'warehouse', glyph: '▦', label: '仓库', route: '/app/settings/warehouses', badge: 0, adminOnly: true },
  { key: 'orders', glyph: '≡', label: '订单', route: '/app/orders', badge: props.orderCount ?? 0 },
  { key: 'intel', glyph: '⌁', label: '情报', route: '/app/audit', badge: 0 },
  { key: 'equipment', glyph: '◫', label: '装备', route: '/app/devices', badge: 0 },
  { key: 'mission', glyph: '!', label: '逾期', route: '/app/overdue', badge: props.overdueCount ?? 0 },
].filter(item => !item.adminOnly || auth.isTenantAdmin))

function handleIconClick(item: typeof topIcons.value[number]) {
  if (item.route) {
    router.push(item.route)
  }
}

function handleExit() {
  if (uiMode.canSwitch) {
    uiMode.setMode('work')
  }
  emit('exit')
}
</script>

<template>
  <div class="hud-canvas-topbar">
    <!-- Top-Left: Exit + Brand -->
    <div class="topbar-left">
      <button class="exit-btn" type="button" aria-label="返回 Work 模式" @click="handleExit">
        <span aria-hidden="true">←</span>
      </button>
      <div class="brand-block">
        <div class="brand-row">
          <div class="brand-icon" aria-hidden="true">
            ◆
          </div>
          <div>
            <div class="brand-name mono">TAL<span class="brand-os">·OS</span></div>
          </div>
        </div>
        <div class="brand-bar"><i :style="{ width: `${utilizationPercent ?? 0}%` }"></i></div>
        <div class="brand-sub mono">设备在外率 {{ utilizationPercent ?? 0 }}%</div>
      </div>
    </div>

    <!-- Top-Right: Icon buttons -->
    <div class="topbar-right">
      <button
        v-for="item in topIcons"
        :key="item.key"
        class="hud-icon-btn"
        type="button"
        :aria-label="item.label"
        @click="handleIconClick(item)"
      >
        <div class="icon-glyph">
          <span aria-hidden="true">{{ item.glyph }}</span>
        </div>
        <span class="icon-label">{{ item.label }}</span>
        <span v-if="item.badge > 0" class="icon-badge mono">{{ item.badge }}</span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.hud-canvas-topbar {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  z-index: 10;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  padding: 28px 32px 0;
  pointer-events: none;
}
.hud-canvas-topbar > * {
  pointer-events: auto;
}

/* ── Left: Exit + Brand ─────────────────────────────────────── */
.topbar-left {
  display: flex;
  align-items: center;
  gap: 16px;
}
.exit-btn {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
  cursor: pointer;
  transition: border-color var(--duration-micro), color var(--duration-micro);
  flex-shrink: 0;
}
.exit-btn:hover, .exit-btn:focus-visible {
  border-color: var(--accent);
  color: var(--accent);
}
.brand-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 190px;
}
.brand-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.brand-icon {
  width: 36px;
  height: 36px;
  border-radius: 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-secondary);
}
.brand-name {
  font-weight: 700;
  font-size: 16px;
  color: var(--text-primary);
}
.brand-os {
  color: var(--text-tertiary);
  font-weight: 400;
}
.brand-bar {
  height: 4px;
  border-radius: 2px;
  background: var(--bg-elevated);
  overflow: hidden;
  margin-top: 2px;
}
.brand-bar i {
  display: block;
  height: 100%;
  width: 75%;
  background: var(--text-primary);
  transition: width var(--duration-slow) ease-out;
}
.brand-sub {
  font-size: 10px;
  color: var(--text-tertiary);
  letter-spacing: 0.06em;
}

/* ── Right: Icon buttons ────────────────────────────────────── */
.topbar-right {
  display: flex;
  gap: 26px;
}
.hud-icon-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  position: relative;
  background: none;
  border: none;
  cursor: pointer;
  color: var(--text-secondary);
  padding: 0;
  transition: color var(--duration-micro);
}
.hud-icon-btn:hover, .hud-icon-btn:focus-visible {
  color: var(--text-primary);
}
.icon-glyph {
  width: 38px;
  height: 38px;
  border-radius: 9px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  transition: border-color var(--duration-micro), background var(--duration-micro);
}
.hud-icon-btn:hover .icon-glyph,
.hud-icon-btn:focus-visible .icon-glyph {
  border-color: var(--border-strong);
  background: var(--bg-field);
}
.icon-label {
  font-size: 11px;
  color: var(--text-tertiary);
  font-family: var(--font-sans);
}
.icon-badge {
  position: absolute;
  top: -4px;
  right: 2px;
  min-width: 14px;
  height: 14px;
  border-radius: 7px;
  background: var(--accent);
  color: #fff;
  font-size: 9px;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 3px;
  box-shadow: 0 0 0 2px var(--bg-base);
}
.icon-badge--alert {
  background: var(--accent);
  animation: badge-pulse 2s ease-in-out infinite;
}
@keyframes badge-pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

/* ── Responsive ─────────────────────────────────────────────── */
@media (max-width: 900px) {
  .hud-canvas-topbar {
    padding: 18px 16px 0;
  }
  .topbar-right {
    gap: 14px;
  }
  .icon-label {
    display: none;
  }
  .brand-block {
    width: 140px;
  }
  .brand-sub {
    display: none;
  }
}
@media (max-width: 600px) {
  .hud-canvas-topbar {
    padding: 12px 10px 0;
  }
  .topbar-right {
    gap: 8px;
  }
  .icon-glyph {
    width: 32px;
    height: 32px;
    border-radius: 7px;
  }
  .exit-btn {
    width: 36px;
    height: 36px;
    border-radius: 8px;
  }
  .brand-name {
    font-size: 13px;
  }
  .brand-block {
    width: 110px;
  }
  .brand-bar { display: none; }
}
@media (prefers-reduced-motion: reduce) {
  .brand-bar i { transition: none; }
  .icon-badge--alert { animation: none; }
}
</style>
