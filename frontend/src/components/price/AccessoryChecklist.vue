<script setup lang="ts">
import { ref, computed } from 'vue'

export interface Accessory {
  key: string
  id: string
  name: string
  price: number
  perDay: boolean
}

const selectedAccessories = defineModel<string[]>('selectedAccessories', { default: () => [] })

function qid(a: Accessory) { return a.key + ':' + a.id }

const accessories = ref<Accessory[]>([
  { key: 'pocket', id: 'tripod', name: '三脚架', price: 15, perDay: true },
  { key: 'pocket', id: 'sd128', name: '128G SD 卡', price: 8, perDay: true },
  { key: 'pocket', id: 'sd256', name: '256G SD 卡', price: 12, perDay: true },
  { key: 'action', id: 'powerbank', name: '充电宝', price: 5, perDay: true },
  { key: 'action', id: 'mic', name: '无线麦克风', price: 20, perDay: true },
  { key: 'pocket', id: 'case', name: '保护壳', price: 0, perDay: false },
  { key: 'pocket', id: 'ndfilter', name: 'ND 滤镜', price: 5, perDay: true },
  { key: 'pocket', id: 'mount', name: '磁吸支架', price: 8, perDay: true },
  { key: 'action', id: 'mount', name: '磁吸支架', price: 8, perDay: true },
])

const selectedSet = computed(() => new Set(selectedAccessories.value))

const totalAccessoryPrice = computed(() => {
  return accessories.value
    .filter(a => selectedSet.value.has(qid(a)))
    .reduce((sum, a) => sum + a.price, 0)
})

const totalAccessoryPerDay = computed(() => {
  return accessories.value
    .filter(a => selectedSet.value.has(qid(a)) && a.perDay)
    .reduce((sum, a) => sum + a.price, 0)
})

defineExpose({ accessories, totalAccessoryPrice, totalAccessoryPerDay })

function toggle(a: Accessory) {
  const id = qid(a)
  const idx = selectedAccessories.value.indexOf(id)
  if (idx >= 0) {
    selectedAccessories.value = selectedAccessories.value.filter(s => s !== id)
  } else {
    selectedAccessories.value = [...selectedAccessories.value, id]
  }
}
</script>

<template>
  <div class="space-y-1">
    <div class="flex flex-wrap gap-1">
      <div
        v-for="acc in accessories"
        :key="qid(acc)"
        class="border cursor-pointer px-2 py-1 transition-colors flex items-center gap-1.5"
        :class="selectedSet.has(qid(acc))
          ? 'border-[var(--color-text-accent)] bg-[var(--color-btn-primary-bg)]'
          : 'border-border hover:border-[var(--color-text-accent)]'"
        @click="toggle(acc)"
      >
        <span class="font-mono text-xs">{{ acc.name }}</span>
        <span v-if="acc.price > 0" class="font-mono text-2xs text-text-muted">
          ¥{{ acc.price }}{{ acc.perDay ? '/天' : '' }}
        </span>
        <span v-else class="font-mono text-2xs text-text-muted">免费</span>
      </div>
    </div>
    <div v-if="selectedAccessories.length > 0" class="text-xs font-mono text-text-muted pt-1">
      配件合计 ¥{{ totalAccessoryPrice.toFixed(2) }}（每日 ¥{{ totalAccessoryPerDay.toFixed(2) }}）
    </div>
  </div>
</template>
