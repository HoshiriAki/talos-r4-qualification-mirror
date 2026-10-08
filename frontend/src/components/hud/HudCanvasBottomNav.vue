<script setup lang="ts">
defineProps<{
  syncLabel: string
  riskLabel: string
}>()
</script>

<template>
  <div class="hud-bottom-nav">
    <div class="bl-row">
      <span class="bl-label mono">数据</span>
      <span class="bl-line">
        <span class="chain" aria-hidden="true">
          <i v-for="n in 6" :key="n"></i>
        </span>
      </span>
      <span class="bl-value mono">{{ syncLabel }}</span>
    </div>
    <div class="bl-row">
      <span class="bl-label mono">风险</span>
      <span class="bl-line">
        <span class="chain" aria-hidden="true">
          <i v-for="n in 6" :key="n"></i>
        </span>
      </span>
      <span class="bl-value mono">{{ riskLabel }}</span>
    </div>
  </div>
</template>

<style scoped>
.hud-bottom-nav {
  display: flex;
  flex-direction: column;
  width: 420px;
}

.bl-row {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 9px 0;
  position: relative;
}

.bl-label {
  font-weight: 700;
  font-size: 15px;
  width: 56px;
  flex-shrink: 0;
  color: var(--text-primary);
}

.bl-line {
  flex: 1;
  height: 1px;
  background: var(--accent);
  opacity: 0.4;
  position: relative;
}

.bl-value {
  width: 176px;
  color: var(--text-secondary);
  font-size: 10px;
  text-align: right;
}

.chain {
  position: absolute;
  right: 0;
  top: -2.5px;
  display: flex;
  gap: 3px;
}
.chain i {
  width: 4px;
  height: 4px;
  border-radius: var(--radius-sm);
  background: var(--accent);
}

@media (max-width: 1100px) {
  .hud-bottom-nav { display: none; }
}
@media (max-width: 600px) {
  .hud-bottom-nav {
    width: auto;
    flex-direction: row;
    gap: 20px;
  }
  .bl-row { padding: 6px 0; }
  .bl-label { width: auto; font-size: 13px; }
  .bl-line { display: none; }
}
</style>
