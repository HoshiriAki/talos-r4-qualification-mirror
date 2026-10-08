<script setup lang="ts">
defineProps<{ message: string; maskOpacity?: number }>()
</script>

<template>
  <div class="loading-overlay" :style="{ '--mask-opacity': String(maskOpacity ?? 0.45) }" role="status" aria-live="polite">
    <span class="loading-orbit" aria-hidden="true" />
    <strong data-lab-slot="loading-message">{{ message }}</strong>
    <small>Local sandbox fixture · no business requests</small>
  </div>
</template>

<style scoped>
.loading-overlay{position:absolute;inset:0;display:grid;place-content:center;justify-items:center;gap:12px;background:rgb(13 13 15 / var(--mask-opacity));text-align:center}.loading-orbit{width:32px;height:32px;border:3px solid var(--accent);border-right-color:transparent;border-radius:50%;animation:spin 1.1s linear infinite}.loading-overlay strong{font-family:var(--font-mono);font-size:var(--font-size-sm)}.loading-overlay small{color:var(--text-tertiary);font-size:var(--font-size-caption)}@keyframes spin{to{transform:rotate(360deg)}}@media(prefers-reduced-motion:reduce){.loading-orbit{animation:none}}
</style>
