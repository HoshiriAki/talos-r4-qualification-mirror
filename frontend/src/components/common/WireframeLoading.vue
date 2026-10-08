<script setup lang="ts">
/**
 * WireframeLoading — 3D-style construction loading
 * Phase: Construction lines → Full wireframe → Solid face
 * Total: 240+360+480 = 1080ms
 */
import { useReducedMotion } from '@/composables/useMotion'
const reduced = useReducedMotion()
</script>

<template>
  <div class="wf-loading" :class="{ 'wf-reduced': reduced }">
    <div class="wf-cube">
      <div class="wf-face front" />
      <div class="wf-face top" />
      <div class="wf-edge e1" />
      <div class="wf-edge e2" />
      <div class="wf-edge e3" />
      <div class="wf-edge e4" />
      <div class="wf-edge e5" />
      <div class="wf-edge e6" />
      <div class="wf-node n1" />
      <div class="wf-node n2" />
      <div class="wf-node n3" />
    </div>
    <span class="wf-label font-mono text-caption text-text-muted uppercase tracking-wider">WIREFRAME — RENDERING</span>
  </div>
</template>

<style scoped>
.wf-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 24px;
}

.wf-cube {
  position: relative;
  width: 100px;
  height: 80px;
}

.wf-face {
  position: absolute;
  border: 1px solid var(--border-default);
  background: var(--bg-surface);
  opacity: 0;
}
.wf-face.front {
  width: 60px; height: 60px;
  left: 20px; top: 10px;
  animation: wf-fill 1800ms steps(1) infinite;
  animation-delay: 600ms;
}
.wf-face.top {
  width: 40px; height: 30px;
  left: 30px; top: 0;
  transform: skewX(-30deg) scaleY(0.6);
  animation: wf-fill 1800ms steps(1) infinite;
  animation-delay: 900ms;
}

.wf-edge {
  position: absolute;
  background: var(--border-hover);
  opacity: 0;
}
.wf-edge.e1 { width: 60px; height: 1px; left: 20px; top: 10px; animation: wf-edge 1800ms steps(1) infinite; }
.wf-edge.e2 { width: 60px; height: 1px; left: 20px; top: 70px; animation: wf-edge 1800ms steps(1) infinite; animation-delay: 180ms; }
.wf-edge.e3 { width: 1px; height: 60px; left: 20px; top: 10px; animation: wf-edge 1800ms steps(1) infinite; animation-delay: 360ms; }
.wf-edge.e4 { width: 1px; height: 60px; left: 80px; top: 10px; animation: wf-edge 1800ms steps(1) infinite; animation-delay: 540ms; }

.wf-node {
  position: absolute;
  width: 4px; height: 4px;
  background: var(--accent);
  opacity: 0;
}
.wf-node.n1 { left: 19px; top: 9px; animation: wf-node-blink 1800ms steps(1) infinite; animation-delay: 1200ms; }
.wf-node.n2 { left: 79px; top: 9px; animation: wf-node-blink 1800ms steps(1) infinite; animation-delay: 1320ms; }
.wf-node.n3 { left: 79px; top: 69px; animation: wf-node-blink 1800ms steps(1) infinite; animation-delay: 1440ms; }

@keyframes wf-edge {
  0%, 33% { opacity: 1; }
  34%, 100% { opacity: 0.3; }
}
@keyframes wf-fill {
  0%, 25% { opacity: 1; }
  26%, 100% { opacity: 0; }
}
@keyframes wf-node-blink {
  0%, 10% { opacity: 1; }
  11%, 100% { opacity: 0; }
}

.wf-reduced .wf-edge { opacity: 0.8; }
.wf-reduced .wf-face.front { opacity: 0.3; }
</style>
