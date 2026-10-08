<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import type { DashboardStats } from '@/api/dashboard'

const props = defineProps<{ model: DashboardStats | null }>()

const svgContainer = ref<SVGSVGElement | null>(null)
const sceneReady = ref(false)

interface IsoBlock {
  col: number
  row: number
  heightRatio: number
  dark: boolean
  deviceId?: string
  clusterLabel?: string
  status?: 'normal' | 'warning' | 'critical'
}

interface BeaconNode {
  col: number
  row: number
  heightRatio: number
  isAlert: boolean
  label?: string
}

// Constants matched to the HTML design
const TILE_W = 74
const TILE_H = 37
const ORIGIN_X = 760
const ORIGIN_Y = 280

const TOP = '#E9E9EB'
const LEFT = '#B7B7BC'
const RIGHT = '#87878C'
const TOP_D = '#CFCFD2'
const LEFT_D = '#9C9CA1'
const RIGHT_D = '#6F6F74'

function project(col: number, row: number): [number, number] {
  return [ORIGIN_X + (col - row) * (TILE_W / 2), ORIGIN_Y + (col + row) * (TILE_H / 2)]
}

const layout = computed<IsoBlock[]>(() => {
  // Base layout from the reference design
  const base: IsoBlock[] = [
    [0, 0, 1.0, 0], [1, 0, 1.0, 0], [2, 0, 1.6, 0], [3, 0, 1.0, 0],
    [0, 1, 1.0, 0], [1, 1, 0.6, 1], [2, 1, 1.6, 0], [3, 1, 1.0, 0],
    [0, 2, 0.6, 1], [1, 2, 1.0, 0], [2, 2, 1.0, 0], [3, 2, 0.6, 1],
    [1, 3, 0.6, 1], [2, 3, 0.6, 1],
  ].map(([col, row, hRatio, dark]) => ({ col: +col!, row: +row!, heightRatio: +hRatio!, dark: !!dark }))

  // Map data to blocks if we have sceneNodes
  const nodes = props.model?.sceneNodes ?? []
  if (nodes.length > 0) {
    return base.map((block, i) => {
      const node = nodes[i % nodes.length]
      if (!node) return block
      return {
        ...block,
        clusterLabel: node.kind === 'device-cluster' ? node.label : undefined,
        status: node.status as 'normal' | 'warning' | 'critical' | undefined,
      }
    })
  }
  return base
})

const beaconNodes = computed<BeaconNode[]>(() => {
  const base: BeaconNode[] = [
    { col: 1, row: 0, heightRatio: 1.6, isAlert: false },
    { col: 2, row: 1, heightRatio: 1.9, isAlert: false },
    { col: 1, row: 2, heightRatio: 1.0, isAlert: false },
    { col: 3, row: 1, heightRatio: 1.0, isAlert: false },
    { col: 0, row: 1, heightRatio: 1.0, isAlert: false },
  ]

  // Mark the middle beacon as alert if there are overdue returns
  const hasOverdue = (props.model?.overdueReturns ?? 0) > 0
  return base.map((b, i) => ({
    ...b,
    isAlert: i === 2 && hasOverdue,
    label: i === 2 && hasOverdue ? 'OVERDUE' : undefined,
  }))
})

const beaconPts = computed(() =>
  beaconNodes.value.map(({ col, row, heightRatio }) => {
    const [x, y] = project(col, row)
    return [x, y - heightRatio * 60] as [number, number]
  })
)

const pinMarkers = computed(() => {
  const markers: Array<[number, number]> = []
  for (let i = 0; i < 8; i++) {
    markers.push(project(4.4, -1.2 + i * 0.5))
  }
  return markers
})

const groundDots = computed(() => {
  const dots: Array<[number, number]> = []
  for (let col = -1; col <= 1; col++) {
    for (let row = 4; row <= 7; row++) {
      dots.push(project(col, row))
    }
  }
  return dots
})

const gridXTicks = computed(() => {
  const lines: Array<{ x1: number; y1: number; x2: number; y2: number }> = []
  for (let i = -2; i <= 10; i++) {
    const [ax, ay] = project(i, -2)
    const [bx, by] = project(i, 10)
    lines.push({ x1: ax, y1: ay, x2: bx, y2: by })
  }
  return lines
})

