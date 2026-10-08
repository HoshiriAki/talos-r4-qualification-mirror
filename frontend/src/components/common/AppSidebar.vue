<script setup lang="ts">
import { computed, inject } from 'vue'
import { useRoute } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useSidebarStore } from '@/stores/sidebar'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'
import { useTenantPreviewStore } from '@/stores/tenantPreview'
import { setLocale, type Locale } from '@/i18n'
import defaultAvatar from '@/assets/images/default-avatar.svg'
import { settingsState } from '@/utils/settings'
import {
  CONTROL_SIDEBAR_GROUPS,
  TENANT_SIDEBAR_GROUPS,
  type SidebarGroup,
  type SidebarGroupItem,
} from '@/constants/sidebarGroups'
import { Icon } from '@iconify/vue'

const { t, locale } = useI18n()
const route = useRoute()
const sidebar = useSidebarStore()
const auth = useAuthStore()
const theme = useThemeStore()
const preview = useTenantPreviewStore()

// ── Inject App.vue callbacks ──
const openProfile = inject<(event?: Event) => void>('openProfile', () => {})
const openSettings = inject<() => void>('openSettings', () => {})
const openHelp = inject<() => void>('openHelp', () => {})
const toggleNotifications = inject<(event: Event) => void>('toggleNotifications', () => {})

function toggleLocale() {
  const next: Locale = locale.value === 'zh-CN' ? 'en' : 'zh-CN'
  locale.value = next
  setLocale(next)
}

function sortItems(items: SidebarGroupItem[]): SidebarGroupItem[] {
  const saved = settingsState.sidebarOrder
  if (saved.length === 0) return items
  const orderMap = new Map(saved.map((p, i) => [p, i]))
  const known = items.filter(it => orderMap.has(it.path))
  const unknown = items.filter(it => !orderMap.has(it.path))
  known.sort((a, b) => (orderMap.get(a.path) ?? 999) - (orderMap.get(b.path) ?? 999))
  return [...known, ...unknown]
}

type VisibleSidebarGroup = SidebarGroup & { visibleItems: SidebarGroupItem[] }

const visibleGroups = computed<VisibleSidebarGroup[]>(() => {
  if (preview.active) {
    return [{
      id: 'tenant-preview',
      labelKey: 'nav.tenantPreview',
      namespace: 'system/tenant-preview',
      items: [],
      visibleItems: [
        { path: preview.routeFor('dashboard'), labelKey: 'nav.status' },
        { path: preview.routeFor('orders'), labelKey: 'nav.processCtrl' },
        { path: preview.routeFor('devices'), labelKey: 'nav.deviceReg' },
        { path: preview.routeFor('audit'), labelKey: 'nav.eventLog' },
      ],
    }]
  }
  const groups = route.path.startsWith('/control') ? CONTROL_SIDEBAR_GROUPS : TENANT_SIDEBAR_GROUPS
  return groups.map(g => ({
    ...g,
    visibleItems: sortItems(g.items.filter(it =>
      (!it.adminOnly || auth.isTenantAdmin)
      && (!it.capability || auth.hasCapability(it.capability))
    ))
  })).filter(g => g.visibleItems.length > 0)
})

function isActive(path: string): boolean {
  return route.path === path
}

// 获取路由图标
function getRouteIcon(path: string): string {
  if (path.includes('/embedded/preview/')) {
    if (path.endsWith('/dashboard')) return 'material-symbols:dashboard'
    if (path.endsWith('/orders')) return 'material-symbols:group'
    if (path.endsWith('/devices')) return 'material-symbols:inventory-2'
    if (path.endsWith('/audit')) return 'material-symbols:description'
  }
  const iconMap: Record<string, string> = {
    '/app/overview': 'material-symbols:dashboard',
    '/app/audit': 'material-symbols:description',
    '/app/design-system': 'material-symbols:palette',
    '/app/pricing': 'material-symbols:calculate',
    '/app/orders': 'material-symbols:group',
    '/app/shipping': 'material-symbols:local-shipping',
    '/app/checkin': 'material-symbols:qr-code-scanner',
    '/app/booking': 'material-symbols:calendar-today',
    '/app/contracts': 'material-symbols:contract',
    '/app/optical-sop': 'material-symbols:science',
    '/app/devices': 'material-symbols:inventory-2',
    '/app/barcode': 'material-symbols:barcode',
    '/app/settings/warehouses': 'material-symbols:warehouse',
    '/app/settings/device-models': 'material-symbols:category',
    '/app/credit': 'material-symbols:credit-card',
    '/app/overdue': 'material-symbols:error',
    '/app/settings/finance': 'material-symbols:payments',
    '/app/settings/dynamic-pricing': 'material-symbols:trending-up',
    '/app/settings/staff': 'material-symbols:admin-panel-settings',
    '/app/settings/integrations': 'material-symbols:hub',
    '/control/overview': 'material-symbols:space-dashboard',
    '/control/operations': 'material-symbols:monitor-heart',
    '/control/business': 'material-symbols:analytics',
    '/control/tenants': 'material-symbols:domain',
    '/control/governance': 'material-symbols:domain-verification',
    '/control/simulation': 'material-symbols:science',
    '/control/support': 'material-symbols:support-agent',
    '/control/audit': 'material-symbols:policy',
    '/control/platform-members': 'material-symbols:manage-accounts',
    '/control/settings': 'material-symbols:settings',
  }
  return iconMap[path] || 'material-symbols:circle'
}

