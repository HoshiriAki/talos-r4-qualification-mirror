<script setup lang="ts">
defineProps<{
  label: string
  subLabel: string
}>()
</script>

<template>
  <div class="hud-hero-type" aria-hidden="true">
    <span class="hero-word">{{ label }}</span>
    <span class="hero-sub mono">{{ subLabel }}</span>
  </div>
</template>

<style scoped>
.hud-hero-type {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  pointer-events: none;
  line-height: 0.85;
  text-align: right;
}

.hero-word {
  font-family: var(--font-sans);
  font-weight: 800;
  font-size: clamp(56px, 9vw, 128px);
  letter-spacing: -0.02em;
  color: var(--text-primary);
}

.hero-sub {
  font-weight: 700;
  font-size: clamp(12px, 1.5vw, 16px);
  color: var(--text-secondary);
  letter-spacing: 0.04em;
  margin-top: 10px;
}

@media (max-width: 1100px) {
  .hero-word { font-size: clamp(42px, 7vw, 80px); }
}
@media (max-width: 600px) {
  .hero-word { font-size: clamp(32px, 6vw, 56px); }
  .hero-sub { font-size: 11px; }
}
</style>
