<script setup lang="ts">
/**
 * AssemblyLoading — Modules arrive sequentially like factory assembly
 * Each module: 240ms entry + 120ms pause
 */
import { useReducedMotion } from '@/composables/useMotion'
const reduced = useReducedMotion()
const modules = ['A-01', 'A-02', 'A-03', 'A-04']
</script>

<template>
  <div class="asm-loading" :class="{ 'asm-reduced': reduced }">
    <div class="asm-track">
      <div
        v-for="(mod, i) in modules"
        :key="mod"
        class="asm-module"
        :style="{ animationDelay: `${i * 360}ms` }"
      >
        <div class="asm-module-frame" />
        <span class="asm-module-label">{{ mod }}</span>
      </div>
    </div>
    <span class="font-mono text-caption text-text-muted uppercase tracking-wider">ASSEMBLY — IN PROGRESS</span>
  </div>
</template>

<style scoped>
.asm-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 24px;
}

.asm-track {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 40px;
}

.asm-module {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  animation: asm-enter 1440ms steps(1) infinite;
  animation-fill-mode: both;
}

.asm-module-frame {
  width: 28px;
  height: 20px;
  border: 1px solid var(--border-active);
}

/* Staggered — modules appear in sequence */
@keyframes asm-enter {
  0%, 12%   { opacity: 0; transform: translateY(8px); }
  13%, 100% { opacity: 1; transform: translateY(0); }
}

.asm-module-label {
  font-family: var(--font-mono);
  font-size: 8px;
  color: var(--text-muted);
}

.asm-reduced .asm-module {
  opacity: 1;
  animation: none;
}
</style>
