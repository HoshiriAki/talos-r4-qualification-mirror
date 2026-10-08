<script setup lang="ts">
import { ref, onMounted } from 'vue'
import type { WorkTask } from '@/api/tasks'
import HudTaskCard from '@/components/tasks/HudTaskCard.vue'

const tasks = ref<WorkTask[]>([])
const loading = ref(false)

onMounted(async () => {
  loading.value = true
  try {
    const { fetchTasks } = await import('@/api/tasks')
    tasks.value = await fetchTasks({ status: 'queued,in_progress', limit: 20 })
  } catch { /* tasks API may not exist yet */ }
  finally { loading.value = false }
})
</script>

<template>
  <div class="hud-compact-home">
    <div class="compact-header">
      <h1 class="compact-brand">TALOS HUD</h1>
      <span class="compact-badge" v-if="tasks.length">{{ tasks.length }} TASKS</span>
    </div>

    <div v-if="loading" class="compact-status">加载中...</div>
    <div v-else-if="tasks.length === 0" class="compact-status">无待处理任务</div>

    <div class="compact-task-list" v-else>
      <HudTaskCard v-for="t in tasks" :key="t.id" :task="t" />
    </div>
  </div>
</template>

<style scoped>
.hud-compact-home { display: flex; flex-direction: column; gap: 16px; padding: 16px; padding-bottom: env(safe-area-inset-bottom, 16px); }
.compact-header { display: flex; justify-content: space-between; align-items: center; }
.compact-brand { font-family: 'Space Mono', monospace; font-weight: 700; font-size: 16px; color: var(--text-primary); margin: 0; }
.compact-badge { font-family: 'Space Mono', monospace; font-size: 11px; color: var(--accent); padding: 4px 8px; border: 1px solid var(--accent); border-radius: 8px; }
.compact-status { font-family: 'Inter', sans-serif; font-size: 13px; color: var(--text-tertiary); text-align: center; padding: 40px 0; }
.compact-task-list { display: flex; flex-direction: column; gap: 10px; }
</style>
