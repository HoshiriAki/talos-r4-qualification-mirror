<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import { useRouter } from 'vue-router'
import type { DashboardStats, DashboardRouteName } from '@/api/dashboard'
import { buildHudScene, buildWorkflowSteps, type HudSceneModule } from './hudSceneModel'

const props = defineProps<{ model: DashboardStats | null }>()
const emit = defineEmits<{ focusChange: [focused: boolean] }>()
const router = useRouter()
const selectedId = ref<string | null>(null)
const focusCloseButton = ref<HTMLButtonElement | null>(null)
const nodeRefs = new Map<string, SVGElement>()

interface SvgHudSceneModule extends HudSceneModule {
  anchorX: number
  anchorY: number
  topPoints: string
  leftPoints: string
  rightPoints: string
  beaconX: number
  beaconY: number
}

function pointString(...values: Array<[number, number]>): string {
  return values.map(([x, y]) => `${Math.round(x)},${Math.round(y)}`).join(' ')
}

function toSvgModule(node: HudSceneModule): SvgHudSceneModule {
  const anchorX = node.position.x
  const anchorY = node.position.y
  const halfWidth = node.width / 2
  const halfDepth = node.depth / 2
  const topY = anchorY - node.height
  return {
    ...node,
    anchorX,
    anchorY,
    topPoints: pointString([anchorX, topY - halfDepth], [anchorX + halfWidth, topY], [anchorX, topY + halfDepth], [anchorX - halfWidth, topY]),
    leftPoints: pointString([anchorX - halfWidth, topY], [anchorX, topY + halfDepth], [anchorX, anchorY + halfDepth], [anchorX - halfWidth, anchorY]),
    rightPoints: pointString([anchorX + halfWidth, topY], [anchorX, topY + halfDepth], [anchorX, anchorY + halfDepth], [anchorX + halfWidth, anchorY]),
    beaconX: anchorX,
    beaconY: topY - halfDepth - 12,
  }
}

const modules = computed<SvgHudSceneModule[]>(() => buildHudScene(props.model?.sceneNodes ?? []).map(toSvgModule))
const selected = computed(() => modules.value.find(node => node.id === selectedId.value) ?? null)
const workflow = computed(() => selected.value ? buildWorkflowSteps(selected.value) : [])
const totalCount = computed(() => modules.value.reduce((sum, node) => sum + node.count, 0))
const sceneViewBox = computed(() => {
  const requiredHeight = modules.value.reduce((max, node) => Math.max(max, node.anchorY + 100), 520)
  return `0 0 920 ${requiredHeight}`
})

const ROUTE_MAP: Record<DashboardRouteName, string> = {
  customers: '/app/orders',
  devices: '/app/devices',
  dashboard: '/',
}

function tone(status: string) {
  if (status === 'critical') return 'critical'
  if (status === 'warning') return 'warning'
  return 'normal'
}

function setNodeRef(id: string, element: unknown) {
  if (element instanceof SVGElement) nodeRefs.set(id, element)
  else nodeRefs.delete(id)
}

async function select(node: HudSceneModule) {
  if (selectedId.value === node.id) {
    await clearSelection()
    return
  }
  selectedId.value = node.id
  emit('focusChange', selectedId.value !== null)
  await nextTick()
  focusCloseButton.value?.focus()
}

async function clearSelection() {
  const previousId = selectedId.value
  selectedId.value = null
  emit('focusChange', false)
  await nextTick()
  if (previousId) nodeRefs.get(previousId)?.focus()
}

function activateFromKeyboard(event: KeyboardEvent, node: HudSceneModule) {
  if (event.key !== 'Enter' && event.key !== ' ') return
  event.preventDefault()
  void select(node)
}

async function openSelected() {
  if (!selected.value) return
  const route = selected.value.route || { name: 'dashboard' as const }
  await router.push({ path: ROUTE_MAP[route.name], query: route.query || {} })
}
</script>

