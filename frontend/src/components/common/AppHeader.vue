<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { Icon } from '@iconify/vue'
import { useSidebarStore } from '@/stores/sidebar'
import { useUiModeStore } from '@/stores/uiMode'
import { useTenantPreviewStore } from '@/stores/tenantPreview'

const { t } = useI18n()
const route = useRoute()
const sidebar = useSidebarStore()
const uiMode = useUiModeStore()
const tenantPreview = useTenantPreviewStore()

// 页面标题映射
const pageTitles: Record<string, string> = {
  '/app/overview': '仪表盘',
  '/app/pricing': '报价计算',
  '/app/orders': '订单工作台',
  '/app/devices': '设备台账',
  '/app/audit': '审计日志',
  '/app/settings/dynamic-pricing': '动态定价',
  '/app/settings/staff': '租户成员',
  '/app/settings/device-models': '设备型号',
  '/app/settings/warehouses': '仓库管理',
  '/app/design-system': '设计系统',
}

const pageTitle = computed(() => {
  return (route.meta.title as string) || pageTitles[route.path] || '系统'
})
</script>

<template>
  <header class="topbar">
    <!-- 左侧：汉堡菜单 + 页面标题 -->
    <div class="topbar-left">
      <button class="topbar-burger" @click="sidebar.toggleMobile()">
        <Icon icon="material-symbols:menu" width="18" height="18" />
      </button>
      <h1 class="page-title">{{ pageTitle }}</h1>
    </div>

    <!-- 中间：搜索框 -->
    <div v-if="!tenantPreview.active" class="topbar-center">
      <div class="search-box">
        <Icon icon="material-symbols:search" width="16" height="16" />
        <input type="text" :placeholder="t('search.placeholder', '搜索订单 / 设备 / 客户')" />
      </div>
    </div>

    <!-- 右侧：功能插槽 -->
    <div class="topbar-right">
      <slot name="actions" />
      <button
        v-if="uiMode.canSwitch && !tenantPreview.active"
        class="mode-switch-btn"
        @click="uiMode.setMode(uiMode.mode === 'work' ? 'hud' : 'work')"
        :title="uiMode.mode === 'work' ? '切至 HUD' : '切至 Work'"
      >{{ uiMode.mode === 'work' ? '⊞ HUD' : '⛶ WORK' }}</button>
    </div>
  </header>
</template>

<style scoped>
/* ══════════════════════════════════════════════════════════════
   Topbar — 玻璃透明高斯模糊
   ══════════════════════════════════════════════════════════════ */
.topbar {
  display: grid;
  grid-template-columns: minmax(auto, 1fr) minmax(auto, 500px) minmax(auto, 1fr);
  align-items: center;
  gap: 20px;
  padding: 22px 36px;
  background: var(--glass-bg);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border-bottom: var(--glass-border);
  flex-shrink: 0;
  position: relative;
}

.topbar::before {
  content: '';
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--glass-highlight);
  z-index: 1;
}

.topbar-left {
  display: flex;
  align-items: center;
  gap: 20px;
}

.topbar-center {
  display: flex;
  justify-content: center;
}

.topbar-right {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* 修复：图标颜色继承 */
.topbar :deep(.iconify) {
  color: currentColor;
}

.topbar-burger {
  display: none;
  width: 34px;
  height: 34px;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  flex-shrink: 0;
  transition: background var(--duration-micro), color var(--duration-micro);
}

.topbar-burger:hover {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.page-title {
  font-family: var(--font-mono);
  font-size: 20px;
  font-weight: 700;
  color: var(--text-primary);
  margin: 0;
  white-space: nowrap;
}

.search-box {
  width: 100%;
  max-width: 500px;
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--glass-bg-elevated);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border: var(--glass-border);
  border-radius: 8px;
  padding: 8px 12px;
  transition: border-color var(--duration-micro);
}

.search-box:hover,
.search-box:focus-within {
  border-color: var(--border-strong);
}

.search-box input {
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  color: var(--text-primary);
  font-size: 13px;
  font-family: var(--font-sans);
}

.search-box input::placeholder {
  color: var(--text-tertiary);
}

/* Responsive */
@media (max-width: 768px) {
  .topbar {
    grid-template-columns: 1fr;
    padding: 16px;
    gap: 12px;
  }

  .topbar-burger {
    display: flex;
  }

  .page-title {
    font-size: 16px;
  }

  .topbar-center,
  .topbar-right {
    display: none;
  }
}
</style>

<style>
/* ══════════════════════════════════════════════════════════════
   Header Action Buttons — global so injected page components
   rendered inside the actions slot can use these classes.
   ══════════════════════════════════════════════════════════════ */
.topbar-actions {
  display: flex;
  gap: 10px;
  align-items: center;
}

.header-action-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-weight: 500;
  font-size: 13px;
  height: 36px;
  padding: 0 18px;
  border-radius: 9px;
  background: transparent;
  color: var(--text-secondary);
  border: 1px solid var(--border-strong);
  cursor: pointer;
  text-decoration: none;
  font-family: var(--font-sans);
  transition: opacity 140ms ease, border-color 140ms ease, color 140ms ease;
}

.header-action-btn:hover {
  color: var(--text-primary);
  border-color: var(--text-secondary);
}

.header-action-btn--primary {
  background: var(--accent);
  color: #fff;
  border: none;
}

.header-action-btn--primary:hover {
  opacity: 0.88;
}

.header-action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.mode-switch-btn {
  font-family: 'Space Mono', monospace; font-size: 11px; padding: 4px 10px;
  border: 1px solid var(--border-base); border-radius: 8px;
  background: var(--bg-elevated); color: var(--text-primary); cursor: pointer;
  white-space: nowrap; transition: border-color 0ms;
}
.mode-switch-btn:hover { border-color: var(--accent); }
</style>
