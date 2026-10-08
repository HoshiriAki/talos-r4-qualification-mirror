<script setup lang="ts">
import type { DashboardStats } from '@/api/dashboard'
import HudCanvasSurface from '@/components/hud/HudCanvasSurface.vue'

defineProps<{ model: DashboardStats | null; loading: boolean; compact?: boolean }>()
</script>

<template>
  <HudCanvasSurface :model="model" :loading="loading" :compact="compact" />
</template>
