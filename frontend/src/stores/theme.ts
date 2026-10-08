import { defineStore } from 'pinia'
import { ref } from 'vue'

type ThemeChoice = 'dark' | 'light' | 'system'

const THEME_KEY = 'talos-theme'

function getStored(): ThemeChoice {
  try {
    const stored = localStorage.getItem(THEME_KEY)
    if (stored === 'light' || stored === 'system') return stored
  } catch {
    console.warn('Failed to read theme from localStorage')
  }
  return 'dark'
}

function resolveSystem(): 'dark' | 'light' {
  if (typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: light)').matches) return 'light'
  return 'dark'
}

function applyTheme(effective: 'dark' | 'light') {
  // 添加视图过渡动画（View Transitions API）
  if (document.startViewTransition && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    document.startViewTransition(() => {
      document.documentElement.setAttribute('data-theme', effective)
    })
  } else {
    document.documentElement.setAttribute('data-theme', effective)
  }
}

// Listen for system preference changes
if (typeof window !== 'undefined') {
  window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
    const stored = getStored()
    if (stored === 'system') applyTheme(resolveSystem())
  })
}

async function syncThemeToServer(t: ThemeChoice) {
  try {
    const { saveUserSettings } = await import('@/api/user-settings')
    const { settingsState } = await import('@/utils/settings')
    // Keep settingsState.theme in sync so saveSettings() doesn't overwrite
    // with a stale value later (e.g. when ProfileDialog saves homePage).
    settingsState.theme = t
    await saveUserSettings({ ...settingsState, theme: t })
  } catch {
    // Fire-and-forget — localStorage is the source of truth
  }
}

export const useThemeStore = defineStore('theme', () => {
  const theme = ref<ThemeChoice>(getStored())

  function init() {
    applyTheme(theme.value === 'system' ? resolveSystem() : theme.value)
  }

  function setTheme(t: ThemeChoice) {
    theme.value = t
    const effective = t === 'system' ? resolveSystem() : t
    applyTheme(effective)
    try {
      localStorage.setItem(THEME_KEY, t)
    } catch {
      console.warn('Failed to persist theme to localStorage')
    }
    syncThemeToServer(t)
  }

  function toggle() {
    setTheme(theme.value === 'dark' ? 'light' : 'dark')
  }

  return { theme, init, setTheme, toggle }
})