// 获取通知徽章数量（示例）
function getBadgeCount(path: string): number | null {
  if (path === '/app/overdue') return 3 // 示例：3 个逾期订单
  return null
}

// 生成会话 ID
const sessionId = computed(() => {
  const timestamp = Date.now().toString(36)
  const random = Math.random().toString(36).substring(2, 6)
  return `${timestamp}-${random}`.toUpperCase()
})
</script>

<template>
  <aside class="sidebar">
    <!-- Brand -->
    <div class="brand">
      <div class="brand-icon">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
          <path d="M12 2L2 7v10c0 5.55 3.84 10.74 9 12 5.16-1.26 9-6.45 9-12V7l-10-5z"/>
        </svg>
      </div>
      <div class="brand-name">
        TAL<span class="brand-dot">·</span>OS
        <small>v1.2</small>
      </div>
    </div>

    <!-- Navigation -->
    <nav class="nav">
      <div
        v-for="group in visibleGroups"
        :key="group.id"
      >
        <!-- Group Label -->
        <div class="nav-group">{{ t(group.labelKey) }}</div>

        <!-- Nav Items -->
        <router-link
          v-for="item in group.visibleItems"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          :class="{ active: isActive(item.path) }"
          @click="sidebar.closeMobile()"
        >
          <Icon :icon="getRouteIcon(item.path)" width="16" height="16" class="n-icon" />
          <span>{{ t(item.labelKey) }}</span>
          <span v-if="getBadgeCount(item.path)" class="nav-badge">
            {{ getBadgeCount(item.path) }}
          </span>
        </router-link>
      </div>
    </nav>

    <!-- Separator -->
    <hr class="sidebar-sep" />

    <!-- User Card -->
    <div v-if="auth.currentUser" class="user-card" :class="{ 'user-card--locked': preview.active }" @click="!preview.active && openProfile($event)">
      <div class="user-avatar">
        <img class="user-avatar-img" :src="defaultAvatar" alt="" />
      </div>
      <div class="user-info">
        <div class="user-name">{{ auth.currentUser?.displayName || auth.currentUser?.username }}</div>
        <div class="user-email">{{ auth.currentUser?.email || auth.currentUser?.username }}</div>
      </div>
    </div>

    <!-- Footer Actions -->
    <div v-if="!preview.active" class="sidebar-actions">
      <!-- 通知 -->
      <button class="action-btn" @click="toggleNotifications($event)" title="通知">
        <Icon icon="material-symbols:notifications" width="18" height="18" />
      </button>

      <!-- 主题切换 -->
      <button class="action-btn theme-toggle-btn" @click="theme.toggle()" title="切换主题">
        <Icon
          v-if="theme.theme === 'dark'"
          icon="line-md:sunny-outline-to-moon-alt-loop-transition"
          width="20"
          height="20"
          key="dark-to-light"
        />
        <Icon
          v-else
          icon="line-md:moon-alt-to-sunny-outline-loop-transition"
          width="20"
          height="20"
          key="light-to-dark"
        />
      </button>

      <!-- 语言切换 -->
      <button class="action-btn" @click="toggleLocale" :title="t('lang.switch')">
        {{ t('lang.label') }}
      </button>

      <!-- 设置 -->
      <button class="action-btn" @click="openSettings()" title="设置">
        <Icon icon="material-symbols:settings" width="18" height="18" />
      </button>

      <!-- 帮助 -->
      <button class="action-btn" @click="openHelp()" title="帮助">
        <Icon icon="material-symbols:help" width="18" height="18" />
      </button>

      <!-- 登出 -->
      <button class="action-btn" @click="auth.logout()" title="登出">
        <Icon icon="material-symbols:logout" width="18" height="18" />
      </button>
    </div>

    <!-- Footer Status -->
    <div class="sidebar-foot">
      <span class="status-dot" :class="{ 'status-dot--preview': preview.active }"></span>
      <span>{{ preview.active ? '只读预览' : '系统正常' }}</span>
    </div>

    <!-- System Info -->
    <div class="system-info">
      <div class="info-line">版本 v1.2.0</div>
      <div class="info-line">会话 #{{ sessionId }}</div>
      <div class="info-line">京ICP备2024000000号</div>
    </div>
  </aside>
