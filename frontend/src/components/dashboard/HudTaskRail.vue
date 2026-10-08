<script setup lang="ts">
import { ref, onMounted } from 'vue'
import HudTaskCard from '@/components/tasks/HudTaskCard.vue'
import type { WorkTask } from '@/api/tasks'

const tasks = ref<WorkTask[]>([])
const loading = ref(false)

onMounted(async () => {
  loading.value = true
  try {
    const { fetchTasks } = await import('@/api/tasks')
    tasks.value = await fetchTasks({ status: 'queued,in_progress', limit: 10 })
  } catch { /* tasks API may not exist yet */ }
  finally { loading.value = false }
})
</script>

<template>
  <aside class="task-rail">
    <h4 class="task-rail-title">TASKS</h4>
    <div v-if="loading" class="text-tertiary text-xs">...</div>
    <div v-else-if="tasks.length === 0" class="text-tertiary text-xs">无待处理任务</div>
    <HudTaskCard v-for="t in tasks" :key="t.id" :task="t" />
  </aside>
</template>

<style scoped>
.task-rail { display: flex; flex-direction: column; gap: 8px; overflow-y: auto; }
.task-rail-title { font-family: 'Space Mono', monospace; font-size: 10px; letter-spacing: 0.12em; color: var(--text-tertiary); text-transform: uppercase; padding: 4px 0; }
</style>