const gridYTicks = computed(() => {
  const lines: Array<{ x1: number; y1: number; x2: number; y2: number }> = []
  for (let i = -2; i <= 10; i++) {
    const [ax, ay] = project(-2, i)
    const [bx, by] = project(10, i)
    lines.push({ x1: ax, y1: ay, x2: bx, y2: by })
  }
  return lines
})

const ridgeLine = computed(() => {
  const p1 = project(0, 0)
  const p2 = project(3, 0)
  return { x1: p1[0], y1: p1[1] - 96, x2: p2[0], y2: p2[1] - 96 }
})

const hasOverdue = computed(() => (props.model?.overdueReturns ?? 0) > 0)

// ── Iso face point generators ───────────────────────────────────
function topFacePoints(col: number, row: number, height: number): string {
  const [x0, y0] = project(col, row)
  const hw = TILE_W / 2
  const hh = TILE_H / 2
  const top = [x0, y0 - hh - height]
  const rightPt = [x0 + hw, y0 - height]
  const bottom = [x0, y0 + hh - height]
  const leftPt = [x0 - hw, y0 - height]
  return [top, rightPt, bottom, leftPt].map(([x, y]) => `${Math.round(x)},${Math.round(y)}`).join(' ')
}

function leftFacePoints(col: number, row: number, height: number): string {
  const [x0, y0] = project(col, row)
  const hw = TILE_W / 2
  const hh = TILE_H / 2
  const leftPt = [x0 - hw, y0 - height]
  const bottom = [x0, y0 + hh - height]
  const groundBottom = [x0, y0 + hh]
  const groundLeft = [x0 - hw, y0]
  return [leftPt, bottom, groundBottom, groundLeft].map(([x, y]) => `${Math.round(x)},${Math.round(y)}`).join(' ')
}

function rightFacePoints(col: number, row: number, height: number): string {
  const [x0, y0] = project(col, row)
  const hw = TILE_W / 2
  const hh = TILE_H / 2
  const rightPt = [x0 + hw, y0 - height]
  const bottom = [x0, y0 + hh - height]
  const groundBottom = [x0, y0 + hh]
  const groundRight = [x0 + hw, y0]
  return [rightPt, bottom, groundBottom, groundRight].map(([x, y]) => `${Math.round(x)},${Math.round(y)}`).join(' ')
}

onMounted(() => {
  sceneReady.value = true
})

onUnmounted(() => {
  sceneReady.value = false
})
</script>

