<script setup lang="ts">
import { onMounted, ref } from 'vue'

interface Props {
  eyebrow?: string
  title: string
  subtitle?: string
  chipTopLabel?: string
  chipTopValue?: string
  chipBottomLabel?: string
  chipBottomValue?: string
  chipBottomAlert?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  eyebrow: '',
  subtitle: '',
  chipTopLabel: '',
  chipTopValue: '',
  chipBottomLabel: '',
  chipBottomValue: '',
  chipBottomAlert: false,
})

const svgRef = ref<SVGSVGElement | null>(null)

onMounted(() => {
  if (!svgRef.value) return

  const svg = svgRef.value
  const ns = 'http://www.w3.org/2000/svg'
  const tw = 66, th = 33
  const originX = 620, originY = 260

  function project(c: number, r: number): [number, number] {
    return [originX + (c - r) * (tw / 2), originY + (c + r) * (th / 2)]
  }

  function isoCube(c: number, r: number, height: number, tones: [string, string, string]) {
    const [x0, y0] = project(c, r)
    const hw = tw / 2, hh = th / 2
    const top = [x0, y0 - hh - height]
    const right = [x0 + hw, y0 - height]
    const bottom = [x0, y0 + hh - height]
    const left = [x0 - hw, y0 - height]
    const groundBottom = [x0, y0 + hh]
    const groundLeft = [x0 - hw, y0]
    const groundRight = [x0 + hw, y0]
    const g = document.createElementNS(ns, 'g')

    const mk = (pts: number[][], fill: string) => {
      const p = document.createElementNS(ns, 'polygon')
      p.setAttribute('points', pts.map(pt => pt.join(',')).join(' '))
      p.setAttribute('fill', fill)
      p.setAttribute('stroke', '#0B0B0D')
      p.setAttribute('stroke-width', '1.2')
      g.appendChild(p)
    }

    mk([top, right, bottom, left], tones[0])
    mk([left, bottom, groundBottom, groundLeft], tones[1])
    mk([right, bottom, groundBottom, groundRight], tones[2])
    svg.appendChild(g)
  }

  const TOP = '#E9E9EB', LEFT = '#B7B7BC', RIGHT = '#87878C'
  const TOP_D = '#CFCFD2', LEFT_D = '#9C9CA1', RIGHT_D = '#6F6F74'

  const layout: [number, number, number, number][] = [
    [0, 0, 1.0, 0], [1, 0, 1.0, 0], [2, 0, 1.5, 0], [3, 0, 1.0, 0],
    [0, 1, 1.0, 0], [1, 1, 0.6, 1], [2, 1, 1.5, 0], [3, 1, 1.0, 0],
    [0, 2, 0.6, 1], [1, 2, 1.0, 0], [2, 2, 1.0, 0], [3, 2, 0.6, 1],
    [1, 3, 0.6, 1], [2, 3, 0.6, 1],
  ]

  layout.forEach(([c, r, hRatio, dark]) => {
    isoCube(c, r, hRatio * 54, dark ? [TOP_D, LEFT_D, RIGHT_D] : [TOP, LEFT, RIGHT])
  })

  // Ridge line
  const p1 = project(0, 0), p2 = project(3, 0)
  const ridge = document.createElementNS(ns, 'line')
  ridge.setAttribute('x1', String(p1[0]))
  ridge.setAttribute('y1', String(p1[1] - 86))
  ridge.setAttribute('x2', String(p2[0]))
  ridge.setAttribute('y2', String(p2[1] - 86))
  ridge.setAttribute('stroke', '#0B0B0D')
  ridge.setAttribute('stroke-width', '9')
  svg.appendChild(ridge)

  // Bolt positions and guide lines
  const boltPositions: [number, number, number][] = [[1, 0, 1.5], [2, 1, 1.8], [1, 2, 1.0], [3, 1, 1.0], [0, 1, 1.0]]
  const boltPts = boltPositions.map(([c, r, hR]) => {
    const [x, y] = project(c, r)
    return [x, y - hR * 54]
  })

  for (let i = 0; i < boltPts.length - 1; i++) {
    const l = document.createElementNS(ns, 'line')
    l.setAttribute('x1', String(boltPts[i][0]))
    l.setAttribute('y1', String(boltPts[i][1]))
    l.setAttribute('x2', String(boltPts[i + 1][0]))
    l.setAttribute('y2', String(boltPts[i + 1][1]))
    l.setAttribute('stroke', '#3D3D42')
    l.setAttribute('stroke-width', '1')
    l.setAttribute('stroke-dasharray', '1 4')
    svg.appendChild(l)
  }

  boltPts.forEach(([x, y], i) => {
    const isAlert = i === 2
    const c = document.createElementNS(ns, 'circle')
    c.setAttribute('cx', String(x))
    c.setAttribute('cy', String(y))
    c.setAttribute('r', isAlert ? '6' : '4.5')
    c.setAttribute('fill', isAlert ? '#FF453A' : '#0B0B0D')
    c.setAttribute('stroke', '#E9E9EB')
    c.setAttribute('stroke-width', '1.3')
    svg.appendChild(c)
  })

  // Ground dots
  for (let c = -1; c <= 1; c++) {
    for (let r = 4; r <= 6; r++) {
      const [x, y] = project(c, r)
      const d = document.createElementNS(ns, 'circle')
      d.setAttribute('cx', String(x))
      d.setAttribute('cy', String(y))
      d.setAttribute('r', '2')
      d.setAttribute('fill', '#242427')
      svg.appendChild(d)
    }
  }
})
</script>

