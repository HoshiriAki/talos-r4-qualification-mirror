<script setup lang="ts">
/**
 * PipelineLoading — Data packets flow through orthogonal pipe segments
 * Total: 480ms travel + 120ms per node pause
 */
import { useReducedMotion } from '@/composables/useMotion'
const reduced = useReducedMotion()
</script>

<template>
  <div class="pipe-loading" :class="{ 'pipe-reduced': reduced }">
    <div class="pipe-network">
      <!-- Pipe segments -->
      <div class="pipe-segment h p1" />
      <div class="pipe-segment v p2" />
      <div class="pipe-segment h p3" />
      <!-- Nodes -->
      <div class="pipe-node n1" />
      <div class="pipe-node n2" />
      <div class="pipe-node n3" />
      <!-- Data packet -->
      <div class="pipe-packet" />
    </div>
    <span class="font-mono text-caption text-text-muted uppercase tracking-wider">PIPELINE — DATA_FLOW_07</span>
  </div>
</template>

<style scoped>
.pipe-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 24px;
}

.pipe-network {
  position: relative;
  width: 200px;
  height: 80px;
}

.pipe-segment {
  position: absolute;
  background: var(--border-default);
}
.pipe-segment.h {
  height: 2px;
}
.pipe-segment.v {
  width: 2px;
}
.pipe-segment.p1 {
  top: 20px; left: 0; width: 80px;
}
.pipe-segment.p2 {
  top: 20px; left: 80px; height: 40px;
}
.pipe-segment.p3 {
  top: 60px; left: 80px; width: 80px;
}

.pipe-node {
  position: absolute;
  width: 6px; height: 6px;
  background: var(--border-hover);
}
.pipe-node.n1 { top: 17px; left: -1px; }
.pipe-node.n2 { top: 57px; left: 77px; }
.pipe-node.n3 { top: 57px; left: 157px; animation: node-active 2400ms steps(1) infinite; animation-delay: 1080ms; }

.pipe-packet {
  position: absolute;
  width: 8px; height: 8px;
  background: var(--accent);
  top: 16px;
  left: 0;
  animation: packet-travel 2400ms steps(4) infinite;
}

@keyframes packet-travel {
  0%   { top: 16px; left: 0; }
  25%  { top: 16px; left: 76px; }
  50%  { top: 54px; left: 76px; }
  75%  { top: 54px; left: 154px; }
  100% { top: 54px; left: 154px; }
}

@keyframes node-active {
  0%, 10%  { background: var(--accent); }
  11%, 100% { background: var(--border-hover); }
}

.pipe-reduced .pipe-packet { animation: none; left: 154px; top: 54px; }
.pipe-reduced .pipe-node { background: var(--accent); }
</style>
