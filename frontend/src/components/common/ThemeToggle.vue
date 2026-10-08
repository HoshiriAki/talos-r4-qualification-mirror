<script setup lang="ts">
import { useThemeStore } from '@/stores/theme'

const theme = useThemeStore()
</script>

<template>
  <button
    id="themeToggle"
    class="flex items-center gap-2 w-full text-xs font-mono text-text-muted hover:text-text-secondary transition-colors uppercase tracking-wider"
    :aria-label="theme.theme === 'dark' ? '切换到日间模式' : '切换到夜间模式'"
    @click="theme.toggle()"
  >
    <span id="themeToggleIcon" class="text-sm">
      {{ theme.theme === 'dark' ? '◑' : '◐' }}
    </span>
    <span id="themeToggleLabel">
      {{ theme.theme === 'dark' ? 'Light Mode' : 'Dark Mode' }}
    </span>
  </button>
</template>
