<script setup lang="ts">
import { computed, reactive, watch } from 'vue'

const modelCounts = defineModel<Record<string, number>>('modelCounts', { default: () => ({}) })
const selectedAccessories = defineModel<string[]>('selectedAccessories', { default: () => [] })

interface AccItem { id: string; name: string }
interface Group { key: string; label: string; models: string[]; accessories: AccItem[] }

const groups: Group[] = [
  {
    key: 'pocket',
    label: 'POCKET 系列',
    models: ['POCKET 3', 'POCKET 4'],
    accessories: [
      { id: 'tripod', name: '三脚架' },
      { id: 'sd128', name: '128G SD 卡' },
      { id: 'sd256', name: '256G SD 卡' },
      { id: 'case', name: '保护壳' },
      { id: 'ndfilter', name: 'ND 滤镜' },
      { id: 'mount', name: '磁吸支架' },
    ],
  },
  {
    key: 'action',
    label: 'ACTION 系列',
    models: ['ACTION 5', 'R50'],
    accessories: [
      { id: 'powerbank', name: '充电宝' },
      { id: 'mic', name: '无线麦克风' },
      { id: 'mount', name: '磁吸支架' },
    ],
  },
]

const allQualifiedIds = groups.flatMap(g => g.accessories.map(a => `${g.key}:${a.id}`))

function qid(groupKey: string, itemId: string) { return `${groupKey}:${itemId}` }

function buildAccCounts(): Record<string, number> {
  const map: Record<string, number> = {}
  for (const id of allQualifiedIds) map[id] = 0
  for (const id of selectedAccessories.value) map[id] = (map[id] || 0) + 1
  return map
}

const accCounts = reactive<Record<string, number>>(buildAccCounts())

watch(selectedAccessories, () => {
  const counts = buildAccCounts()
  for (const id of allQualifiedIds) accCounts[id] = counts[id] || 0
}, { deep: true })

function syncAccToParent() {
  const flat: string[] = []
  for (const id of allQualifiedIds) {
    for (let i = 0; i < (accCounts[id] || 0); i++) flat.push(id)
  }
  selectedAccessories.value = flat
}

type CountRecord = Record<string, number>
type SyncFn = () => void

interface LongPressState {
  timers: Record<string, ReturnType<typeof setTimeout>>
  starts: Record<string, number>
  pressMode: 'inc' | 'dec'
}

function createLongPressHandler(getRec: () => CountRecord, setRec: (rec: CountRecord) => void, sync: SyncFn) {
  const state: LongPressState = { timers: {}, starts: {}, pressMode: 'inc' }

  function interval(elapsed: number) {
    const t = Math.min(elapsed / 8000, 1)
    return Math.round(300 - t * 260)
  }

  function start(key: string, mode: 'inc' | 'dec') {
    state.pressMode = mode
    state.starts[key] = Date.now()
    function tick() {
      const e = Date.now() - (state.starts[key] || Date.now())
      const rec = getRec()
      rec[key] = state.pressMode === 'dec'
        ? Math.max(0, (rec[key] || 0) - 1)
        : (rec[key] || 0) + 1
      setRec(rec)
      sync()
      state.timers[key] = setTimeout(tick, interval(e))
    }
    state.timers[key] = setTimeout(tick, interval(0))
  }

  function stop(key: string) {
    if (state.timers[key]) {
      clearTimeout(state.timers[key])
      delete state.timers[key]
    }
    delete state.starts[key]
  }

  function onDown(key: string, e: MouseEvent) {
    if (e.button === 0) start(key, 'inc')
    else if (e.button === 2) start(key, 'dec')
  }

  function onUp(key: string) {
    const elapsed = Date.now() - (state.starts[key] || 0)
    if (elapsed < 250 && state.timers[key]) {
      stop(key)
      const rec = getRec()
      rec[key] = state.pressMode === 'dec'
        ? Math.max(0, (rec[key] || 0) - 1)
        : (rec[key] || 0) + 1
      setRec(rec)
      sync()
    } else {
      stop(key)
    }
  }

  function onContext(_key: string, e: MouseEvent) { e.preventDefault() }
  return { onDown, onUp, stop, onContext }
}

