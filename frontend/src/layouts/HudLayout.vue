<script setup lang="ts">
import { computed } from 'vue'
import { RouterLink, useRoute } from 'vue-router'
import { Icon } from '@iconify/vue'
import { useUiModeStore } from '@/stores/uiMode'
import { useAuthStore } from '@/stores/auth'

defineProps<{ compact?: boolean }>()
const uiMode = useUiModeStore()
const auth = useAuthStore()
const route = useRoute()

const actions = [
  { label: '总览', code: 'FIELD', path: '/', icon: 'material-symbols:grid-view-outline-rounded' },
  { label: '扫码', code: 'SCAN', path: '/checkin', icon: 'material-symbols:qr-code-scanner-rounded' },
  { label: '订单', code: 'ORDER', path: '/customers', icon: 'material-symbols:order-approve-outline-rounded' },
  { label: '设备', code: 'ASSET', path: '/device-list', icon: 'material-symbols:camera-outline-rounded' },
]
const topCommands = [
  { label: '扫码', path: '/checkin', icon: 'material-symbols:qr-code-scanner-rounded' },
  { label: '订单', path: '/customers', icon: 'material-symbols:order-approve-outline-rounded' },
  { label: '设备', path: '/device-list', icon: 'material-symbols:camera-outline-rounded' },
  { label: '日志', path: '/audit', icon: 'material-symbols:assignment-outline-rounded' },
]

const operator = computed(() => auth.user?.username || 'OPERATOR')
</script>

<template>
  <div class="hud-layout" :class="{ 'hud-compact': compact }">
    <header class="hud-top-rail">
      <div class="hud-brand-lockup">
        <button v-if="uiMode.canSwitch" class="hud-exit-button" type="button" aria-label="返回 Work 模式" @click="uiMode.setMode('work')">
          <Icon icon="material-symbols:arrow-back-rounded" width="28" height="28" />
          <small>返回</small>
        </button>
        <span class="hud-brand-mark" aria-hidden="true"></span>
        <div><strong>TALOS</strong><small>OPERATIONS / HUD</small></div>
      </div>

      <div class="hud-system-state"><i></i><span>AUTO / {{ uiMode.mode === 'hud-compact' ? 'COMPACT' : 'FIELD ONLINE' }}</span></div>

      <nav class="hud-rail-actions" aria-label="HUD 系统入口">
        <RouterLink v-for="command in topCommands" :key="command.path" :to="command.path" class="hud-top-command" :aria-label="command.label">
          <Icon :icon="command.icon" width="21" height="21" />
          <span>{{ command.label }}</span>
        </RouterLink>
        <span class="hud-badge" aria-label="任务流在线"><i></i></span>
        <span class="hud-profile">{{ operator }}</span>
      </nav>
    </header>

    <main class="hud-scene"><slot /></main>

    <footer v-if="!compact" class="hud-action-dock" aria-label="HUD 快速操作">
      <RouterLink v-for="action in actions" :key="action.path" :to="action.path" class="hud-dock-btn" :class="{ active: route.path === action.path }">
        <Icon :icon="action.icon" width="19" height="19" />
        <span><b>{{ action.label }}</b><small>{{ action.code }}</small></span>
      </RouterLink>
      <slot name="dock" />
      <div class="dock-progress" aria-label="自动化进程在线"><span>AUTO</span><small>LIVE<br>ONLINE</small></div>
    </footer>

    <footer v-else class="hud-action-dock" aria-label="HUD Compact 快速操作">
      <RouterLink v-for="action in actions.slice(0, 3)" :key="action.path" :to="action.path" class="hud-dock-btn" :class="{ active: route.path === action.path }">
        <Icon :icon="action.icon" width="19" height="19" />
        <span><b>{{ action.label }}</b><small>{{ action.code }}</small></span>
      </RouterLink>
      <slot name="dock-compact" />
    </footer>
  </div>
</template>

