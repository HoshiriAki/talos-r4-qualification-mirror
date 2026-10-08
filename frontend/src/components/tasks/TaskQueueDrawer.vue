<script setup lang="ts">
import { ref, watch } from 'vue'
import { fetchTasks, type WorkTask } from '@/api/tasks'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()
const tasks = ref<WorkTask[]>([])
const loading = ref(false)

watch(() => props.open, async (v) => {
  if (!v) return
  loading.value = true
  try { tasks.value = await fetchTasks({ status: 'queued,in_progress', limit: 50 }) }
  catch { tasks.value = [] }
  finally { loading.value = false }
})
</script>

<template>
  <Teleport to="body">
    <div v-if="open" class="drawer-overlay" @click.self="emit('close')">
      <div class="drawer">
        <div class="drawer-header">
          <h3 class="drawer-title">任务队列</h3>
          <button class="drawer-close" @click="emit('close')">×</button>
        </div>
        <div class="drawer-body">
          <div v-if="loading">加载中...</div>
          <div v-else-if="tasks.length === 0">队列为空</div>
          <div v-for="t in tasks" :key="t.id" class="queue-item">
            <span class="mono-label">{{ t.kind }}</span>
            <span>{{ t.title }}</span>
            <span class="text-xs text-tertiary">{{ t.sourceId }}</span>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.drawer-overlay { position: fixed; inset: 0; background: rgba(0,0,0,.5); z-index: 100; display: flex; justify-content: flex-end; }
.drawer { width: 400px; max-width: 90vw; background: var(--bg-surface); border-left: 1px solid var(--border-base); display: flex; flex-direction: column; height: 100%; }
.drawer-header { display: flex; justify-content: space-between; align-items: center; padding: 16px 20px; border-bottom: 1px solid var(--border-base); }
.drawer-title { font-family: 'Space Mono', monospace; font-weight: 700; font-size: 14px; color: var(--text-primary); margin: 0; }
.drawer-close { background: none; border: none; font-size: 20px; color: var(--text-tertiary); cursor: pointer; }
.drawer-body { flex: 1; overflow-y: auto; padding: 16px 20px; display: flex; flex-direction: column; gap: 10px; }
.queue-item { display: flex; flex-direction: column; gap: 2px; padding: 10px; border: 1px solid var(--border-base); border-radius: 8px; }
</style>
