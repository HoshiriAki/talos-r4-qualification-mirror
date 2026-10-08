// ── 统一 Registry value contract 入口（薄封装）─────────────────────────────
// 权威实现位于 lab-session.ts 的 validateLabPathValue；此处 re-export 并
// 提供批量校验，供 Host/Sandbox/协议/Snapshot 复用同一套规则。

import type { UiComponentDefinition } from '@/ui'
import { validateLabPathValue } from './lab-session'

export { validateLabPathValue } from './lab-session'

export interface LabValueValidationResult {
  ok: boolean
  reason?: string
}

/**
 * 批量校验（用于整 session / snapshot 遍历）。返回第一个 reason。
 */
export function validateLabPathValues(
  component: UiComponentDefinition,
  entries: Array<{ path: string; value: unknown }>,
): LabValueValidationResult {
  for (const entry of entries) {
    const reason = validateLabPathValue(component, entry.path, entry.value)
    if (reason) return { ok: false, reason }
  }
  return { ok: true }
}
