<script setup lang="ts">
defineProps<{ total: number; selectedCount: number }>()
const emit = defineEmits<{ 'update:keyword': [v: string]; 'bulkDelete': []; 'export': [] }>()
</script>

<template>
  <div class="workbench-toolbar">
    <div class="toolbar-left">
      <input
        class="input-sm"
        type="text"
        placeholder="搜索订单号 / 客户..."
        @input="(e: Event) => emit('update:keyword', (e.target as HTMLInputElement).value)"
      />
      <span class="mono-label">{{ total }} 条结果</span>
    </div>
    <div class="toolbar-right">
      <button class="btn btn-secondary" :disabled="selectedCount === 0" @click="emit('bulkDelete')">
        批量删除 ({{ selectedCount }})
      </button>
      <button class="btn btn-secondary" @click="emit('export')">导出</button>
    </div>
  </div>
</template>

<style scoped>
.workbench-toolbar { display: flex; justify-content: space-between; align-items: center; gap: 16px; }
.toolbar-left { display: flex; align-items: center; gap: 12px; }
.toolbar-right { display: flex; align-items: center; gap: 8px; }
.input-sm { background: var(--bg-field); border: 1px solid var(--border-base); border-radius: 8px; padding: 6px 12px; font-family: 'Inter', sans-serif; font-size: 13px; color: var(--text-primary); width: 240px; }
</style>
