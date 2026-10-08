// ── Motion target resolver ────────────────────────────────────────────────
// 只接受 Registry 可验证的 target 类型：
//   specimen          → 组件根节点
//   probe             → 明确的 [data-lab-probe] 节点
//   slot:<id>         → 对应 [data-lab-slot="<id>"] 节点
//   selector:<safe>   → 白名单内选择器（由 sandbox-renderer 声明）
// target 不存在时返回 null（调用方产生 Diagnostics），不静默降级到根节点。

import type { UiLabMotionTarget } from '@/ui'

export function isLabMotionTarget(value: string): value is UiLabMotionTarget {
  if (value === 'specimen' || value === 'probe') return true
  if (value.startsWith('slot:') && value.length > 'slot:'.length) return true
  if (value.startsWith('selector:') && value.length > 'selector:'.length) return true
  return false
}

export interface ResolveMotionTargetOptions {
  /** probe 选择器白名单（sandbox-renderer 提供）。 */
  safeSelectors?: ReadonlySet<string>
  /** 当 probe target 无 [data-lab-probe] 时的回退选择器。 */
  probeSelector?: string | null
}

export function resolveMotionTarget(
  root: Element | null,
  target: UiLabMotionTarget,
  options: ResolveMotionTargetOptions = {},
): HTMLElement | null {
  if (!root) return null
  if (target === 'specimen') return root as HTMLElement
  if (target === 'probe') {
    const probe = root.querySelector<HTMLElement>('[data-lab-probe]')
    if (probe) return probe
    if (options.probeSelector) return root.querySelector<HTMLElement>(options.probeSelector)
    return null
  }
  if (target.startsWith('slot:')) {
    const slotId = target.slice('slot:'.length)
    const real = root.querySelector<HTMLElement>(`[data-lab-slot="${slotId}"]`)
    if (real) return real
    // 组件无内部 anchor 时，sandbox 以 data-lab-slot-adapter 承载该 slot。
    return root.querySelector<HTMLElement>(`[data-lab-slot-adapter="${slotId}"]`)
  }
  if (target.startsWith('selector:')) {
    const selector = target.slice('selector:'.length)
    if (options.safeSelectors?.has(selector)) {
      return root.querySelector<HTMLElement>(selector)
    }
    return null
  }
  return null
}