<template>
  <section class="operations-field" :class="{ 'is-focused': selected }">
    <header class="field-directive" aria-live="polite">
      <div class="directive-index">{{ selected ? 'FOCUS / 02' : 'FIELD / 01' }}</div>
      <div class="directive-copy">
        <span class="directive-kicker">{{ selected ? '当前行动对象' : '自动化运营地图' }}</span>
        <strong>{{ selected?.label ?? '设备与归还态势' }}</strong>
      </div>
      <div class="directive-metric">
        <span>{{ selected ? selected.count : totalCount }}</span>
        <small>{{ selected ? '关联项' : '地图对象' }}</small>
      </div>
    </header>

    <div class="field-canvas">
      <div class="field-axis field-axis--x" aria-hidden="true">X / OPERATIONS</div>
      <div class="field-axis field-axis--y" aria-hidden="true">Y / CAPACITY</div>

      <svg :viewBox="sceneViewBox" class="hud-scene-svg" aria-label="实时运营轴测地图">
        <defs>
          <pattern id="hudDotGrid" width="18" height="18" patternUnits="userSpaceOnUse">
            <circle cx="1" cy="1" r="0.7" fill="var(--hud-grid-dot)" />
          </pattern>
          <pattern id="hudIsoGrid" width="72" height="36" patternUnits="userSpaceOnUse" patternTransform="skewY(26.565)">
            <path d="M 72 0 L 0 0 0 36" fill="none" stroke="var(--hud-grid-line)" stroke-width="0.7" />
          </pattern>
          <filter id="hudHardShadow" x="-20%" y="-20%" width="160%" height="160%">
            <feDropShadow dx="5" dy="5" stdDeviation="0" flood-color="var(--hud-shadow)" flood-opacity="0.9" />
          </filter>
        </defs>

        <rect width="920" height="520" fill="url(#hudDotGrid)" />
        <path class="terrain" d="M95 355 420 188 824 345 495 500Z" />
        <path class="terrain-grid" d="M95 355 420 188 824 345 495 500Z" fill="url(#hudIsoGrid)" />
        <path class="terrain-ridge" d="M108 354 420 202 810 346" />

        <g class="trace-layer" aria-hidden="true">
          <path d="M175 386 390 277 540 339 750 232" />
          <path d="M264 430 465 330 642 401" />
          <circle cx="175" cy="386" r="4" />
          <circle cx="750" cy="232" r="4" />
        </g>

        <g v-if="modules.length === 0" class="scene-empty" aria-live="polite">
          <rect x="285" y="210" width="350" height="110" />
          <text x="460" y="255">NO LIVE OBJECTS</text>
          <text x="460" y="282" class="scene-empty-sub">等待 Dashboard v2 场景节点</text>
        </g>

        <g
          v-for="node in modules"
          :key="node.id"
          class="iso-module"
          :class="[
            `is-${tone(node.status)}`,
            { 'is-selected': selectedId === node.id, 'is-dimmed': selected && selectedId !== node.id },
          ]"
          role="button"
          tabindex="0"
          :aria-pressed="selectedId === node.id"
          :aria-label="`${node.label}，${node.count}，状态 ${node.status}。选择以查看流程。`"
          :ref="element => setNodeRef(node.id, element)"
          @click="select(node)"
          @keydown="activateFromKeyboard($event, node)"
        >
          <path class="module-shadow" :d="`M ${node.anchorX - 58} ${node.anchorY + 14} l 58 30 58 -30 -58 -30 Z`" />
          <polygon class="module-face module-face--left" :points="node.leftPoints" />
          <polygon class="module-face module-face--right" :points="node.rightPoints" />
          <polygon class="module-face module-face--top" :points="node.topPoints" />
          <path class="module-cut" :d="`M ${node.anchorX - 30} ${node.anchorY - node.height} l 30 15 30 -15`" />
          <line class="beacon-stem" :x1="node.beaconX" :y1="node.beaconY + 5" :x2="node.beaconX" :y2="node.anchorY - node.height - 25" />
          <circle class="beacon-ring" :cx="node.beaconX" :cy="node.beaconY" r="10" />
          <circle class="beacon-core" :cx="node.beaconX" :cy="node.beaconY" r="4" />
          <text class="node-count" :x="node.anchorX" :y="node.anchorY - node.height + 5">{{ node.count }}</text>
          <text class="node-label" :x="node.anchorX" :y="node.anchorY + 58">{{ node.label }}</text>
          <text class="node-code" :x="node.anchorX" :y="node.anchorY + 75">{{ node.kind.toUpperCase() }}</text>
        </g>
      </svg>

      <div class="scene-legend" aria-label="状态图例">
        <span><i class="legend-mark legend-mark--normal"></i>正常</span>
        <span><i class="legend-mark legend-mark--warning"></i>注意</span>
        <span><i class="legend-mark legend-mark--critical"></i>阻断</span>
      </div>

      <aside v-if="selected" class="focus-console">
        <button ref="focusCloseButton" class="focus-close" type="button" aria-label="返回地图总览" @click="clearSelection">×</button>
        <span class="focus-eyebrow">OBJECT / {{ selected.kind }}</span>
        <div class="focus-heading">
          <strong>{{ selected.label }}</strong>
          <span :class="`status-${tone(selected.status)}`">{{ selected.status }}</span>
        </div>
        <div class="workflow-caption">SOP / REFERENCE · 非实时阶段</div>
        <div class="workflow" aria-label="对象处理参考步骤">
          <div v-for="(step, index) in workflow" :key="step.id" class="workflow-step" :class="`is-${step.state}`">
            <span>{{ String(index + 1).padStart(2, '0') }}</span>
            <i></i>
            <b>{{ step.label }}</b>
          </div>
        </div>
        <button class="open-surface" type="button" @click="openSelected">
          打开工作面 <span>↗</span>
        </button>
      </aside>

      <div class="field-watermark" aria-hidden="true">
        <span>{{ selected ? 'FOCUS' : 'ACTION' }}</span>
        <small>LIVE OPERATIONS / SHANGHAI</small>
      </div>
    </div>
  </section>
