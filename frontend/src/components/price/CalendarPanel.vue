<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue'

const props = defineProps<{
  selectedStart: string
  selectedEnd: string
}>()

const emit = defineEmits<{
  'update:selectedStart': [value: string]
  'update:selectedEnd': [value: string]
}>()

// Responsive: 3 months when wide (≥720px), 2 when medium (≥480px), 1 when narrow
const containerRef = ref<HTMLElement | null>(null)
const numberOfMonths = ref(2)

let observer: ResizeObserver | null = null

onMounted(() => {
  if (containerRef.value) {
    observer = new ResizeObserver(([entry]) => {
      const w = entry.contentRect.width
      numberOfMonths.value = w >= 720 ? 3 : w >= 480 ? 2 : 1
    })
    observer.observe(containerRef.value)
  }
})

onUnmounted(() => {
  observer?.disconnect()
})

// PrimeVue DatePicker range mode uses an array [start, end] of Date objects
const modelDates = computed<Date[] | null>({
  get: () => {
    const result: Date[] = []
    if (props.selectedStart) result.push(new Date(props.selectedStart + 'T00:00:00'))
    if (props.selectedEnd) result.push(new Date(props.selectedEnd + 'T00:00:00'))
    return result
  },
  set: (val: Date[] | null) => {
    if (!val || val.length === 0 || (val.length === 1 && !val[0])) {
      emit('update:selectedStart', '')
      emit('update:selectedEnd', '')
    } else if (val.length === 1 && val[0]) {
      const s = toDateKey(val[0])
      if (s !== props.selectedStart || props.selectedEnd) {
        emit('update:selectedStart', s)
        emit('update:selectedEnd', '')
      }
    } else if (val.length === 2 && val[0] && val[1]) {
      let [a, b] = val
      if (a > b) [a, b] = [b, a]
      emit('update:selectedStart', toDateKey(a))
      emit('update:selectedEnd', toDateKey(b))
    }
  },
})

function toDateKey(d: Date | null): string {
  if (!d) return ''
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

const dayCount = computed(() => {
  if (!props.selectedStart || !props.selectedEnd) return 0
  const s = new Date(props.selectedStart + 'T00:00:00')
  const e = new Date(props.selectedEnd + 'T00:00:00')
  return Math.round((e.getTime() - s.getTime()) / 86400000) + 1
})

const minDate = new Date()
minDate.setHours(0, 0, 0, 0)
</script>

<template>
  <div ref="containerRef" class="w-full datepicker-wrapper">
    <DatePicker
      v-model="modelDates"
      selection-mode="range"
      inline
      :number-of-months="numberOfMonths"
      :min-date="minDate"
      class="w-full"
      date-format="yy-mm-dd"
    />
    <div v-if="dayCount > 0" class="text-center mt-2 font-mono text-xs text-text-secondary">
      共 {{ dayCount }} 天
    </div>
  </div>
</template>

<style>
/* Square day cells, sharp date range — Terminal Brutalist override for inline calendar */
/* NOTE: unscoped block so :is() penetrates PrimeVue's internal component shadow */
.datepicker-wrapper {
  --p-datepicker-day-cell-border-radius: 0;
  --p-datepicker-day-border-radius: 0;
}

/* Fallback — directly override PrimeVue's rounded day cells */
.datepicker-wrapper :is(.p-datepicker-day-cell, .p-datepicker-day, .p-datepicker-calendar-day, .p-day-picker-day) {
  border-radius: 0 !important;
}

.datepicker-wrapper :is(.p-datepicker-day-selected) {
  border-radius: 0 !important;
}
</style>
