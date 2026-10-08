<script setup lang="ts">
import { inject, computed } from 'vue'
import AppSidebar from '@/components/common/AppSidebar.vue'
import AppHeader from '@/components/common/AppHeader.vue'
import TenantPreviewBanner from '@/components/preview/TenantPreviewBanner.vue'
import { useSidebarStore } from '@/stores/sidebar'
import { HEADER_ACTIONS_KEY } from '@/constants/keys'

const sidebar = useSidebarStore()
const headerActionsRef = inject(HEADER_ACTIONS_KEY, null)
const HeaderActions = computed(() => headerActionsRef?.value ?? null)
</script>

<template>
  <div class="work-layout">
    <AppSidebar />
    <div class="work-layout-main">
      <AppHeader class="work-layout-header">
        <template #actions>
          <component v-if="HeaderActions" :is="HeaderActions" />
          <slot name="header-actions" />
        </template>
      </AppHeader>
      <TenantPreviewBanner />
      <main class="work-layout-content">
        <slot />
      </main>
    </div>
    <div
      v-if="sidebar.mobileOpen"
      class="sidebar-overlay"
      @click="sidebar.closeMobile()"
    />
  </div>
</template>

<style scoped>
.work-layout {
  display: grid;
  grid-template-columns: 220px 1fr;
  height: 100vh;
  overflow: hidden;
}
.work-layout-main {
  display: flex;
  flex-direction: column;
  overflow-y: auto;
}
.work-layout-header {
  position: sticky;
  top: 0;
  z-index: 10;
  flex-shrink: 0;
}
.work-layout-content {
  flex: 1;
  padding: 24px 32px;
  overflow-y: auto;
}
.sidebar-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,0.5);
  z-index: 20;
}
@media (max-width: 767px) {
  .work-layout { grid-template-columns: 1fr; }
  .work-layout-content { padding: 16px; }
}
</style>
