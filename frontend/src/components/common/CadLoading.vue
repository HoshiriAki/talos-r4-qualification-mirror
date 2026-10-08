<script setup lang="ts">
/**
 * CadLoading — Blueprint-style loading
 * Phase sequence: Wireframe → Stroke → Fill → Number → Data
 * Total: 120+120+120+120+120 = 600ms
 */
import { useReducedMotion } from '@/composables/useMotion'
const reduced = useReducedMotion()

const steps = ['wireframe', 'stroke', 'fill', 'number', 'data'] as const
</script>

<template>
  <div class="cad-loading" :class="{ 'cad-reduced': reduced }">
    <div class="cad-frame">
      <!-- Construction lines -->
      <div class="cad-grid-line h" />
      <div class="cad-grid-line v" />
      <!-- Wireframe→Stroke→Fill layers -->
      <div class="cad-layer wireframe" />
      <div class="cad-layer stroke" />
      <div class="cad-layer fill" />
      <div class="cad-layer number">
        <span class="cad-number-text">A-01</span>
      </div>
    </div>
    <div class="cad-label">
      <span class="cad-label-text">DRAWING — LOADING</span>
    </div>
  </div>
</template>

<style scoped>
.cad-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 24px;
  user-select: none;
}

.cad-frame {
  position: relative;
  width: 200px;
  height: 120px;
  border: 1px dashed var(--border-default);
  overflow: hidden;
}

/* Grid lines */
.cad-grid-line {
  position: absolute;
  background: var(--grid-line);
  animation: none;
}
.cad-grid-line.h {
  top: 50%;
  left: 0;
  width: 100%;
  height: 1px;
}
.cad-grid-line.v {
  left: 50%;
  top: 0;
  width: 1px;
  height: 100%;
}

/* Layers — stacked, animated in sequence */
.cad-layer {
  position: absolute;
  inset: 0;
  opacity: 0;
}

.cad-layer.wireframe {
  border: 1px solid var(--border-hover);
  animation: cad-phase 2400ms steps(1) infinite;
  animation-delay: 0ms;
}

.cad-layer.stroke {
  border: 2px solid var(--border-active);
  animation: cad-phase 2400ms steps(1) infinite;
  animation-delay: 480ms;
}

.cad-layer.fill {
  background: var(--bg-elevated);
  animation: cad-phase 2400ms steps(1) infinite;
  animation-delay: 960ms;
}

.cad-layer.number {
  display: flex;
  align-items: flex-end;
  justify-content: flex-start;
  padding: 8px;
  animation: cad-phase 2400ms steps(1) infinite;
  animation-delay: 1440ms;
}

.cad-number-text {
  font-family: var(--font-mono);
  font-size: var(--font-size-caption);
  color: var(--accent);
  letter-spacing: 0.5px;
}

.cad-label {
  text-align: center;
}

.cad-label-text {
  font-family: var(--font-mono);
  font-size: var(--font-size-caption);
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 1px;
}

@keyframes cad-phase {
  0%, 20% { opacity: 1; }
  21%, 100% { opacity: 0; }
}

/* Reduced motion: static wireframe */
.cad-reduced .cad-layer.wireframe,
.cad-reduced .cad-layer.stroke {
  opacity: 1;
}
.cad-reduced .cad-layer.fill {
  opacity: 0.3;
}
.cad-reduced .cad-label-text::after {
  content: ' — please wait';
}
</style>
