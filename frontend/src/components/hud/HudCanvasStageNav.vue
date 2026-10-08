<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'

interface Stage {
  key: string
  glyph: string
  eyebrow: string
  title: string
  route: string
}

const stages: Stage[] = [
  { key: 'order', glyph: '录', eyebrow: '订单录入', title: '新建订单 · 定价核算', route: '/app/pricing' },
  { key: 'fulfill', glyph: '发', eyebrow: '订单履约', title: '分拣出库 · 扫码发货', route: '/app/shipping' },
  { key: 'return', glyph: '还', eyebrow: '归还检查', title: '扫码入库 · 光学检测', route: '/app/checkin' },
  { key: 'review', glyph: '核', eyebrow: '订单复核', title: '状态追踪 · 异常处理', route: '/app/orders' },
]

const currentIndex = ref(1) // Default: 订单履约
const router = useRouter()

const current = computed(() => stages[currentIndex.value])
const canPrev = computed(() => currentIndex.value > 0)
const canNext = computed(() => currentIndex.value < stages.length - 1)

function prev() {
  if (canPrev.value) currentIndex.value--
}
function next() {
  if (canNext.value) currentIndex.value++
}

function enterStage() {
  router.push(current.value.route)
}
</script>

<template>
  <div class="hud-stage-nav">
    <button
      class="stage-arrow"
      type="button"
      :disabled="!canPrev"
      :aria-label="`上一阶段`"
      @click="prev"
    >
      <span aria-hidden="true">‹</span>
    </button>

    <button class="stage-card" type="button" @click="enterStage">
      <div class="stage-icon">
        <span aria-hidden="true">{{ current.glyph }}</span>
      </div>
      <div>
        <div class="stage-eyebrow mono">流程入口 · {{ current.eyebrow }}</div>
        <div class="stage-title">{{ current.title }}</div>
      </div>
      <span class="stage-enter" aria-hidden="true">→</span>
    </button>

    <button
      class="stage-arrow"
      type="button"
      :disabled="!canNext"
      :aria-label="`下一阶段`"
      @click="next"
    >
      <span aria-hidden="true">›</span>
    </button>
  </div>
</template>

<style scoped>
.hud-stage-nav {
  display: flex;
  align-items: center;
  gap: 10px;
}

.stage-arrow {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: border-color var(--duration-micro), color var(--duration-micro);
  flex-shrink: 0;
}
.stage-arrow:hover:not(:disabled), .stage-arrow:focus-visible:not(:disabled) {
  border-color: var(--border-strong);
  color: var(--text-primary);
}
.stage-arrow:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.stage-card {
  background: var(--bg-elevated);
  border: 1px solid var(--border-base);
  border-radius: 10px;
  padding: 10px 18px;
  display: flex;
  align-items: center;
  gap: 12px;
  color: inherit;
  cursor: pointer;
  text-align: left;
  transition: border-color var(--duration-micro), background var(--duration-micro);
}

.stage-card:hover,
.stage-card:focus-visible {
  border-color: var(--border-strong);
  background: var(--bg-field);
}

.stage-icon {
  color: var(--text-primary);
  display: flex;
  align-items: center;
}

.stage-eyebrow {
  font-size: 10px;
  color: var(--text-tertiary);
  letter-spacing: 0.06em;
}

.stage-title {
  font-family: var(--font-mono);
  font-weight: 700;
  font-size: 14px;
  color: var(--text-primary);
  margin-top: 2px;
}

.stage-enter {
  color: var(--text-tertiary);
  margin-left: auto;
}

@media (max-width: 900px) {
  .hud-stage-nav { gap: 6px; }
  .stage-card { padding: 8px 12px; gap: 8px; }
  .stage-title { font-size: 12px; }
  .stage-eyebrow { font-size: 9px; }
}

@media (max-width: 600px) {
  .stage-arrow { width: 26px; height: 26px; border-radius: 7px; }
  .stage-card { padding: 6px 10px; }
  .stage-title { font-size: 11px; }
}
@media (prefers-reduced-motion: reduce) {
  .stage-arrow, .stage-card { transition: none; }
}
</style>
