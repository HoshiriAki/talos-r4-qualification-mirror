<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import PrimeInputText from 'primevue/inputtext'
import PrimePaginator from 'primevue/paginator'
import type { UiComponentDefinition } from '@/ui'
import { labText, type UiLabLocale } from '../locale'

const props = defineProps<{ components: readonly UiComponentDefinition[]; selectedId: string; query: string; locale: UiLabLocale }>()
const emit = defineEmits<{ 'update:query': [value: string]; select: [value: string] }>()
const list = ref<HTMLElement | null>(null)
const page = ref(0)
const listHeight = ref(300)
let observer: ResizeObserver | undefined
const rows = computed(() => Math.max(2, Math.floor(listHeight.value / 82)))
const pagedComponents = computed(() => props.components.slice(page.value * rows.value, page.value * rows.value + rows.value))
watch(() => [props.components.length, rows.value], () => { if (page.value * rows.value >= props.components.length) page.value = 0 })
onMounted(() => { observer = new ResizeObserver(([entry]) => { listHeight.value = entry.contentRect.height }); if (list.value) observer.observe(list.value) })
onBeforeUnmount(() => observer?.disconnect())
</script>

<template>
  <aside class="registry-pane"><header><span>{{ labText(locale, 'registry') }}</span><small>{{ components.length }} {{ labText(locale, 'assets') }}</small></header><PrimeInputText :model-value="query" :placeholder="labText(locale, 'filter')" @update:model-value="emit('update:query', String($event))" /><nav ref="list" :aria-label="labText(locale, 'registry')"><button v-for="component in pagedComponents" :key="component.id" type="button" :class="{ selected: component.id === selectedId }" @click="emit('select', component.id)"><strong>{{ component.title }}</strong><small>{{ component.id }}</small><em>{{ component.category }}</em></button></nav><PrimePaginator v-if="components.length > rows" :first="page * rows" :rows="rows" :total-records="components.length" @page="page = $event.page" /></aside>
</template>

<style scoped>
.registry-pane{display:grid;grid-template-rows:auto auto minmax(0,1fr) auto;gap:8px;min-width:0;min-height:0;padding:10px;border-right:1px solid var(--border-strong);background:var(--bg-surface)}header{display:flex;justify-content:space-between;font-family:var(--font-mono);font-size:var(--font-size-xs)}header span{color:var(--text-primary)}header small,button small,button em{color:var(--text-tertiary);font-size:var(--font-size-caption)}nav{display:grid;align-content:start;gap:4px;min-height:0;overflow:hidden}button{display:grid;gap:4px;padding:10px;border:1px solid transparent;background:var(--bg-field);color:var(--text-secondary);text-align:left;cursor:pointer}button:hover{border-color:var(--border-strong)}button.selected{border-color:var(--accent);box-shadow:inset 2px 0 var(--accent);color:var(--text-primary)}button strong{font-size:var(--font-size-sm)}button em{text-transform:uppercase;font-style:normal}
</style>
