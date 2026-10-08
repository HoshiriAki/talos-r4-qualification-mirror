<script setup lang="ts">
import { computed } from 'vue'
import PrimeInputText from 'primevue/inputtext'
import PrimeSelect from 'primevue/select'
import PrimeSlider from 'primevue/slider'
import PrimeTabs from 'primevue/tabs'
import PrimeTabList from 'primevue/tablist'
import PrimeTab from 'primevue/tab'
import PrimeTabPanels from 'primevue/tabpanels'
import PrimeTabPanel from 'primevue/tabpanel'
import PrimeToggleSwitch from 'primevue/toggleswitch'
import type { UiComponentDefinition, UiLabControlDefinition, UiLabDesignSlotDefinition } from '@/ui'
import { controlLabel, labText, type UiLabLocale } from '../locale'

const props = defineProps<{
  component: UiComponentDefinition
  values: Record<string, unknown>
  slots: UiLabDesignSlotDefinition[]
  slotValues: Record<string, { enabled: boolean; content: string | null }>
  sceneId: string
  locale: UiLabLocale
}>()
const emit = defineEmits<{
  change: [control: UiLabControlDefinition, value: unknown]
  slotChange: [slotId: string, patch: { enabled?: boolean; content?: string | null }]
  resetPage: []
  resetAll: []
}>()

const componentControls = computed(() => props.component.lab?.controls?.filter((control) => control.scope === 'component') ?? [])
const sceneControls = computed(() => props.component.lab?.controls?.filter((control) => control.scope === 'scene') ?? [])
const motions = computed(() => props.component.lab?.motions ?? [])

const localizedLabels: Record<string, string> = {
  'Execution primary': '主执行', Secondary: '次级', Danger: '危险', Ghost: '透明',
  Default: '默认', Executing: '执行中', Disabled: '禁用', Small: '小型', Large: '大型',
  Completed: '已完成', Active: '活跃', Pending: '待处理', Overdue: '已逾期', Cancelled: '已取消',
}
const localizedEffects: Record<string, string> = {
  'Changes the semantic action treatment.': '调整操作的语义强度。',
  'Changes the component interaction state.': '调整组件交互状态。',
  'Changes the rendered control size.': '调整控件的渲染尺寸。',
  'Changes the visible action label.': '调整可见操作标签。',
  'Changes the semantic status and accessible label.': '调整语义状态及无障碍标签。',
  'Changes the primary loading message.': '调整主加载提示。',
  'Changes the foreground separation of the local loading scene.': '调整本地加载场景的前景分离度。',
}

function valueFor(control: UiLabControlDefinition) {
  return props.values[`${control.scope}:${control.path}`] ?? control.default
}
function optionsFor(control: UiLabControlDefinition) {
  return control.options?.map((option) => ({
    ...option,
    label: props.locale === 'zh-CN' ? (localizedLabels[option.label] ?? option.label) : option.label,
  }))
}
function effectFor(control: UiLabControlDefinition) {
  return props.locale === 'zh-CN' ? (localizedEffects[control.semanticEffect] ?? control.semanticEffect) : control.semanticEffect
}
</script>

