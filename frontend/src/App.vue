<script setup lang="ts">
import TenantShell from '@/layouts/TenantShell.vue'
import ControlShell from '@/layouts/ControlShell.vue'
import EmbeddedPreviewShell from '@/layouts/EmbeddedPreviewShell.vue'
import ErrorBoundary from '@/components/common/ErrorBoundary.vue'
import BackgroundGlow from '@/components/common/BackgroundGlow.vue'
import NotificationCenter from '@/components/notify/NotificationCenter.vue'
import { HEADER_ACTIONS_KEY } from '@/constants/keys'
import { computed, onMounted, provide, ref, shallowRef } from 'vue'
import type { Component } from 'vue'
import { settingsState, applyGlassOpacity, applyScanlines, applyDotGrid, applyBorderRadius } from '@/utils/settings'
import { useRoute } from 'vue-router'
import Toast from 'primevue/toast'
import ConfirmDialog from 'primevue/confirmdialog'
import { useToast } from 'primevue/usetoast'
import ProfileDialog from '@/components/profile/ProfileDialog.vue'

const route = useRoute()
const toast = useToast()

// ── Profile / Settings dialog ──
const showProfileDialog = ref(false)
const profileTab = ref('profile')
provide('openProfile', () => { profileTab.value = 'profile'; showProfileDialog.value = true })
provide('openSettings', () => { profileTab.value = 'preferences'; showProfileDialog.value = true })

// ── Notifications ──
const notificationCenterRef = ref<InstanceType<typeof NotificationCenter> | null>(null)
provide('toggleNotifications', (event: Event) => {
  notificationCenterRef.value?.toggleOverlay(event)
})

// ── Help ──
provide('openHelp', () => {
  window.open('https://github.com/HoshiriAki/TESSERACT-WAREHOUSiNG/wiki', '_blank')
})

// Header actions — ancestor-owned ref: page writes, DefaultLayout reads
const headerActions = shallowRef<Component | null>(null)
provide(HEADER_ACTIONS_KEY, headerActions)

const layout = computed(() => {
  if (route.meta.layout === 'blank') return undefined
  if (route.meta.shell === 'control') return ControlShell
  if (route.meta.shell === 'embedded') return EmbeddedPreviewShell
  return TenantShell
})

onMounted(() => {
  // Apply persisted settings on boot
  applyGlassOpacity(settingsState.glassOpacity)
  applyScanlines(settingsState.showScanlines)
  applyDotGrid(settingsState.showDotGrid)
  applyBorderRadius(settingsState.borderRadius)

  const msg = sessionStorage.getItem('routeGuardMessage')
  if (msg) {
    sessionStorage.removeItem('routeGuardMessage')
    toast.add({ severity: 'warn', summary: '无访问权限', detail: msg, life: 3000 })
  }
})
</script>

<template>
  <BackgroundGlow />
  <ErrorBoundary>
    <template v-if="layout">
      <component :is="layout">
        <router-view v-slot="{ Component, route: r }">
          <Transition name="page-slide" mode="out-in">
            <div :key="r.fullPath" v-if="Component">
              <component :is="Component" />
            </div>
          </Transition>
        </router-view>
      </component>
    </template>
    <router-view v-else />
  </ErrorBoundary>
  <Toast position="top-right" />
  <ConfirmDialog />
  <ProfileDialog v-model:visible="showProfileDialog" :initial-tab="profileTab" />
  <NotificationCenter ref="notificationCenterRef" style="position:fixed;left:-9999px;top:0" />
</template>
