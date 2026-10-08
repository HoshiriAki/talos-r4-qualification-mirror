// @ts-nocheck -- Executed directly by Node 22's type-strip test runner; not part of the browser TS program.
import test from 'node:test'
import assert from 'node:assert/strict'
import { buildHudScene, buildWorkflowSteps } from '../hudSceneModel.ts'

const node = {
  id: 'devices-out',
  kind: 'device-cluster',
  label: '在外设备',
  status: 'normal',
  count: 12,
  route: { name: 'devices' as const },
}

test('projects live nodes into deterministic isometric modules', () => {
  const [module] = buildHudScene([node])

  assert.equal(module.id, 'devices-out')
  assert.equal(module.count, 12)
  assert.equal(module.route.name, 'devices')
  assert.deepEqual(module.position, { x: 390, y: 210 })
  assert.equal(module.width, 124)
  assert.equal(module.depth, 62)
  assert.equal('topPoints' in module, false)
})

test('projects more than six nodes without truncating or overlapping positions', () => {
  const modules = buildHudScene(Array.from({ length: 9 }, (_, index) => ({
    ...node,
    id: `node-${index}`,
    count: index * 9,
  })))

  assert.equal(modules.length, 9)
  assert.equal(new Set(modules.map(item => `${item.position.x}:${item.position.y}`)).size, 9)
  assert.ok(modules.every(item => item.height >= 24 && item.height <= 92))
})

test('preserves critical status and clamps negative counts', () => {
  const [module] = buildHudScene([{ ...node, status: 'critical', count: -2 }])

  assert.equal(module.status, 'critical')
  assert.equal(module.count, 0)
})

test('builds reference SOP steps without claiming a live current stage', () => {
  const steps = buildWorkflowSteps({
    ...node,
    id: 'return-risk',
    kind: 'return-risk',
    label: '逾期归还',
    status: 'critical',
    count: 3,
    route: { name: 'customers' as const },
  })

  assert.deepEqual(steps.map(step => step.label), ['识别', '联系', '回收', '验收'])
  assert.ok(steps.every(step => step.state === 'reference'))
})

test('returns an empty scene for an empty dashboard projection', () => {
  assert.deepEqual(buildHudScene([]), [])
})
