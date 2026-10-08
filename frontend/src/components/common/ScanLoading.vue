<script setup lang="ts">
/**
 * ScanLoading — Horizontal scan line sweeps top to bottom
 * Scanned areas turn from wireframe to solid, like a barcode reader.
 * Total: 480ms per pass
 */
import { useReducedMotion } from '@/composables/useMotion'
const reduced = useReducedMotion()
</script>

<template>
  <div class="scan-loading" :class="{ 'scan-reduced': reduced }">
    <div class="scan-area">
      <!-- Background rows — wireframe initially -->
      <div class="scan-row r1" />
      <div class="scan-row r2" />
      <div class="scan-row r3" />
      <div class="scan-row r4" />
      <div class="scan-row r5" />
      <!-- Scan line -->
      <div class="scan-line" />
    </div>
    <span class="font-mono text-caption text-text-muted uppercase tracking-wider">SCANNING — DATA_DUMP</span>
  </div>
</template>

<style scoped>
.scan-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 24px;
}

.scan-area {
  position: relative;
  width: 200px;
  height: 100px;
  border: 1px solid var(--border-default);
  overflow: hidden;
}

.scan-row {
  position: absolute;
  left: 0;
  width: 100%;
  height: 20px;
  border-bottom: 1px dashed var(--border-default);
}
.scan-row.r1 { top: 0; }
.scan-row.r2 { top: 20px; }
.scan-row.r3 { top: 40px; }
.scan-row.r4 { top: 60px; animation: scan-solid 2400ms steps(1) infinite; animation-delay: 0ms; }
.scan-row.r5 { top: 80px; animation: scan-solid 2400ms steps(1) infinite; animation-delay: 480ms; }

.scan-line {
  position: absolute;
  left: 0;
  width: 100%;
  height: 2px;
  background: var(--accent);
  animation: scan-sweep 2400ms steps(4) infinite;
}

@keyframes scan-sweep {
  0%   { top: 0%; }
  25%  { top: 25%; }
  50%  { top: 50%; }
  75%  { top: 75%; }
  100% { top: 100%; }
}

@keyframes scan-solid {
  0%, 20%  { background: var(--bg-elevated); border-style: solid; }
  21%, 100% { background: transparent; border-style: dashed; }
}

.scan-reduced .scan-line { display: none; }
.scan-reduced .scan-row { border-style: solid; }
</style>
