import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import type { UiMode } from '@/types/uiMode'
import { defaultMode, useDeviceClass } from '@/composables/useDeviceClass'

const STORAGE_KEY = 'talos-ui-mode'

export const useUiModeStore = defineStore('uiMode', () => {
  const { deviceClass } = useDeviceClass()
  const savedMode = localStorage.getItem(STORAGE_KEY) as UiMode | null

  const mode = ref<UiMode>(resolveInitial())

  function resolveInitial(): UiMode {
    if (deviceClass.value === 'phone') return 'hud-compact'
    if (savedMode === 'work' || savedMode === 'hud') return savedMode
    return defaultMode(deviceClass.value)
  }

  const canSwitch = computed(() => deviceClass.value !== 'phone')

  function setMode(m: Exclude<UiMode, 'hud-compact'>) {
    if (deviceClass.value === 'phone') return
    mode.value = m
    localStorage.setItem(STORAGE_KEY, m)
  }

  // When device class changes (resize / orientation), re-evaluate
  watch(deviceClass, (next) => {
    if (next === 'phone') {
      mode.value = 'hud-compact'
      return
    }
    // Leaving phone: restore saved preference or default
    const saved = localStorage.getItem(STORAGE_KEY) as UiMode | null
    mode.value = (saved === 'work' || saved === 'hud') ? saved : defaultMode(next)
  })

  return { mode, deviceClass, canSwitch, setMode }
})
