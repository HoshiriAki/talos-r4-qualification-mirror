<script setup lang="ts">
import { computed, ref } from 'vue'
import PrimeButton from 'primevue/button'
import PrimePaginator from 'primevue/paginator'
import PrimeSelect from 'primevue/select'
import PrimeSlider from 'primevue/slider'
import PrimeTabs from 'primevue/tabs'
import PrimeTabList from 'primevue/tablist'
import PrimeTab from 'primevue/tab'
import PrimeTabPanels from 'primevue/tabpanels'
import PrimeTabPanel from 'primevue/tabpanel'
import PrimeToggleSwitch from 'primevue/toggleswitch'
import PrimeSelectButton from 'primevue/selectbutton'
import type { UiComponentDefinition, UiLabMotionEngine } from '@/ui'
import type { LabJournalEntry } from '../state/change-journal'
import { labText, type UiLabLocale } from '../locale'

const props = defineProps<{
  component: UiComponentDefinition
  changes: LabJournalEntry[]
  prompt: string
  snapshot: string
  diff: string
  diagnostics: string[]
  locale: UiLabLocale
  progress: number
  activeMotionId: string | null
  playbackRate: number
  loop: boolean
  engine: UiLabMotionEngine | 'auto'
  reducedMotion: boolean
}>()
const emit = defineEmits<{
  copy: [text: string]
  download: [name: string, text: string]
  restore: [text: string]
  restoreError: [message: string]
  selectMotion: [id: string]
  play: []
  pause: []
  restart: []
  seek: [value: number]
  settings: [value: { playbackRate: number; loop: boolean; engine: UiLabMotionEngine | 'auto' }]
  toggleReducedMotion: [value: boolean]
  toggleCollapsed: []
}>()

const trackPage = ref(0)
const rows = 3
const timelineZoom = ref(1)
const collapsed = ref(false)
const restoreInput = ref<HTMLInputElement | null>(null)
const motions = computed(() => props.component.lab?.motions ?? [])
const tracks = computed(() =>
  motions.value.flatMap((motion) =>
    motion.tracks.map((track) => ({ motion, track, layer: motion.target })),
  ),
)
const pagedTracks = computed(() => tracks.value.slice(trackPage.value * rows, trackPage.value * rows + rows))
const motionOptions = computed(() =>
  motions.value.map((motion) => ({ label: motion.title, value: motion.id })),
)
const activeMotion = computed(() =>
  motions.value.find((motion) => motion.id === props.activeMotionId) ?? motions.value[0],
)
const rates = [
  { label: '0.5×', value: 0.5 },
  { label: '1×', value: 1 },
  { label: '1.5×', value: 1.5 },
  { label: '2×', value: 2 },
]
const engines = [
  { label: 'Auto', value: 'auto' },
  { label: 'Anime', value: 'anime' },
  { label: 'GSAP', value: 'gsap' },
  { label: 'Native', value: 'native' },
]
function updateSettings(patch: Partial<{ playbackRate: number; loop: boolean; engine: UiLabMotionEngine | 'auto' }>) {
  emit('settings', {
    playbackRate: patch.playbackRate ?? props.playbackRate,
    loop: patch.loop ?? props.loop,
    engine: patch.engine ?? props.engine,
  })
}
async function readRestoreFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  try {
    if (file.size > 2_000_000) throw new Error('Snapshot file exceeds the 2 MB limit.')
    emit('restore', await file.text())
  } catch (error) {
    emit('restoreError', error instanceof Error ? error.message : String(error))
  } finally {
    input.value = ''
  }
}
</script>

