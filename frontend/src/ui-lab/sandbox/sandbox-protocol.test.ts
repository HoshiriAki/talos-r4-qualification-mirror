// ── Sandbox 协议测试 ─────────────────────────────────────────────────────
// envelope 校验、per-type payload validator、revision/sequence guard。

import { describe, expect, it } from 'vitest'
import {
  createLabMessage,
  isFiniteNonNegativeInteger,
  isLabProtocolEnvelope,
  UI_LAB_PROTOCOL_VERSION,
  validateLabMessage,
} from './sandbox-protocol'
import { createInitialLabSession, LAB_DEFAULT_VIEWPORTS } from '../state/lab-session'

const session = createInitialLabSession('talos.ui.button', LAB_DEFAULT_VIEWPORTS['desktop-1600'])

describe('envelope validation', () => {
  it('accepts a valid envelope', () => {
    const message = createLabMessage('lab-1', 1, 0, 'LAB_PATCH', 'talos.ui.button', {
      revision: 1, path: 'component.props.variant', value: 'secondary', source: 'inspector',
    })
    expect(isLabProtocolEnvelope(message)).toBe(true)
    expect(validateLabMessage(message)).toEqual({ ok: true })
  })

  it('rejects unknown message types', () => {
    const message = createLabMessage('lab-1', 1, 0, 'LAB_BOGUS' as never, null, {})
    expect(isLabProtocolEnvelope(message)).toBe(false)
  })

  it('rejects negative or fractional sequence/revision', () => {
    expect(isFiniteNonNegativeInteger(-1)).toBe(false)
    expect(isFiniteNonNegativeInteger(1.5)).toBe(false)
    expect(isFiniteNonNegativeInteger(Number.NaN)).toBe(false)
    expect(isFiniteNonNegativeInteger(Infinity)).toBe(false)
    expect(isFiniteNonNegativeInteger(0)).toBe(true)
  })
})

describe('per-type payload validation', () => {
  it('validates LAB_INIT requires a session', () => {
    const bad = createLabMessage('lab-1', 1, 0, 'LAB_INIT', null, {})
    expect(validateLabMessage(bad).ok).toBe(false)
    const good = createLabMessage('lab-1', 1, 0, 'LAB_INIT', null, { session })
    expect(validateLabMessage(good).ok).toBe(true)
  })

  it('validates LAB_PATCH source and revision', () => {
    const badSource = createLabMessage('lab-1', 1, 0, 'LAB_PATCH', null, {
      revision: 1, path: 'component.props.variant', value: 1, source: 'rogue',
    })
    expect(validateLabMessage(badSource).ok).toBe(false)
    const badRevision = createLabMessage('lab-1', 1, 0, 'LAB_PATCH', null, {
      revision: -1, path: 'x', value: 1, source: 'inspector',
    })
    expect(validateLabMessage(badRevision).ok).toBe(false)
  })

  it('validates LAB_COMMAND known commands and seek progress', () => {
    const unknown = createLabMessage('lab-1', 1, 0, 'LAB_COMMAND', null, { command: 'fly' })
    expect(validateLabMessage(unknown).ok).toBe(false)
    const play = createLabMessage('lab-1', 1, 0, 'LAB_COMMAND', null, { command: 'motion.play' })
    expect(validateLabMessage(play).ok).toBe(true)
    const badSeek = createLabMessage('lab-1', 1, 0, 'LAB_COMMAND', null, { command: 'motion.seek', progress: 'x' })
    expect(validateLabMessage(badSeek).ok).toBe(false)
    const seek = createLabMessage('lab-1', 1, 0, 'LAB_COMMAND', null, { command: 'motion.seek', progress: 0.5 })
    expect(validateLabMessage(seek).ok).toBe(true)
  })

  it('validates SANDBOX_SNAPSHOT requires session + revision', () => {
    const bad = createLabMessage('lab-1', 1, 5, 'SANDBOX_SNAPSHOT', null, { revision: 5 })
    expect(validateLabMessage(bad).ok).toBe(false)
    const good = createLabMessage('lab-1', 1, 5, 'SANDBOX_SNAPSHOT', null, { revision: 5, session })
    expect(validateLabMessage(good).ok).toBe(true)
  })

  it('validates SANDBOX_ERROR requires message', () => {
    const bad = createLabMessage('lab-1', 1, 0, 'SANDBOX_ERROR', null, {})
    expect(validateLabMessage(bad).ok).toBe(false)
  })
})

describe('protocol version stability', () => {
  it('pins the protocol version constant', () => {
    expect(UI_LAB_PROTOCOL_VERSION).toBe('1.0')
  })
})
