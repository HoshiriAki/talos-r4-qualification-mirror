<script setup lang="ts">
import { ref } from 'vue'
import { taskAction, sendTaskToPc, type WorkTask } from '@/api/tasks'

const props = defineProps<{ task: WorkTask }>()
const busy = ref(false)
const error = ref('')

const riskLabel = { low: 'LOW', medium: 'MEDIUM', high: 'HIGH' } as const
const riskColor = { low: 'var(--success)', medium: 'var(--warning)', high: 'var(--accent)' } as const

const emit = defineEmits<{ 'changed': [] }>()

async function act(action: string) {
  busy.value = true; error.value = ''
  try {
    if (action === 'send_to_pc') {
      await sendTaskToPc(props.task.id, props.task.version)
    } else {
      await taskAction(props.task.id, action, props.task.version)
    }
    emit('changed')
  } catch (e: any) {
    error.value = e.message || '操作失败'
  } finally { busy.value = false }
}
</script>

<template>
  <div class="task-card" :class="`risk-${task.risk}`">
    <div class="task-header">
      <span class="task-kind">{{ task.kind }}</span>
      <span class="task-risk" :style="{ color: riskColor[task.risk] }">{{ riskLabel[task.risk] }}</span>
    </div>
    <p class="task-title">{{ task.title }}</p>
    <p class="task-summary">{{ task.summary }}</p>
    <div class="task-actions">
      <button
        v-for="cap in task.capabilities" :key="cap"
        class="task-btn"
        :disabled="busy"
        @click="act(cap)"
      >{{ cap === 'send_to_pc' ? '转PC' : cap === 'confirm' ? '确认' : cap === 'acknowledge' ? '已知悉' : cap === 'scan' ? '扫描' : cap === 'defer' ? '稍后' : cap }}</button>
    </div>
    <p v-if="error" class="task-error">{{ error }}</p>
  </div>
</template>

<style scoped>
.task-card { padding: 12px; border: 1px solid var(--border-base); border-radius: 10px; background: var(--bg-surface); display: flex; flex-direction: column; gap: 6px; }
.task-card.risk-high { border-color: var(--accent); border-width: 2px; }
.task-card.risk-medium { border-color: var(--warning); }
.task-header { display: flex; justify-content: space-between; align-items: center; }
.task-kind { font-family: 'Space Mono', monospace; font-size: 10px; color: var(--text-tertiary); text-transform: uppercase; }
.task-risk { font-family: 'Space Mono', monospace; font-size: 10px; font-weight: 700; }
.task-title { font-family: 'Inter', sans-serif; font-size: 13px; font-weight: 600; color: var(--text-primary); margin: 0; }
.task-summary { font-family: 'Inter', sans-serif; font-size: 11px; color: var(--text-secondary); margin: 0; }
.task-actions { display: flex; gap: 6px; flex-wrap: wrap; }
.task-btn { font-family: 'Space Mono', monospace; font-size: 10px; padding: 4px 10px; border: 1px solid var(--border-base); border-radius: 6px; background: var(--bg-elevated); color: var(--text-secondary); cursor: pointer; }
.task-btn:hover { border-color: var(--accent); color: var(--text-primary); }
.task-btn:disabled { opacity: 0.4; cursor: not-allowed; }
.task-error { font-size: 10px; color: var(--accent); margin: 0; }
</style>
