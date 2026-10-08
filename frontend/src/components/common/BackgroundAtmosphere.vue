<script setup lang="ts">
/**
 * BackgroundAtmosphere — Circuit board trace / subway route background
 *
 * PCB-style routes with right-angle turns + moving square data packets
 * + photoluminescent persistence trail (光学残留拖尾)
 *
 * Dark mode: additive blending (screen), multi-layer glow, white-hot core,
 *   exponential decay trail with 6 ghost layers simulating phosphor persistence.
 * Light mode: matte physical squares, no glow (like fluorescent under bright lights).
 *
 * Rules:
 *   - Eye-peripheral only — must never compete with content
 *   - Glow = dark mode only (photoluminescent persistence)
 *   - Light mode = matte squares only
 *   - Real network requests trigger faster movement
 */
import { ref, onMounted, onUnmounted } from 'vue'

const idleThreshold = 8000
let idleTimer: ReturnType<typeof setTimeout> | null = null
let observer: PerformanceObserver | null = null
const hasActivity = ref(false)

function onNetworkActivity() {
  hasActivity.value = true
  if (idleTimer) clearTimeout(idleTimer)
  idleTimer = setTimeout(() => {
    hasActivity.value = false
  }, idleThreshold)
}

onMounted(() => {
  if (typeof PerformanceObserver !== 'undefined') {
    try {
      observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries() as PerformanceResourceTiming[]) {
          if (entry.initiatorType === 'fetch' || entry.initiatorType === 'xmlhttprequest') {
            onNetworkActivity()
          }
        }
      })
      observer.observe({ entryTypes: ['resource'] })
    } catch {
      // PerformanceObserver not supported
    }
  }
})

onUnmounted(() => {
  if (idleTimer) clearTimeout(idleTimer)
  if (observer) observer.disconnect()
})
</script>

