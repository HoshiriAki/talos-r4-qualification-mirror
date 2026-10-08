// ── Motion target resolver 测试 ──────────────────────────────────────────
// specimen / probe / slot / selector 白名单、缺失 target 返回 null。

import { describe, expect, it } from 'vitest'
import { isLabMotionTarget, resolveMotionTarget } from './motion-target'

function probeRoot(probe: Element | null): Element {
  return {
    querySelector(selector: string) {
      return selector === '[data-lab-probe]' ? probe : null
    },
  } as unknown as Element
}

describe('isLabMotionTarget', () => {
  it('accepts Registry-verifiable target kinds', () => {
    expect(isLabMotionTarget('specimen')).toBe(true)
    expect(isLabMotionTarget('probe')).toBe(true)
    expect(isLabMotionTarget('slot:leading-signal')).toBe(true)
    expect(isLabMotionTarget('selector:.talos-button__signal')).toBe(true)
  })

  it('rejects arbitrary values', () => {
    expect(isLabMotionTarget('')).toBe(false)
    expect(isLabMotionTarget('slot:')).toBe(false)
    expect(isLabMotionTarget('selector:')).toBe(false)
    expect(isLabMotionTarget('window.__proto__')).toBe(false)
  })
})

describe('resolveMotionTarget', () => {
  it('specimen resolves to the root node', () => {
    const root = probeRoot(null)
    expect(resolveMotionTarget(root, 'specimen')).toBe(root)
  })

  it('probe prefers [data-lab-probe] and falls back to the declared probe selector', () => {
    const probe = {} as unknown as HTMLElement
    expect(resolveMotionTarget(probeRoot(probe), 'probe')).toBe(probe)

    const fallback = { __marker: 'signal' } as unknown as HTMLElement
    const fallbackRoot = {
      querySelector(selector: string) {
        return selector === '.talos-button__signal' ? fallback : null
      },
    } as unknown as Element
    expect(resolveMotionTarget(fallbackRoot, 'probe', { probeSelector: '.talos-button__signal' })).toBe(fallback)
  })

  it('returns null when probe has no node and no fallback', () => {
    expect(resolveMotionTarget(probeRoot(null), 'probe')).toBeNull()
  })

  it('slot resolves to the matching data-lab-slot node', () => {
    const slot = {} as unknown as HTMLElement
    const slotRoot = {
      querySelector(selector: string) {
        return selector === '[data-lab-slot="leading-signal"]' ? slot : null
      },
    } as unknown as Element
    expect(resolveMotionTarget(slotRoot, 'slot:leading-signal')).toBe(slot)
    expect(resolveMotionTarget(slotRoot, 'slot:missing-slot')).toBeNull()
  })

  it('selector only resolves from the safe whitelist', () => {
    const safe = {} as unknown as HTMLElement
    const safeRoot = {
      querySelector(selector: string) {
        return selector === '.talos-button__signal' ? safe : null
      },
    } as unknown as Element
    const allowed = new Set(['.talos-button__signal'])
    expect(resolveMotionTarget(safeRoot, 'selector:.talos-button__signal', { safeSelectors: allowed })).toBe(safe)
    // 不在白名单 → null，不注入任意选择器
    expect(resolveMotionTarget(safeRoot, 'selector:#admin-panel', { safeSelectors: allowed })).toBeNull()
  })

  it('returns null for an empty root', () => {
    expect(resolveMotionTarget(null, 'specimen')).toBeNull()
    expect(resolveMotionTarget(null, 'probe')).toBeNull()
  })
})
