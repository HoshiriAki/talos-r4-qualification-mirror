<script setup lang="ts">
import { computed } from 'vue'
import PrimeButton from 'primevue/button'
import PrimeSelect from 'primevue/select'
import PrimeSelectButton from 'primevue/selectbutton'
import type { UiComponentDefinition } from '@/ui'
import { labText, sceneLabel } from '../locale'
import { LAB_SCENE_CATALOG } from '../state/lab-scene-catalog'

const props = defineProps<{
  component: UiComponentDefinition
  sceneId: string
  viewportId: string
  zoom: number
  theme: 'dark' | 'light'
  locale: 'zh-CN' | 'en'
  direction: 'ltr' | 'rtl'
  reducedMotion: boolean
  dirty: boolean
}>()
const emit = defineEmits<{
  'update:sceneId': [value: string]
  'update:viewportId': [value: string]
  'update:zoom': [value: number]
  'update:theme': [value: 'dark' | 'light']
  'update:locale': [value: 'zh-CN' | 'en']
  'update:direction': [value: 'ltr' | 'rtl']
  toggleRegistry: []
  toggleInspector: []
  reset: []
}>()

const scenes = computed(() =>
  LAB_SCENE_CATALOG
    .filter((scene) => props.component.lab?.preview?.supportedScenes.includes(scene.id))
    .map((scene) => ({ label: sceneLabel(props.locale, scene.labelKey), value: scene.id })),
)
const viewports = [
  { label: 'Desktop · 1600×900', value: 'desktop-1600' },
  { label: 'Laptop · 1440×900', value: 'laptop-1440' },
  { label: 'Tablet · 1024×1366', value: 'tablet-1024' },
  { label: 'Mobile · 390×844', value: 'mobile-390' },
]
const zooms = [
  { label: '50%', value: 0.5 },
  { label: '67%', value: 0.67 },
  { label: '80%', value: 0.8 },
  { label: '100%', value: 1 },
]
const locales = [{ label: '中文', value: 'zh-CN' }, { label: 'EN', value: 'en' }]
</script>

<template>
  <header class="lab-toolbar">
    <div class="identity"><b>LAB</b><span>{{ component.title }}</span></div>
    <div class="toolbar-controls">
      <PrimeSelect :model-value="sceneId" :options="scenes" option-label="label" option-value="value" @update:model-value="emit('update:sceneId', $event)" />
      <PrimeSelect :model-value="viewportId" :options="viewports" option-label="label" option-value="value" @update:model-value="emit('update:viewportId', $event)" />
      <PrimeSelect :model-value="zoom" :options="zooms" option-label="label" option-value="value" @update:model-value="emit('update:zoom', $event)" />
      <PrimeSelectButton :model-value="theme" :allow-empty="false" :options="[{ label: labText(locale, 'dark'), value: 'dark' }, { label: labText(locale, 'light'), value: 'light' }]" option-label="label" option-value="value" @update:model-value="emit('update:theme', $event)" />
      <PrimeSelectButton :model-value="direction" :allow-empty="false" :options="[{ label: 'LTR', value: 'ltr' }, { label: 'RTL', value: 'rtl' }]" option-label="label" option-value="value" @update:model-value="emit('update:direction', $event)" />
      <PrimeSelectButton :model-value="locale" :allow-empty="false" :options="locales" option-label="label" option-value="value" @update:model-value="emit('update:locale', $event)" />
      <span class="reduced-flag" :class="{ active: reducedMotion }" :title="labText(locale, 'reducedMotionSimulation')">RM</span>
    </div>
    <div class="mobile-actions">
      <PrimeButton :label="labText(locale, 'registry')" severity="secondary" size="small" @click="emit('toggleRegistry')" />
      <PrimeButton :label="labText(locale, 'inspector')" severity="secondary" size="small" @click="emit('toggleInspector')" />
    </div>
    <div class="revision">
      <span :class="{ dirty }">{{ dirty ? labText(locale, 'dirty') : labText(locale, 'clean') }}</span>
      <small>REV 0.7.0</small>
      <PrimeButton :label="labText(locale, 'reset')" severity="secondary" size="small" @click="emit('reset')" />
    </div>
  </header>
</template>

<style scoped>
.lab-toolbar { display: flex; align-items: center; gap: 12px; height: 38px; padding: 0 10px; border: 1px solid var(--border-strong); background: var(--bg-surface); font-family: var(--font-mono); font-size: var(--font-size-xs); }
.identity { display: flex; align-items: center; gap: 10px; min-width: 180px; }
.identity b { color: var(--accent); letter-spacing: 0.12em; }
.identity span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.toolbar-controls { display: flex; align-items: center; gap: 6px; flex: 1; min-width: 0; }
.reduced-flag { font-size: var(--font-size-caption); color: var(--text-tertiary); border: 1px solid var(--border-default); padding: 1px 5px; border-radius: 4px; }
.reduced-flag.active { color: var(--accent); border-color: var(--accent); }
.mobile-actions { display: none; gap: 5px; }
.revision { display: flex; align-items: center; gap: 8px; white-space: nowrap; }
.revision span { color: var(--text-tertiary); }
.revision span.dirty { color: var(--accent); }
.revision small { color: var(--text-tertiary); }
@media (max-width: 1199px) { .mobile-actions { display: flex; } }
@media (max-width: 900px) {
  .toolbar-controls :deep(.p-select), .toolbar-controls :deep(.p-selectbutton) { display: none; }
  .identity { min-width: 0; }
  .revision small { display: none; }
}
@media (max-width: 620px) {
  .identity span, .revision span { display: none; }
  .lab-toolbar { gap: 6px; padding: 0 6px; }
  .mobile-actions :deep(.p-button-label) { font-size: var(--font-size-caption); }
}
</style>