<template>
  <section class="bottom-dock" :class="{ collapsed }">
    <PrimeTabs :value="motions.length ? 'motion' : 'changes'">
      <PrimeTabList>
        <PrimeTab v-if="motions.length" value="motion">{{ labText(locale, 'motion') }}</PrimeTab>
        <PrimeTab value="changes">{{ labText(locale, 'changes') }} <b v-if="changes.length">{{ changes.length }}</b></PrimeTab>
        <PrimeTab value="prompt">{{ labText(locale, 'prompt') }}</PrimeTab>
        <PrimeTab value="diagnostics">{{ labText(locale, 'diagnostics') }}</PrimeTab>
        <PrimeButton class="collapse" :label="collapsed ? labText(locale, 'expand') : labText(locale, 'collapse')" severity="secondary" size="small" @click="collapsed = !collapsed; emit('toggleCollapsed')" />
      </PrimeTabList>
      <PrimeTabPanels v-if="!collapsed">
        <PrimeTabPanel v-if="motions.length" value="motion">
          <div class="motion-head">
            <PrimeSelect v-if="motionOptions.length > 1" :model-value="activeMotion?.id" :options="motionOptions" option-label="label" option-value="value" :placeholder="labText(locale, 'motion')" @update:model-value="emit('selectMotion', String($event))" />
            <span v-else>{{ activeMotion?.title }}</span>
            <div class="motion-actions">
              <PrimeButton :label="labText(locale, 'play')" size="small" @click="emit('play')" />
              <PrimeButton :label="labText(locale, 'pause')" severity="secondary" size="small" @click="emit('pause')" />
              <PrimeButton :label="labText(locale, 'restart')" severity="secondary" size="small" @click="emit('restart')" />
            </div>
          </div>
          <div class="motion-controls">
            <PrimeSlider :model-value="progress" :min="0" :max="1" :step="0.01" @update:model-value="emit('seek', Number($event))" />
            <PrimeSelectButton :model-value="playbackRate" :options="rates" option-label="label" option-value="value" @update:model-value="updateSettings({ playbackRate: Number($event) })" />
            <label>
              <PrimeToggleSwitch :model-value="loop" @update:model-value="updateSettings({ loop: Boolean($event) })" />
              {{ labText(locale, 'loop') }}
            </label>
            <PrimeSelectButton :model-value="engine" :options="engines" option-label="label" option-value="value" @update:model-value="updateSettings({ engine: $event })" />
            <label :class="{ active: reducedMotion }">
              <PrimeToggleSwitch :model-value="reducedMotion" @update:model-value="emit('toggleReducedMotion', Boolean($event))" />
              {{ labText(locale, 'reducedMotionSimulation') }}
            </label>
          </div>
          <div class="timeline" :style="{ '--timeline-zoom': timelineZoom }">
            <div v-for="row in pagedTracks" :key="`${row.motion.id}-${row.track}`" class="track">
              <strong>{{ row.layer }} / {{ row.track }}</strong>
              <div class="rail">
                <i v-for="frame in row.motion.keyframes" :key="frame.offset" :style="{ left: `${frame.offset * 100}%` }" :title="frame.label" />
                <b :style="{ left: `${progress * 100}%` }" />
              </div>
              <small>{{ row.motion.durationMs }}ms · {{ row.motion.delayMs }}ms delay · {{ row.motion.easing.native }}</small>
            </div>
          </div>
          <div class="timeline-footer">
            <label>{{ labText(locale, 'timelineZoom') }} <PrimeSlider v-model="timelineZoom" :min="0.75" :max="2" :step="0.25" /></label>
            <PrimePaginator v-if="tracks.length > rows" :first="trackPage * rows" :rows="rows" :total-records="tracks.length" @page="trackPage = $event.page" />
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel value="changes">
          <div class="changes">
            <p v-if="!changes.length">{{ labText(locale, 'noChanges') }}</p>
            <article v-for="change in changes" :key="change.id">
              <strong>{{ change.path }}</strong>
              <code>{{ change.source }} · r{{ change.revision }}</code>
              <span>{{ JSON.stringify(change.before) }} → {{ JSON.stringify(change.after) }}</span>
            </article>
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel value="prompt">
          <div class="export">
            <textarea readonly :value="prompt" />
            <div>
              <PrimeButton :label="labText(locale, 'copyPrompt')" size="small" @click="emit('copy', prompt)" />
              <PrimeButton :label="labText(locale, 'copyDiff')" severity="secondary" size="small" @click="emit('copy', diff)" />
              <PrimeButton :label="labText(locale, 'copySnapshot')" severity="secondary" size="small" @click="emit('copy', snapshot)" />
              <PrimeButton :label="labText(locale, 'download')" severity="secondary" size="small" @click="emit('download', 'talos-ui-lab-export.json', snapshot)" />
              <PrimeButton :label="labText(locale, 'restoreSnapshot')" severity="secondary" size="small" @click="restoreInput?.click()" />
              <input ref="restoreInput" type="file" accept="application/json,.json" hidden @change="readRestoreFile" />
            </div>
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel value="diagnostics">
          <ul class="diagnostics">
            <li>{{ labText(locale, 'sandboxIsolation') }}</li>
            <li>{{ labText(locale, 'protocolGuard') }}</li>
            <li>{{ labText(locale, 'rendererMap') }}</li>
            <li>{{ labText(locale, 'enginePreference') }}: {{ engine }}</li>
            <li>{{ labText(locale, 'sandboxBoundary') }}</li>
            <li v-for="diagnostic in diagnostics" :key="diagnostic">{{ diagnostic }}</li>
          </ul>
        </PrimeTabPanel>
      </PrimeTabPanels>
    </PrimeTabs>
  </section>
