<script setup lang="ts">
import { ref } from 'vue'

const modelCounts = defineModel<Record<string, number>>('modelCounts', { default: () => ({}) })

const MODELS = ['POCKET 3', 'POCKET 4', 'ACTION 5', 'R50'] as const

// Use ref so template reactivity is reliable with dynamic keys
const counts = ref<Record<string, number>>({ ...modelCounts.value })
// Ensure all keys exist
for (const m of MODELS) {
  if (!(m in counts.value)) counts.value[m] = 0
}

function syncFromParent() {
  const next: Record<string, number> = {}
  for (const m of MODELS) {
    next[m] = modelCounts.value[m] ?? 0
  }
  counts.value = next
}
syncFromParent()

function syncToParent() {
  modelCounts.value = { ...counts.value }
}

// ── Inline edit via double-click on badge ────────────────────────
const editingModel = ref<string | null>(null)
const editValue = ref('')

function onCountDblClick(model: string) {
  editValue.value = String(counts.value[model] || 0)
  editingModel.value = model
}

function commitEdit() {
  if (editingModel.value) {
    const n = parseInt(editValue.value, 10)
    if (Number.isFinite(n) && n >= 0) {
      counts.value = { ...counts.value, [editingModel.value]: n }
      syncToParent()
    }
    editingModel.value = null
    editValue.value = ''
  }
}

function onEditKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') commitEdit()
  if (e.key === 'Escape') { editingModel.value = null; editValue.value = '' }
}

// ── Long-press → auto ±1 with shrinking interval ─────────────────
const longPressTimers: Record<string, ReturnType<typeof setTimeout>> = {}
const longPressStart: Record<string, number> = {}
const longPressMode = ref<'inc' | 'dec'>('inc')

function nextInterval(elapsed: number): number {
  // 300ms → 40ms smoothly over ~8 seconds of holding
  const t = Math.min(elapsed / 8000, 1)
  return Math.round(300 - t * 260)
}

function startLongPress(model: string, mode: 'inc' | 'dec') {
  longPressMode.value = mode
  longPressStart[model] = Date.now()

  function tick() {
    const elapsed = Date.now() - (longPressStart[model] || Date.now())
    if (longPressMode.value === 'dec') {
      counts.value = { ...counts.value, [model]: Math.max(0, (counts.value[model] || 0) - 1) }
    } else {
      counts.value = { ...counts.value, [model]: (counts.value[model] || 0) + 1 }
    }
    syncToParent()
    longPressTimers[model] = setTimeout(tick, nextInterval(elapsed))
  }

  longPressTimers[model] = setTimeout(tick, nextInterval(0))
}

function stopLongPress(model: string) {
  if (longPressTimers[model]) {
    clearTimeout(longPressTimers[model])
    delete longPressTimers[model]
  }
  delete longPressStart[model]
}

// ── Mouse handlers ───────────────────────────────────────────────
function onMouseDown(model: string, e: MouseEvent) {
  if (e.button === 2 || e.button === 0) {
    const mode = e.button === 2 ? 'dec' : 'inc'
    startLongPress(model, mode)
  }
}

function onMouseUp(model: string) {
  const elapsed = Date.now() - (longPressStart[model] || 0)
  if (elapsed < 250 && longPressTimers[model]) {
    // Short press → single action
    stopLongPress(model)
    if (longPressMode.value === 'dec') {
      counts.value = { ...counts.value, [model]: Math.max(0, (counts.value[model] || 0) - 1) }
    } else {
      counts.value = { ...counts.value, [model]: (counts.value[model] || 0) + 1 }
    }
    syncToParent()
  } else {
    stopLongPress(model)
  }
}

function onContextMenu(model: string, e: MouseEvent) {
  e.preventDefault()
}

function clearAll() {
  for (const m of MODELS) {
    counts.value = { ...counts.value, [m]: 0 }
  }
  syncToParent()
}
</script>

<template>
  <div class="flex items-center justify-center gap-3 flex-wrap">
    <div
      v-for="model in MODELS"
      :key="model"
      class="relative inline-flex"
    >
      <button
        class="font-mono text-xs px-4 py-2 border select-none cursor-pointer min-w-[100px] relative transition-colors"
        :class="(counts[model] || 0) > 0
          ? 'border-[var(--text-accent)] text-[var(--text-accent)]'
          : 'border-border text-text-primary hover:border-[var(--text-accent)]'"
        @mousedown="onMouseDown(model, $event)"
        @mouseup="onMouseUp(model)"
        @mouseleave="stopLongPress(model)"
        @contextmenu="onContextMenu(model, $event)"
      >
        {{ model }}
        <!-- Count badge -->
        <span
          v-if="editingModel !== model"
          class="absolute -top-2.5 -right-2.5 min-w-[20px] h-[20px] px-1 flex items-center justify-center rounded-full font-mono text-2xs font-bold leading-none cursor-pointer select-none z-10 border"
          :class="(counts[model] || 0) > 0
            ? ''
            : 'bg-[var(--bg-surface-raised)] text-text-muted border-border'"
          :style="(counts[model] || 0) > 0
            ? { backgroundColor: '#3D3D3D', borderColor: '#000000', color: '#FFFFFF' }
            : {}"
          @click.stop
          @dblclick.stop="onCountDblClick(model)"
        >
          {{ counts[model] || 0 }}
        </span>
        <!-- Inline edit input -->
        <input
          v-else
          v-model="editValue"
          type="text"
          inputmode="numeric"
          class="absolute -top-2.5 -right-2.5 w-[48px] h-[20px] font-mono text-2xs text-center border border-[var(--text-accent)] text-text-primary outline-none z-10 rounded-none"
          style="background: var(--bg-base); -moz-appearance: textfield"
          @blur="commitEdit"
          @keydown="onEditKeydown"
        />
      </button>
    </div>

    <!-- Clear all -->
    <button
      class="btn-danger font-mono text-xs px-3 py-1.5 select-none cursor-pointer"
      @click="clearAll"
    >
      清空
    </button>
  </div>
</template>
