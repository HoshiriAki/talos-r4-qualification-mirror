<script setup lang="ts">
import { computed } from 'vue'
import { useUiModeStore } from '@/stores/uiMode'
import WorkLayout from '@/layouts/WorkLayout.vue'

const uiMode = useUiModeStore()

const isHud = computed(() => uiMode.mode === 'hud' || uiMode.mode === 'hud-compact')
</script>

<template>
  <!-- Work mode: standard sidebar + header -->
  <WorkLayout v-if="!isHud">
    <slot />
  </WorkLayout>

  <!-- HUD / HUD-Compact: canvas provides its own chrome — bare container -->
  <div v-else class="hud-bare-viewport">
    <slot />
  </div>
</template>

<style scoped>
.hud-bare-viewport {
  width: 100vw;
  height: 100vh;
  height: 100dvh;
  overflow: hidden;
  background: var(--bg-base);
}
</style>
