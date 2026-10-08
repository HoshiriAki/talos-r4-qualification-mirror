<script setup lang="ts">
import type { HudCanvasContent } from './hudCanvasContent'

defineProps<{ content: HudCanvasContent }>()
</script>

<template>
  <div class="hud-progress-card">
    <div class="br-tags">
      <span class="br-tag mono">在外 {{ content.progressValue }}</span>
      <span class="br-tag mono">利用率 {{ content.utilizationPercent }}%</span>
    </div>
    <div class="br-title">{{ content.progressTitle }}</div>
    <div class="br-progress">
      <span class="br-arrow" aria-hidden="true">→</span>
      <span class="br-num mono">{{ content.progressValue }}<small>/{{ content.progressTotal }}</small></span>
    </div>
    <div class="br-caption">{{ content.progressCaption }}</div>
  </div>
</template>

<style scoped>
.hud-progress-card {
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  border-radius: 12px;
  padding: 18px 22px;
  min-width: 260px;
}

.br-tags {
  display: flex;
  gap: 8px;
  margin-bottom: 12px;
}
.br-tag {
  font-size: 11px;
  padding: 3px 9px;
  border-radius: 6px;
  background: var(--bg-field);
  color: var(--text-secondary);
  font-family: var(--font-mono);
}

.br-title {
  font-family: var(--font-sans);
  font-weight: 800;
  font-size: 24px;
  color: var(--text-primary);
}

.br-progress {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-top: 10px;
}

.br-arrow {
  color: var(--accent);
  font-size: 16px;
  font-family: var(--font-mono);
}

.br-num {
  font-weight: 700;
  font-size: 26px;
  color: var(--text-primary);
}
.br-num small {
  font-size: 14px;
  color: var(--text-tertiary);
  font-weight: 400;
}

.br-caption {
  font-size: 11px;
  color: var(--text-tertiary);
  margin-top: 4px;
}

@media (max-width: 900px) {
  .hud-progress-card {
    min-width: 200px;
    padding: 14px 16px;
  }
  .br-title { font-size: 20px; }
  .br-num { font-size: 22px; }
}

@media (max-width: 600px) {
  .hud-progress-card {
    min-width: 160px;
    padding: 12px 14px;
  }
  .br-title { font-size: 17px; }
  .br-num { font-size: 19px; }
}
</style>
