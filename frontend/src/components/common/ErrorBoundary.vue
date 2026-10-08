<script setup lang="ts">
import { ref, onErrorCaptured } from 'vue'

const caught = ref<{ message: string; stack?: string } | null>(null)

onErrorCaptured((err, _instance, _info) => {
  caught.value = {
    message: err instanceof Error ? err.message : String(err),
    stack: err instanceof Error ? err.stack : undefined,
  }
  return false // prevent propagation
})

function retry() {
  caught.value = null
}
</script>

<template>
  <slot v-if="!caught" />
  <div v-else class="flex items-center justify-center min-h-[50vh] p-6">
    <div class="panel p-6 max-w-lg w-full space-y-4 text-center">
      <div class="font-mono text-sm font-bold text-[var(--red)]">页面发生错误</div>
      <div class="font-mono text-xs text-text-muted break-all">{{ caught.message }}</div>
      <button class="btn text-xs" @click="retry">重试</button>
    </div>
  </div>
</template>
