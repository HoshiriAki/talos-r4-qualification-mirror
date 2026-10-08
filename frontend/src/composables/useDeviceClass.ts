import { ref, onMounted, onUnmounted } from 'vue'
import type { DeviceClass, UiMode } from '@/types/uiMode'

/**
 * Classify the viewport into desktop/tablet/phone.
 * Uses width + coarse-pointer detection, not User-Agent.
 */
export function classifyViewport(width: number, coarsePointer: boolean): DeviceClass {
  if (width < 600) return 'phone'
  if (width < 1180 || coarsePointer) return 'tablet'
  return 'desktop'
}

export function defaultMode(device: DeviceClass): UiMode {
  if (device === 'phone') return 'hud-compact'
  if (device === 'tablet') return 'hud'
  return 'work'
}

export function useDeviceClass() {
  const deviceClass = ref<DeviceClass>('desktop')
  let timer: ReturnType<typeof setTimeout> | null = null

  function update() {
    const coarse = window.matchMedia('(pointer: coarse)').matches
    deviceClass.value = classifyViewport(window.innerWidth, coarse)
  }

  onMounted(() => {
    update()
    window.addEventListener('resize', () => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(update, 200)
    })
  })

  onUnmounted(() => {
    window.removeEventListener('resize', update)
    if (timer) clearTimeout(timer)
  })

  return { deviceClass }
}