</template>

<style scoped>
.bottom-dock { min-height: 220px; border: 1px solid var(--border-strong); background: var(--bg-surface); overflow: hidden; }
.bottom-dock.collapsed { min-height: 40px; }
.collapse { margin-left: auto; }
.motion-head, .motion-controls, .timeline-footer { display: flex; align-items: center; gap: 8px; padding: 8px 12px; font-family: var(--font-mono); font-size: var(--font-size-xs); }
.motion-head { justify-content: space-between; }
.motion-head :deep(.p-select) { min-width: 160px; }
.motion-actions, .export div { display: flex; flex-wrap: wrap; gap: 6px; }
.motion-controls :deep(.p-slider) { width: clamp(110px, 18vw, 280px); }
.motion-controls label { display: flex; align-items: center; gap: 5px; white-space: nowrap; color: var(--text-secondary); }
.motion-controls label.active { color: var(--accent); }
.timeline { display: grid; gap: 7px; padding: 0 12px 8px; }
.track { display: grid; grid-template-columns: 130px minmax(0, 1fr) 210px; align-items: center; gap: 10px; min-height: 28px; font-size: var(--font-size-xs); }
.track strong { font-family: var(--font-mono); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.track small { overflow: hidden; color: var(--text-tertiary); text-overflow: ellipsis; white-space: nowrap; }
.rail { position: relative; height: 7px; background: var(--bg-field); border: 1px solid var(--border-base); transform: scaleX(var(--timeline-zoom)); transform-origin: left center; }
.rail i { position: absolute; top: 50%; width: 8px; height: 8px; border-radius: 50%; background: var(--accent); transform: translate(-50%, -50%); }
.rail b { position: absolute; top: -4px; bottom: -4px; width: 1px; background: var(--text-primary); }
.timeline-footer { justify-content: space-between; }
.timeline-footer label { display: flex; align-items: center; gap: 8px; }
.timeline-footer :deep(.p-slider) { width: 120px; }
.changes { display: grid; gap: 6px; padding: 10px; }
.changes p { margin: 0; color: var(--text-tertiary); font-size: var(--font-size-xs); }
.changes article { display: grid; grid-template-columns: 180px 1fr; gap: 4px; padding: 7px; background: var(--bg-field); font-size: var(--font-size-xs); }
.changes code { color: var(--text-tertiary); }
.export { display: grid; gap: 8px; padding: 10px; }
.export textarea { min-height: 115px; resize: none; padding: 8px; border: 1px solid var(--border-base); background: var(--bg-field); color: var(--text-secondary); font-family: var(--font-mono); font-size: var(--font-size-caption); line-height: 1.5; }
.diagnostics { display: grid; gap: 7px; margin: 0; padding: 12px 26px; color: var(--text-secondary); font-family: var(--font-mono); font-size: var(--font-size-xs); }
@media (max-width: 900px) {
  .track { grid-template-columns: 90px minmax(0, 1fr); }
  .track small, .motion-controls :deep(.p-selectbutton):last-child { display: none; }
  .motion-controls { flex-wrap: wrap; }
}
</style>