</template>

<style scoped>
.operations-field {
  --hud-grid-dot: color-mix(in srgb, var(--text-primary) 24%, transparent);
  --hud-grid-line: color-mix(in srgb, var(--text-primary) 18%, transparent);
  --hud-shadow: var(--bg-base);
  position: relative;
  min-width: 0;
  height: 100%;
  overflow: hidden;
  border: 0;
  border-radius: 0;
  background: transparent;
  box-shadow: none;
}
.field-directive {
  position: absolute;
  top: 18px;
  left: 50%;
  z-index: 4;
  display: grid;
  grid-template-columns: auto minmax(180px, 1fr) auto;
  align-items: stretch;
  width: min(540px, calc(100% - 48px));
  min-height: 70px;
  transform: translateX(-50%);
  border: 2px solid var(--bg-base);
  border-radius: 0 12px 0 12px;
  background: var(--text-primary);
  box-shadow: 4px 4px 0 var(--bg-base);
}
.directive-index {
  display: grid;
  place-items: center;
  padding: 0 14px;
  border-right: 2px solid var(--bg-base);
  color: var(--bg-base);
  font: 700 11px/1 var(--font-mono);
  letter-spacing: .12em;
  writing-mode: vertical-rl;
  transform: rotate(180deg);
}
.directive-copy { display: flex; flex-direction: column; justify-content: center; padding: 12px 16px; }
.directive-kicker { color: color-mix(in srgb, var(--bg-base) 65%, transparent); font: 600 11px/1.2 var(--font-mono); letter-spacing: .12em; text-transform: uppercase; }
.directive-copy strong { margin-top: 6px; color: var(--bg-base); font: 700 clamp(16px, 2vw, 23px)/1 var(--font-mono); letter-spacing: -.03em; }
.directive-metric { display: flex; flex-direction: column; justify-content: center; min-width: 88px; padding: 10px 14px; border-left: 2px solid var(--bg-base); text-align: right; }
.directive-metric span { color: var(--accent); font: 700 26px/1 var(--font-mono); }
.directive-metric small { margin-top: 5px; color: var(--bg-base); font: 11px/1 var(--font-sans); }
.field-canvas { position: relative; width: 100%; height: 100%; min-height: 520px; }
.hud-scene-svg { width: 100%; height: 100%; min-height: 520px; }
.terrain { fill: color-mix(in srgb, var(--text-primary) 18%, transparent); stroke: color-mix(in srgb, var(--text-primary) 55%, transparent); stroke-width: 1.3; }
.terrain-grid { opacity: .72; }
.terrain-ridge { fill: none; stroke: var(--bg-base); stroke-width: 9; opacity: .3; }
.trace-layer path { fill: none; stroke: var(--bg-base); stroke-width: 1.4; stroke-dasharray: 3 7; opacity: .55; }
.trace-layer circle { fill: var(--text-primary); opacity: .85; }
.iso-module { cursor: pointer; transform-box: fill-box; transform-origin: center bottom; transition: opacity 200ms ease, transform 420ms cubic-bezier(.2,.8,.2,1); outline: none; }
.iso-module:hover, .iso-module:focus-visible { transform: translateY(-8px); }
.iso-module:focus-visible .module-face--top { stroke: var(--accent); stroke-width: 3; }
.iso-module.is-selected { transform: translateY(-20px) scale(1.05); }
.iso-module.is-dimmed { opacity: .18; }
.module-shadow { fill: var(--bg-base); opacity: .78; }
.module-face { stroke: var(--bg-base); stroke-width: 1.5; }
.module-face--top { fill: var(--text-primary); }
.module-face--left { fill: color-mix(in srgb, var(--text-primary) 60%, var(--bg-elevated)); }
.module-face--right { fill: color-mix(in srgb, var(--text-primary) 35%, var(--bg-base)); }
.module-cut { fill: none; stroke: var(--bg-base); stroke-width: 5; opacity: .78; }
.beacon-stem { stroke: var(--text-tertiary); stroke-width: 1; stroke-dasharray: 2 3; }
.beacon-ring { fill: var(--bg-base); stroke: var(--text-primary); stroke-width: 1.4; }
.beacon-core { fill: var(--status-success); }
.is-warning .beacon-core { fill: var(--status-warning); }
.is-critical .beacon-ring { stroke: var(--accent); }
.is-critical .beacon-core { fill: var(--accent); }
.node-count { fill: var(--bg-base); font: 700 18px/1 var(--font-mono); text-anchor: middle; dominant-baseline: middle; pointer-events: none; }
.node-label { fill: var(--text-primary); font: 700 12px/1 var(--font-sans); text-anchor: middle; paint-order: stroke; stroke: color-mix(in srgb, var(--bg-base) 45%, transparent); stroke-width: 2px; pointer-events: none; }
.node-code { fill: color-mix(in srgb, var(--text-primary) 76%, transparent); font: 11px/1 var(--font-mono); letter-spacing: .08em; text-anchor: middle; pointer-events: none; }
.scene-empty rect { fill: var(--bg-elevated); stroke: var(--border-base); stroke-dasharray: 5 5; }
.scene-empty text { fill: var(--text-secondary); font: 700 16px/1 var(--font-mono); text-anchor: middle; }
.scene-empty .scene-empty-sub { fill: var(--text-tertiary); font: 11px/1 var(--font-sans); }
.field-axis { position: absolute; z-index: 2; color: var(--text-primary); font: 11px/1 var(--font-mono); letter-spacing: .14em; opacity: .72; pointer-events: none; }
.field-axis--x { left: 18px; bottom: 14px; }
.field-axis--y { top: 112px; left: 12px; writing-mode: vertical-rl; }
.scene-legend { position: absolute; left: 50%; bottom: 28px; z-index: 3; display: flex; gap: 16px; transform: translateX(-50%); color: var(--text-primary); font: 700 11px/1 var(--font-sans); }
.scene-legend span { display: inline-flex; align-items: center; gap: 6px; }
.legend-mark { width: 8px; height: 8px; border-radius: 4px; background: var(--status-success); }
.legend-mark--warning { background: var(--status-warning); }
.legend-mark--critical { background: var(--accent); }
.focus-console { position: absolute; right: 18px; bottom: 18px; z-index: 5; width: min(330px, calc(100% - 36px)); padding: 18px; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--bg-elevated); box-shadow: 5px 5px 0 var(--bg-base); }
.focus-close { position: absolute; top: 10px; right: 10px; width: 36px; height: 36px; border: 1px solid transparent; border-radius: 8px; background: transparent; color: var(--text-secondary); font: 20px/1 var(--font-mono); cursor: pointer; }
.focus-close:hover, .focus-close:focus-visible { border-color: var(--accent); color: var(--text-primary); }
.focus-eyebrow { display: block; padding-right: 38px; color: var(--text-tertiary); font: 11px/1 var(--font-mono); letter-spacing: .12em; text-transform: uppercase; }
.focus-heading { display: flex; align-items: end; justify-content: space-between; gap: 12px; margin: 10px 0 18px; }
.focus-heading strong { color: var(--text-primary); font: 700 19px/1.1 var(--font-mono); }
.focus-heading span { padding: 4px 10px; border: 1px solid currentColor; border-radius: 10px; font: 11px/1 var(--font-mono); text-transform: uppercase; }
.status-normal { color: var(--status-success); }
.status-warning { color: var(--status-warning); }
.status-critical { color: var(--accent); }
.workflow { display: grid; grid-template-columns: repeat(4, 1fr); gap: 4px; }
.workflow-caption { margin-bottom: 8px; color: var(--text-tertiary); font: 11px/1 var(--font-mono); letter-spacing: .08em; }
.workflow-step { position: relative; display: grid; gap: 6px; color: var(--text-tertiary); font-family: var(--font-mono); }
.workflow-step span { font-size: 11px; }
.workflow-step i { height: 4px; border-radius: 4px; background: var(--border-strong); }
.workflow-step b { font-size: 11px; font-weight: 500; }
.workflow-step.is-done i { background: var(--text-secondary); }
.workflow-step.is-current { color: var(--text-primary); }
.workflow-step.is-current i { background: var(--accent); }
.open-surface { display: flex; align-items: center; justify-content: space-between; width: 100%; min-height: 44px; margin-top: 18px; padding: 0 14px; border: 1px solid var(--accent); border-radius: 10px; background: var(--accent); color: var(--btn-primary-text); font: 700 11px/1 var(--font-mono); letter-spacing: .05em; cursor: pointer; box-shadow: 3px 3px 0 color-mix(in srgb, var(--accent) 35%, var(--bg-base)); }
.open-surface:hover { background: var(--accent-hover); }
.field-watermark { position: absolute; right: 24px; top: 112px; display: flex; flex-direction: column; align-items: end; pointer-events: none; }
.field-watermark span { color: var(--text-primary); font: 700 clamp(54px, 9vw, 132px)/.8 var(--font-mono); letter-spacing: -.09em; opacity: .2; }
.field-watermark small { margin-top: 14px; color: var(--text-primary); font: 700 11px/1 var(--font-mono); letter-spacing: .13em; }
@media (max-width: 900px) {
  .field-directive { left: 18px; width: calc(100% - 36px); transform: none; }
  .field-watermark { display: none; }
  .scene-legend { bottom: 18px; }
}
@media (prefers-reduced-motion: reduce) {
  .iso-module { transition: none; }
}
</style>
