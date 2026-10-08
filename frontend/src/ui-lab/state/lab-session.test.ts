// ── UI Lab 状态模型测试 ──────────────────────────────────────────────────
// 覆盖 Registry path 语义、嵌套路径、非法/危险路径、prototype pollution、
// 未注册路径拒绝，以及 revision/baseline 语义。

import { describe, expect, it } from 'vitest'
import {
  assertSafeLabPath,
  cloneLabSession,
  collectLabControlPaths,
  collectLabSlotPaths,
  createInitialLabSession,
  labPathForControl,
  LAB_DEFAULT_VIEWPORTS,
  readLabPath,
  writeLabPath,
} from './lab-session'

const defaultViewport = LAB_DEFAULT_VIEWPORTS['desktop-1600']

function makeSession() {
  return createInitialLabSession('talos.ui.button', defaultViewport)
}

describe('labPathForControl', () => {
  it('maps props.variant to component.props.variant', () => {
    expect(labPathForControl('component', 'props.variant')).toBe('component.props.variant')
  })

  it('maps props.label to component.props.label', () => {
    expect(labPathForControl('component', 'props.label')).toBe('component.props.label')
  })

  it('maps props.state to component.props.state', () => {
    expect(labPathForControl('component', 'props.state')).toBe('component.props.state')
  })

  it('keeps an already-prefixed component path', () => {
    expect(labPathForControl('component', 'component.props.variant')).toBe('component.props.variant')
  })

  it('maps scene paths to scene domain', () => {
    expect(labPathForControl('scene', 'scene.maskOpacity')).toBe('scene.maskOpacity')
    expect(labPathForControl('scene', 'maskOpacity')).toBe('scene.maskOpacity')
  })

  it('maps motion and environment paths', () => {
    expect(labPathForControl('motion', 'motion.playbackRate')).toBe('motion.playbackRate')
    expect(labPathForControl('environment', 'environment.theme')).toBe('environment.theme')
  })

  it('rejects unknown scope', () => {
    expect(() => labPathForControl('bogus', 'x')).toThrow(/Unknown lab control scope/)
  })
})

describe('readLabPath / writeLabPath', () => {
  it('writes and reads nested object paths', () => {
    const session = makeSession()
    writeLabPath(session, 'component.props.config', { deep: { value: 1 } })
    expect(readLabPath(session, 'component.props.config.deep.value')).toBe(1)
  })

  it('returns undefined for a missing path', () => {
    const session = makeSession()
    expect(readLabPath(session, 'component.props.nope')).toBeUndefined()
    expect(readLabPath(session, 'scene.unknown')).toBeUndefined()
  })

  it('writes through missing intermediate objects only when safe', () => {
    const session = makeSession()
    writeLabPath(session, 'component.props.a.b.c', 42)
    expect(readLabPath(session, 'component.props.a.b.c')).toBe(42)
  })

  it('throws when writing through a non-object leaf', () => {
    const session = makeSession()
    writeLabPath(session, 'component.props.variant', 'secondary')
    expect(() => writeLabPath(session, 'component.props.variant.nested', 1)).toThrow(
      /Cannot write through a non-object/,
    )
  })

  it('rejects empty paths', () => {
    const session = makeSession()
    expect(() => writeLabPath(session, '', 1)).toThrow(/Empty lab path/)
  })
})

describe('prototype pollution protection', () => {
  const dangerousPaths = [
    'component.__proto__.polluted',
    'component.prototype.x',
    'component.constructor.y',
    '__proto__',
    'prototype',
    'constructor',
    'component.props.__proto__.polluted',
  ]

  it('throws on dangerous path segments', () => {
    for (const path of dangerousPaths) {
      expect(() => assertSafeLabPath(path), path).toThrow(/Forbidden lab path segment/)
    }
  })

  it('never writes to a dangerous path', () => {
    const session = makeSession()
    expect(() => writeLabPath(session, 'component.__proto__.polluted', true)).toThrow()
    expect(() => writeLabPath(session, 'component.prototype.x', true)).toThrow()
    expect(() => writeLabPath(session, 'component.constructor.y', true)).toThrow()
    expect(({} as Record<string, unknown>).polluted).toBeUndefined()
  })

  it('rejects dangerous paths on read', () => {
    const session = makeSession()
    expect(() => readLabPath(session, 'constructor')).toThrow()
    expect(() => readLabPath(session, 'component.__proto__')).toThrow()
  })
})

describe('registry-declared path enforcement', () => {
  it('rejects paths not declared in the Registry', () => {
    const session = makeSession()
    const allowed = collectLabControlPaths([
      { scope: 'component', path: 'props.variant' },
      { scope: 'component', path: 'props.label' },
    ])
    expect(() =>
      writeLabPath(session, 'component.props.variant', 'secondary', { allowedPaths: allowed }),
    ).not.toThrow()
    expect(() =>
      writeLabPath(session, 'component.props.arbitrary', 'x', { allowedPaths: allowed }),
    ).toThrow(/not declared in the Registry/)
  })

  it('collects slot paths alongside control paths', () => {
    const control = collectLabControlPaths([{ scope: 'component', path: 'props.variant' }])
    const slots = collectLabSlotPaths([{ id: 'leading-signal' }, { id: 'trailing-meta' }])
    expect(control.has('component.props.variant')).toBe(true)
    expect(slots.has('component.slots.leading-signal.enabled')).toBe(true)
    expect(slots.has('component.slots.leading-signal.content')).toBe(true)
    expect(slots.has('component.slots.trailing-meta.enabled')).toBe(true)
  })
})

describe('session lifecycle', () => {
  it('starts at revision 0 with an empty component props map', () => {
    const session = makeSession()
    expect(session.revision).toBe(0)
    expect(session.component.id).toBe('talos.ui.button')
    expect(session.component.props).toEqual({})
  })

  it('clone is deep and independent', () => {
    const session = makeSession()
    writeLabPath(session, 'component.props.variant', 'danger')
    const copy = cloneLabSession(session)
    writeLabPath(copy, 'component.props.variant', 'ghost')
    expect(readLabPath(session, 'component.props.variant')).toBe('danger')
    expect(readLabPath(copy, 'component.props.variant')).toBe('ghost')
  })
})