<template>
  <aside class="inspector">
    <header><span>{{ labText(locale, 'inspector') }}</span><small>{{ component.id }}</small></header>
    <PrimeTabs value="component">
      <PrimeTabList>
        <PrimeTab value="component">{{ labText(locale, 'component') }}</PrimeTab>
        <PrimeTab v-if="slots.length" value="slots">{{ labText(locale, 'slot') }}</PrimeTab>
        <PrimeTab v-if="sceneControls.length" value="scene">{{ labText(locale, 'scene') }}</PrimeTab>
        <PrimeTab v-if="motions.length" value="motion">{{ labText(locale, 'motion') }}</PrimeTab>
        <PrimeTab value="validation">{{ labText(locale, 'validation') }}</PrimeTab>
      </PrimeTabList>
      <PrimeTabPanels>
        <PrimeTabPanel value="component">
          <div class="control-list">
            <label v-for="control in componentControls" :key="control.id">
              <span>{{ controlLabel(locale, control.labelKey) }}</span>
              <PrimeSelect v-if="control.type === 'select'" :model-value="valueFor(control)" :options="optionsFor(control)" option-label="label" option-value="value" @update:model-value="emit('change', control, $event)" />
              <PrimeToggleSwitch v-else-if="control.type === 'toggle'" :model-value="Boolean(valueFor(control))" @update:model-value="emit('change', control, $event)" />
              <PrimeSlider v-else-if="control.type === 'range' || control.type === 'number' || control.type === 'duration'" :model-value="Number(valueFor(control))" :min="control.min" :max="control.max" :step="control.step" @update:model-value="emit('change', control, $event)" />
              <PrimeInputText v-else :model-value="String(valueFor(control))" @update:model-value="emit('change', control, $event)" />
              <small>{{ effectFor(control) }}</small>
            </label>
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel v-if="slots.length" value="slots">
          <div class="control-list">
            <label v-for="slot in slots" :key="slot.id" class="slot-row">
              <span class="slot-title">{{ slot.title }}<em>{{ slot.kind }}</em></span>
              <span class="slot-toggle">
                {{ slotValues[slot.id]?.enabled ? labText(locale, 'slotEnabled') : labText(locale, 'slotDisabled') }}
                <PrimeToggleSwitch :model-value="Boolean(slotValues[slot.id]?.enabled)" @update:model-value="emit('slotChange', slot.id, { enabled: Boolean($event) })" />
              </span>
              <PrimeInputText v-if="slotValues[slot.id]?.enabled" :model-value="slotValues[slot.id]?.content ?? ''" :placeholder="`data-lab-slot=&quot;${slot.id}&quot;`" @update:model-value="emit('slotChange', slot.id, { content: String($event) || null })" />
            </label>
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel v-if="sceneControls.length" value="scene">
          <div class="control-list">
            <label v-for="control in sceneControls" :key="control.id">
              <span>{{ controlLabel(locale, control.labelKey) }}</span>
              <PrimeSlider v-if="control.type === 'range'" :model-value="Number(valueFor(control))" :min="control.min" :max="control.max" :step="control.step" @update:model-value="emit('change', control, $event)" />
              <PrimeInputText v-else :model-value="String(valueFor(control))" @update:model-value="emit('change', control, $event)" />
              <small>{{ effectFor(control) }}</small>
            </label>
          </div>
        </PrimeTabPanel>
        <PrimeTabPanel v-if="motions.length" value="motion">
          <p>{{ motions.length }} {{ labText(locale, 'declared') }}</p>
        </PrimeTabPanel>
        <PrimeTabPanel value="validation">
          <dl>
            <dt>{{ labText(locale, 'implementation') }}</dt><dd>{{ component.implementation.path }}</dd>
            <dt>{{ labText(locale, 'reducedMotion') }}</dt><dd>{{ component.conformance.reducedMotion ? labText(locale, 'declared') : labText(locale, 'missing') }}</dd>
            <dt>{{ labText(locale, 'accessibility') }}</dt><dd>{{ component.conformance.accessibility ? labText(locale, 'declared') : labText(locale, 'missing') }}</dd>
          </dl>
        </PrimeTabPanel>
      </PrimeTabPanels>
    </PrimeTabs>
    <footer>
      <button type="button" @click="emit('resetPage')">{{ labText(locale, 'resetPage') }}</button>
      <button type="button" @click="emit('resetAll')">{{ labText(locale, 'resetAll') }}</button>
    </footer>
  </aside>
</template>

<style scoped>
.inspector { display: grid; grid-template-rows: auto minmax(0, 1fr) auto; min-width: 0; min-height: 0; border-left: 1px solid var(--border-strong); background: var(--bg-surface); }
header { display: flex; justify-content: space-between; gap: 8px; padding: 10px; border-bottom: 1px solid var(--border-base); font-family: var(--font-mono); font-size: var(--font-size-xs); }
header small { overflow: hidden; color: var(--text-tertiary); text-overflow: ellipsis; white-space: nowrap; }
.control-list { display: grid; gap: 12px; padding: 12px; }
.control-list label { display: grid; gap: 6px; font-size: var(--font-size-xs); }
.control-list small { color: var(--text-tertiary); line-height: 1.4; }
.slot-title { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.slot-title em { font-style: normal; color: var(--text-tertiary); font-size: var(--font-size-caption); text-transform: uppercase; }
.slot-toggle { display: flex; align-items: center; justify-content: space-between; gap: 8px; color: var(--text-secondary); }
p, dl { padding: 12px; margin: 0; color: var(--text-secondary); font-size: var(--font-size-xs); line-height: 1.5; }
dt { color: var(--text-tertiary); }
dd { margin: 2px 0 9px; overflow-wrap: anywhere; }
footer { display: grid; grid-template-columns: 1fr 1fr; gap: 1px; border-top: 1px solid var(--border-base); }
footer button { padding: 9px; border: 0; background: var(--bg-field); color: var(--text-secondary); cursor: pointer; }
footer button:hover { color: var(--text-primary); }
</style>