<template>
  <div class="bg-atmosphere" aria-hidden="true">
    <svg class="bg-circuit" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      <defs>
        <!-- ═══ Multi-layer glow for photoluminescent persistence ═══ -->
        <!-- Layer 1: broad bloom — the surrounding aura -->
        <filter id="glow-bloom" x="-200%" y="-200%" width="500%" height="500%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="12" result="bloom" />
        </filter>

        <!-- Layer 2: mid glow — the colored halo -->
        <filter id="glow-mid" x="-100%" y="-100%" width="300%" height="300%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        <!-- Layer 3: tight glow — sharp edge for white-hot core -->
        <filter id="glow-tight" x="-80%" y="-80%" width="260%" height="260%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="2" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>

        <!-- Active state: stronger bloom (network activity) -->
        <filter id="glow-bloom-active" x="-200%" y="-200%" width="500%" height="500%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="18" result="bloom" />
        </filter>

        <filter id="glow-mid-active" x="-100%" y="-100%" width="300%" height="300%">
          <feGaussianBlur in="SourceGraphic" stdDeviation="8" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <!-- ═══ Circuit board traces — right-angle routes ═══ -->
      <g class="traces">
        <!-- Route A: horizontal bus with vertical branches -->
        <polyline points="0,180 400,180 400,320 700,320 700,180 1200,180" class="trace" />
        <polyline points="0,540 300,540 300,400 600,400 600,540 900,540 900,400 1200,400" class="trace" />
        <polyline points="0,760 500,760 500,640 800,640 800,760 1400,760" class="trace" />

        <!-- Route B: vertical spines with horizontal taps -->
        <polyline points="200,0 200,240 360,240 360,480 200,480 200,720 360,720 360,1080" class="trace" />
        <polyline points="1000,0 1000,160 1160,160 1160,400 1000,400 1000,600 1160,600 1160,1080" class="trace" />
        <polyline points="1600,0 1600,300 1440,300 1440,540 1600,540 1600,800 1440,800 1440,1080" class="trace" />

        <!-- Route C: branches -->
        <polyline points="400,180 400,320" class="trace trace--branch" />
        <polyline points="700,320 700,180" class="trace trace--branch" />
        <polyline points="300,540 300,400" class="trace trace--branch" />
        <polyline points="600,400 600,540" class="trace trace--branch" />
        <polyline points="900,540 900,400" class="trace trace--branch" />
        <polyline points="200,240 360,240" class="trace trace--branch" />
        <polyline points="360,480 200,480" class="trace trace--branch" />
        <polyline points="1000,160 1160,160" class="trace trace--branch" />
        <polyline points="1160,400 1000,400" class="trace trace--branch" />

        <!-- Dashed secondary routes -->
        <polyline points="0,900 240,900 240,820 480,820 480,900 720,900" class="trace trace--dashed" />
        <polyline points="1200,180 1200,320 1360,320 1360,180 1200,180" class="trace trace--dashed" />
        <polyline points="800,640 800,760" class="trace trace--dashed" />
      </g>

      <!-- ═══ Junction nodes — 8×8px squares at intersections ═══ -->
      <g class="nodes">
        <rect x="396" y="176" width="8" height="8" rx="1" class="node" />
        <rect x="696" y="316" width="8" height="8" rx="1" class="node" />
        <rect x="696" y="176" width="8" height="8" rx="1" class="node" />
        <rect x="296" y="536" width="8" height="8" rx="1" class="node" />
        <rect x="596" y="396" width="8" height="8" rx="1" class="node" />
        <rect x="896" y="536" width="8" height="8" rx="1" class="node" />
        <rect x="896" y="396" width="8" height="8" rx="1" class="node" />
        <rect x="496" y="756" width="8" height="8" rx="1" class="node" />
        <rect x="796" y="636" width="8" height="8" rx="1" class="node" />
        <rect x="196" y="236" width="8" height="8" rx="1" class="node" />
        <rect x="356" y="476" width="8" height="8" rx="1" class="node" />
        <rect x="196" y="476" width="8" height="8" rx="1" class="node" />
        <rect x="356" y="716" width="8" height="8" rx="1" class="node" />
        <rect x="996" y="156" width="8" height="8" rx="1" class="node" />
        <rect x="1156" y="396" width="8" height="8" rx="1" class="node" />
        <rect x="1596" y="296" width="8" height="8" rx="1" class="node" />
        <rect x="1436" y="536" width="8" height="8" rx="1" class="node" />
        <rect x="1596" y="536" width="8" height="8" rx="1" class="node" />
      </g>

      <!-- ═══════════════════════════════════════════════════════════════════
      Moving data packets — photoluminescent persistence trail (光学残留拖尾)

      Each packet group uses:
        - mix-blend-mode: screen — additive light blending (dark mode)
        - 6 trail layers with exponential opacity decay (pow(ratio, 2.5))
        - White-hot core + colored outer glow (the incandescent center)
        - Multi-layer SVG filters (bloom → mid → tight glow)

      Trail keyframes shift backward from the main packet:
        k0(head) = 1.0, k1 = 0.98, k2 = 0.95, k3 = 0.91, k4 = 0.86, k5 = 0.79
        These approximate pow(ratio, 2.5) exponential decay in SVG's keyPoints system.
      ═══════════════════════════════════════════════════════════════════════ -->
      <g class="packets" :class="{ 'packets--active': hasActivity }">

        <!-- ══ Packet 1: cyan — Route A horizontal ══ -->
        <g class="packet-group packet-group--cyan">
          <!-- Broad bloom aura (dark mode only) -->
          <rect width="14" height="14" rx="1" class="trail trail--bloom">
            <animateMotion dur="12s" repeatCount="indefinite"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- Ghost trail layers — exponential opacity decay (pow(ratio, 2.5)) -->
          <!-- k1: opacity 0.60 — tail near the head -->
          <rect width="5" height="5" rx="1" class="trail trail--k1">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.98" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- k2: opacity 0.38 -->
          <rect width="4" height="4" rx="1" class="trail trail--k2">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.95" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- k3: opacity 0.22 -->
          <rect width="3.5" height="3.5" rx="1" class="trail trail--k3">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.91" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- k4: opacity 0.11 — mid-tail, fading -->
          <rect width="3" height="3" rx="1" class="trail trail--k4">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.86" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- k5: opacity 0.04 — long phosphor tail -->
          <rect width="2.5" height="2.5" rx="0.5" class="trail trail--k5">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.79" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- k6: opacity 0.01 — barely visible, the longest decay -->
          <rect width="2" height="2" rx="0.5" class="trail trail--k6">
            <animateMotion dur="12s" repeatCount="indefinite" keyPoints="0;0.70" keyTimes="0;1" calcMode="linear"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
          </rect>
          <!-- White-hot core (center) + colored outer (glow) -->
          <g class="packet-core">
            <animateMotion dur="12s" repeatCount="indefinite"
              path="M0,180 L400,180 L400,320 L700,320 L700,180 L1200,180" />
            <rect width="6" height="6" rx="1" class="packet packet--cyan" />
            <rect width="2.5" height="2.5" rx="0.5" class="packet-core-white" />
          </g>
        </g>

        <!-- ══ Packet 2: green — Route B vertical ══ -->
        <g class="packet-group packet-group--green">
          <rect width="14" height="14" rx="1" class="trail trail--bloom">
            <animateMotion dur="15s" repeatCount="indefinite"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="5" height="5" rx="1" class="trail trail--k1">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.98" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="4" height="4" rx="1" class="trail trail--k2">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.95" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="3.5" height="3.5" rx="1" class="trail trail--k3">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.91" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="3" height="3" rx="1" class="trail trail--k4">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.86" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="2.5" height="2.5" rx="0.5" class="trail trail--k5">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.79" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <rect width="2" height="2" rx="0.5" class="trail trail--k6">
            <animateMotion dur="15s" repeatCount="indefinite" keyPoints="0;0.70" keyTimes="0;1" calcMode="linear"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
          </rect>
          <g class="packet-core">
            <animateMotion dur="15s" repeatCount="indefinite"
              path="M200,0 L200,240 L360,240 L360,480 L200,480 L200,720 L360,720 L360,1080" />
            <rect width="6" height="6" rx="1" class="packet packet--green" />
            <rect width="2.5" height="2.5" rx="0.5" class="packet-core-white" />
          </g>
        </g>

        <!-- ══ Packet 3: magenta — Route C lower horizontal ══ -->
        <g class="packet-group packet-group--magenta">
          <rect width="14" height="14" rx="1" class="trail trail--bloom">
            <animateMotion dur="10s" repeatCount="indefinite"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="5" height="5" rx="1" class="trail trail--k1">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.98" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="4" height="4" rx="1" class="trail trail--k2">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.95" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="3.5" height="3.5" rx="1" class="trail trail--k3">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.91" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="3" height="3" rx="1" class="trail trail--k4">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.86" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="2.5" height="2.5" rx="0.5" class="trail trail--k5">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.79" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <rect width="2" height="2" rx="0.5" class="trail trail--k6">
            <animateMotion dur="10s" repeatCount="indefinite" keyPoints="0;0.70" keyTimes="0;1" calcMode="linear"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
          </rect>
          <g class="packet-core">
            <animateMotion dur="10s" repeatCount="indefinite"
              path="M0,540 L300,540 L300,400 L600,400 L600,540 L900,540 L900,400 L1200,400" />
            <rect width="6" height="6" rx="1" class="packet packet--magenta" />
            <rect width="2.5" height="2.5" rx="0.5" class="packet-core-white" />
          </g>
        </g>

        <!-- ══ Packet 4: yellow — right-side vertical ══ -->
        <g class="packet-group packet-group--yellow">
          <rect width="14" height="14" rx="1" class="trail trail--bloom">
            <animateMotion dur="18s" repeatCount="indefinite"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="5" height="5" rx="1" class="trail trail--k1">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.98" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="4" height="4" rx="1" class="trail trail--k2">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.95" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="3.5" height="3.5" rx="1" class="trail trail--k3">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.91" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="3" height="3" rx="1" class="trail trail--k4">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.86" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="2.5" height="2.5" rx="0.5" class="trail trail--k5">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.79" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <rect width="2" height="2" rx="0.5" class="trail trail--k6">
            <animateMotion dur="18s" repeatCount="indefinite" keyPoints="0;0.70" keyTimes="0;1" calcMode="linear"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
          </rect>
          <g class="packet-core">
            <animateMotion dur="18s" repeatCount="indefinite"
              path="M1600,0 L1600,300 L1440,300 L1440,540 L1600,540 L1600,800 L1440,800 L1440,1080" />
            <rect width="6" height="6" rx="1" class="packet packet--yellow" />
            <rect width="2.5" height="2.5" rx="0.5" class="packet-core-white" />
          </g>
        </g>

        <!-- ══ Packet 5: violet — lower horizontal ══ -->
        <g class="packet-group packet-group--violet">
          <rect width="14" height="14" rx="1" class="trail trail--bloom">
            <animateMotion dur="14s" repeatCount="indefinite"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="5" height="5" rx="1" class="trail trail--k1">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.98" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="4" height="4" rx="1" class="trail trail--k2">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.95" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="3.5" height="3.5" rx="1" class="trail trail--k3">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.91" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="3" height="3" rx="1" class="trail trail--k4">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.86" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="2.5" height="2.5" rx="0.5" class="trail trail--k5">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.79" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <rect width="2" height="2" rx="0.5" class="trail trail--k6">
            <animateMotion dur="14s" repeatCount="indefinite" keyPoints="0;0.70" keyTimes="0;1" calcMode="linear"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
          </rect>
          <g class="packet-core">
            <animateMotion dur="14s" repeatCount="indefinite"
              path="M0,760 L500,760 L500,640 L800,640 L800,760 L1400,760" />
            <rect width="6" height="6" rx="1" class="packet packet--violet" />
            <rect width="2.5" height="2.5" rx="0.5" class="packet-core-white" />
          </g>
        </g>
      </g>
    </svg>
  </div>
