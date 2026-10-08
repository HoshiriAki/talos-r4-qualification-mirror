<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'

const router = useRouter()
const countdown = ref(5)
let timer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  timer = setInterval(() => {
    countdown.value--
    if (countdown.value <= 0) {
      if (timer) clearInterval(timer)
      router.push('/')
    }
  }, 1000)
})

onUnmounted(() => {
  if (timer) clearInterval(timer)
})

function goHome() {
  if (timer) clearInterval(timer)
  router.push('/')
}
</script>

<template>
  <div class="flex flex-col items-center justify-center min-h-screen gap-4 bg-base font-mono">
    <div class="text-[96px] font-bold text-text-muted leading-none">404</div>
    <div class="text-lg text-text-secondary">页面不存在</div>
    <p class="text-sm text-text-muted mb-4">{{ countdown }} 秒后自动返回首页</p>
    <button class="btn-secondary" @click="goHome">返回首页</button>
  </div>
</template>