const modelPress = createLongPressHandler(
  () => modelCounts.value as CountRecord,
  rec => { modelCounts.value = rec as Record<string, number> },
  () => {},
)
const accPress = createLongPressHandler(
  () => accCounts as CountRecord,
  rec => { Object.assign(accCounts, rec) },
  syncAccToParent,
)

function clearAll() {
  for (const model of groups.flatMap(g => g.models)) {
    modelCounts.value = { ...modelCounts.value, [model]: 0 }
  }
  for (const id of allQualifiedIds) accCounts[id] = 0
  syncAccToParent()
}

const selectedAccessoryCount = computed(() => selectedAccessories.value.length)

function btnClass(count: number) {
  return count > 0
    ? 'border-[var(--text-accent)] text-[var(--text-accent)]'
    : 'border-border text-text-primary hover:border-[var(--text-accent)]'
}

const badgeStyle = computed(() => ({
  backgroundColor: 'var(--text-primary)',
  borderColor: 'var(--bg-base)',
  color: 'var(--bg-base)',
}))
</script>

<template>
  <div class="space-y-4">
    <div class="flex items-center justify-between">
      <p class="font-mono text-2xs text-text-muted">左键 +1 · 右键 -1 · 长按连加/连减 · 价格由服务器报价</p>
      <button class="btn-danger font-mono text-2xs px-2 py-1 select-none cursor-pointer" @click="clearAll">清空</button>
    </div>

    <template v-for="group in groups" :key="group.label">
      <span class="font-mono text-xs font-bold text-text-primary border-b border-[var(--text-accent)] pb-0.5">{{ group.label }}</span>

      <div class="flex gap-3 items-start">
        <div class="flex flex-col gap-1.5 shrink-0 pt-0.5">
          <button
            v-for="model in group.models"
            :key="model"
            class="font-mono text-xs px-3 py-1.5 border transition-colors select-none cursor-pointer min-w-[100px] text-center relative"
            :class="btnClass(modelCounts[model] || 0)"
            @mousedown="modelPress.onDown(model, $event)"
            @mouseup="modelPress.onUp(model)"
            @mouseleave="modelPress.stop(model)"
            @contextmenu="modelPress.onContext(model, $event)"
          >
            {{ model }}
            <span
              v-if="(modelCounts[model] || 0) > 0"
              class="absolute -top-2 -right-2 min-w-[20px] h-[20px] px-1 flex items-center justify-center rounded-full font-mono text-2xs font-bold leading-none z-10 border"
              :style="badgeStyle"
            >{{ modelCounts[model] }}</span>
          </button>
        </div>

        <div class="flex flex-wrap gap-1.5 flex-1 items-start pt-0.5 ml-2 border border-[var(--bg-surface-raised)] p-2">
          <button
            v-for="acc in group.accessories"
            :key="qid(group.key, acc.id)"
            class="font-mono text-xs px-2.5 py-1.5 border transition-colors select-none cursor-pointer text-center relative"
            :class="btnClass(accCounts[qid(group.key, acc.id)] || 0)"
            @mousedown="accPress.onDown(qid(group.key, acc.id), $event)"
            @mouseup="accPress.onUp(qid(group.key, acc.id))"
            @mouseleave="accPress.stop(qid(group.key, acc.id))"
            @contextmenu="accPress.onContext(qid(group.key, acc.id), $event)"
          >
            {{ acc.name }}
            <span class="text-2xs text-text-muted ml-1">SERVER PRICE</span>
            <span
              v-if="(accCounts[qid(group.key, acc.id)] || 0) > 0"
              class="absolute -top-2 -right-2 min-w-[20px] h-[20px] px-1 flex items-center justify-center rounded-full font-mono text-2xs font-bold leading-none z-10 border"
              :style="badgeStyle"
            >{{ accCounts[qid(group.key, acc.id)] }}</span>
          </button>
        </div>
      </div>
    </template>

    <div v-if="selectedAccessoryCount > 0" class="text-xs font-mono text-text-muted pt-1 border-t border-border">
      已选择 {{ selectedAccessoryCount }} 件配件；价格将在服务器 Quote 快照中返回。
    </div>
  </div>
</template>
