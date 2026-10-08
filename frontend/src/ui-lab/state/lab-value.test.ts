// ── 统一 Registry value contract 测试（失败基线 + 修复后全绿）──────────────
// 覆盖：select 枚举、viewport id↔尺寸一致性、未注册 motion/scene、slot 类型、
// 危险值（NaN/Infinity/function/symbol/cycle/危险 key）。这些断言在
// validateLabSession / commitLabPatch / validateLabMessage 接入 validator 之前
// 会失败（失败基线），接入后转绿。

import { describe, expect, it } from 'vitest'
import { getComponentDefinition } from '@/ui'
import {
  createInitialLabSession,
  LAB_DEFAULT_VIEWPORTS,
  validateLabSession,
} from './lab-session'
import { validateLabPathValue } from './lab-value'

const button = getComponentDefinition('talos.ui.button')
const status = getComponentDefinition('talos.ui.status-indicator')
const loading = getComponentDefinition('talos.ui.loading-overlay')

function sessionFor(componentId: string) {
  const session = createInitialLabSession(componentId, LAB_DEFAULT_VIEWPORTS['desktop-1600'])
  session.scene.id = button!.lab!.preview!.defaultScene
  return session
}

describe('validateLabPathValue', () => {
  it('rejects a select value outside Registry options', () => {
    expect(validateLabPathValue(button!, 'component.props.variant', 'bogus')).toBeTruthy()
    expect(validateLabPathValue(button!, 'component.props.variant', 'secondary')).toBeUndefined()
    expect(validateLabPathValue(status!, 'component.props.status', 'active')).toBeUndefined()
    expect(validateLabPathValue(status!, 'component.props.status', 'bogus')).toBeTruthy()
  })

  it('accepts step-aligned range values and rejects out-of-range or off-step values', () => {
    expect(validateLabPathValue(loading!, 'scene.maskOpacity', 0.5)).toBeUndefined()
    expect(validateLabPathValue(loading!, 'scene.maskOpacity', 0.53)).toMatch(/align to step 0.05/)
    expect(validateLabPathValue(loading!, 'scene.maskOpacity', 2)).toBeTruthy()
    expect(validateLabPathValue(loading!, 'scene.maskOpacity', -1)).toBeTruthy()
  })

  it('rejects a viewport whose id does not match its dimensions', () => {
    expect(validateLabPathValue(button!, 'environment.viewport', { id: 'desktop-1600', width: 1600, height: 900 })).toBeUndefined()
    expect(validateLabPathValue(button!, 'environment.viewport', { id: 'desktop-1600', width: 999, height: 900 })).toMatch(/must be 1600×900/)
    expect(validateLabPathValue(button!, 'environment.viewport', { id: 'unknown', width: 1, height: 1 })).toMatch(/unknown/)
  })

  it('rejects an activeMotionId not declared by the component', () => {
    expect(validateLabPathValue(button!, 'motion.activeMotionId', null)).toBeUndefined()
    expect(validateLabPathValue(button!, 'motion.activeMotionId', 'press-response')).toBeUndefined()
    expect(validateLabPathValue(button!, 'motion.activeMotionId', 'not-a-motion')).toMatch(/not declared/)
  })

  it('rejects a scene id outside supportedScenes', () => {
    expect(validateLabPathValue(button!, 'scene.id', 'surface')).toBeUndefined()
    expect(validateLabPathValue(button!, 'scene.id', 'not-a-scene')).toMatch(/not supported/)
  })

  it('rejects slot content of the wrong type and undeclared slot ids', () => {
    expect(validateLabPathValue(button!, 'component.slots.leading-signal.content', 'signal')).toBeUndefined()
    expect(validateLabPathValue(button!, 'component.slots.leading-signal.content', 42)).toMatch(/string or null/)
    expect(validateLabPathValue(button!, 'component.slots.nope.enabled', true)).toMatch(/not declared/)
    expect(validateLabPathValue(button!, 'component.slots.leading-signal.enabled', 'yes')).toMatch(/boolean/)
  })

  it('rejects NaN / Infinity / function / symbol / bigint / cycle / dangerous keys', () => {
    const cyclic: Record<string, unknown> = { self: null as unknown }
    cyclic.self = cyclic
    expect(validateLabPathValue(button!, 'component.props.label', Number.NaN)).toMatch(/finite/)
    expect(validateLabPathValue(button!, 'component.props.label', Number.POSITIVE_INFINITY)).toMatch(/finite/)
    expect(validateLabPathValue(button!, 'component.props.label', () => 1)).toMatch(/unsupported type/)
    expect(validateLabPathValue(button!, 'component.props.label', Symbol('x'))).toMatch(/unsupported type/)
    expect(validateLabPathValue(button!, 'component.props.label', 10n)).toMatch(/unsupported type/)
    expect(validateLabPathValue(button!, 'component.props.label', cyclic)).toMatch(/not serializable/)
    expect(validateLabPathValue(button!, 'component.__proto__.polluted', true)).toMatch(/[Ff]orbidden/)
  })

  it('rejects unregistered component prop paths', () => {
    expect(validateLabPathValue(button!, 'component.props.arbitrary', 'x')).toMatch(/not declared/)
  })
})

describe('validateLabSession strict value contract (integration)', () => {
  it('rejects a session whose viewport id does not match its dimensions', () => {
    const session = sessionFor('talos.ui.button')
    session.environment.viewport = { id: 'desktop-1600', width: 999, height: 999 }
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
  })

  it('rejects a session with an undeclared activeMotionId', () => {
    const session = sessionFor('talos.ui.button')
    session.motion.activeMotionId = 'not-a-motion'
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
  })

  it('rejects a session with a scene id outside supportedScenes', () => {
    const session = sessionFor('talos.ui.button')
    session.scene.id = 'not-a-scene'
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
  })

  it('rejects a session with an out-of-enum component prop', () => {
    const session = sessionFor('talos.ui.button')
    session.component.props.variant = 'bogus'
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
  })

  it('rejects a session whose slot map carries an undeclared or malformed slot', () => {
    const session = sessionFor('talos.ui.button')
    session.component.slots['nope'] = { enabled: true, content: null }
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
    session.component.slots = { ...session.component.slots, 'leading-signal': { enabled: 'yes' as never, content: null } }
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(false)
  })

  it('accepts a clean initial session', () => {
    const session = sessionFor('talos.ui.button')
    expect(validateLabSession(session, { componentId: 'talos.ui.button', component: button }).ok).toBe(true)
  })
})
