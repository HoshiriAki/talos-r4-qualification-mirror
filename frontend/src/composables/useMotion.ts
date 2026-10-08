/**
 * Industrial motion composables — mechanical blueprint movement language.
 * Phase 5: counting animation + staggered entrance + hover micro-response.
 */

import { ref, onMounted, type Ref } from 'vue'

export function useReducedMotion(): boolean {
  if (typeof window === 'undefined') return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

/**
 * Shared entrance/exit/hover helpers — kept for backward compat.
 */
export function useMotion() {
  function enter(direction: 'right' | 'left' | 'down' | 'up' = 'right', distance = 16): Record<string, string> {
    if (useReducedMotion()) return {}
    const axis = direction === 'right' || direction === 'left' ? 'X' : 'Y'
    const sign = direction === 'right' || direction === 'down' ? '' : '-'
    return {
      transform: 'translate(0, 0)',
      opacity: '1',
      'transition-property': 'transform, opacity',
      'transition-duration': '0.2s',
      'transition-timing-function': 'ease-out',
      '--_enter-offset': `translate${axis}(${sign}${distance}px)`,
    } as Record<string, string>
  }

  function exit(direction: 'right' | 'left' | 'down' | 'up' = 'left', distance = 16): Record<string, string> {
    if (useReducedMotion()) return {}
    const axis = direction === 'right' || direction === 'left' ? 'X' : 'Y'
    const sign = direction === 'right' || direction === 'down' ? '' : '-'
    return {
      transform: `translate${axis}(${sign}${distance}px)`,
      opacity: '0',
      'transition-property': 'transform, opacity',
      'transition-duration': '0.15s',
      'transition-timing-function': 'ease-in',
    } as Record<string, string>
  }

  function hover(): Record<string, string> {
    if (useReducedMotion()) return {}
    return {
      transform: 'translateX(2px)',
      'transition-property': 'transform, border-color',
      'transition-duration': '0.12s',
      'transition-timing-function': 'ease-out',
    } as Record<string, string>
  }

  return { enter, exit, hover }
}

/**
 * Animated number counter — mechanical precision feel.
 * Counts from 0 to target over `duration` ms.
 */
export function useAnimatedNumber(target: Ref<number>, duration = 800) {
  const display = ref(0)

  function animate() {
    const reduce = useReducedMotion()
    if (reduce) { display.value = target.value; return }

    const start = 0
    const end = target.value
    if (end === start) { display.value = start; return }

    const startTime = performance.now()
    function tick(now: number) {
      const elapsed = now - startTime
      const progress = Math.min(elapsed / duration, 1)
      // Ease-out cubic — mechanical deceleration
      const eased = 1 - Math.pow(1 - progress, 3)
      display.value = Math.round(start + (end - start) * eased)
      if (progress < 1) requestAnimationFrame(tick)
    }
    requestAnimationFrame(tick)
  }

  return { display, animate }
}

/**
 * Staggered module entrance — sequential build-up like an assembly line.
 * Returns a CSS variable for animation-delay based on item index.
 */
export function useStaggerEntrance(itemCount: number, staggerMs = 80) {
  const reduce = useReducedMotion()
  const visible = ref(false)

  onMounted(() => {
    // Trigger after one frame to let DOM settle
    requestAnimationFrame(() => {
      visible.value = true
    })
  })

  function delayFor(index: number): number {
    if (reduce) return 0
    return index * staggerMs
  }

  return { visible, delayFor }
}
