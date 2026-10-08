<script setup lang="ts">
import { shanghaiBusinessDate } from '@/utils/businessDate'
</script>

<template>
  <div class="login-visual">
    <div class="visual-content">
      <span class="visual-line">|</span>
      <h1 class="visual-title">TALOS</h1>
      <p class="visual-subtitle">设备租赁管理系统</p>
      <div class="visual-divider" />
      <p class="visual-meta">MODULE.AUTH · SHANGHAI</p>
      <p class="visual-date">{{ shanghaiBusinessDate() }}</p>
      <p class="visual-version">2026 — V1.06 — REV.A</p>
    </div>
  </div>
</template>

<style scoped>
.login-visual {
  display: flex; align-items: center; justify-content: center;
  min-height: 100%; padding: 48px;
}
.visual-content { display: flex; flex-direction: column; gap: 8px; max-width: 320px; }
.visual-line { color: var(--accent); font-size: 20px; }
.visual-title { font-family: 'Space Mono', monospace; font-size: 48px; font-weight: 700; color: var(--text-primary); margin: 0; line-height: 1.1; }
.visual-subtitle { font-family: 'Inter', sans-serif; font-size: 14px; color: var(--text-secondary); margin: 4px 0; }
.visual-divider { width: 60px; height: 2px; background: var(--accent); }
.visual-meta, .visual-date, .visual-version { font-family: 'Space Mono', monospace; font-size: 10px; color: var(--text-tertiary); letter-spacing: 0.1em; }
</style>
