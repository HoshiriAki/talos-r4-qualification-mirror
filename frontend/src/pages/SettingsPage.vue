<script setup lang="ts">
import { onMounted } from 'vue'
import { useRouter } from 'vue-router'

const router = useRouter()
onMounted(() => router.replace('/app/overview'))
</script>

<template>
  <div class="space-y-4">
    <!-- MODULE-20 bar -->
    <div class="module-bar">
      <span class="module-number-label">MODULE-20</span>
      <div class="structure-line" />
      <span class="module-page-label">设置</span>
    </div>

    <div class="flex items-center justify-center h-64">
      <p class="font-mono text-sm text-text-muted">设置已移至个人中心，正在跳转...</p>
    </div>

    <div class="version-footer">2026 — V1.05 — REV.N</div>
  </div>
</template>