</template>

<style scoped>
.bg-atmosphere {
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  overflow: hidden;
}

.bg-circuit {
  width: 100%;
  height: 100%;
}

/* ═══ Traces — PCB routes ═══ */
.traces {
  opacity: 0.06;
}

.trace {
  fill: none;
  stroke: var(--3d-guide);
  stroke-width: 1;
  stroke-linecap: square;
  stroke-linejoin: miter;
}

.trace--branch {
  stroke-width: 1;
  opacity: 0.7;
}

.trace--dashed {
  stroke-dasharray: 8 6;
  opacity: 0.5;
}

/* ═══ Junction nodes — 8×8px squares ═══ */
.nodes {
  opacity: 0.08;
}

.node {
  fill: transparent;
  stroke: var(--border-hover);
  stroke-width: 1;
}

/* ═══ Moving data packets ═══ */
.packets {
  opacity: 0;
  transition: opacity 0.8s ease;
}

.packets--active {
  opacity: 1;
}

/* Default: heartbeat idle pulse */
.packets:not(.packets--active) {
  animation: packet-idle 8s ease-in-out infinite;
}

@keyframes packet-idle {
  0%, 85%, 100% { opacity: 0; }
  90% { opacity: 0.4; }
}

/* ═══════════════════════════════════════════════════════════════════════
   Dark Mode: Photoluminescent Persistence Trail (光学残留拖尾)

   Physics model:
   - mix-blend-mode: screen = additive light blending (光叠加变白)
   - Multi-layer bloom filter simulates phosphor afterglow
   - White-hot core + colored halo = incandescent center
   - 6 trail layers with exponential opacity decay (pow(ratio, 2.5)):
     k1=0.60, k2=0.38, k3=0.22, k4=0.11, k5=0.04, k6=0.01
   - Trail size tapers from 5px → 2px as it decays
   ═══════════════════════════════════════════════════════════════════════ */

