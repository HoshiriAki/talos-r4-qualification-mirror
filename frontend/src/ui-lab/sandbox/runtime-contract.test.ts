import { describe, expect, it } from 'vitest'
import { createLabMessage, validateLabMessage } from './sandbox-protocol'
import { createInitialLabSession, LAB_DEFAULT_VIEWPORTS, validateLabSession } from '../state/lab-session'

const viewport = LAB_DEFAULT_VIEWPORTS['desktop-1600']

describe('UI Lab runtime context contract', () => {
  it('rejects zero sequences, wrong epochs, and patch revision mismatches', () => {
    const message = createLabMessage('lab-1', 1, 2, 'SANDBOX_PATCH', 'talos.ui.button', {
      revision: 2, baseRevision: 1, path: 'component.props.label', value: 'C',
    }, 'epoch-a')
    expect(validateLabMessage({ ...message, sequence: 0 })).toMatchObject({ ok: false })
    expect(validateLabMessage(message, { sessionId: 'lab-1', frameEpoch: 'epoch-b' })).toMatchObject({ ok: false, reason: 'frame epoch mismatch' })
    expect(validateLabMessage({ ...message, revision: 3 }, { frameEpoch: 'epoch-a' })).toMatchObject({ ok: false })
  })

  it('validates complete sessions and rejects prototype pollution/non-finite values', () => {
    const session = createInitialLabSession('talos.ui.button', viewport)
    expect(validateLabSession(session, { componentId: 'talos.ui.button' })).toEqual({ ok: true })
    const polluted = { ...session, component: { ...session.component, props: JSON.parse('{"__proto__":{"polluted":true}}') } }
    expect(validateLabSession(polluted, { componentId: 'talos.ui.button' }).ok).toBe(false)
    const nonFinite = { ...session, environment: { ...session.environment, zoom: Number.NaN } }
    expect(validateLabSession(nonFinite, { componentId: 'talos.ui.button' }).ok).toBe(false)
    const unknownViewport = { ...session, environment: { ...session.environment, viewport: { ...viewport, id: 'unknown' } } }
    expect(validateLabSession(unknownViewport, { componentId: 'talos.ui.button' }).ok).toBe(false)
  })
})