<template>
  <div class="hero-banner">
    <!-- Grid background -->
    <div class="hero-grid"></div>

    <!-- Isometric 3D scene -->
    <svg ref="svgRef" class="iso-scene" viewBox="0 0 1400 900" preserveAspectRatio="xMidYMid meet"></svg>

    <!-- Radial mask overlay -->
    <div class="hero-overlay"></div>

    <!-- Text content (left) -->
    <div class="hero-copy">
      <div>
        <div v-if="eyebrow" class="hero-eyebrow">{{ eyebrow }}</div>
        <div class="hero-title">
          {{ title }}
          <small v-if="subtitle">{{ subtitle }}</small>
        </div>
      </div>
    </div>

    <!-- Data chips (right) -->
    <div v-if="chipTopLabel" class="hero-chip chip-tl">
      <div>
        <div class="lab">{{ chipTopLabel }}</div>
        <div class="val">{{ chipTopValue }}</div>
      </div>
    </div>

    <div v-if="chipBottomLabel" class="hero-chip chip-bl">
      <div>
        <div class="lab">{{ chipBottomLabel }}</div>
        <div class="val" :class="{ 'val-alert': chipBottomAlert }">{{ chipBottomValue }}</div>
      </div>
      <span v-if="chipBottomAlert" class="chip-badge">!</span>
    </div>
  </div>
</template>

<style scoped>
/* ══════════════════════════════════════════════════════════════
   Hero Banner — 280px height, 12px radius, isometric 3D scene
   参考 standerd_mode_computer.html
   ══════════════════════════════════════════════════════════════ */
.hero-banner {
  position: relative;
  background: var(--bg-surface);
  border: 1px solid var(--border-base);
  border-radius: 12px;
  overflow: hidden;
  height: 280px;
  display: flex;
  align-items: stretch;
  box-shadow: var(--shadow-offset-default);
}

/* Grid texture (28×28px) */
.hero-grid {
  position: absolute;
  inset: 0;
  background-image:
    linear-gradient(var(--grid-line) 1px, transparent 1px),
    linear-gradient(90deg, var(--grid-line) 1px, transparent 1px);
  background-size: 28px 28px;
  pointer-events: none;
}

/* Isometric scene SVG */
.iso-scene {
  position: absolute;
  right: -40px;
  top: -20px;
  width: 640px;
  height: 340px;
  z-index: 1;
  pointer-events: none;
}

/* Radial gradient mask */
.hero-overlay {
  content: '';
  position: absolute;
  inset: 0;
  pointer-events: none;
  background: radial-gradient(90% 120% at 68% 40%, transparent 55%, rgba(0, 0, 0, 0.45) 100%);
  z-index: 2;
}

/* Text content area */
.hero-copy {
  position: relative;
  z-index: 3;
  padding: 28px 32px;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  max-width: 280px;
}

.hero-eyebrow {
  font-family: var(--font-mono);
  font-size: 11px;
  letter-spacing: 0.14em;
  color: var(--text-tertiary);
  text-transform: uppercase;
}

.hero-title {
  font-family: var(--font-sans);
  font-size: 40px;
  font-weight: 800;
  letter-spacing: -0.02em;
  margin-top: 10px;
  line-height: 1.05;
  color: var(--text-primary);
}

.hero-title small {
  display: block;
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 13px;
  color: var(--text-secondary);
  margin-top: 10px;
  letter-spacing: 0.02em;
}

/* Data chips */
.hero-chip {
  position: absolute;
  z-index: 3;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  border-radius: 9px;
  padding: 9px 13px;
  display: flex;
  align-items: center;
  gap: 8px;
  box-shadow: var(--shadow-offset-default);
}

.chip-tl {
  top: 24px;
  right: 32px;
}

.chip-bl {
  bottom: 24px;
  right: 32px;
}

.hero-chip .lab {
  font-size: 10px;
  color: var(--text-tertiary);
  letter-spacing: 0.06em;
  text-transform: uppercase;
  font-family: var(--font-mono);
}

.hero-chip .val {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 14px;
  margin-top: 1px;
  color: var(--text-primary);
}

.hero-chip .val-alert {
  color: var(--accent);
}

.chip-badge {
  min-width: 15px;
  height: 15px;
  padding: 0 4px;
  border-radius: 8px;
  background: var(--accent);
  color: #fff;
  font-size: 9px;
  font-family: var(--font-mono);
  font-weight: 700;
  display: flex;
  align-items: center;
  justify-content: center;
  margin-left: 6px;
}

/* Responsive */
@media (max-width: 1100px) {
  .hero-banner {
    height: auto;
    flex-direction: column;
  }

  .iso-scene {
    position: relative;
    width: 100%;
    height: 220px;
    right: 0;
    top: 0;
  }

  .hero-copy {
    max-width: none;
  }
}

@media (prefers-reduced-motion: reduce) {
  * {
    transition: none !important;
  }
}
</style>