</template>

<style scoped>
/* ============ Sidebar (参考 v6 standerd_mode_computer.html) ============ */
.sidebar {
  background: var(--glass-bg);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border-right: var(--glass-border);
  position: relative;
  padding: 26px 18px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  height: 100%;
  overflow-y: auto;
}

.sidebar::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 1px;
  background: var(--glass-highlight);
  z-index: 1;
}

/* Brand */
.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 6px;
  margin-bottom: 24px;
}

.brand-icon {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  color: var(--text-primary);
}

.brand-name {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 15px;
  color: var(--text-primary);
}

.brand-dot {
  color: var(--accent);
}

.brand-name small {
  color: var(--text-tertiary);
  font-weight: 400;
  margin-left: 4px;
}

/* Navigation */
.nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  flex: 1;
}

.nav-group {
  font-size: 10px;
  letter-spacing: 0.12em;
  color: var(--text-tertiary);
  text-transform: uppercase;
  padding: 14px 10px 6px;
  font-family: var(--font-mono);
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 10px;
  border-radius: 8px;
  font-size: 13px;
  color: var(--text-secondary);
  position: relative;
  transition: background var(--duration-micro), color var(--duration-micro);
  text-decoration: none;
}

.nav-item:hover {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

.nav-item.active {
  background: var(--bg-elevated);
  color: var(--text-primary);
  box-shadow: inset 2px 0 0 var(--text-primary);
}

.n-icon {
  width: 16px;
  text-align: center;
  font-size: 13px;
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.nav-item.active .n-icon {
  color: var(--text-primary);
}

.nav-badge {
  margin-left: auto;
  min-width: 16px;
  height: 16px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--accent);
  color: #fff;
  font-size: 10px;
  font-family: var(--font-mono);
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
}

/* Separator */
.sidebar-sep {
  width: calc(100% - 12px);
  margin: -4px 6px;
  border: none;
  border-top: 1px solid var(--border-base);
}

/* User Card */
.user-card {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 6px 8px 20px;
  margin-top: auto;
  margin-bottom: -4px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background var(--duration-micro);
}

.user-card:hover {
  background: var(--bg-elevated);
}
.user-card--locked { cursor: default; }
.user-card--locked:hover { background: transparent; }

.user-avatar {
  width: 32px;
  height: 32px;
  border-radius: var(--radius-sm);
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  overflow: hidden;
}

.user-avatar-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}

.user-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
  flex: 1;
  min-width: 0;
}

.user-name {
  font-family: var(--font-sans);
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.user-email {
  font-family: var(--font-mono);
  font-size: 11px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Footer Actions - 2x3 Grid */
.sidebar-actions {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
  padding: 0 6px;
}

.action-btn {
  width: 100%;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  border-radius: 8px;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: all var(--duration-micro);
  font-family: var(--font-sans);
  font-size: 13px;
}

.action-btn:hover {
  background: var(--bg-elevated);
  color: var(--text-primary);
}

/* Footer Status */
.sidebar-foot {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
  color: var(--text-tertiary);
  padding: 2px 10px 0;
  font-family: var(--font-mono);
}

.status-dot {
  width: 6px;
  height: 6px;
  border-radius: 2px;
  background: var(--status-success);
}
.status-dot--preview { background: var(--status-info); }

/* System Info */
.system-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 10px 4px;
  font-family: var(--font-mono);
  font-size: 10px;
  color: var(--text-tertiary);
  opacity: 0.4;
  transition: opacity var(--duration-micro);
}

.system-info:hover {
  opacity: 1;
}

.info-line {
  line-height: 1.4;
  letter-spacing: 0.02em;
  transition: color var(--duration-micro);
}

.info-line:hover {
  color: var(--text-secondary);
}

/* Responsive */
@media (max-width: 768px) {
  .sidebar {
    position: fixed;
    left: 0;
    top: 0;
    bottom: 0;
    z-index: 1000;
    transform: translateX(-100%);
    transition: transform var(--duration-normal);
  }

  .sidebar.mobile-open {
    transform: translateX(0);
  }
}
</style>
