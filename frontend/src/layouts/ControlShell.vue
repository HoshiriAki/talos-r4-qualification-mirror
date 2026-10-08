<script setup lang="ts">
import { computed, inject } from 'vue'
import AppSidebar from '@/components/common/AppSidebar.vue'
import AppHeader from '@/components/common/AppHeader.vue'
import { useSidebarStore } from '@/stores/sidebar'
import { HEADER_ACTIONS_KEY } from '@/constants/keys'

const sidebar = useSidebarStore()
const headerActionsRef = inject(HEADER_ACTIONS_KEY, null)
const HeaderActions = computed(() => headerActionsRef?.value ?? null)
</script>

<template>
  <div class="control-shell">
    <AppSidebar />
    <div class="control-shell__main">
      <AppHeader class="control-shell__header">
        <template #actions><component v-if="HeaderActions" :is="HeaderActions" /></template>
      </AppHeader>
      <div class="plane-marker"><span>TALOS CONTROL</span><span>PLATFORM AUTHORITY</span></div>
      <main class="control-shell__content"><slot /></main>
    </div>
    <div v-if="sidebar.mobileOpen" class="sidebar-overlay" @click="sidebar.closeMobile()" />
  </div>
</template>

<style scoped>
.control-shell { display: grid; grid-template-columns: 220px 1fr; height: 100vh; overflow: hidden; }
.control-shell__main { display: flex; flex-direction: column; min-width: 0; overflow-y: auto; }
.control-shell__header { position: sticky; top: 0; z-index: 10; flex-shrink: 0; }
.plane-marker { display: flex; justify-content: space-between; padding: 7px 32px; border-bottom: 1px solid var(--border-default); background: var(--bg-surface); color: var(--text-tertiary); font-family: var(--font-mono); font-size: var(--font-size-xs); letter-spacing: .08em; }
.plane-marker span:first-child { color: var(--accent); }
.control-shell__content { flex: 1; padding: 24px 32px; overflow-y: auto; }
.sidebar-overlay { position: fixed; inset: 0; z-index: 20; background: rgb(0 0 0 / 50%); }
@media (max-width: 767px) { .control-shell { grid-template-columns: 1fr; } .control-shell__content { padding: 16px; } .plane-marker { padding-inline: 16px; } }
</style>
