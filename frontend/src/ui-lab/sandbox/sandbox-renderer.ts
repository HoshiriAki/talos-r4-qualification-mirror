import TalosButton from '@/ui/components/TalosButton.vue'
import TalosStatusIndicator from '@/ui/components/TalosStatusIndicator.vue'

export const sandboxRendererMap = Object.freeze({
  'talos.ui.button': TalosButton,
  'talos.ui.status-indicator': TalosStatusIndicator,
})

export function supportsSandboxComponent(componentId: string | null): componentId is keyof typeof sandboxRendererMap | 'talos.ui.loading-overlay' {
  return componentId === 'talos.ui.loading-overlay' || Boolean(componentId && componentId in sandboxRendererMap)
}

// probe 目标：每个组件内部的可视化探针节点。由 sandbox 在真实组件 DOM 上
// 标记 [data-lab-probe]，resolveMotionTarget('probe') 指向它。
export const LAB_SANDBOX_PROBE_SELECTORS: Readonly<Record<string, string>> = Object.freeze({
  'talos.ui.button': '.talos-button__signal',
  'talos.ui.status-indicator': '.talos-status__mark',
  'talos.ui.loading-overlay': '.loading-orbit',
})

export function probeSelectorFor(componentId: string): string | null {
  return LAB_SANDBOX_PROBE_SELECTORS[componentId] ?? null
}

// 组件源码中带真实 [data-lab-slot] anchor 的 slot id。这些节点是权威 target；
// 未在此列表中的 slot 由 sandbox 以 [data-lab-slot-adapter] 显式渲染，避免
// 与真实 data-lab-slot 共享同一选择器。
export const LAB_SANDBOX_REAL_SLOT_IDS: Readonly<Record<string, ReadonlySet<string>>> = Object.freeze({
  'talos.ui.button': new Set(['leading-signal', 'primary-content']),
  'talos.ui.status-indicator': new Set(['status-mark', 'status-label']),
  'talos.ui.loading-overlay': new Set(['loading-message']),
})

export function realSlotIdsFor(componentId: string): ReadonlySet<string> {
  return LAB_SANDBOX_REAL_SLOT_IDS[componentId] ?? new Set<string>()
}

// selector: 形式的 motion target 白名单 —— 只允许 sandbox 声明的安全选择器，
// 不允许任意未经验证的选择器注入。
export const LAB_SANDBOX_SAFE_SELECTORS: ReadonlySet<string> = new Set(
  Object.values(LAB_SANDBOX_PROBE_SELECTORS),
)