<style scoped>
.hud-layout { position: relative; display: flex; flex-direction: column; height: 100vh; height: 100dvh; overflow: hidden; background: var(--bg-base); }
.hud-top-rail { display: flex; align-items: center; justify-content: space-between; min-height: 72px; padding: 8px 18px; background: var(--bg-base); border-bottom: 1px solid var(--border-base); z-index: 30; flex-shrink: 0; }
.hud-brand-lockup { display: flex; align-items: center; gap: 10px; }
.hud-exit-button { display: flex; flex-direction: column; align-items: center; justify-content: center; width: 48px; min-height: 48px; padding: 3px; border: 1px solid var(--border-base); border-radius: 8px; background: var(--bg-elevated); color: var(--text-primary); cursor: pointer; }
.hud-exit-button small { color: var(--text-secondary); font: 11px/1 var(--font-sans); }
.hud-exit-button:hover, .hud-exit-button:focus-visible { border-color: var(--accent); color: var(--accent); }
.hud-brand-mark { width: 22px; height: 22px; border: 6px solid var(--text-primary); border-top-color: transparent; border-radius: 8px; transform: rotate(45deg); }
.hud-brand-lockup div { display: flex; flex-direction: column; }
.hud-brand-lockup strong { color: var(--text-primary); font: 700 14px/1 var(--font-mono); letter-spacing: .08em; }
.hud-brand-lockup div small { margin-top: 5px; color: var(--text-tertiary); font: 11px/1 var(--font-mono); letter-spacing: .08em; }
.hud-system-state { position: absolute; left: 50%; display: flex; align-items: center; gap: 8px; transform: translateX(-50%); color: var(--text-secondary); font: 700 11px/1 var(--font-mono); letter-spacing: .08em; }
.hud-system-state i { width: 8px; height: 8px; border-radius: 4px; background: var(--status-success); box-shadow: 0 0 0 3px color-mix(in srgb, var(--status-success) 18%, transparent); }
.hud-rail-actions { display: flex; align-items: center; gap: 8px; }
.hud-top-command { display: flex; flex-direction: column; align-items: center; gap: 3px; min-width: 46px; min-height: 46px; padding: 4px; border: 1px solid transparent; border-radius: 8px; color: var(--text-secondary); text-decoration: none; }
.hud-top-command span { font: 11px/1 var(--font-sans); }
.hud-top-command:hover, .hud-top-command:focus-visible, .hud-top-command.router-link-active { border-color: var(--accent); color: var(--text-primary); background: var(--bg-elevated); }
.hud-badge { display: inline-flex; align-items: center; padding: 8px; color: var(--accent); }
.hud-badge i { width: 8px; height: 8px; border: 2px solid var(--accent); border-radius: 4px; }
.hud-profile { max-width: 100px; overflow: hidden; color: var(--text-tertiary); font: 700 11px/1 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
.hud-scene { flex: 1; overflow: hidden; position: relative; }
.hud-action-dock { position: absolute; top: 96px; left: 18px; z-index: 20; display: flex; flex-direction: column; align-items: stretch; gap: 8px; width: 142px; padding: 0; border: 0; background: transparent; }
.hud-dock-btn { display: flex; align-items: center; gap: 10px; min-width: 0; min-height: 62px; padding: 8px 12px; border: 2px solid var(--text-primary); border-radius: 0 12px 0 12px; background: var(--text-primary); color: var(--bg-base); text-decoration: none; box-shadow: 4px 4px 0 var(--bg-base); transition: background var(--duration-micro), border-color var(--duration-micro), color var(--duration-micro); }
.hud-dock-btn span { display: flex; flex-direction: column; gap: 4px; }
.hud-dock-btn b { color: inherit; font: 700 12px/1 var(--font-mono); }
.hud-dock-btn small { color: color-mix(in srgb, var(--bg-base) 58%, transparent); font: 11px/1 var(--font-mono); letter-spacing: .08em; }
.hud-dock-btn:hover, .hud-dock-btn:focus-visible { background: var(--bg-base); color: var(--text-primary); }
.hud-dock-btn:hover small, .hud-dock-btn:focus-visible small { color: var(--text-tertiary); }
.hud-dock-btn.active { border-color: var(--accent); background: var(--bg-base); color: var(--accent); transform: translateX(7px); }
.hud-dock-btn.active small { color: var(--text-secondary); }
.dock-progress { display: flex; align-items: center; gap: 8px; margin-top: 4px; padding: 10px 12px; border: 1px solid var(--border-strong); background: var(--bg-base); color: var(--text-primary); }
.dock-progress span { color: var(--status-success); font: 700 18px/1 var(--font-mono); }
.dock-progress small { color: var(--text-tertiary); font: 11px/1.15 var(--font-mono); }
.hud-compact .hud-top-rail { padding: 8px 12px; }
@media (max-width: 900px) {
  .hud-system-state, .hud-badge, .hud-top-command span { display: none; }
  .hud-top-command { min-width: 40px; }
  .hud-action-dock { width: 122px; }
  .dock-progress { display: none; }
}
@media (max-width: 600px) {
  .hud-brand-lockup div small, .hud-profile { display: none; }
  .hud-top-command { min-width: 36px; padding-inline: 2px; }
  .hud-action-dock { position: static; flex-direction: row; justify-content: space-around; width: auto; padding: 6px; border-top: 1px solid var(--border-base); background: var(--bg-base); }
  .hud-dock-btn { justify-content: center; min-width: 0; flex: 1; padding-inline: 5px; }
  .hud-dock-btn small { display: none; }
}
@media (prefers-reduced-motion: reduce) { .hud-dock-btn { transition: none; } }
</style>
