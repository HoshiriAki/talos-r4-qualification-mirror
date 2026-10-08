<script setup lang="ts">
import { computed } from 'vue'
import type { DashboardStats } from '@/api/dashboard'
import HudCanvasTopBar from './HudCanvasTopBar.vue'
import HudCanvasLeftStack from './HudCanvasLeftStack.vue'
import HudCanvasStageNav from './HudCanvasStageNav.vue'
import HudCanvasIsoScene from './HudCanvasIsoScene.vue'
import HudCanvasHeroType from './HudCanvasHeroType.vue'
import HudCanvasBottomNav from './HudCanvasBottomNav.vue'
import HudCanvasProgressCard from './HudCanvasProgressCard.vue'
import { buildHudCanvasContent } from './hudCanvasContent'

const props = defineProps<{
  model: DashboardStats | null
  loading: boolean
  compact?: boolean
}>()

defineEmits<{ exit: [] }>()

const content = computed(() => buildHudCanvasContent(props.model))
</script>

<template>
  <div class="hud-canvas" :class="{ compact }">
    <!-- Grid overlay -->
    <div class="canvas-grid" aria-hidden="true"></div>

    <!-- Full-viewport isometric scene (background) -->
    <div class="iso-stage">
      <HudCanvasIsoScene :model="model" />
    </div>

    <!-- Floating UI layers (above SVG) -->
    <HudCanvasTopBar
      :utilization-percent="content.utilizationPercent"
      :overdue-count="model?.overdueReturns ?? 0"
      :order-count="model?.activeOrders ?? 0"
      :notification-count="model?.returnsDueToday ?? 0"
      @exit="$emit('exit')"
    />

    <div class="canvas-mid-left">
      <HudCanvasLeftStack :model="model" />
      <HudCanvasStageNav v-if="!compact" />
    </div>

    <div class="canvas-hero">
      <HudCanvasHeroType
        :label="content.heroLabel"
        :sub-label="content.heroSubLabel"
      />
    </div>

    <div class="canvas-bottom">
      <HudCanvasBottomNav
        :sync-label="content.syncLabel"
        :risk-label="content.riskLabel"
      />
      <HudCanvasProgressCard :content="content" />
    </div>
  </div>
</template>

<style scoped>
.hud-canvas {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 100vh;
  min-height: 100dvh;
  overflow: hidden;
  background: var(--bg-base);
}

/* ── Grid overlay ─────────────────────────────────────────────── */
.canvas-grid {
  position: absolute;
  inset: 0;
  z-index: 1;
  pointer-events: none;
  background-image:
    linear-gradient(rgba(255, 255, 255, 0.025) 1px, transparent 1px),
    linear-gradient(90deg, rgba(255, 255, 255, 0.025) 1px, transparent 1px);
  background-size: 34px 34px;
}
.canvas-grid::after {
  content: '';
  position: absolute;
  inset: 0;
  background: radial-gradient(120% 100% at 62% 45%, transparent 55%, rgba(0, 0, 0, 0.5) 100%);
}

/* ── ISO stage (background SVG) ───────────────────────────────── */
.iso-stage {
  position: absolute;
  inset: 0;
  z-index: 2;
}

/* ── Mid-left column (stack + stage nav) ──────────────────────── */
.canvas-mid-left {
  position: absolute;
  top: 130px;
  left: 32px;
  z-index: 5;
  display: flex;
  flex-direction: column;
  gap: 30px;
}

/* ── Hero typography (bottom-right of scene) ──────────────────── */
.canvas-hero {
  position: absolute;
  right: 5%;
  bottom: 24%;
  z-index: 4;
  pointer-events: none;
}

/* ── Bottom row ───────────────────────────────────────────────── */
.canvas-bottom {
  position: absolute;
  bottom: 34px;
  left: 32px;
  right: 32px;
  z-index: 5;
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
}

/* ── Responsive ───────────────────────────────────────────────── */
@media (max-width: 1100px) {
  .canvas-mid-left {
    left: 24px;
    gap: 20px;
  }
  .canvas-hero {
    right: 4%;
    bottom: 27%;
  }
  .canvas-bottom {
    left: 24px;
    right: 24px;
  }
}

@media (max-width: 900px) {
  .canvas-mid-left {
    top: 100px;
    left: 16px;
  }
  .canvas-bottom {
    bottom: 24px;
    left: 16px;
    right: 16px;
  }
}

@media (max-width: 600px) {
  .canvas-mid-left {
    top: 80px;
    left: 10px;
    right: 10px;
  }
  .canvas-hero {
    right: 2%;
    bottom: 22%;
  }
  .canvas-bottom {
    bottom: 16px;
    left: 10px;
    right: 10px;
    flex-direction: column;
    align-items: stretch;
    gap: 12px;
  }
}

/* Compact variant: further simplified for phone */
.compact .canvas-mid-left {
  gap: 14px;
}
.compact .canvas-hero {
  bottom: 26%;
}

@media (prefers-reduced-motion: reduce) {
  .canvas-grid::after { background: none; }
}
</style>
