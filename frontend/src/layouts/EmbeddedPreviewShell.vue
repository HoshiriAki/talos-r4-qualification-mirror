<script setup lang="ts">
import TenantPreviewBanner from '@/components/preview/TenantPreviewBanner.vue'
</script>

<template>
  <div class="embedded-preview-shell">
    <TenantPreviewBanner />
    <main><slot /></main>
  </div>
</template>

<style scoped>
.embedded-preview-shell { min-height: 100vh; background: var(--bg-base); }
.embedded-preview-shell main { padding: 20px 24px; }
</style>