/* ── Packet group: screen blending for additive light ── */
.packet-group {
  mix-blend-mode: screen;
  isolation: isolate;
}

/* ── Bloom aura — broad soft glow behind the packet ── */
.trail--bloom {
  fill: none;
  opacity: 0;
}

[data-theme="dark"] .trail--bloom,
:root .trail--bloom {
  opacity: 0.08;
  filter: url(#glow-bloom);
}

.packets--active .trail--bloom {
  opacity: 0.14;
  filter: url(#glow-bloom-active);
}

/* ── Trail layers — exponential opacity decay ── */
.trail--k1 { opacity: 0.60; }
.trail--k2 { opacity: 0.38; }
.trail--k3 { opacity: 0.22; }
.trail--k4 { opacity: 0.11; }
.trail--k5 { opacity: 0.04; }
.trail--k6 { opacity: 0.01; }

/* Trail base fill — inherited from parent packet-group color */
.trail {
  fill: currentColor;
}

/* ── Main packet — colored outer body ── */
.packet {
  fill: currentColor;
}

/* ── White-hot core — incandescent center, the brightest point ── */
.packet-core-white {
  fill: #ffffff;
  opacity: 0.95;
}

/* ── Packet core wrapper: mid glow on the whole core unit ── */
.packet-core {
  filter: url(#glow-mid);
}

.packets--active .packet-core {
  filter: url(#glow-mid-active);
}

/* ── Neon color assignments per packet group ── */
.packet-group--cyan {
  color: #00f3ff;
}

.packet-group--green {
  color: #39ff14;
}

.packet-group--magenta {
  color: #ff0055;
}

.packet-group--yellow {
  color: #ffea00;
}

.packet-group--violet {
  color: #b500ff;
}

/* ═══════════════════════════════════════════════════════════════════════
   Light Mode: NO glow — fluorescent matte effect

   Like real fluorescent elements under bright lights:
   only the body is visible, no phosphor persistence.
   Screen blending disabled → normal composite (physical squares).
   ═══════════════════════════════════════════════════════════════════════ */

[data-theme="light"] .packet-group {
  mix-blend-mode: normal;
  filter: none;
}

[data-theme="light"] .trail--bloom {
  display: none;
}

[data-theme="light"] .trail {
  display: none;
}

[data-theme="light"] .packet-core-white {
  display: none;
}

[data-theme="light"] .packet-core {
  filter: none;
}

[data-theme="light"] .packet {
  opacity: 0.6;
  /* Use accent color instead of neon for light mode */
  fill: var(--accent);
}

[data-theme="light"] .traces {
  opacity: 0.04;
}

[data-theme="light"] .nodes {
  opacity: 0.06;
}

/* ═══ Reduced motion ═══ */
@media (prefers-reduced-motion: reduce) {
  .packets { display: none; }
  .traces { opacity: 0.03; }
}
</style>