<template>
  <svg
    ref="svgContainer"
    class="hud-iso-scene"
    viewBox="0 0 1400 900"
    preserveAspectRatio="xMidYMid meet"
    xmlns="http://www.w3.org/2000/svg"
    aria-label="仓库运营等角投影地图"
    :class="{ ready: sceneReady }"
  >
    <defs>
      <filter id="hudIsoShadow" x="-10%" y="-10%" width="130%" height="140%">
        <feDropShadow dx="2" dy="3" stdDeviation="0" flood-color="#0B0B0D" flood-opacity="0.6" />
      </filter>
    </defs>

    <!-- Grid lines -->
    <g class="grid-layer" aria-hidden="true">
      <line
        v-for="(l, i) in gridXTicks" :key="'gx' + i"
        :x1="l.x1" :y1="l.y1" :x2="l.x2" :y2="l.y2"
        stroke="rgba(255,255,255,0.04)" stroke-width="1"
      />
      <line
        v-for="(l, i) in gridYTicks" :key="'gy' + i"
        :x1="l.x1" :y1="l.y1" :x2="l.x2" :y2="l.y2"
        stroke="rgba(255,255,255,0.04)" stroke-width="1"
      />
    </g>

    <!-- Ground scatter dots (behind blocks) -->
    <g class="ground-dots" aria-hidden="true">
      <circle
        v-for="([cx, cy], i) in groundDots" :key="'gd' + i"
        :cx="cx" :cy="cy" r="2" fill="#242427"
      />
    </g>

    <!-- Iso blocks -->
    <g class="iso-blocks" filter="url(#hudIsoShadow)">
      <g
        v-for="(block, i) in layout"
        :key="'block' + i"
        class="iso-block"
        :class="{
          'is-dark': block.dark,
          'is-warning': block.status === 'warning',
          'is-critical': block.status === 'critical',
        }"
      >
        <polygon
          class="face-top"
          :points="topFacePoints(block.col, block.row, block.heightRatio * 60)"
          :fill="block.dark ? TOP_D : TOP"
          stroke="#0B0B0D" stroke-width="1.3"
        />
        <polygon
          class="face-left"
          :points="leftFacePoints(block.col, block.row, block.heightRatio * 60)"
          :fill="block.dark ? LEFT_D : LEFT"
          stroke="#0B0B0D" stroke-width="1.3"
        />
        <polygon
          class="face-right"
          :points="rightFacePoints(block.col, block.row, block.heightRatio * 60)"
          :fill="block.dark ? RIGHT_D : RIGHT"
          stroke="#0B0B0D" stroke-width="1.3"
        />
      </g>
    </g>

    <!-- Ridge beam -->
    <line
      class="ridge-beam"
      :x1="ridgeLine.x1" :y1="ridgeLine.y1"
      :x2="ridgeLine.x2" :y2="ridgeLine.y2"
      stroke="#0B0B0D" stroke-width="10"
    />

    <!-- Beacon connection lines -->
    <g class="beacon-lines" aria-hidden="true">
      <line
        v-for="(_, i) in beaconPts.slice(0, -1)"
        :key="'bl' + i"
        :x1="beaconPts[i][0]" :y1="beaconPts[i][1]"
        :x2="beaconPts[i + 1][0]" :y2="beaconPts[i + 1][1]"
        stroke="#3D3D42" stroke-width="1" stroke-dasharray="1 4"
      />
    </g>

    <!-- Beacon nodes -->
    <g class="beacon-nodes">
      <circle
        v-for="([cx, cy], i) in beaconPts"
        :key="'bn' + i"
        :cx="cx" :cy="cy"
        :r="beaconNodes[i].isAlert ? 7 : 5"
        :fill="beaconNodes[i].isAlert ? '#FF453A' : '#0B0B0D'"
        :stroke="beaconNodes[i].isAlert ? '#FF6B5A' : '#E9E9EB'"
        stroke-width="1.5"
        :class="{ 'beacon-alert': beaconNodes[i].isAlert }"
      />
    </g>

    <!-- Pin markers (right side) -->
    <g class="pin-markers" aria-hidden="true">
      <template v-for="([px, py], i) in pinMarkers" :key="'pm' + i">
        <line :x1="px" :y1="py" :x2="px" :y2="py - 16" stroke="#3D3D42" stroke-width="1.5" />
        <circle :cx="px" :cy="py - 16" r="3" fill="none" stroke="#57575E" stroke-width="1.3" />
      </template>
    </g>
  </svg>
</template>

<style scoped>
.hud-iso-scene {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  transition: opacity var(--duration-slow) ease-out;
}
.hud-iso-scene.ready {
  opacity: 1;
}

.iso-block { transition: transform var(--duration-normal) ease-out; }
.iso-block:hover { transform: translateY(-6px); }
.iso-block.is-warning .face-top { fill: #FFE0A0; }
.iso-block.is-warning .face-left { fill: #D4B896; }
.iso-block.is-warning .face-right { fill: #A89070; }
.iso-block.is-critical .face-top { fill: #FFC0C0; }
.iso-block.is-critical .face-left { fill: #D4A0A0; }
.iso-block.is-critical .face-right { fill: #A88080; }

.beacon-alert {
  animation: beacon-blink 1.6s ease-in-out infinite;
}
@keyframes beacon-blink {
  0%, 100% { fill: #FF453A; stroke: #FF6B5A; }
  50% { fill: #8B2520; stroke: #FF453A; }
}

@media (prefers-reduced-motion: reduce) {
  .hud-iso-scene { transition: none; }
  .iso-block { transition: none; }
  .beacon-alert { animation: none; fill: #FF453A; stroke: #FF6B5A; }
}
</style>
